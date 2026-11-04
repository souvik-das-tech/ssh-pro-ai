"""ChatGPT brain through the OpenAI Codex CLI, signed in with the user's ChatGPT account.

Verified codex-cli 0.166 interfaces:
  * Models:  codex debug models  -> {"models": [{"slug", "display_name", "visibility", "priority", ...}]}
  * Turn 1:  codex exec --json [flags] -          (prompt on stdin)
  * Turn n:  codex exec resume --json [flags] <thread_id> -
    stdout : JSONL {"type": "thread.started"|"item.completed"|"turn.completed"|"turn.failed"|"error", ...}
    The reply is the last item of type "agent_message". ~6 s for the first turn, ~4 s later.

Tokens: the app's instructions replace Codex's own coding-agent prompt (model_instructions_file),
and the skills list and environment block are left out, so a new chat costs ~5k fewer input tokens.

Like AGY, Codex only plans: its shell, sub-agent, plugin and browser tools are disabled and the sandbox
is read-only with approvals off, so it cannot run anything. The app executes every step.
"""

from __future__ import annotations

import asyncio
import glob
import json
import logging
import os
import shutil
from collections.abc import Sequence
from pathlib import Path

from sshpro.agy import (
    NO_WINDOW, AIAuthError, AIModelError, AINotInstalled, AIProcessError, AITimeout, kill_tree,
)
from sshpro.paths import data_dir

log = logging.getLogger(__name__)

PREFIX = "chatgpt:"  # model ids in the app's dropdown: "chatgpt:<codex slug>"
CODEX_INSTALL_URL = "https://developers.openai.com/codex/cli"
EFFORT = os.environ.get("SSHPRO_CODEX_EFFORT", "low")  # low: fastest plans; the app does the work
_STREAM_LIMIT = 16 * 1024 * 1024
_AUTH_MARKERS = ("not logged in", "codex login", "unauthorized", "401", "please sign in", "login required")
_AUTH_HINT = "ChatGPT is not signed in. Open a terminal, run `codex login` and sign in."
# Tools Codex could otherwise use on this PC. The app must stay the only one that executes anything.
_DISABLED = (
    "shell_tool", "unified_exec", "multi_agent", "image_generation", "computer_use", "browser_use",
    "apps", "plugins", "hooks", "goals", "view_image", "skill_search", "tool_suggest",
)


def is_chatgpt(model: str) -> bool:
    return model.startswith(PREFIX)


def find_codex(configured: str | None = None) -> Path:
    local = os.environ.get("LOCALAPPDATA", "")
    bundled = sorted(  # the ChatGPT/Codex desktop app and the VS Code extension ship their own copy
        glob.glob(os.path.join(local, "OpenAI", "Codex", "bin", "*", "codex.exe"))
        + glob.glob(os.path.join(str(Path.home()), ".vscode", "extensions", "openai.chatgpt-*", "bin", "*", "codex.exe")),
        key=os.path.getmtime, reverse=True,
    ) if local else []
    candidates: list[str | None] = [configured, os.environ.get("SSHPRO_CODEX"), shutil.which("codex"), *bundled]
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise AINotInstalled(f"Codex CLI not found. Install it and run `codex login`: {CODEX_INSTALL_URL}")


def codex_available() -> str | None:
    try:
        return str(find_codex())
    except AINotInstalled:
        return None


def codex_workspace() -> Path:
    """An empty, app-owned working directory so Codex never sees project files."""
    path = data_dir() / "codex_workspace"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _looks_like_auth_error(*texts: str) -> bool:
    blob = " ".join(texts).lower()
    return any(marker in blob for marker in _AUTH_MARKERS)


async def _run(args: Sequence[str], stdin: bytes | None, timeout: float, cwd: Path | None = None) -> tuple[int, str, str]:
    proc = await asyncio.create_subprocess_exec(
        *args, cwd=str(cwd) if cwd else None,
        stdin=asyncio.subprocess.PIPE if stdin is not None else asyncio.subprocess.DEVNULL,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        limit=_STREAM_LIMIT, creationflags=NO_WINDOW,
    )
    try:
        out, err = await asyncio.wait_for(proc.communicate(stdin), timeout)
    except asyncio.TimeoutError:
        await kill_tree(proc.pid)
        raise AITimeout(f"ChatGPT did not answer within {timeout:.0f}s")
    except asyncio.CancelledError:
        await asyncio.shield(kill_tree(proc.pid))
        raise
    return proc.returncode or 0, out.decode("utf-8", "replace"), err.decode("utf-8", "replace")


async def list_models(command: Sequence[str] | None = None, timeout: float = 60) -> list[dict[str, str]]:
    """ChatGPT models the signed-in account can use in Codex, from its live catalog."""
    command = list(command) if command else [str(find_codex())]
    code, stdout, stderr = await _run([*command, "debug", "models"], None, timeout)
    try:
        catalog = json.loads(stdout)
    except ValueError:
        if _looks_like_auth_error(stdout, stderr):
            raise AIAuthError(_AUTH_HINT)
        raise AIProcessError(f"codex debug models returned no JSON (exit {code}): {(stderr or stdout)[-300:]}")
    entries = catalog.get("models", []) if isinstance(catalog, dict) else []
    listed = sorted((m for m in entries if isinstance(m, dict) and m.get("slug") and m.get("visibility") == "list"),
                    key=lambda m: m.get("priority", 999))
    return [{"id": PREFIX + m["slug"], "label": f"ChatGPT · {m.get('display_name') or m['slug']}"} for m in listed]


class CodexProvider:
    """AIProvider for ChatGPT via `codex exec`. One process per turn; the conversation lives in a
    Codex thread that later turns resume, so nothing has to stay running between requests."""

    name = "codex"

    def __init__(self, command: Sequence[str] | None = None, model: str = "", workdir: Path | None = None,
                 timeout: float = 150, effort: str = EFFORT):
        self._command = list(command) if command else [str(find_codex())]
        self.model = model[len(PREFIX):] if is_chatgpt(model) else model
        self._workdir = workdir
        self._timeout = timeout
        self._effort = effort
        self._instructions = ""
        self._instructions_file: Path | None = None
        self._thread: str | None = None
        self._lock = asyncio.Lock()

    def _flags(self) -> list[str]:
        flags = ["--json", "--skip-git-repo-check", "--ignore-user-config", "--ignore-rules",
                 "-c", "sandbox_mode=read-only", "-c", "approval_policy=never",
                 "-c", "skills.bundled.enabled=false", "-c", "include_environment_context=false"]
        if self._instructions_file is not None:
            flags += ["-c", f"model_instructions_file={self._instructions_file.as_posix()}"]
        for feature in _DISABLED:
            flags += ["--disable", feature]
        if self.model:
            flags += ["-m", self.model]
        if self._effort:
            flags += ["-c", f"model_reasoning_effort={self._effort}"]
        return flags

    async def start(self, instructions: str) -> None:
        """Nothing to warm up: the instructions become the model's system prompt (in place of
        Codex's own), so they cost no extra turn and are cached across conversations."""
        lines = [ln for ln in instructions.splitlines() if not ln.startswith("Acknowledge these instructions")]
        path = (self._workdir or codex_workspace()) / "instructions.md"
        path.write_text("\n".join(lines), encoding="utf-8")
        self._instructions_file = path
        self._instructions = ""

    async def ask(self, message: str) -> str:
        async with self._lock:
            if self._thread is None:
                args = [*self._command, "exec", *self._flags(), "-"]
                prompt = f"{self._instructions}\n\n{message}" if self._instructions else message
            else:
                args = [*self._command, "exec", "resume", *self._flags(), self._thread, "-"]
                prompt = message
            started = asyncio.get_running_loop().time()
            code, stdout, stderr = await _run(args, prompt.encode("utf-8"), self._timeout,
                                              self._workdir or codex_workspace())
            reply = self._parse(code, stdout, stderr)
            log.info("ChatGPT turn (%s, thread=%s): %.1fs", self.model, self._thread,
                     asyncio.get_running_loop().time() - started)
            return reply

    def _parse(self, code: int, stdout: str, stderr: str) -> str:
        reply: str | None = None
        error: str | None = None
        for line in stdout.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            try:
                event = json.loads(line)
            except ValueError:
                continue
            kind = event.get("type")
            if kind == "thread.started" and event.get("thread_id"):
                self._thread = event["thread_id"]
            elif kind == "item.completed" and (event.get("item") or {}).get("type") == "agent_message":
                reply = str(event["item"].get("text") or "")
            elif kind == "turn.failed":
                error = str((event.get("error") or {}).get("message") or "turn failed")
            elif kind == "error":
                error = str(event.get("message") or "error")
        if reply is not None and error is None:
            return reply
        detail = error or stderr.strip()[-400:] or f"exit {code}, no reply"
        if _looks_like_auth_error(detail, stderr):
            raise AIAuthError(_AUTH_HINT)
        if error is not None:
            raise AIModelError(f"ChatGPT error: {error}")
        raise AIProcessError(f"Codex produced no reply (exit {code}): {detail}")

    async def reset(self) -> None:
        self._thread = None  # the next ask() starts a new conversation

    async def close(self) -> None:
        self._thread = None
