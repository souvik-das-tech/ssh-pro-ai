"""Server-to-server copy and pipe, end to end: a local SSH server that runs commands in a real bash
(Git Bash on Windows), two "servers" on it, and real tar / gzip / sha256sum doing the work."""

import asyncio
import hashlib
import json
import os
import shutil
from pathlib import Path

import asyncssh
import pytest

from sshpro.brain import Brain
from sshpro.db import Database
from sshpro.sshpool import SSHManager


def _find_bash() -> str | None:
    if os.name == "nt":  # Git Bash, not WSL's bash.exe
        for path in (r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"):
            if os.path.exists(path):
                return path
        return None
    return shutil.which("bash")


BASH = _find_bash()
pytestmark = pytest.mark.skipif(BASH is None, reason="needs bash (Git Bash on Windows)")


def posix(path: Path) -> str:
    text = str(path).replace("\\", "/")
    if os.name == "nt" and len(text) > 1 and text[1] == ":":
        text = f"/{text[0].lower()}{text[2:]}"
    return text


class _Server(asyncssh.SSHServer):
    def begin_auth(self, username):
        return True

    def password_auth_supported(self):
        return True

    def validate_password(self, username, password):
        return password == "pw"


# A stand-in for sudo (Git Bash has none) that behaves like the real one: `-n` fails (a password is
# required), `-S` reads the password line from stdin, one byte at a time, then runs the command with
# the rest of stdin.
SUDO = {"shim": None}
_SHIM = """
[ "$1" = "-n" ] && exit 1
if [ "$1" = "-S" ]; then
  shift; [ "$1" = "-p" ] && shift 2
  IFS= read -r pw; [ "$pw" = "pw" ] || { echo "sudo: wrong password" >&2; exit 1; }
fi
exec "$@"
"""


async def _handle(process):
    """Run the command in bash, wiring the SSH channel's stdin / stdout / stderr to it."""
    command = process.command or "true"
    if SUDO["shim"]:
        command = f'sudo() {{ bash {SUDO["shim"]} "$@"; }}; export -f sudo; {command}'
    proc = await asyncio.create_subprocess_exec(BASH, "-c", command, stdin=asyncio.subprocess.PIPE,
                                                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)

    async def feed():
        try:
            while data := await process.stdin.read(65536):
                proc.stdin.write(data)
                await proc.stdin.drain()
        except Exception:
            pass
        finally:
            try:
                proc.stdin.close()
            except Exception:
                pass

    async def out(src, dst):
        while chunk := await src.read(65536):
            try:
                dst.write(chunk)
            except Exception:  # the client closed the channel: keep draining so bash can finish
                pass

    feeder = asyncio.create_task(feed())
    try:
        await asyncio.gather(out(proc.stdout, process.stdout), out(proc.stderr, process.stderr))
        rc = await proc.wait()
    finally:
        feeder.cancel()
        if proc.returncode is None:
            proc.kill()
            await proc.wait()
    try:
        process.exit(rc)
    except Exception:
        pass


async def _setup(tmp_path):
    key = asyncssh.generate_private_key("ssh-ed25519")
    server = await asyncssh.create_server(_Server, "127.0.0.1", 0, server_host_keys=[key],
                                         process_factory=_handle, encoding=None)
    port = server.sockets[0].getsockname()[1]
    db = Database(tmp_path / "t.sqlite3")
    ids = [db.create_server({"name": name, "host": "127.0.0.1", "port": port, "username": "deploy",
                             "auth_type": "password", "password": "pw"}) for name in ("old", "new")]
    return server, db, ids


async def _close(server) -> None:
    server.close()
    await server.wait_closed()
    await asyncio.sleep(0.3)  # let the bash processes of the test server finish


def _tree(root: Path) -> dict[str, str]:
    return {str(p.relative_to(root)).replace("\\", "/"): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*")) if p.is_file()}


def _make_data(root: Path) -> None:
    (root / "workspaces" / "topp" / "states").mkdir(parents=True)
    (root / "workspaces" / "topp" / "states" / "layer.xml").write_text("<layer>states</layer>", encoding="utf-8")
    (root / "global.xml").write_text("<global/>", encoding="utf-8")
    (root / "data" ).mkdir()
    (root / "data" / "raster.tif").write_bytes(os.urandom(3 * 1024 * 1024))  # binary, bigger than one chunk
    (root / "with space.txt").write_text("ok", encoding="utf-8")


def test_copy_folder_between_servers_verified(tmp_path):
    async def main():
        server, db, (old, new) = await _setup(tmp_path)
        src = tmp_path / "srv-old" / "geoserver_data"
        _make_data(src)
        (tmp_path / "srv-new").mkdir()
        ssh = SSHManager(db)
        seen = []
        res = await ssh.copy(old, posix(src), new, posix(tmp_path / "srv-new" / "geo") + "/",
                             progress=lambda sent, t: seen.append(sent))
        await ssh.close()
        await _close(server)
        return res, src, seen

    res, src, seen = asyncio.run(main())
    assert res["exit"] == 0, res
    assert "SHA-256 of every file matches" in res["stdout"] and "4 files" in res["stdout"]
    assert _tree(tmp_path / "srv-new" / "geo" / "geoserver_data") == _tree(src)
    assert seen and seen[-1] > 0


def test_copy_never_overwrites_and_checks_the_source(tmp_path):
    async def main():
        server, db, (old, new) = await _setup(tmp_path)
        src = tmp_path / "a" / "data"
        src.mkdir(parents=True)
        (src / "x.txt").write_text("new", encoding="utf-8")
        existing = tmp_path / "b" / "data"
        existing.mkdir(parents=True)
        (existing / "x.txt").write_text("keep me", encoding="utf-8")
        ssh = SSHManager(db)
        blocked = await ssh.copy(old, posix(src), new, posix(tmp_path / "b"))
        missing = await ssh.copy(old, posix(tmp_path / "nope"), new, posix(tmp_path / "b"))
        merged = await ssh.copy(old, posix(src), new, posix(tmp_path / "b"), overwrite=True, verify="size")
        await ssh.close()
        await _close(server)
        return blocked, missing, merged, existing

    blocked, missing, merged, existing = asyncio.run(main())
    assert blocked["exit"] == 1 and "already exists" in blocked["stderr"]
    assert missing["exit"] == 3 and "Not found" in missing["stderr"]
    assert merged["exit"] == 0 and "file count and sizes match" in merged["stdout"]
    assert (existing / "x.txt").read_text(encoding="utf-8") == "new"


def test_pipe_streams_a_dump_into_a_restore(tmp_path):
    async def main():
        server, db, (old, new) = await _setup(tmp_path)
        dump = tmp_path / "dump.bin"
        dump.write_bytes(os.urandom(2 * 1024 * 1024 + 7))
        out = tmp_path / "restored.bin"
        ssh = SSHManager(db)
        ok = await ssh.pipe(old, f"cat {posix(dump)} | gzip -1", new, f"gzip -dc > {posix(out)}")
        bad = await ssh.pipe(old, f"cat {posix(dump)}", new, "head -c 10 >/dev/null; echo 'restore error' >&2; exit 4")
        broken_source = await ssh.pipe(old, "echo partial; exit 2", new, "cat >/dev/null")
        await ssh.close()
        await _close(server)
        return ok, bad, broken_source, dump, out

    ok, bad, broken_source, dump, out = asyncio.run(main())
    assert ok["exit"] == 0, ok
    assert out.read_bytes() == dump.read_bytes()
    assert "streamed" in ok["stdout"] and "from old to new" in ok["stdout"]
    assert bad["exit"] == 4 and "restore error" in bad["stderr"] and "failed on new" in bad["stderr"]
    assert broken_source["exit"] == 2 and "failed on old" in broken_source["stderr"]


def test_sudo_password_goes_before_the_streamed_data(tmp_path):
    shim = tmp_path / "sudo-shim.sh"
    shim.write_bytes(_SHIM.encode())  # LF line endings: bash on Windows chokes on CRLF
    SUDO["shim"] = posix(shim)

    async def main():
        server, db, (old, new) = await _setup(tmp_path)
        dump = tmp_path / "dump.bin"
        dump.write_bytes(os.urandom(1024 * 1024))
        out = tmp_path / "restored.bin"
        src = tmp_path / "root-owned" / "data"
        src.mkdir(parents=True)
        (src / "a.bin").write_bytes(os.urandom(300_000))
        (tmp_path / "dest").mkdir()
        ssh = SSHManager(db)
        piped = await ssh.pipe(old, f"sudo cat {posix(dump)}", new, f"sudo cat > {posix(out)}")
        twice = await ssh.pipe(old, "echo x", new, "sudo cat | sudo cat > /dev/null")
        copied = await ssh.copy(old, posix(src), new, posix(tmp_path / "dest"), sudo=True)
        await ssh.close()
        await _close(server)
        return piped, twice, copied, dump, out, src

    try:
        piped, twice, copied, dump, out, src = asyncio.run(main())
    finally:
        SUDO["shim"] = None
    assert piped["exit"] == 0, piped
    assert out.read_bytes() == dump.read_bytes()  # the password line did not end up in the data
    assert "sudo only once" in twice["error"]
    assert copied["exit"] == 0 and "SHA-256 of every file matches" in copied["stdout"], copied
    assert _tree(tmp_path / "dest" / "data") == _tree(src)


class FakeAI:
    def __init__(self, replies):
        self.replies = list(replies)
        self.messages = []

    async def start(self, instructions):
        pass

    async def ask(self, message):
        self.messages.append(message)
        return json.dumps(self.replies.pop(0))

    async def reset(self):
        pass

    async def close(self):
        pass


def test_job_runs_steps_in_order_and_stops_after_a_failed_copy(tmp_path):
    async def main():
        server, db, (old, new) = await _setup(tmp_path)
        src = tmp_path / "s" / "data"
        src.mkdir(parents=True)
        (src / "f.txt").write_text("1", encoding="utf-8")
        (tmp_path / "d" / "data").mkdir(parents=True)  # target exists: the copy must refuse
        marker = tmp_path / "after.txt"
        plan = {"say": "moving", "action": "run", "risk": "change", "steps": [
            {"servers": [old], "command": f"echo stop-service > {posix(tmp_path / 'before.txt')}"},
            {"copy": {"from_server": old, "from": posix(src), "to_server": new, "into": posix(tmp_path / "d")}},
            {"servers": [new], "command": f"echo start > {posix(marker)}"},
        ]}
        done = {"action": "answer", "answer": {"summary": "stopped"}}
        events = []
        brain = None

        async def emit(ev):
            events.append(ev)
            if ev["type"] == "ai.plan" and ev["needs_approval"]:
                asyncio.get_running_loop().call_soon(brain.approve, True, "parallel")

        ssh = SSHManager(db)
        ai = FakeAI([plan, done])
        brain = Brain(db, ssh, emit, provider=ai)
        brain.ask("move data", None)
        await brain._job
        await ssh.close()
        await _close(server)
        return events, ai, marker

    events, ai, marker = asyncio.run(main())
    plan = next(e for e in events if e["type"] == "ai.plan")
    assert plan["needs_approval"] and plan["tasks"][-1]["name"] == "old → new"
    results = [e for e in events if e["type"] == "ai.result"]
    assert [r["server_id"] for r in results] == [results[0]["server_id"], "x1"]
    assert "already exists" in results[1]["output"]
    assert not marker.exists()  # the step after the failed copy never ran
    assert "skipped: an earlier step in this round failed" in ai.messages[1]


def test_phases_keep_order_and_drop_invalid_cross_steps():
    by_id = {1: {"name": "old"}, 2: {"name": "new"}}
    local, phases = Brain._phases([
        {"servers": [1], "command": "systemctl stop geoserver"},
        {"copy": {"from_server": "old", "from": "/var/lib/geoserver/data", "to_server": 2, "into": "/var/lib/geoserver/", "sudo": True}},
        {"copy": {"from_server": 1, "from": "/x", "to_server": 1, "into": "/y"}},  # same server: dropped
        {"pipe": {"from_server": 1, "command": "pg_dump -Fc gis", "to_server": 9, "into_command": "x"}},  # 9 unknown
        {"pipe": {"from_server": 1, "command": "sudo -u postgres pg_dump -Fc gis", "to_server": 2,
                  "into_command": "sudo -u postgres pg_restore -d gis"}},
        {"servers": ["all"], "command": "systemctl start geoserver"},
    ], by_id)
    assert not local
    assert [k for k, _ in phases] == ["servers", "cross", "cross", "servers"]
    copy, pipe = phases[1][1], phases[2][1]
    assert copy["kind"] == "copy" and copy["src"] == 1 and copy["dst"] == 2 and copy["sudo"] and copy["verify"] == "checksum"
    assert pipe["kind"] == "pipe" and "==>" in pipe["label"]
    assert not Brain._reads_only(copy) and not Brain._reads_only(pipe)
    assert set(phases[3][1]) == {1, 2}
