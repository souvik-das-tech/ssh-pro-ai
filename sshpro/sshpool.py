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
from sshpro.safety import prepare

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
