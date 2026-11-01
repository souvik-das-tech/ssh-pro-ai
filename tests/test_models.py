import asyncio
import sys

import pytest
from aiohttp.test_utils import TestClient, TestServer

import sshpro.agy as agy
from sshpro.brain import Brain
from sshpro.db import Database
from sshpro.server import Backend

FAKE_AGY = """
import sys
assert sys.argv[1] == "models"
print("Fetching available models...")
print("gemini-9-flash-low\\tGemini 9 Flash (Low)")
print("claude-sonnet-9\\tClaude Sonnet 9 (Thinking)")
"""


@pytest.fixture
def fake_agy(tmp_path):
    script = tmp_path / "fake_agy.py"
    script.write_text(FAKE_AGY, encoding="utf-8")
    return [sys.executable, str(script)]


@pytest.fixture
def data(monkeypatch, tmp_path):
    monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))


def test_list_models_parses_agy_output(fake_agy):
    models = asyncio.run(agy.list_models(fake_agy))
    assert models == [
        {"id": "gemini-9-flash-low", "label": "Gemini 9 Flash (Low)"},
        {"id": "claude-sonnet-9", "label": "Claude Sonnet 9 (Thinking)"},
    ]


def test_list_models_empty_output_is_an_error(tmp_path):
    script = tmp_path / "empty.py"
    script.write_text("print('Fetching available models...')", encoding="utf-8")
    with pytest.raises(agy.AIProcessError):
        asyncio.run(agy.list_models([sys.executable, str(script)]))


def test_model_choice_is_saved(data, tmp_path):
    async def main():
        db = Database(tmp_path / "t.sqlite3")
        brain = Brain(db, None, None, provider=None)
        brain.warm_up = lambda: None  # don't start AGY
        await brain.set_model("claude-sonnet-9")
        assert brain.model == "claude-sonnet-9"
        assert Brain(db, None, None).model == "claude-sonnet-9"

    asyncio.run(main())


def test_models_api(data, monkeypatch, fake_agy, tmp_path):
    async def main():
        backend = Backend(Database(tmp_path / "t.sqlite3"), token="tok")
        monkeypatch.setattr(backend.brain, "models", lambda: agy.list_models(fake_agy))
        backend.brain.warm_up = lambda: None
        async with TestClient(TestServer(backend.app())) as client:
            body = await (await client.get("/api/ai/models?t=tok")).json()
            assert [m["id"] for m in body["models"]] == ["gemini-9-flash-low", "claude-sonnet-9"]
            assert body["current"] == backend.brain.model
            resp = await client.post("/api/ai/model?t=tok", json={"model": "gemini-9-flash-low"})
            assert (await resp.json())["model"] == "gemini-9-flash-low"
            assert (await client.post("/api/ai/model?t=tok", json={"model": "bad id"})).status == 400

    asyncio.run(main())
