"""Favourite chats never expire, can be renamed, and remember which servers they targeted."""

import asyncio
import sqlite3

import pytest
from aiohttp.test_utils import TestClient, TestServer

from sshpro.db import Database
from sshpro.server import Backend


def _age(db, chat_id, days=40):
    db.conn.execute("UPDATE chats SET updated_at = datetime('now', ?) WHERE id = ?", (f"-{days} days", chat_id))
    db.conn.commit()


def test_favourites_survive_the_purge_and_can_be_renamed(tmp_path):
    db = Database(tmp_path / "t.sqlite3")
    keep, drop = db.create_chat("nginx status"), db.create_chat("old one")
    db.update_chat(keep, favorite=True, title="Prod nginx")
    _age(db, keep)
    _age(db, drop)
    assert db.purge_chats(15) == 1
    (chat,) = db.list_chats()
    assert chat["id"] == keep and chat["favorite"] is True and chat["title"] == "Prod nginx"
    with pytest.raises(ValueError):
        db.update_chat(keep, title="  ")
    with pytest.raises(KeyError):
        db.update_chat(9999, favorite=True)


def test_old_database_gets_the_favorite_column(tmp_path):
    path = tmp_path / "old.sqlite3"
    conn = sqlite3.connect(path)
    conn.execute("CREATE TABLE chats (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL,"
                 " created_at TEXT NOT NULL, updated_at TEXT NOT NULL)")
    conn.execute("INSERT INTO chats (title, created_at, updated_at) VALUES ('x', '2026-01-01', '2026-01-01')")
    conn.commit()
    conn.close()
    db = Database(path)
    assert db.list_chats()[0]["favorite"] is False


def test_messages_keep_their_servers_and_the_list_shows_the_last_request(tmp_path, monkeypatch):
    monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))

    async def main():
        db = Database(tmp_path / "t.sqlite3")
        a = db.create_server({"name": "web1", "host": "127.0.0.1", "username": "u"}, source="manual")
        backend = Backend(db, token="tok")
        backend.ssh.run = _fake_run  # no real SSH
        async with TestClient(TestServer(backend.app())) as client:
            resp = await client.post("/api/exec?t=tok", json={
                "win": "w2", "command": "uptime", "ids": [a], "target": "Custom (1): web1",
                "scope": {"spec": "custom", "ids": [a, "x"]}})
            assert resp.status == 200
            await asyncio.sleep(0.1)
            (chat,) = (await (await client.get("/api/chats?t=tok")).json())["chats"]
            assert chat["last"] == {"text": "uptime", "mode": "direct", "target": "Custom (1): web1",
                                    "scope": {"spec": "custom", "ids": [a]}}
            assert chat["favorite"] is False
            resp = await client.patch(f"/api/chats/{chat['id']}?t=tok", json={"favorite": True, "title": "Uptime web"})
            assert resp.status == 200
            opened = await (await client.post(f"/api/chats/{chat['id']}/open?t=tok", json={"win": "w2"})).json()
            assert opened["favorite"] is True and opened["title"] == "Uptime web"
            assert opened["events"][0]["scope"] == {"spec": "custom", "ids": [a]}

    asyncio.run(main())


async def _fake_run(server_id, command, timeout=None):
    return {"exit": 0, "stdout": "up 3 days", "stderr": "", "error": None, "duration": 0.01}
