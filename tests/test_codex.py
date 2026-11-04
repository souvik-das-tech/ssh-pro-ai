import asyncio
import json
import sys

import pytest

import sshpro.brain as brain_mod
import sshpro.codex as codex
from sshpro.agy import AIAuthError, AIModelError
from sshpro.brain import Brain
from sshpro.db import Database

# Fake `codex`: prints the model catalog, or answers a turn and echoes how it was called.
FAKE_CODEX = r"""
import json, sys
args = sys.argv[1:]
if args[:2] == ["debug", "models"]:
    print(json.dumps({"models": [
        {"slug": "gpt-hidden", "display_name": "Hidden", "visibility": "hide", "priority": 1},
        {"slug": "gpt-9-terra", "display_name": "GPT-9-Terra", "visibility": "list", "priority": 8},
        {"slug": "gpt-9-luna", "display_name": "GPT-9-Luna", "visibility": "list", "priority": 5},
    ]}))
    sys.exit(0)
prompt = sys.stdin.read()
resume = args[1] == "resume"
model = args[args.index("-m") + 1]
if model == "bad-model":
    print(json.dumps({"type": "thread.started", "thread_id": "t-bad"}))
    print(json.dumps({"type": "turn.failed", "error": {"message": "model is not supported"}}))
    sys.exit(1)
if model == "no-login":
    print("Error: not logged in. Run codex login", file=sys.stderr)
    sys.exit(1)
thread = args[-2] if resume else "t-123"
print(json.dumps({"type": "thread.started", "thread_id": thread}))
print(json.dumps({"type": "turn.started"}))
base = next((a.split("=", 1)[1] for a in args if a.startswith("model_instructions_file=")), None)
info = {"resume": resume, "thread": thread, "prompt": prompt, "base": open(base, encoding="utf-8").read() if base else None,
        "shell_disabled": "shell_tool" in args, "read_only": "sandbox_mode=read-only" in args,
        "no_skills": "skills.bundled.enabled=false" in args}
print(json.dumps({"type": "item.completed", "item": {"type": "agent_message", "text": json.dumps(info)}}))
print(json.dumps({"type": "turn.completed", "usage": {}}))
"""


@pytest.fixture
def fake_codex(tmp_path):
    script = tmp_path / "fake_codex.py"
    script.write_text(FAKE_CODEX, encoding="utf-8")
    return [sys.executable, str(script)]


def test_list_models_shows_listed_models_by_priority(fake_codex):
    models = asyncio.run(codex.list_models(fake_codex))
    assert models == [
        {"id": "chatgpt:gpt-9-luna", "label": "ChatGPT · GPT-9-Luna"},
        {"id": "chatgpt:gpt-9-terra", "label": "ChatGPT · GPT-9-Terra"},
    ]


def test_conversation_resumes_thread_and_stays_plan_only(fake_codex, tmp_path):
    async def main():
        p = codex.CodexProvider(fake_codex, "chatgpt:gpt-9-luna", workdir=tmp_path)
        await p.start("RULES\nAcknowledge these instructions with {}")
        first = json.loads(await p.ask("hello"))
        # the instructions are the system prompt (no ack line), not part of the message
        assert not first["resume"] and first["prompt"].strip() == "hello" and first["base"] == "RULES"
        assert first["shell_disabled"] and first["read_only"] and first["no_skills"]
        second = json.loads(await p.ask("again"))
        assert second["resume"] and second["thread"] == "t-123" and second["prompt"].strip() == "again"
        await p.reset()
        assert not json.loads(await p.ask("fresh"))["resume"]

    asyncio.run(main())


def test_errors_are_reported(fake_codex, tmp_path):
    async def main():
        with pytest.raises(AIModelError, match="not supported"):
            await codex.CodexProvider(fake_codex, "chatgpt:bad-model", workdir=tmp_path).ask("x")
        with pytest.raises(AIAuthError, match="codex login"):
            await codex.CodexProvider(fake_codex, "chatgpt:no-login", workdir=tmp_path).ask("x")

    asyncio.run(main())


def test_brain_picks_provider_by_model_and_merges_lists(monkeypatch, fake_codex, tmp_path):
    monkeypatch.setenv("SSHPRO_DATA_DIR", str(tmp_path / "data"))

    async def agy_models():
        return [{"id": "gemini-9-flash-low", "label": "Gemini 9 Flash (Low)"}]

    monkeypatch.setattr(brain_mod, "agy_available", lambda: "agy.exe")
    monkeypatch.setattr(brain_mod, "list_models", agy_models)
    monkeypatch.setattr(codex, "codex_available", lambda: "codex.exe")
    real_list_models = codex.list_models
    monkeypatch.setattr(codex, "list_models", lambda: real_list_models(fake_codex))
    monkeypatch.setattr(codex, "find_codex", lambda configured=None: fake_codex[0])

    async def main():
        brain = Brain(Database(tmp_path / "t.sqlite3"), None, None)
        brain.warm_up = lambda: None
        ids = [m["id"] for m in await brain.models()]
        assert ids == ["gemini-9-flash-low", "chatgpt:gpt-9-luna", "chatgpt:gpt-9-terra"]
        await brain.set_model("chatgpt:gpt-9-luna")
        provider = brain._make_provider()
        assert isinstance(provider, codex.CodexProvider) and provider.model == "gpt-9-luna"

    asyncio.run(main())

