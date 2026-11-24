"""SSH engine: one cached asyncssh connection per server, shared by commands and terminals.

Host keys are trust-on-first-use: the first fingerprint seen is stored; a later mismatch refuses
the connection *before* any password is sent.
"""

from __future__ import annotations

import asyncio
import logging
import os
import posixpath
import shlex
import tarfile
import tempfile
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

import asyncssh

from sshpro.db import Database
from sshpro.localexec import expand
from sshpro.safety import prepare, sudo_count

log = logging.getLogger(__name__)
logging.getLogger("asyncssh").setLevel(logging.WARNING)

MAX_OUTPUT = 200_000

PROBE = (
    '. /etc/os-release 2>/dev/null; echo "os=${PRETTY_NAME:-$(uname -s)}"; '
    'echo "kernel=$(uname -r)"; echo "uptime=$(cut -d. -f1 /proc/uptime 2>/dev/null)"; '
    'echo "load=$(cut -d" " -f1-3 /proc/loadavg 2>/dev/null)"; echo "cpus=$(nproc 2>/dev/null)"; '
    "free -m 2>/dev/null | awk '/^Mem:/{print \"mem=\"$2\" \"$7}'; "
    "df -Pk / 2>/dev/null | awk 'NR==2{print \"disk=\"$2\" \"$4}'"
)


CHUNK = 256 * 1024  # server-to-server relay: bytes read from the source per write to the destination
COPY_MARGIN = 64 * 1024 * 1024  # free space a copy must leave on the destination

HAS_TAR = "command -v tar >/dev/null 2>&1 && command -v gzip >/dev/null 2>&1"
# `sudo` is dropped for root and gets the stored sudo password otherwise (safety.prepare).
INSTALL_TAR = (
    "if command -v apt-get >/dev/null 2>&1; then"
    " sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tar gzip"
    " || { sudo apt-get update -qq && sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tar gzip; };"
    " elif command -v dnf >/dev/null 2>&1; then sudo dnf install -y -q tar gzip;"
    " elif command -v yum >/dev/null 2>&1; then sudo yum install -y -q tar gzip;"
    " elif command -v apk >/dev/null 2>&1; then sudo apk add -q tar gzip;"
    " elif command -v zypper >/dev/null 2>&1; then sudo zypper -n -q install tar gzip;"
    " else echo 'no known package manager' >&2; exit 3; fi"
)


class SSHError(Exception):
    """A connection problem, phrased for the user."""


class _Client(asyncssh.SSHClient):
    def __init__(self, expected: str | None):
        self.expected = expected
        self.seen: str | None = None

    def validate_host_public_key(self, host: str, addr: str, port: int, key: asyncssh.SSHKey) -> bool:
        self.seen = key.get_fingerprint("sha256")
        return self.expected is None or self.seen == self.expected


def _friendly(exc: BaseException, client: _Client | None = None) -> str:
    if client is not None and client.expected and client.seen and client.seen != client.expected:
        return ("HOST KEY CHANGED — the server's identity does not match the saved one "
                f"(saved {client.expected}, got {client.seen}). Not connecting. If the server was "
                "reinstalled, use 'Forget host key' in Edit.")
    if isinstance(exc, asyncssh.PermissionDenied):
        return "Authentication failed (wrong key / password or user)."
    if isinstance(exc, (asyncio.TimeoutError, TimeoutError)):
        return "Connection timed out."
    if isinstance(exc, ConnectionRefusedError):
        return "Connection refused (is SSH running on that port?)."
    if isinstance(exc, OSError) and getattr(exc, "winerror", None) in (11001, 11004):
        return "Host name not found."
    if isinstance(exc, asyncssh.KeyImportError):
        return f"Private key could not be read: {exc}"
    return f"{type(exc).__name__}: {exc}"


def parse_facts(text: str) -> dict[str, Any]:
    raw = dict(line.split("=", 1) for line in text.splitlines() if "=" in line)
    facts: dict[str, Any] = {"os": raw.get("os") or None, "kernel": raw.get("kernel") or None}
    try:
        facts["uptime"] = int(raw.get("uptime") or 0)
    except ValueError:
        facts["uptime"] = None
    load = (raw.get("load") or "").split()
    facts["load"] = [float(x) for x in load] if len(load) == 3 else None
    facts["cpus"] = int(raw["cpus"]) if (raw.get("cpus") or "").isdigit() else None
    mem = (raw.get("mem") or "").split()
    if len(mem) == 2 and all(m.isdigit() for m in mem):
        total, avail = map(int, mem)
        facts["mem"] = {"total_mb": total, "used_pct": round(100 * (total - avail) / total, 1) if total else 0}
    disk = (raw.get("disk") or "").split()
    if len(disk) == 2 and all(d.isdigit() for d in disk):
        total, avail = map(int, disk)
        facts["disk"] = {"total_gb": round(total / 1048576, 1), "free_gb": round(avail / 1048576, 1),
                         "used_pct": round(100 * (total - avail) / total, 1) if total else 0}
    return facts


class SSHManager:
    def __init__(self, db: Database, connect_timeout: float = 12):
        self.db = db
        self.connect_timeout = connect_timeout
        self._conns: dict[int, asyncssh.SSHClientConnection] = {}
        self._locks: dict[int, asyncio.Lock] = {}
        self.remote_tmp = "/tmp"  # where folder uploads land as an archive before unpacking
        self._has_tar: set[int] = set()  # servers known to have tar + gzip

    async def connection(self, server_id: int) -> asyncssh.SSHClientConnection:
        lock = self._locks.setdefault(server_id, asyncio.Lock())
        async with lock:
            conn = self._conns.get(server_id)
            if conn is not None and not conn.is_closed():
                return conn
            conn = await self._connect(server_id)
            self._conns[server_id] = conn
            return conn

    async def _connect(self, server_id: int) -> asyncssh.SSHClientConnection:
        server = self.db.get_server(server_id)
        if server is None:
            raise SSHError("Server not found.")
        secrets = self.db.get_secrets(server_id)
        client = _Client(server["host_key"])
        options: dict[str, Any] = dict(
            host=server["host"], port=server["port"], username=server["username"],
            known_hosts=([], [], []), client_factory=lambda: client,
            connect_timeout=self.connect_timeout, keepalive_interval=30, keepalive_count_max=3,
            agent_path=None, client_keys=[], config=None,
        )
        try:
            if secrets["private_key"]:
                options["client_keys"] = [
                    asyncssh.import_private_key(secrets["private_key"], secrets["key_passphrase"])
                ]
            if secrets["password"]:
                options["password"] = secrets["password"]
            conn = await asyncssh.connect(**options)
        except Exception as exc:
            raise SSHError(_friendly(exc, client)) from exc
        if server["host_key"] is None and client.seen:
            self.db.set_host_key(server_id, client.seen)
        log.info("Connected to %s (%s)", server["name"], server_id)
        return conn

    def drop(self, server_id: int) -> None:
        self._has_tar.discard(server_id)  # the server may have been edited to point elsewhere
        conn = self._conns.pop(server_id, None)
        if conn is not None:
            conn.close()

    async def close(self) -> None:
        for server_id in list(self._conns):
            self.drop(server_id)

    def sudo_password(self, server: dict, secrets: dict) -> str | None:
        if secrets["sudo_password"]:
            return secrets["sudo_password"]
        return secrets["password"] if server["auth_type"] == "password" else None

    async def run(self, server_id: int, command: str, timeout: float = 60, raw: bool = False) -> dict[str, Any]:
        """Run one command. Never raises for remote failures: they come back in the result."""
        server = self.db.get_server(server_id)
        if server is None:
            return {"exit": None, "stdout": "", "stderr": "", "error": "Server not found.", "duration": 0}
        if raw:
            line, stdin = command, None
        else:
            line, stdin = prepare(command, server["username"],
                                  self.sudo_password(server, self.db.get_secrets(server_id)))
        started = time.monotonic()
        for attempt in (1, 2):
            try:
                conn = await self.connection(server_id)
                result = await conn.run(line, input=stdin, check=False, timeout=timeout)
                break
            except SSHError as exc:
                return self._result(started, error=str(exc))
            except asyncssh.ProcessError as exc:  # timeout
                return self._result(started, exit_code=None, stdout=_text(exc.stdout),
                                    stderr=_text(exc.stderr), error=f"Timed out after {timeout:.0f}s")
            except (asyncssh.ConnectionLost, asyncssh.ChannelOpenError, asyncssh.DisconnectError,
                    BrokenPipeError, ConnectionResetError) as exc:
                self.drop(server_id)
                if attempt == 2:
                    return self._result(started, error=_friendly(exc))
        exit_code = result.exit_status if result.exit_status is not None else result.returncode
        return self._result(started, exit_code=exit_code, stdout=_text(result.stdout), stderr=_text(result.stderr))

    @staticmethod
    def _result(started: float, exit_code: int | None = None, stdout: str = "", stderr: str = "",
                error: str | None = None) -> dict[str, Any]:
        return {"exit": exit_code, "stdout": stdout[-MAX_OUTPUT:], "stderr": stderr[-MAX_OUTPUT:],
                "error": error, "duration": round(time.monotonic() - started, 2)}

    async def _ensure_tar(self, server_id: int) -> str | None:
        """Folder uploads need tar + gzip on the server; install them when missing.
        Returns a note for the result ("" when they were already there), None if unavailable."""
        if server_id in self._has_tar:
            return ""
        res = await self.run(server_id, HAS_TAR, timeout=30, raw=True)
        if res["error"]:
            return None
        note = ""
        if res["exit"] != 0:
            res = await self.run(server_id, f"{INSTALL_TAR}\n{HAS_TAR}", timeout=600)
            if res["error"] or res["exit"] != 0:
                log.warning("Could not install tar/gzip on server %s: %s %s", server_id,
                            res["error"] or "", res["stderr"][-300:])
                return None
            note = "installed missing tar/gzip on the server; "
        self._has_tar.add(server_id)
        return note

    async def upload(self, server_id: int, local: str, remote: str,
                     progress: Callable[[str, str, int, int], None] | None = None) -> dict[str, Any]:
        """Copy a local file or folder to the server, as the SSH user (like `scp -r`: a `remote`
        ending in "/" or naming an existing directory receives it inside).

        A folder (a build output can be thousands of small files) is packed into one .tar.gz here,
        sent in one transfer, unpacked into the destination with `tar`, and the archive is deleted
        on both sides.
        """
        started = time.monotonic()
        src = Path(expand(local))
        if not src.exists():
            return self._result(started, error=f"Not found on this PC: {src}")
        archive: Path | None = None
        try:
            tools = await self._ensure_tar(server_id) if src.is_dir() else ""
            if tools is not None and src.is_dir():
                fd, name = tempfile.mkstemp(prefix="sshpro-upload-", suffix=".tar.gz")
                os.close(fd)
                archive = Path(name)
                size, count = await asyncio.to_thread(_pack, src, archive)
                note = f"{tools}{count} files, {human_size(size)}, sent as one {human_size(archive.stat().st_size)} archive"
            elif src.is_dir():
                note = "tar/gzip missing on the server and could not be installed: sent file by file (slower)"
            else:
                note = human_size(src.stat().st_size)
            for attempt in (1, 2):
                try:
                    conn = await self.connection(server_id)
                    async with conn.start_sftp_client() as sftp:
                        target = remote
                        if remote.endswith("/"):
                            await sftp.makedirs(remote, exist_ok=True)
                        if remote.endswith("/") or await sftp.isdir(remote):
                            target = posixpath.join(remote, src.name)
                        sent = posixpath.join(self.remote_tmp, archive.name) if archive else target
                        await sftp.put(str(archive or src), sent, recurse=src.is_dir() and not archive,
                                       progress_handler=progress)
                    break
                except SSHError as exc:
                    return self._result(started, error=str(exc))
                except (asyncssh.ConnectionLost, asyncssh.ChannelOpenError, asyncssh.DisconnectError,
                        BrokenPipeError, ConnectionResetError) as exc:
                    self.drop(server_id)
                    if attempt == 2:
                        return self._result(started, error=_friendly(exc))
                except (asyncssh.SFTPError, OSError) as exc:
                    return self._result(started, exit_code=1, stderr=f"Upload failed: {exc}")
            if archive:
                t, a = shlex.quote(target), shlex.quote(sent)
                res = await self.run(server_id, f"mkdir -p {t} && tar -xzf {a} -C {t}; rc=$?; rm -f {a}; exit $rc",
                                     timeout=1800, raw=True)
                if res["error"] or res["exit"] != 0:
                    return self._result(started, exit_code=res["exit"], stdout=res["stdout"], error=res["error"],
                                        stderr=f"Uploaded, but unpacking into {target} failed: {res['stderr']}")
        finally:
            if archive:
                archive.unlink(missing_ok=True)
        return self._result(started, exit_code=0, stdout=f"uploaded {src} -> {target} ({note})")

    async def download(self, server_id: int, remote: str, local: str,
                       progress: Callable[[str, str, int, int], None] | None = None) -> dict[str, Any]:
        """Copy a server file or folder to this PC, as the SSH user (like `scp -r`: a `local` ending
        in a slash or naming an existing folder receives it inside). Never overwrites: an existing
        target gets a " (2)" style name instead.

        A folder is packed into one .tar.gz on the server, sent in one transfer, unpacked here, and
        the archive is deleted on both sides.
        """
        started = time.monotonic()
        dest = Path(expand(local))
        archive: Path | None = None
        packed: str | None = None  # archive path on the server
        try:
            for attempt in (1, 2):
                try:
                    conn = await self.connection(server_id)
                    async with conn.start_sftp_client() as sftp:
                        try:
                            is_dir = await sftp.isdir(remote)
                            if not is_dir and not await sftp.isfile(remote):
                                return self._result(started, exit_code=1, stderr=f"Not found on the server: {remote}")
                        except asyncssh.SFTPError as exc:
                            return self._result(started, exit_code=1, stderr=f"Cannot read {remote}: {exc}")
                    break
                except SSHError as exc:
                    return self._result(started, error=str(exc))
                except (asyncssh.ConnectionLost, asyncssh.ChannelOpenError, asyncssh.DisconnectError,
                        BrokenPipeError, ConnectionResetError) as exc:
                    self.drop(server_id)
                    if attempt == 2:
                        return self._result(started, error=_friendly(exc))
            name = posixpath.basename(remote.rstrip("/")) or "download"
            if local.rstrip().endswith(("/", "\\")) or dest.is_dir():
                dest = dest / name
            dest = _free_path(dest)
            dest.parent.mkdir(parents=True, exist_ok=True)

            tools = await self._ensure_tar(server_id) if is_dir else ""
            if is_dir and tools is not None:
                packed = posixpath.join(self.remote_tmp, f"sshpro-download-{int(time.time() * 1000)}.tar.gz")
                res = await self.run(server_id, f"tar -czf {shlex.quote(packed)} -C {shlex.quote(remote)} .",
                                     timeout=1800, raw=True)
                if res["error"] or res["exit"] != 0:
                    return self._result(started, exit_code=res["exit"], stdout=res["stdout"], error=res["error"],
                                        stderr=f"Packing {remote} on the server failed: {res['stderr']}")
                fd, tmp = tempfile.mkstemp(prefix="sshpro-download-", suffix=".tar.gz")
                os.close(fd)
                archive = Path(tmp)
            try:
                conn = await self.connection(server_id)
                async with conn.start_sftp_client() as sftp:
                    if packed:
                        await sftp.get(packed, str(archive), progress_handler=progress)
                    else:
                        await sftp.get(remote, str(dest), recurse=is_dir, progress_handler=progress)
            except SSHError as exc:
                return self._result(started, error=str(exc))
            except (asyncssh.SFTPError, OSError, asyncssh.Error) as exc:
                return self._result(started, exit_code=1, stderr=f"Download failed: {exc}")
            if archive:
                sent = archive.stat().st_size
                size, count = await asyncio.to_thread(_unpack, archive, dest)
                note = f"{tools}{count} files, {human_size(size)}, sent as one {human_size(sent)} archive"
            elif is_dir:
                note = "tar/gzip missing on the server and could not be installed: sent file by file (slower)"
            else:
                note = human_size(dest.stat().st_size)
        finally:
            if packed:
                await self.run(server_id, f"rm -f {shlex.quote(packed)}", timeout=30, raw=True)
            if archive:
                archive.unlink(missing_ok=True)
        return self._result(started, exit_code=0, stdout=f"downloaded {remote} -> {dest} ({note})")

    # ---- server to server --------------------------------------------------------------------

    async def _receiver(self, server_id: int, command: str) -> tuple[str, bytes]:
        """Command line + stdin prefix for a process that reads streamed data on stdin.

        A sudo password goes in front of the data (sudo -S reads it byte by byte, so what follows
        reaches the command intact). When sudo needs no password (NOPASSWD) it must get none, or
        that line would end up inside the data, so that is checked first.
        """
        server = self.db.get_server(server_id)
        if server is None:
            raise SSHError("Server not found.")
        password = None
        if sudo_count(command) and server["username"] != "root":
            check = await self.run(server_id, "sudo -n true", timeout=20, raw=True)
            if check["error"]:
                raise SSHError(check["error"])
            if check["exit"] != 0:
                if sudo_count(command) > 1:
                    raise SSHError("The receiving command may use sudo only once; wrap it: sudo bash -c '…'")
                password = self.sudo_password(server, self.db.get_secrets(server_id))
        line, stdin = prepare("set -o pipefail; " + command, server["username"], password)
        return line, (stdin or "").encode()

    async def pipe(self, src_id: int, src_command: str, dst_id: int, dst_command: str,
                   timeout: float = 6 * 3600,
                   progress: Callable[[int, float], None] | None = None) -> dict[str, Any]:
        """Stream the output of `src_command` on one server into `dst_command` on another, e.g.
        `pg_dump -Fc db` into `pg_restore -d db`, or `tar -cf - dir` into `tar -xf -`.

        The data is relayed through this app's own SSH connections (nothing is stored on this PC),
        so the servers need no network path or SSH keys to each other. Both sides run with
        `pipefail`; the result fails if either side fails.
        """
        started = time.monotonic()
        src_server, dst_server = self.db.get_server(src_id), self.db.get_server(dst_id)
        if src_server is None or dst_server is None:
            return self._result(started, error="Server not found.")
        procs: list[asyncssh.SSHClientProcess] = []
        try:
            dst_line, dst_prefix = await self._receiver(dst_id, dst_command)
            src_line, src_stdin = prepare("set -o pipefail; " + src_command, src_server["username"],
                                          self.sudo_password(src_server, self.db.get_secrets(src_id)))
            dst = await (await self.connection(dst_id)).create_process(dst_line, encoding=None)
            procs.append(dst)
            src = await (await self.connection(src_id)).create_process(src_line, encoding=None)
            procs.append(src)
        except SSHError as exc:
            for proc in procs:
                proc.close()
            return self._result(started, error=str(exc))
        except (asyncssh.Error, OSError) as exc:
            for proc in procs:
                proc.close()
            return self._result(started, error=_friendly(exc))

        sent = 0

        async def collect(stream: Any) -> bytes:
            data = bytearray()
            while chunk := await stream.read(65536):
                data += chunk
                del data[:-MAX_OUTPUT]
            return bytes(data)

        async def pump() -> None:
            nonlocal sent
            if src_stdin:
                src.stdin.write(src_stdin.encode())
            src.stdin.write_eof()
            if dst_prefix:
                dst.stdin.write(dst_prefix)
            while chunk := await src.stdout.read(CHUNK):
                dst.stdin.write(chunk)
                await dst.stdin.drain()
                sent += len(chunk)
                if progress:
                    progress(sent, time.monotonic() - started)
            dst.stdin.write_eof()

        readers = [asyncio.create_task(collect(src.stderr)), asyncio.create_task(collect(dst.stdout)),
                   asyncio.create_task(collect(dst.stderr))]
        error = None
        try:
            await asyncio.wait_for(pump(), timeout)
            await asyncio.wait_for(asyncio.gather(src.wait_closed(), dst.wait_closed()), 900)
        except asyncio.TimeoutError:
            error = f"Timed out after {timeout:.0f}s"
        except (BrokenPipeError, ConnectionResetError, asyncssh.Error, OSError) as exc:
            error = f"The transfer broke off: {_friendly(exc) if isinstance(exc, asyncssh.Error) else exc}"
        finally:
            for proc in procs:
                if proc.exit_status is None:
                    proc.close()
            await asyncio.gather(*(p.wait_closed() for p in procs), return_exceptions=True)
        src_err, dst_out, dst_err = [_text(r) if not isinstance(r, BaseException) else ""
                                     for r in await asyncio.gather(*readers, return_exceptions=True)]
        if error and not dst_err and dst.exit_status not in (None, 0):
            error = None  # the receiver failed: its own exit code and stderr say why
        src_rc, dst_rc = src.exit_status, dst.exit_status
        took = time.monotonic() - started
        summary = (f"streamed {human_size(sent)} from {src_server['name']} to {dst_server['name']} in {took:.0f}s"
                   f" ({human_size(sent / took if took else 0)}/s)")
        stderr = "\n".join(x for x in (src_err and f"[{src_server['name']}] {src_err.strip()}",
                                      dst_err and f"[{dst_server['name']}] {dst_err.strip()}") if x)
        if src_rc not in (0, None) or dst_rc not in (0, None):
            exit_code = src_rc if src_rc not in (0, None) else dst_rc
            side = src_server["name"] if src_rc not in (0, None) else dst_server["name"]
            stderr = f"failed on {side} (exit {exit_code})\n{stderr}".strip()
        else:
            exit_code = None if error else 0
        return self._result(started, exit_code=exit_code, stdout=f"{summary}\n{dst_out}".strip(),
                            stderr=stderr, error=error)

    async def copy(self, src_id: int, src_path: str, dst_id: int, into: str, verify: str = "checksum",
                   sudo: bool = False, overwrite: bool = False, timeout: float = 6 * 3600,
                   progress: Callable[[int, float], None] | None = None) -> dict[str, Any]:
        """Copy a file or folder from one server into a folder on another (`into` receives it,
        like `cp -r`), keeping permissions, times and owners (owners need `sudo`).

        Built to lose nothing: the source is only read; the destination's free space is checked
        first; an existing target is never overwritten unless `overwrite`; afterwards both sides
        are compared (file count, bytes and, with "checksum", a SHA-256 over every file) and any
        difference fails the step.
        """
        started = time.monotonic()
        q = shlex.quote
        src_path = src_path.rstrip("/")
        parent, name = posixpath.split(src_path)
        if not src_path.startswith("/") or not into.startswith("/") or not name or name in (".", ".."):
            return self._result(started, error="copy needs absolute paths, and the source must be a file or folder (not /)")
        parent = parent or "/"
        into = into.rstrip("/") or "/"
        target = posixpath.join(into, name)
        sudo_ = "sudo " if sudo else ""

        def as_root(script: str) -> str:
            return f"sudo bash -c {q('set -o pipefail; ' + script)}" if sudo else script

        for sid in (src_id, dst_id):
            if await self._ensure_tar(sid) is None:
                return self._result(started, error=f"tar/gzip are missing on server {sid} and could not be installed")

        # 1. before: the source exists; the destination has room and no target in the way
        src_check, dst_check = await asyncio.gather(
            self.run(src_id, f"{sudo_}test -e {q(src_path)} || {{ echo NOT_FOUND; exit 3; }}; "
                             f"{sudo_}du -sk {q(src_path)} | cut -f1", timeout=1800),
            self.run(dst_id, f'p={q(into)}; while [ ! -d "$p" ]; do p=$(dirname "$p"); done; '
                             f'df -Pk "$p" | awk \'NR==2{{print $4}}\'; '
                             f'if {sudo_}test -e {q(target)}; then echo EXISTS; fi', timeout=120))
        for res, where in ((src_check, "source"), (dst_check, "destination")):
            if res["error"]:
                return self._result(started, error=f"{where}: {res['error']}")
        if "NOT_FOUND" in src_check["stdout"]:
            return self._result(started, exit_code=3, stderr=f"Not found on the source server: {src_path}")
        if src_check["exit"] != 0:
            return self._result(started, exit_code=src_check["exit"], stderr=f"Could not read the source: {src_check['stderr']}")
        try:
            size = int(src_check["stdout"].split()[-1]) * 1024
            free = int(dst_check["stdout"].split()[0]) * 1024
        except (ValueError, IndexError):
            return self._result(started, exit_code=1, stderr="Could not measure the source size or the free space on the destination")
        if "EXISTS" in dst_check["stdout"] and not overwrite:
            return self._result(started, exit_code=1, stderr=(
                f"{target} already exists on the destination and was left untouched. Move it aside first "
                f"(e.g. mv {target} {target}.old-$(date +%F)) or set overwrite: true to merge into it."))
        if size + COPY_MARGIN > free:
            return self._result(started, exit_code=1, stderr=(
                f"Not enough space on the destination: {human_size(size)} needed, {human_size(free)} free in {into}"))

        # 2. stream it: tar | gzip on the source, gunzip | tar on the destination
        res = await self.pipe(
            src_id, as_root(f"tar -C {q(parent)} -cf - {q(name)} | gzip -1"),
            dst_id, as_root(f"mkdir -p {q(into)} && gzip -dc | tar -C {q(into)} -xpf -"),
            timeout=timeout, progress=progress)
        if res["error"] or res["exit"] != 0:
            res["stderr"] = f"Copy failed — the source is unchanged.\n{res['stderr']}".strip()
            return res
        note = res["stdout"].splitlines()[0]

        # 3. after: compare both sides
        if verify == "none":
            return self._result(started, exit_code=0, stdout=f"copied {src_path} -> {target} ({note}); not verified")
        digest = ("find {n} -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum | sha256sum | cut -d' ' -f1"
                  if verify == "checksum" else "echo -")
        script = (f"cd {{dir}} && {{{{ {digest}; find {{n}} -type f | wc -l; "
                  "find {n} -type f -exec stat -c %s {{}} + | awk '{{s+=$1}} END {{print s+0}}'; }}")
        a, b = await asyncio.gather(
            self.run(src_id, as_root(script.format(dir=q(parent), n=q(name))), timeout=timeout),
            self.run(dst_id, as_root(script.format(dir=q(into), n=q(name))), timeout=timeout))
        if a["error"] or b["error"] or a["exit"] != 0 or b["exit"] != 0:
            return self._result(started, exit_code=1, stdout=f"copied {src_path} -> {target} ({note})",
                                stderr=f"Copied, but the check could not run: {a['error'] or a['stderr']} {b['error'] or b['stderr']}")
        left, right = a["stdout"].split(), b["stdout"].split()
        if len(left) != 3 or left != right:
            return self._result(started, exit_code=1, stdout=f"copied {src_path} -> {target} ({note})",
                                stderr=(f"VERIFY FAILED: the copy differs from the source (source: {' '.join(left)}; "
                                        f"destination: {' '.join(right)}). Nothing was deleted. If the source was "
                                        "being written to, stop that service and copy again."))
        files, total = int(left[1]), int(left[2])
        check = "SHA-256 of every file matches" if verify == "checksum" else "file count and sizes match"
        return self._result(started, exit_code=0, stdout=(
            f"copied {src_path} -> {target}: {files} files, {human_size(total)} ({note}); verified: {check}"))

    async def probe(self, server_id: int) -> dict[str, Any]:
        result = await self.run(server_id, PROBE, timeout=20, raw=True)
        if result["error"]:
            self.db.set_status(server_id, "offline")
            return {"status": "offline", "error": result["error"]}
        facts = parse_facts(result["stdout"])
        self.db.set_status(server_id, "online", facts)
        return {"status": "online", "facts": facts}

    async def open_shell(self, server_id: int, cols: int, rows: int) -> asyncssh.SSHClientProcess:
        try:
            conn = await self.connection(server_id)
            return await conn.create_process(term_type="xterm-256color", term_size=(cols, rows), encoding=None)
        except SSHError:
            raise
        except (asyncssh.ConnectionLost, asyncssh.ChannelOpenError, asyncssh.DisconnectError):
            self.drop(server_id)
            conn = await self.connection(server_id)
            return await conn.create_process(term_type="xterm-256color", term_size=(cols, rows), encoding=None)


def _pack(src: Path, archive: Path) -> tuple[int, int]:
    """Pack the folder's contents into a .tar.gz with Linux-friendly owners and modes (Windows has
    none: files 644, directories and *.sh 755). Returns (bytes, files) packed."""
    size = count = 0

    def normalise(info: tarfile.TarInfo) -> tarfile.TarInfo:
        nonlocal size, count
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        info.mode = 0o755 if info.isdir() or info.name.endswith(".sh") else 0o644
        if info.isfile():
            size += info.size
            count += 1
        return info

    with tarfile.open(archive, "w:gz", compresslevel=6) as tar:
        for child in sorted(src.iterdir()):
            tar.add(child, arcname=child.name, filter=normalise)
    return size, count


def _unpack(archive: Path, dest: Path) -> tuple[int, int]:
    """Unpack a downloaded .tar.gz into dest. Entries the "data" filter refuses (paths or links
    pointing outside dest, device files) are skipped. Returns (bytes, files) unpacked."""
    size = count = 0

    def safe(info: tarfile.TarInfo, path: str) -> tarfile.TarInfo | None:
        nonlocal size, count
        try:
            info = tarfile.data_filter(info, path)
        except tarfile.FilterError:
            return None
        if info.isfile():
            size += info.size
            count += 1
        return info

    dest.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as tar:
        tar.extractall(dest, filter=safe)
    return size, count


def _free_path(path: Path) -> Path:
    """path, or "name (2).sql.gz", "name (3).sql.gz"… if it already exists (downloads never overwrite)."""
    if not path.exists():
        return path
    dot = -1 if path.is_dir() else path.name.find(".", 1)  # whole extension: db.sql.gz -> "db (2).sql.gz"
    stem, suffix = (path.name[:dot], path.name[dot:]) if dot > 0 else (path.name, "")
    n = 2
    while (candidate := path.with_name(f"{stem} ({n}){suffix}")).exists():
        n += 1
    return candidate


def human_size(n: float) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if n < 1024 or unit == "GB":
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024
    return ""


def _text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    return str(value)
