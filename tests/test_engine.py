"""SSH engine + AI job loop against a local fake SSH server and a scripted fake AGY."""

import asyncio
import json
import os
import shlex
import tarfile
import tempfile
from pathlib import Path

import asyncssh
import pytest

from sshpro.brain import Brain, extract_json
from sshpro.db import Database
from sshpro.sshpool import SSHManager, parse_facts


class _Server(asyncssh.SSHServer):
    def begin_auth(self, username):
        return True

    def password_auth_supported(self):
        return True

    def validate_password(self, username, password):
        return password == "pw"


FAKE = {"tar": True, "installable": True, "installs": 0}


@pytest.fixture(autouse=True)
def _reset_fake():
    FAKE.update(tar=True, installable=True, installs=0)


async def _handle(process):
    cmd = process.command or ""
    stdin = ""
    if "sudo -S" in cmd:
        stdin = await process.stdin.readline()
    if "install -y -qq tar gzip" in cmd:  # installing tar/gzip works only when the fake allows it
        FAKE["tar"] = FAKE["installable"]
        FAKE["installs"] += 1
        process.exit(0 if FAKE["tar"] else 1)
    elif "command -v tar" in cmd:
        process.exit(0 if FAKE["tar"] else 1)
    elif " tar -xzf " in cmd:  # folder upload: unpack the way the server's tar would
        words = [w.rstrip(";") for w in shlex.split(cmd)]
        archive, target = words[words.index("-xzf") + 1], words[words.index("-C") + 1]
        with tarfile.open(archive) as tar:
            tar.extractall(target, filter="data")
        os.remove(archive)
        process.exit(0)
    elif cmd.startswith("tar -czf "):  # folder download: pack the way the server's tar would
        words = shlex.split(cmd)
        with tarfile.open(words[2], "w:gz") as tar:
            tar.add(words[4], arcname=".")
        process.exit(0)
    elif cmd.startswith("rm -f "):
        Path(shlex.split(cmd)[2]).unlink(missing_ok=True)
        process.exit(0)
    elif "FAIL" in cmd:
        process.stdout.write("boom\n")
        process.exit(1)
    else:
        process.stdout.write(f"ran:{cmd}|stdin:{stdin.strip()}\n")
        process.exit(0)


async def _fake_server():
    key = asyncssh.generate_private_key("ssh-ed25519")
    server = await asyncssh.create_server(_Server, "127.0.0.1", 0, server_host_keys=[key], process_factory=_handle,
                                         sftp_factory=True)
    return server, server.sockets[0].getsockname()[1]


def _db(tmp_path, port, n=2, user="deploy"):
    db = Database(tmp_path / "t.sqlite3")
    ids = [db.create_server({"name": f"s{i}", "host": "127.0.0.1", "port": port, "username": user,
                             "auth_type": "password", "password": "pw"}) for i in range(n)]
    return db, ids


def test_run_trust_on_first_use_and_host_key_change(tmp_path):
    async def main():
        server, port = await _fake_server()
        db, (sid, _) = _db(tmp_path, port)
        ssh = SSHManager(db)
        res = await ssh.run(sid, "sudo whoami")
        assert res["exit"] == 0 and res["error"] is None
        assert "bash -lc" in res["stdout"] and "stdin:pw" in res["stdout"]  # login password reused for sudo
        assert db.get_server(sid)["host_key"].startswith("SHA256:")

        await ssh.close()
        db.set_host_key(sid, "SHA256:something-else")
        res = await ssh.run(sid, "uptime")
        assert res["exit"] is None and "HOST KEY CHANGED" in res["error"]

        db.update_server(sid, {"password": "wrong", "host": "127.0.0.1"})
        ssh.drop(sid)
        res = await ssh.run(sid, "uptime")
        assert "Authentication failed" in res["error"]
        await ssh.close()
        server.close()

    asyncio.run(main())


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


def _run_job(tmp_path, replies, decide):
    async def main():
        server, port = await _fake_server()
        db, ids = _db(tmp_path, port, n=3)
        ssh = SSHManager(db)
        events = []
        brain = None

        async def emit(ev):
            events.append(ev)
            if ev["type"] == "ai.plan" and ev["needs_approval"]:
                asyncio.get_running_loop().call_soon(brain.approve, *decide)

        ai = FakeAI(replies)
        brain = Brain(db, ssh, emit, provider=ai)
        brain.ask("update node", None)
        await brain._job
        await ssh.close()
        server.close()
        return events, ai, db

    return asyncio.run(main())


READ = {"say": "checking", "action": "run", "risk": "read", "steps": [{"servers": ["all"], "command": "node -v"}]}
CHANGE = {"say": "updating", "action": "run", "risk": "read", "rollout": "canary",
          "steps": [{"servers": ["all"], "command": "sudo apt-get install -y nodejs"}]}
DONE = {"action": "answer", "answer": {"summary": "All updated", "table": {"columns": ["Server"], "rows": [["s0"]]}}}


def test_job_read_then_approved_canary_change(tmp_path):
    events, ai, db = _run_job(tmp_path, [READ, CHANGE, DONE], (True, "canary"))
    plans = [e for e in events if e["type"] == "ai.plan"]
    assert [p["needs_approval"] for p in plans] == [False, True]
    assert plans[1]["risk"] == "change"  # the allowlist overrides the AI's "read" label
    results = [e for e in events if e["type"] == "ai.result"]
    assert len(results) == 6 and all(r["exit"] == 0 for r in results)
    change_results = [r for r in results if r["round"] == 2]
    assert change_results[0]["name"] == "s0"  # canary first
    job = plans[0]["job"]
    assert events[-2]["type"] == "ai.answer" and events[-1] == {"type": "ai.status", "job": job, "state": "done",
                                                                "chat": plans[0]["chat"]}
    assert "RESULTS of round 1" in ai.messages[1] and "TARGET SERVERS" in ai.messages[0]
    assert len(db.list_audit()) == 6


def test_job_rejected_change_runs_nothing(tmp_path):
    events, ai, db = _run_job(tmp_path, [READ, CHANGE], (False, "canary"))
    assert not [e for e in events if e["type"] == "ai.result" and e["round"] == 2]
    assert events[-1]["state"] == "cancelled"
    assert len(db.list_audit()) == 3


def test_job_is_kept_in_chat_history(tmp_path):
    events, ai, db = _run_job(tmp_path, [READ, CHANGE, DONE], (True, "parallel"))
    chat_id = events[0]["chat"]
    chat = db.get_chat(chat_id)
    assert chat["title"] == "update node"
    kinds = [e["type"] for e in chat["events"]]
    assert kinds[0] == "chat.user" and "ai.status" not in kinds
    assert kinds.count("ai.result") == 6 and kinds[-1] == "ai.answer"
    assert {"type": "ai.decision", "approve": True, "mode": "parallel"}.items() <= chat["events"][kinds.index("ai.decision")].items()
    assert db.list_chats()[0]["messages"] == 1

    async def reopen():
        brain = Brain(db, None, None, provider=FakeAI([]))
        await brain.open_chat(chat_id)
        return brain

    brain = asyncio.run(reopen())
    assert brain.chat == chat_id
    assert "USER: update node" in brain._note and "YOU: All updated" in brain._note


def test_canary_failure_skips_the_rest(tmp_path):
    bad = dict(CHANGE, steps=[{"servers": ["all"], "command": "sudo apt-get install -y FAIL"}])
    events, ai, db = _run_job(tmp_path, [bad, DONE], (True, "canary"))
    results = [e for e in events if e["type"] == "ai.result"]
    assert len(results) == 1 and results[0]["exit"] == 1
    assert "skipped: canary server failed" in ai.messages[1]


def test_extract_json_and_facts():
    assert extract_json('sure:\n```json\n{"a": 1}\n```') == {"a": 1}
    assert extract_json('x {"b": {"c": 2}} y') == {"b": {"c": 2}}
    with pytest.raises(ValueError):
        extract_json("nothing")
    facts = parse_facts("os=Ubuntu 22.04.4 LTS\nkernel=5.15\nuptime=90061\nload=0.50 0.40 0.30\ncpus=4\n"
                        "mem=7900 3950\ndisk=104857600 52428800")
    assert facts["os"].startswith("Ubuntu") and facts["cpus"] == 4 and facts["load"][0] == 0.5
    assert facts["mem"]["used_pct"] == 50.0 and facts["disk"] == {"total_gb": 100.0, "free_gb": 50.0, "used_pct": 50.0}


def test_steps_never_leave_the_chosen_targets():
    by_id = {1: {"name": "a"}, 2: {"name": "b"}}  # e.g. only the "Prod" folder
    steps = [{"servers": ["all"], "command": "df -h"},
             {"servers": [2, 9], "command": "uptime"},       # 9 = a server outside the folder
             {"servers": ["other-folder-server"], "command": "reboot"}]
    local, tasks = Brain._expand(steps, by_id)
    assert not local and set(tasks) == {1, 2}
    assert [c["command"] for c in tasks[2]] == ["df -h", "uptime"]


def test_upload_file_and_folder(tmp_path):
    async def main():
        server, port = await _fake_server()
        db, (sid, _) = _db(tmp_path, port)
        ssh = SSHManager(db)
        (tmp_path / "rtmp").mkdir()
        ssh.remote_tmp = (tmp_path / "rtmp").as_posix()
        (tmp_path / "dump.sql.gz").write_bytes(b"x" * 5000)
        (tmp_path / "dist" / "js").mkdir(parents=True)
        (tmp_path / "dist" / "js" / "app.js").write_text("ok")
        for i in range(300):  # a build output: many small files
            (tmp_path / "dist" / f"chunk{i}.js").write_text(str(i))
        remote = (tmp_path / "remote").as_posix() + "/"  # the fake SFTP server writes to this disk
        seen = []
        res = await ssh.upload(sid, str(tmp_path / "dump.sql.gz"), remote, lambda *a: seen.append(a[2:]))
        assert res["exit"] == 0 and res["error"] is None and "dump.sql.gz" in res["stdout"]
        assert (tmp_path / "remote" / "dump.sql.gz").read_bytes() == b"x" * 5000 and seen[-1] == (5000, 5000)
        seen.clear()
        res = await ssh.upload(sid, str(tmp_path / "dist"), remote, lambda *a: seen.append(a[2:]))  # copied inside
        assert res["exit"] == 0 and "301 files" in res["stdout"] and "one" in res["stdout"]
        assert len({total for _, total in seen}) == 1  # one transfer, not one per file
        out = tmp_path / "remote" / "dist"
        assert (out / "js" / "app.js").read_text() == "ok" and (out / "chunk299.js").read_text() == "299"
        assert not list((tmp_path / "rtmp").iterdir())  # archive removed on the server
        assert not list(Path(tempfile.gettempdir()).glob("sshpro-upload-*"))  # … and on this PC
        res = await ssh.upload(sid, str(tmp_path / "nope.zip"), remote)
        assert res["exit"] is None and "Not found on this PC" in res["error"]
        await ssh.close()
        server.close()

    asyncio.run(main())


@pytest.mark.parametrize("installable", [True, False])
def test_folder_upload_without_tar_installs_it_or_falls_back(tmp_path, installable):
    FAKE.update(tar=False, installable=installable)

    async def main():
        server, port = await _fake_server()
        db, (sid, _) = _db(tmp_path, port)
        ssh = SSHManager(db)
        (tmp_path / "rtmp").mkdir()
        ssh.remote_tmp = (tmp_path / "rtmp").as_posix()
        (tmp_path / "dist" / "css").mkdir(parents=True)
        (tmp_path / "dist" / "css" / "a.css").write_text("css")
        (tmp_path / "dist" / "index.html").write_text("html")
        remote = (tmp_path / "remote").as_posix() + "/"
        res = await ssh.upload(sid, str(tmp_path / "dist"), remote)
        assert res["exit"] == 0 and res["error"] is None
        if installable:
            assert "installed missing tar/gzip" in res["stdout"] and "one" in res["stdout"]
            await ssh.upload(sid, str(tmp_path / "dist"), remote)
            assert FAKE["installs"] == 1  # remembered: no second install
        else:
            assert "sent file by file" in res["stdout"]
        assert (tmp_path / "remote" / "dist" / "css" / "a.css").read_text() == "css"
        assert (tmp_path / "remote" / "dist" / "index.html").read_text() == "html"
        await ssh.close()
        server.close()

    asyncio.run(main())


def test_job_local_build_upload_then_server_steps(tmp_path):
    (tmp_path / "app").mkdir()
    remote = (tmp_path / "remote").as_posix() + "/"
    deploy = {"say": "deploying", "action": "run", "risk": "change", "steps": [
        {"local": "Set-Content build.txt built", "cwd": str(tmp_path / "app")},
        {"servers": [1], "upload": {"from": str(tmp_path / "app" / "build.txt"), "to": remote}},
        {"servers": [1], "command": "pm2 restart api && pm2 save"},
    ]}
    events, ai, db = _run_job(tmp_path, [deploy, DONE], (True, "canary"))
    plan = next(e for e in events if e["type"] == "ai.plan")
    assert plan["needs_approval"] and [t["name"] for t in plan["tasks"]] == ["This PC", "s0"]
    results = [e for e in events if e["type"] == "ai.result"]
    assert [r["name"] for r in results] == ["This PC", "s0", "s0"] and all(r["exit"] == 0 for r in results)
    assert (tmp_path / "remote" / "build.txt").read_text().strip() == "built"
    assert '["local","This PC"]' in ai.messages[1]


def test_download_file_and_folder(tmp_path):
    async def main():
        server, port = await _fake_server()
        db, (sid, _) = _db(tmp_path, port)
        ssh = SSHManager(db)
        (tmp_path / "rtmp").mkdir()
        ssh.remote_tmp = (tmp_path / "rtmp").as_posix()
        srv = tmp_path / "srv"  # the fake SFTP server reads from this disk
        (srv / "backups" / "uploads").mkdir(parents=True)
        (srv / "backup.sql.gz").write_bytes(b"y" * 7000)
        (srv / "backups" / "uploads" / "a.jpg").write_bytes(b"img")
        for i in range(200):
            (srv / "backups" / f"f{i}.txt").write_text(str(i))
        local = tmp_path / "Downloads"
        seen = []
        res = await ssh.download(sid, (srv / "backup.sql.gz").as_posix(), str(local) + "\\", lambda *a: seen.append(a[2:]))
        assert res["exit"] == 0 and res["error"] is None and seen[-1] == (7000, 7000)
        assert (local / "backup.sql.gz").read_bytes() == b"y" * 7000
        res = await ssh.download(sid, (srv / "backup.sql.gz").as_posix(), str(local))  # existing folder: inside
        assert (local / "backup.sql.gz (2)").exists() is False and (local / "backup (2).sql.gz").exists()
        assert "backup (2).sql.gz" in res["stdout"]  # never overwritten; the real path is reported

        seen.clear()
        res = await ssh.download(sid, (srv / "backups").as_posix(), str(local), lambda *a: seen.append(a[2:]))
        assert res["exit"] == 0 and "201 files" in res["stdout"] and "one" in res["stdout"]
        assert len({total for _, total in seen}) == 1  # one transfer, not one per file
        assert (local / "backups" / "uploads" / "a.jpg").read_bytes() == b"img"
        assert (local / "backups" / "f199.txt").read_text() == "199"
        assert not list((tmp_path / "rtmp").iterdir())  # archive removed on the server
        assert not list(Path(tempfile.gettempdir()).glob("sshpro-download-*"))  # … and on this PC

        res = await ssh.download(sid, (srv / "nope.sql").as_posix(), str(local))
        assert res["exit"] == 1 and "Not found on the server" in res["stderr"]
        await ssh.close()
        server.close()

    asyncio.run(main())


def test_download_folder_without_tar_falls_back(tmp_path):
    FAKE.update(tar=False, installable=False)

    async def main():
        server, port = await _fake_server()
        db, (sid, _) = _db(tmp_path, port)
        ssh = SSHManager(db)
        (tmp_path / "srv" / "site" / "css").mkdir(parents=True)
        (tmp_path / "srv" / "site" / "css" / "a.css").write_text("css")
        res = await ssh.download(sid, (tmp_path / "srv" / "site").as_posix(), str(tmp_path / "copy"))
        assert res["exit"] == 0 and "sent file by file" in res["stdout"]
        assert (tmp_path / "copy" / "css" / "a.css").read_text() == "css"  # "to" is the new name
        await ssh.close()
        server.close()

    asyncio.run(main())


def test_download_steps_get_a_folder_per_server():
    by_id = {1: {"name": "web: 1"}, 2: {"name": "db"}}
    local, tasks = Brain._expand([{"servers": ["all"], "download": {"from": "/var/backups/x.gz", "to": "D:\\bk"}},
                                  {"servers": [2], "download": {"from": "/etc/hosts"}}], by_id)
    assert [t["to"] for t in tasks[1]] == [os.path.join("D:\\bk", "web_ 1", "")]
    assert tasks[2][0]["to"] == os.path.join("D:\\bk", "db", "")
    assert tasks[2][1]["to"] == os.path.join(Path.home(), "Downloads", "")  # default: Downloads
    assert not Brain._reads_only(tasks[2][1])  # writes to this PC: needs approval


def test_job_download_backup_needs_approval(tmp_path):
    (tmp_path / "srv").mkdir()
    (tmp_path / "srv" / "db.sql.gz").write_bytes(b"dump")
    fetch = {"say": "downloading", "action": "run", "risk": "read", "steps": [
        {"servers": [1], "download": {"from": (tmp_path / "srv" / "db.sql.gz").as_posix(),
                                      "to": str(tmp_path / "bk") + "\\"}}]}
    events, ai, db = _run_job(tmp_path, [fetch, DONE], (True, "canary"))
    plan = next(e for e in events if e["type"] == "ai.plan")
    assert plan["needs_approval"] and plan["risk"] == "change"
    assert (tmp_path / "bk" / "db.sql.gz").read_bytes() == b"dump"
    assert "downloaded" in ai.messages[1]


def test_failed_local_step_runs_nothing_on_servers(tmp_path):
    bad = {"action": "run", "steps": [{"local": "exit 3"}, {"servers": ["all"], "command": "uptime"}]}
    events, ai, db = _run_job(tmp_path, [bad, DONE], (True, "parallel"))
    results = [e for e in events if e["type"] == "ai.result"]
    assert len(results) == 1 and results[0]["exit"] == 3
    # the three skipped servers are one grouped entry for the AI
    assert ai.messages[1].count("skipped: a local step failed") == 1
    assert '"on":[[1,"s0"],[2,"s1"],[3,"s2"]]' in ai.messages[1]
