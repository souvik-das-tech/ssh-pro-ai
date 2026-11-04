import asyncio
import json

import pytest

from sshpro.agy import AITimeout
from sshpro.brain import Brain
from sshpro.db import Database
from sshpro.router import AUTO, FAST_PREFERENCE, SMART_PREFERENCE, classify, pick_default


@pytest.mark.parametrize("prompt", [
    "disk space koto ache dekho", "node version kitna hai", "uptime", "hi", "ram usage show karo",
    "kon kon server online?", "php -v sob server e",
])
def test_quick_requests_go_to_fast(prompt):
    assert classify(prompt)[0] == "fast", prompt


@pytest.mark.parametrize("prompt,why", [
    ("nginx 502 dicche keno?", "troubleshooting"),
    ("website kaj korche na, thik koro", "troubleshooting"),
    ("site nahi chal raha, check karo", "troubleshooting"),
    ("is PHP 8.3 vulnerable to CVE-2024-4577?", "security check"),
    ("sob server e node 22 install koro", "change to servers"),
    ("deploy the build to web1", "change to servers"),
    ("compare mysql performance on all servers", "analysis"),
    ("x " * 150, "long request"),
])
def test_hard_requests_go_to_smart(prompt, why):
    assert classify(prompt) == ("smart", why)


def test_defaults_prefer_fast_and_strong_models():
    ids = ["gemini-3.8-flash-high", "gemini-3.8-flash-medium", "gemini-3.8-flash-low", "claude-sonnet-4-6",
           "claude-opus-4-6-thinking", "chatgpt:gpt-6-luna", "chatgpt:gpt-5.6-luna"]
    assert pick_default(ids, FAST_PREFERENCE) == "gemini-3.8-flash-low"
    assert pick_default(ids, SMART_PREFERENCE) == "chatgpt:gpt-6-luna"
    assert pick_default(["only-one"], SMART_PREFERENCE, avoid="only-one") == "only-one"


class ModelAI:
    """Fake provider that answers at once and records which model got which message."""

    def __init__(self, model, log, fail=False):
        self.model, self.log, self.fail = model, log, fail

    async def start(self, instructions):
        pass

    async def ask(self, message):
        self.log.append((self.model, message))
        if self.fail:
            raise AITimeout("too slow")
        return json.dumps({"say": "ok", "action": "answer", "answer": {"summary": f"answered by {self.model}"}})

    async def reset(self):
        pass

    async def close(self):
        pass


def _brain(tmp_path, monkeypatch, failing=()):
    monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))
    monkeypatch.setattr(Brain, "available", property(lambda self: True))  # CI has no AGY / Codex
    db = Database(tmp_path / "t.sqlite3")
    db.create_server({"name": "web1", "host": "127.0.0.1", "username": "u"}, source="manual")
    events, log = [], []

    async def emit(ev):
        events.append(ev)

    brain = Brain(db, None, emit)
    brain.model = AUTO
    brain.auto = {"fast": "fast-model", "smart": "smart-model"}
    brain._make_provider = lambda: ModelAI(brain._current(), log, fail=brain._current() in failing)
    return brain, events, log


async def _ask(brain, prompt):
    brain.ask(prompt, None)
    await brain._job


def test_auto_routes_each_request_and_smart_sticks_for_the_chat(tmp_path, monkeypatch):
    async def main():
        brain, events, log = _brain(tmp_path, monkeypatch)
        await _ask(brain, "uptime dekho")
        await _ask(brain, "nginx 502 dicche keno?")
        await _ask(brain, "uptime abar dekho")  # same chat: stays on the smart model
        assert [m for m, _ in log] == ["fast-model", "smart-model", "smart-model"]
        # the smart model got the chat so far as context, not the new request twice
        handover = log[1][1]
        assert "USER: uptime dekho" in handover and handover.count("nginx 502") == 1
        thinking = [e for e in events if e["type"] == "ai.status" and e["state"] == "thinking"]
        assert thinking[0]["model"] == "fast-model" and "fast" in thinking[0]["why"]
        assert thinking[1]["model"] == "smart-model" and "troubleshooting" in thinking[1]["why"]
        await brain.new_chat()
        await _ask(brain, "uptime")  # a new chat starts fast again
        assert log[-1][0] == "fast-model"

    asyncio.run(main())


def test_auto_hands_a_failed_fast_round_to_the_smart_model(tmp_path, monkeypatch):
    async def main():
        brain, events, log = _brain(tmp_path, monkeypatch, failing={"fast-model"})
        await _ask(brain, "disk space koto")
        answer = next(e for e in events if e["type"] == "ai.answer")
        assert answer["summary"] == "answered by smart-model"
        assert log[-1][0] == "smart-model" and "USER REQUEST: disk space koto" in log[-1][1]
        assert any("taking over" in (e.get("text") or "") for e in events)

    asyncio.run(main())


def test_choosing_auto_keeps_the_old_model_as_smart(tmp_path, monkeypatch):
    async def main():
        monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))
        db = Database(tmp_path / "t.sqlite3")
        brain = Brain(db, None, None)
        brain.warm_up = lambda: None
        await brain.set_model("chatgpt:gpt-6-luna")
        await brain.set_model(AUTO)
        assert brain.auto["smart"] == "chatgpt:gpt-6-luna"
        again = Brain(db, None, None)
        assert again.model == AUTO and again.auto["smart"] == "chatgpt:gpt-6-luna"

    asyncio.run(main())
