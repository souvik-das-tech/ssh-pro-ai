import asyncio

import pytest
from aiohttp.test_utils import TestClient, TestServer

import sshpro.agy as agy
import sshpro.brain as brain_mod
from sshpro.brain import Brain
from sshpro.db import Database
from sshpro.server import Backend


@pytest.fixture
def no_agy(monkeypatch):
    monkeypatch.setattr(agy.shutil, "which", lambda name: None)
    monkeypatch.setenv("LOCALAPPDATA", "Z:/nowhere")
    monkeypatch.delenv("SSHPRO_AGY", raising=False)
    monkeypatch.setattr(brain_mod, "agy_available", agy.agy_available)
    monkeypatch.setattr(brain_mod.codex, "codex_available", lambda: None)  # ChatGPT brain missing too
    monkeypatch.setattr("sshpro.server.codex_available", lambda: None)


def test_find_agy_raises_with_install_link(no_agy):
    with pytest.raises(agy.AINotInstalled, match="antigravity-cli"):
        agy.find_agy()
    assert agy.agy_available() is None


def test_ai_chat_refused_but_app_runs(no_agy, tmp_path):
    async def main():
        db = Database(tmp_path / "t.sqlite3")
        db.create_server({"name": "a", "host": "127.0.0.1", "username": "u"}, source="manual")
        backend = Backend(db, token="tok")
        assert not backend.brain.available
        backend.brain.warm_up()  # must not try to start AGY
        assert backend.brain._start_task is None
        async with TestClient(TestServer(backend.app())) as client:
            meta = await (await client.get("/api/meta?t=tok")).json()
            assert meta["agy"] is None and meta["agy_install_url"] == agy.AGY_INSTALL_URL
            resp = await client.post("/api/ai/ask?t=tok", json={"prompt": "disk space"})
            assert resp.status == 400
            assert "AGY CLI" in (await resp.json())["error"]
            assert (await client.get("/api/servers?t=tok")).status == 200
            assert (await client.post("/api/voice/start?t=tok", json={})).status in (404, 405)

    asyncio.run(main())


def test_brain_with_injected_provider_is_available(no_agy, tmp_path):
    assert Brain(Database(tmp_path / "t.sqlite3"), None, None, provider=object()).available
