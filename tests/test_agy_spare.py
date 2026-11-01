"""AgyProvider keeps a warm spare session, so a new chat does not wait for AGY to start again."""

import asyncio
import sys
import time

from sshpro.agy import AgyProvider

# Speaks AGY's stream-json: every turn takes 0.5 s (a real warm-up takes ~10 s) and names its process.
FAKE_AGY = r'''
import json, os, sys, time
for line in sys.stdin:
    text = json.loads(line)["message"]["content"]
    time.sleep(0.5)
    reply = {"status": "SUCCESS", "response": f"{os.getpid()}:{text}"}
    print(json.dumps({"event": "result", "result": reply}), flush=True)
'''


def test_reset_swaps_in_the_warm_spare(tmp_path):
    script = tmp_path / "fake_agy.py"
    script.write_text(FAKE_AGY, encoding="utf-8")

    async def main():
        ai = AgyProvider(command=[sys.executable, str(script)], workdir=tmp_path)
        await ai.start("instructions")
        first = (await ai.ask("hi")).split(":")
        await asyncio.sleep(1.5)  # the spare finishes its warm-up in the background

        started = time.monotonic()
        await ai.reset()
        assert time.monotonic() - started < 0.2  # no restart, no waiting for a lock

        started = time.monotonic()
        second = (await ai.ask("hi again")).split(":")
        assert time.monotonic() - started < 1.0  # one turn; the instructions were sent already
        assert second[1] == "hi again" and second[0] != first[0]  # a different (fresh) process

        await ai.reset()  # the next spare may still be warming up: reset must not wait for it either
        assert (await ai.ask("third")).endswith(":third")
        await ai.close()
        assert not ai._bg

    asyncio.run(main())
