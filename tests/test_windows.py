"""Several chat windows work at the same time, each with its own AI session, job and chat."""

import asyncio
import json

import pytest

from sshpro.brain import MAX_WINDOWS, Brain, Windows
from sshpro.db import Database


class SlowAI:
    """Answers after a pause, so two windows' jobs overlap; records who asked what."""

    def __init__(self, log):
        self.log = log

    async def start(self, instructions):
        pass

    async def ask(self, message):
        self.log.append(("start", message))
        await asyncio.sleep(0.2)
        self.log.append(("end", message))
        return json.dumps({"say": "ok", "action": "answer", "answer": {"summary": "done"}})

    async def reset(self):
        pass

    async def close(self):
        pass


def _windows(tmp_path, monkeypatch):
    monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))
    monkeypatch.setattr(Brain, "available", property(lambda self: True))
    db = Database(tmp_path / "t.sqlite3")
    db.create_server({"name": "web1", "host": "127.0.0.1", "username": "u"}, source="manual")
    db.create_server({"name": "db1", "host": "127.0.0.2", "username": "u"}, source="manual")
    events, log = [], []

    async def emit(ev):
        events.append(ev)

    monkeypatch.setattr(Brain, "_make_provider", lambda self: SlowAI(log))
    return Windows(db, None, emit), db, events, log


def test_two_windows_work_at_the_same_time(tmp_path, monkeypatch):
    async def main():
        wins, db, events, log = _windows(tmp_path, monkeypatch)
        web, dbs = [s["id"] for s in db.list_servers()]
        first = wins.get("main")
        second = wins.get("w2")
        first.ask("web check", [web], "web1")
        second.ask("db check", [dbs], "db1")  # the first window is still busy: no "still working" error
        assert first.busy and second.busy
        await asyncio.gather(first._job, second._job)
        # both AI turns started before either finished
        starts = [i for i, (kind, _) in enumerate(log) if kind == "start"]
        ends = [i for i, (kind, _) in enumerate(log) if kind == "end"]
        assert max(starts[:2]) < min(ends)
        # each window's events are tagged with it and belong to its own chat
        by_win = {w: {e["chat"] for e in events if e.get("win") == w} for w in ("main", "w2")}
        assert len(by_win["main"]) == 1 and len(by_win["w2"]) == 1 and by_win["main"] != by_win["w2"]
        assert {c["title"] for c in db.list_chats()} == {"web check", "db check"}
        assert wins.state()["w2"] == {"chat": second.chat, "busy": False, "waiting": False}

    asyncio.run(main())


def test_a_chat_is_open_in_one_window_only(tmp_path, monkeypatch):
    async def main():
        wins, db, events, log = _windows(tmp_path, monkeypatch)
        brain = wins.get("main")
        brain.ask("uptime", None)
        await brain._job
        with pytest.raises(RuntimeError, match="another window"):
            await wins.open_chat("w2", brain.chat)
        assert wins.holder(brain.chat) == "main"
        await wins.delete_chat(brain.chat)  # the window showing it starts a new chat
        assert brain.chat is None and not db.list_chats()

    asyncio.run(main())


def test_window_limit_ids_and_close(tmp_path, monkeypatch):
    async def main():
        wins, db, events, log = _windows(tmp_path, monkeypatch)
        for i in range(MAX_WINDOWS - 1):
            wins.get(f"w{i}")
        with pytest.raises(RuntimeError, match="At most"):
            wins.get("one-too-many")
        with pytest.raises(ValueError):
            wins.get("bad id!")
        with pytest.raises(KeyError):
            wins.get("w0x", create=False)
        await wins.close_window("w0")
        wins.get("w-new")  # room again
        await wins.close_window("main")  # the main window only starts a fresh chat
        assert wins.get("main", create=False) is wins.main

    asyncio.run(main())


def test_model_choice_applies_to_every_window(tmp_path, monkeypatch):
    async def main():
        wins, db, events, log = _windows(tmp_path, monkeypatch)
        monkeypatch.setattr(Brain, "warm_up", lambda self: None)
        other = wins.get("w2")
        await wins.set_model("claude-sonnet-9")
        assert wins.main.model == other.model == "claude-sonnet-9"
        assert wins.get("w3").model == "claude-sonnet-9"  # new windows start on it too

    asyncio.run(main())
