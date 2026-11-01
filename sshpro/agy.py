"""AGY (Antigravity) CLI adapter. The AI chat depends on it; SSH, terminals and manual commands do not.

Verified AGY 1.4.7 interfaces:
  * One-shot:   agy -p <prompt> --output-format json --json-schema <file>
                -> one JSON object {status, response, structured_output?, ...}
  * Persistent: agy -p= --input-format stream-json --output-format stream-json
                stdin  : one {"event":"user","message":{"role":"user","content":...}} per line
                stdout : {"event":"init"|"step_update"|"result", ...} NDJSON
    Process start costs ~10 s; each later turn ~3 s. So the session is the primary path.

AGY is itself an agent. The app never passes --dangerously-skip-permissions: in headless mode any
tool that needs approval is auto-denied, so AGY can only *plan*; the app executes. Its own web search
needs no approval and is allowed by the prompt for live questions (news, CVEs, versions).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import subprocess
import sys
from collections import deque
from collections.abc import Sequence
from pathlib import Path

from sshpro.paths import data_dir

AGY_INSTALL_URL = "https://docs.flutter.dev/ai/antigravity-cli"
# Prevent console windows flashing up when the windowed app spawns processes.
NO_WINDOW = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0


class AIError(Exception):
    """Base class for AGY failures. Messages are safe to show to the user."""


class AINotInstalled(AIError):
    pass


class AIAuthError(AIError):
    pass


class AITimeout(AIError):
    pass


class AIModelError(AIError):
    """The model/service returned an error result."""


class AIProcessError(AIError):
    """The AGY process crashed or produced unreadable output."""


async def kill_tree(pid: int) -> None:
    """Kill a process and all its children (agy.exe may spawn helpers)."""
    if sys.platform == "win32":
        proc = await asyncio.create_subprocess_exec(
            "taskkill", "/T", "/F", "/PID", str(pid),
            stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL, creationflags=NO_WINDOW,
        )
        await proc.wait()
    else:
        import signal

        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def agy_available() -> str | None:
    """Path of the AGY CLI, or None when it is not installed."""
    try:
        return str(find_agy())
    except AINotInstalled:
        return None


log = logging.getLogger(__name__)

# flash-low: same plan quality as flash-medium in the live tests, ~3-4 s/turn vs ~5-10 s (no thinking).
DEFAULT_MODEL = "gemini-3.8-flash-low"
_AUTH_MARKERS = ("not authenticated", "[auth error]", "sign in", "login required", "unauthenticated")
_STREAM_LIMIT = 16 * 1024 * 1024


def find_agy(configured: str | None = None) -> Path:
    candidates: list[str | None] = [
        configured,
        os.environ.get("SSHPRO_AGY"),
        shutil.which("agy"),
        os.path.join(os.environ.get("LOCALAPPDATA", ""), "agy", "bin", "agy.exe"),
    ]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise AINotInstalled(f"AGY CLI not found. Install it: {AGY_INSTALL_URL}")


def agy_workspace() -> Path:
    """An empty, app-owned working directory so AGY never sees project files."""
    path = data_dir() / "agy_workspace"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _looks_like_auth_error(*texts: str) -> bool:
    blob = " ".join(texts).lower()
    return any(marker in blob for marker in _AUTH_MARKERS)


async def agy_version(command: Sequence[str], timeout: float = 20) -> str:
    proc = await asyncio.create_subprocess_exec(
        *command, "--version",
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        stdin=asyncio.subprocess.DEVNULL, creationflags=NO_WINDOW,
    )
    try:
        out, _ = await asyncio.wait_for(proc.communicate(), timeout)
    except asyncio.TimeoutError:
        await kill_tree(proc.pid)
        raise AITimeout("agy --version timed out")
    return out.decode("utf-8", "replace").strip()


async def list_models(command: Sequence[str] | None = None, timeout: float = 60) -> list[dict[str, str]]:
    """Models the signed-in AGY account can use, straight from `agy models`, so new ones show up
    without a code change. Each line is `<id>\\t<label>`; the "Fetching..." banner has no tab."""
    command = list(command) if command else [str(find_agy())]
    proc = await asyncio.create_subprocess_exec(
        *command, "models",
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        stdin=asyncio.subprocess.DEVNULL, creationflags=NO_WINDOW,
    )
    try:
        out, err = await asyncio.wait_for(proc.communicate(), timeout)
    except asyncio.TimeoutError:
        await kill_tree(proc.pid)
        raise AITimeout("agy models timed out")
    stdout = out.decode("utf-8", "replace")
    stderr = err.decode("utf-8", "replace")
    models = []
    for line in stdout.splitlines():
        model_id, sep, label = line.strip().partition("\t")
        if sep and model_id and " " not in model_id:
            models.append({"id": model_id, "label": label.strip() or model_id})
    if not models:
        if _looks_like_auth_error(stdout, stderr):
            raise AIAuthError("AGY is not signed in. Open a terminal, run `agy` and sign in.")
        raise AIProcessError(f"agy models returned no models (exit {proc.returncode}): {(stderr or stdout)[-300:]}")
    return models


class AgySession:
    """One long-running `agy` stream-json conversation. Turns are serialized."""

    def __init__(
        self,
        command: Sequence[str],
        model: str = DEFAULT_MODEL,
        workdir: Path | None = None,
        start_timeout: float = 90,
        turn_timeout: float = 60,
        max_turns: int = 40,
    ):
        self._command = list(command)
        self.model = model
        self._workdir = workdir
        self._start_timeout = start_timeout
        self._turn_timeout = turn_timeout
        self._max_turns = max_turns
        self._instructions = ""
        self._proc: asyncio.subprocess.Process | None = None
        self._stderr_tail: deque[str] = deque(maxlen=50)
        self._stderr_task: asyncio.Task | None = None
        self._lock = asyncio.Lock()
        self._rewarm_task: asyncio.Task | None = None
        self._turns = 0
        self._last_usage: dict[str, int] = {}
        self.conversation_id: str | None = None

    @property
    def alive(self) -> bool:
        return self._proc is not None and self._proc.returncode is None

    async def start(self, instructions: str) -> None:
        self._instructions = instructions
        async with self._lock:
            await self._ensure_started()

    async def _ensure_started(self) -> None:
        if self.alive and self._turns < self._max_turns:
            return
        await self._kill()
        args = [
            *self._command, "-p=",
            "--input-format", "stream-json", "--output-format", "stream-json",
            "--model", self.model, "--disable-slash-commands",
        ]
        log.info("Starting AGY session (model=%s)", self.model)
        self._proc = await asyncio.create_subprocess_exec(
            *args,
            cwd=str(self._workdir or agy_workspace()),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            limit=_STREAM_LIMIT,
            creationflags=NO_WINDOW,
        )
        self._stderr_tail.clear()
        self._stderr_task = asyncio.create_task(self._drain_stderr(self._proc))
        self._turns = 0
        self._last_usage = {}
        if self._instructions:
            # The standing instructions are the first turn; the model acknowledges them.
            await self._turn(self._instructions, self._start_timeout)

    async def ask(self, message: str) -> str:
        async with self._lock:
            await self._ensure_started()
            reply = await self._turn(message, self._turn_timeout)
        if self._turns >= self._max_turns and self._rewarm_task is None:
            # used up: start the fresh session now (~10 s), so the next command doesn't wait for it
            self._rewarm_task = asyncio.create_task(self._rewarm())
        return reply

    async def _rewarm(self) -> None:
        try:
            async with self._lock:
                await self._ensure_started()
        except Exception as exc:  # the next ask() starts it again and reports the error
            log.warning("AGY background restart failed: %s", exc)
        finally:
            self._rewarm_task = None

    async def _turn(self, text: str, timeout: float) -> str:
        assert self._proc is not None and self._proc.stdin is not None
        line = json.dumps({"event": "user", "message": {"role": "user", "content": text}})
        started = asyncio.get_running_loop().time()
        try:
            self._proc.stdin.write(line.encode("utf-8") + b"\n")
            await self._proc.stdin.drain()
            result = await asyncio.wait_for(self._read_result(), timeout)
        except asyncio.TimeoutError:
            await self._kill()
            raise AITimeout(f"AGY did not answer within {timeout:.0f}s")
        except asyncio.CancelledError:
            # The turn state is unknown; the session cannot be reused.
            await asyncio.shield(self._kill())
            raise
        except (BrokenPipeError, ConnectionResetError) as exc:
            await self._kill()
            raise AIProcessError(f"AGY session closed unexpectedly: {exc}") from exc
        self._turns += 1
        # AGY reports usage cumulatively per session; log the per-turn delta.
        usage = {k: int(v or 0) for k, v in (result.get("usage") or {}).items() if isinstance(v, (int, float))}
        delta = {k: v - self._last_usage.get(k, 0) for k, v in usage.items()}
        self._last_usage = usage
        log.info(
            "AGY turn %d: %.1fs (in=%s out=%s think=%s)",
            self._turns, asyncio.get_running_loop().time() - started,
            delta.get("input_tokens"), delta.get("output_tokens"), delta.get("thinking_tokens"),
        )
        return self._check_result(result)

    async def _read_result(self) -> dict:
        proc = self._proc  # local: _kill() may clear self._proc while we wait (e.g. app shutdown)
        assert proc is not None and proc.stdout is not None
        while True:
            raw = await proc.stdout.readline()
            if not raw:
                await proc.wait()
                tail = "\n".join(self._stderr_tail)
                if _looks_like_auth_error(tail):
                    raise AIAuthError("AGY is not signed in. Open a terminal, run `agy` and sign in.")
                raise AIProcessError(f"AGY exited (code {proc.returncode}). {tail[-400:]}")
            try:
                event = json.loads(raw)
            except json.JSONDecodeError:
                log.debug("AGY non-JSON line: %s", raw[:200])
                continue
            kind = event.get("event")
            if kind == "init":
                self.conversation_id = event.get("conversation_id")
            elif kind == "result":
                return event.get("result") or {}

    def _check_result(self, result: dict) -> str:
        status = result.get("status")
        if result.get("denied_actions"):
            log.info("AGY tried tools that were denied (expected): %s", result["denied_actions"])
        if status != "SUCCESS":
            error = str(result.get("error") or "unknown error")
            if _looks_like_auth_error(error, *self._stderr_tail):
                raise AIAuthError("AGY is not signed in. Open a terminal, run `agy` and sign in.")
            raise AIModelError(f"AGY returned {status}: {error}")
        return str(result.get("response") or "")

    async def _drain_stderr(self, proc: asyncio.subprocess.Process) -> None:
        assert proc.stderr is not None
        async for raw in proc.stderr:
            text = raw.decode("utf-8", "replace").rstrip()
            if text:
                self._stderr_tail.append(text)
                log.debug("agy stderr: %s", text)

    async def reset(self) -> None:
        async with self._lock:
            await self._kill()

    async def _kill(self) -> None:
        proc, self._proc = self._proc, None
        if proc is not None and proc.returncode is None:
            try:
                if proc.stdin is not None:
                    proc.stdin.close()
            except Exception:
                pass
            await kill_tree(proc.pid)
            try:
                await asyncio.wait_for(proc.wait(), 5)
            except asyncio.TimeoutError:
                log.warning("AGY process %s did not exit after kill", proc.pid)
        if self._stderr_task is not None:
            self._stderr_task.cancel()
            self._stderr_task = None

    async def close(self) -> None:
        if self._rewarm_task is not None:
            self._rewarm_task.cancel()
        await self._kill()


class AgyOneShot:
    """Fallback: one process per request, with AGY's own JSON-schema enforcement."""

    def __init__(
        self,
        command: Sequence[str],
        model: str = DEFAULT_MODEL,
        schema: dict | None = None,
        workdir: Path | None = None,
        timeout: float = 90,
    ):
        self._command = list(command)
        self.model = model
        self._schema = schema
        self._workdir = workdir
        self._timeout = timeout
        self._instructions = ""

    async def start(self, instructions: str) -> None:
        self._instructions = instructions

    async def ask(self, message: str) -> str:
        workdir = self._workdir or agy_workspace()
        prompt = f"{self._instructions}\n\n{message}" if self._instructions else message
        args = [
            *self._command, "-p", prompt, "--output-format", "json",
            "--print-timeout", f"{int(self._timeout)}s",
            "--model", self.model, "--disable-slash-commands",
        ]
        if self._schema is not None:
            schema_file = workdir / "plan.schema.json"
            schema_file.write_text(json.dumps(self._schema), encoding="utf-8")
            args += ["--json-schema", str(schema_file)]

        proc = await asyncio.create_subprocess_exec(
            *args, cwd=str(workdir),
            stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
            limit=_STREAM_LIMIT, creationflags=NO_WINDOW,
        )
        try:
            out, err = await asyncio.wait_for(proc.communicate(), self._timeout + 15)
        except asyncio.TimeoutError:
            await kill_tree(proc.pid)
            raise AITimeout(f"AGY did not answer within {self._timeout:.0f}s")
        except asyncio.CancelledError:
            await asyncio.shield(kill_tree(proc.pid))
            raise
        stdout = out.decode("utf-8", "replace")
        stderr = err.decode("utf-8", "replace")

        result = None
        for line in reversed(stdout.splitlines()):
            line = line.strip()
            if line.startswith("{"):
                try:
                    result = json.loads(line)
                    break
                except json.JSONDecodeError:
                    continue
        if result is None:
            if _looks_like_auth_error(stdout, stderr):
                raise AIAuthError("AGY is not signed in. Open a terminal, run `agy` and sign in.")
            raise AIProcessError(f"AGY produced no JSON (exit {proc.returncode}): {stderr[-400:]}")
        if result.get("status") != "SUCCESS":
            error = str(result.get("error") or stderr[-300:] or "unknown error")
            if _looks_like_auth_error(error, stderr):
                raise AIAuthError("AGY is not signed in. Open a terminal, run `agy` and sign in.")
            raise AIModelError(f"AGY returned {result.get('status')}: {error}")
        if result.get("structured_output") is not None:
            return json.dumps(result["structured_output"], ensure_ascii=False)
        return str(result.get("response") or "")

    async def reset(self) -> None:
        return None

    async def close(self) -> None:
        return None


class AgyProvider:
    """AIProvider for AGY: persistent session first, restart once, then one-shot fallback.

    A spare session is kept warm in the background: a fresh session spends ~10 s on the standing
    instructions, so reset() (new chat, reopening an old one) swaps the spare in instead of waiting.
    """

    name = "agy"

    def __init__(
        self,
        command: Sequence[str] | None = None,
        model: str = DEFAULT_MODEL,
        schema: dict | None = None,
        workdir: Path | None = None,
        session_kwargs: dict | None = None,
        oneshot_kwargs: dict | None = None,
    ):
        self._command = list(command) if command else [str(find_agy())]
        self.model = model
        self._workdir = workdir
        self._session_kwargs = session_kwargs or {}
        self.session = self._new_session()
        self.oneshot = AgyOneShot(self._command, model, schema, workdir, **(oneshot_kwargs or {}))
        self.mode = "session"
        self._instructions = ""
        self._spare: AgySession | None = None
        self._bg: set[asyncio.Task] = set()  # spare warm-ups and old sessions being closed

    def _new_session(self) -> AgySession:
        return AgySession(self._command, self.model, self._workdir, **self._session_kwargs)

    def _background(self, coro, what: str) -> None:
        async def run() -> None:
            try:
                await coro
            except asyncio.CancelledError:
                pass
            except Exception as exc:  # the session restarts on its first ask() and reports it then
                log.warning("AGY %s failed: %s", what, exc)

        task = asyncio.create_task(run())
        self._bg.add(task)
        task.add_done_callback(self._bg.discard)

    def _warm_spare(self) -> None:
        if self.mode == "session" and self._spare is None:
            self._spare = self._new_session()
            self._background(self._spare.start(self._instructions), "spare session warm-up")

    async def start(self, instructions: str) -> None:
        self._instructions = instructions
        await self.oneshot.start(instructions)
        try:
            await self.session.start(instructions)
            self.mode = "session"
        except (AIProcessError, AITimeout, AIModelError) as exc:
            log.warning("AGY session failed to start (%s); using one-shot mode", exc)
            self.mode = "oneshot"
        self._warm_spare()

    async def ask(self, message: str) -> str:
        if self.mode == "session":
            # A timeout is raised straight away (the killed session restarts on the next ask);
            # only a crashed process is retried, then we fall back to one-shot.
            for attempt in (1, 2):
                try:
                    return await self.session.ask(message)
                except AIProcessError as exc:
                    log.warning("AGY session turn failed (attempt %d): %s", attempt, exc)
                    if attempt == 2:
                        log.warning("Falling back to one-shot mode")
                        self.mode = "oneshot"
        return await self.oneshot.ask(message)

    async def reset(self) -> None:
        """Fresh conversation without the wait: the warm spare (or one still warming up) takes over."""
        old, self.session = self.session, self._spare or self._new_session()
        self._spare = None
        if not self.session._instructions:
            self.session._instructions = self._instructions  # its first ask() sends them
        self._background(old.close(), "session close")
        self._warm_spare()

    async def close(self) -> None:
        await self.session.close()
        if self._spare is not None:
            await self._spare.close()  # its warm-up then ends with "AGY exited"
            self._spare = None
        if self._bg:
            await asyncio.wait(list(self._bg), timeout=10)
