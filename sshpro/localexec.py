"""Commands on this Windows PC for the AI: find a file, build a project, pack it before an upload.

Runs Windows PowerShell non-interactively without a console window. Results have the same shape
as SSHManager.run so the AI loop treats this PC like one more server.
"""

from __future__ import annotations

import asyncio
import base64
import os
import subprocess
import time
from typing import Any

MAX_OUTPUT = 200_000
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)

# UTF-8 output, no progress bars; the exit code reflects the last statement (cmdlet or program).
_PRELUDE = "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [Text.Encoding]::UTF8\n"
_EPILOGUE = "\nif (-not $?) { if ($LASTEXITCODE) { exit $LASTEXITCODE } else { exit 1 } }\n"


def expand(path: str) -> str:
    return os.path.expanduser(os.path.expandvars(path.strip()))


def _kill_tree(pid: int) -> None:
    subprocess.run(["taskkill", "/T", "/F", "/PID", str(pid)], capture_output=True, creationflags=NO_WINDOW)


def _result(started: float, exit_code: int | None = None, stdout: str = "", stderr: str = "",
            error: str | None = None) -> dict[str, Any]:
    return {"exit": exit_code, "stdout": stdout[-MAX_OUTPUT:], "stderr": stderr[-MAX_OUTPUT:],
            "error": error, "duration": round(time.monotonic() - started, 2)}


def _text(data: bytes | None) -> str:
    return (data or b"").decode("utf-8", "replace").replace("\r\n", "\n")


async def run_local(command: str, cwd: str | None = None, timeout: float = 600) -> dict[str, Any]:
    """Run one PowerShell command. Never raises for command failures: they come back in the result."""
    started = time.monotonic()
    if cwd:
        cwd = expand(cwd)
        if not os.path.isdir(cwd):
            return _result(started, error=f"Folder not found on this PC: {cwd}")
    script = base64.b64encode((_PRELUDE + command + _EPILOGUE).encode("utf-16-le")).decode()
    try:
        proc = await asyncio.create_subprocess_exec(
            "powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
            "-EncodedCommand", script, cwd=cwd or None, stdin=asyncio.subprocess.DEVNULL,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE, creationflags=NO_WINDOW)
    except OSError as exc:
        return _result(started, error=f"Could not start PowerShell: {exc}")
    try:
        out, err = await asyncio.wait_for(proc.communicate(), timeout)
    except asyncio.TimeoutError:
        _kill_tree(proc.pid)
        await proc.wait()
        return _result(started, error=f"Timed out after {timeout:.0f}s")
    except asyncio.CancelledError:
        _kill_tree(proc.pid)
        raise
    return _result(started, exit_code=proc.returncode, stdout=_text(out), stderr=_text(err))
