"""Local backend for the UI: REST + WebSockets on 127.0.0.1 with a random per-launch token.

The token keeps other local programs and web pages from driving the API (it can run commands
on every server), so every /api and /ws request must carry it.
"""

from __future__ import annotations

import asyncio
import json
import logging
import secrets
import shutil
import webbrowser
from typing import Any

from aiohttp import WSMsgType, web

from sshpro import __version__
from sshpro.agy import AGY_INSTALL_URL, AIError, agy_available
from sshpro.backup import export_backup, import_backup
from sshpro.brain import Brain
from sshpro.db import Database
from sshpro.importer import import_termix
from sshpro.paths import TERMIX_DB, WEB_DIR, data_dir
from sshpro.sshpool import SSHError, SSHManager

log = logging.getLogger(__name__)

AUDIT_DAYS = 5
CHAT_DAYS = 15


class Backend:
    def __init__(self, db: Database, token: str | None = None):
        self.db = db
        self.token = token or secrets.token_urlsafe(24)
        self.ssh = SSHManager(db)
        self.clients: set[web.WebSocketResponse] = set()
        self.brain = Brain(db, self.ssh, self.emit)
        self._tasks: set[asyncio.Task] = set()
        self.errors: dict[int, str] = {}  # last connection error per server (not persisted)

    # ---- plumbing ------------------------------------------------------------------------

    async def emit(self, event: dict[str, Any]) -> None:
        data = json.dumps(event, ensure_ascii=False)
        for ws in list(self.clients):
            try:
                await ws.send_str(data)
            except Exception:
                self.clients.discard(ws)

    def spawn(self, coro) -> None:
        task = asyncio.create_task(coro)
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)

    @web.middleware
    async def guard(self, request: web.Request, handler):
        path = request.path
        if path.startswith(("/api/", "/ws/")):
            given = request.headers.get("X-Token") or request.query.get("t")
            if not given or not secrets.compare_digest(given, self.token):
                return web.json_response({"error": "forbidden"}, status=403)
        try:
            return await handler(request)
        except (ValueError, KeyError, FileNotFoundError, RuntimeError, AIError) as exc:
            return web.json_response({"error": str(exc)}, status=400)

    def app(self) -> web.Application:
        app = web.Application(middlewares=[self.guard], client_max_size=32 * 1024 * 1024)
        r = app.router
        r.add_get("/", self.index)
        r.add_static("/static/", WEB_DIR, append_version=False)
        r.add_get("/api/meta", self.meta)
        r.add_get("/api/servers", self.list_servers)
        r.add_post("/api/servers", self.create_server)
        r.add_put("/api/servers/{id}", self.update_server)
        r.add_delete("/api/servers/{id}", self.delete_server)
        r.add_post("/api/servers/{id}/forget-key", self.forget_key)
        r.add_post("/api/probe", self.probe)
        r.add_post("/api/import", self.import_db)
        r.add_post("/api/backup/export", self.backup_export)
        r.add_post("/api/backup/import", self.backup_import)
        r.add_get("/api/audit", self.audit)
        r.add_post("/api/open", self.open_url)
        r.add_post("/api/exec", self.exec_direct)
        r.add_post("/api/ai/ask", self.ai_ask)
        r.add_post("/api/ai/approve", self.ai_approve)
        r.add_post("/api/ai/stop", self.ai_stop)
        r.add_post("/api/ai/reset", self.ai_reset)
        r.add_get("/api/ai/models", self.ai_models)
        r.add_post("/api/ai/model", self.ai_set_model)
        r.add_get("/api/chats", self.list_chats)
        r.add_post("/api/chats/{id}/open", self.open_chat)
        r.add_delete("/api/chats/{id}", self.delete_chat)
        r.add_get("/ws/events", self.ws_events)
        r.add_get("/ws/term", self.ws_term)
        app.on_shutdown.append(self._shutdown)
        return app

    async def startup(self) -> None:
        """First launch: pull in the Termix servers, then check every server's health."""
        if not self.db.list_servers() and TERMIX_DB.is_file():
            try:
                log.info("Termix import: %s", import_termix(self.db, TERMIX_DB))
            except Exception:
                log.exception("Termix import failed")
        self.brain.warm_up()
        self.spawn(self._probe_all([s["id"] for s in self.db.list_servers()]))
        self.spawn(self._purge_loop())

    async def _purge_loop(self) -> None:
        """Audit rows are kept for AUDIT_DAYS, chats for CHAT_DAYS after their last message;
        checked at start and every hour after."""
        while True:
            removed = self.db.purge_audit(AUDIT_DAYS)
            if removed:
                log.info("Audit: removed %d entries older than %d days", removed, AUDIT_DAYS)
            removed = self.db.purge_chats(CHAT_DAYS)
            if removed:
                log.info("Chats: removed %d idle for more than %d days", removed, CHAT_DAYS)
            await asyncio.sleep(3600)

    async def _shutdown(self, app: web.Application) -> None:
        for ws in list(self.clients):
            await ws.close()
        await self.brain.close()
        await self.ssh.close()

    # ---- pages / meta --------------------------------------------------------------------

    async def index(self, request: web.Request) -> web.StreamResponse:
        return web.FileResponse(WEB_DIR / "index.html", headers={"Cache-Control": "no-store"})

    async def meta(self, request: web.Request) -> web.Response:
        return web.json_response({
            "version": __version__, "model": self.brain.model, "agy": agy_available(),
            "agy_install_url": AGY_INSTALL_URL, "data_dir": str(data_dir()),
            "termix_db": str(TERMIX_DB) if TERMIX_DB.is_file() else None, "ssh_exe": shutil.which("ssh"),
            "chat": self.brain.chat, "chat_days": CHAT_DAYS,
        })

    # ---- servers -------------------------------------------------------------------------

    async def list_servers(self, request: web.Request) -> web.Response:
        servers = self.db.list_servers()
        for server in servers:
            server["last_error"] = self.errors.get(server["id"])
        return web.json_response(servers)

    async def create_server(self, request: web.Request) -> web.Response:
        server_id = self.db.create_server(await request.json(), source="manual")
        self.spawn(self._probe_all([server_id]))
        return web.json_response(self.db.get_server(server_id))

    async def update_server(self, request: web.Request) -> web.Response:
        server_id = int(request.match_info["id"])
        self.db.update_server(server_id, await request.json())
        self.ssh.drop(server_id)  # reconnect with the new settings
        self.spawn(self._probe_all([server_id]))
        return web.json_response(self.db.get_server(server_id))

    async def delete_server(self, request: web.Request) -> web.Response:
        server_id = int(request.match_info["id"])
        self.ssh.drop(server_id)
        self.db.delete_server(server_id)
        return web.json_response({"ok": True})

    async def forget_key(self, request: web.Request) -> web.Response:
        server_id = int(request.match_info["id"])
        self.db.conn.execute("UPDATE servers SET host_key = NULL WHERE id = ?", (server_id,))
        self.db.conn.commit()
        self.ssh.drop(server_id)
        return web.json_response({"ok": True})

    async def probe(self, request: web.Request) -> web.Response:
        body = await request.json() if request.can_read_body else {}
        ids = body.get("ids") or [s["id"] for s in self.db.list_servers()]
        self.spawn(self._probe_all([int(i) for i in ids]))
        return web.json_response({"ok": True})

    async def _probe_all(self, ids: list[int]) -> None:
        sem = asyncio.Semaphore(16)

        async def one(server_id: int) -> None:
            await self.emit({"type": "probe", "server_id": server_id, "status": "checking"})
            async with sem:
                result = await self.ssh.probe(server_id)
            if result.get("error"):
                self.errors[server_id] = result["error"]
            else:
                self.errors.pop(server_id, None)
            await self.emit({"type": "probe", "server_id": server_id, **result})

        await asyncio.gather(*(one(i) for i in ids))

    async def import_db(self, request: web.Request) -> web.Response:
        body = await request.json() if request.can_read_body else {}
        result = import_termix(self.db, body.get("path") or TERMIX_DB)
        new_ids = [s["id"] for s in self.db.list_servers() if s["last_status"] is None]
        self.spawn(self._probe_all(new_ids))
        return web.json_response(result)

    async def backup_export(self, request: web.Request) -> web.Response:
        blob = export_backup(self.db)
        return web.json_response(blob, headers={"Cache-Control": "no-store"})

    async def backup_import(self, request: web.Request) -> web.Response:
        body = await request.json()
        result = import_backup(self.db, body.get("backup"))
        new_ids = [s["id"] for s in self.db.list_servers() if s["last_status"] is None]
        self.spawn(self._probe_all(new_ids))
        return web.json_response(result)

    async def audit(self, request: web.Request) -> web.Response:
        return web.json_response(self.db.list_audit(int(request.query.get("limit", 300))))

    async def open_url(self, request: web.Request) -> web.Response:
        url = str((await request.json()).get("url") or "")
        if not url.startswith(("http://", "https://")):
            raise ValueError("only http(s) links can be opened")
        webbrowser.open(url)
        return web.json_response({"ok": True})

    # ---- commands / AI -------------------------------------------------------------------

    async def exec_direct(self, request: web.Request) -> web.Response:
        body = await request.json()
        command = str(body.get("command") or "").strip()
        ids = [int(i) for i in body.get("ids") or []]
        if not command or not ids:
            raise ValueError("command and servers are required")
        self.spawn(self.brain.direct(command, ids, _target(body)))
        return web.json_response({"ok": True})

    async def ai_ask(self, request: web.Request) -> web.Response:
        body = await request.json()
        prompt = str(body.get("prompt") or "").strip()
        if not prompt:
            raise ValueError("empty prompt")
        ids = [int(i) for i in body.get("ids") or []] or None
        job_id = self.brain.ask(prompt, ids, _target(body))
        return web.json_response({"job": job_id})

    async def ai_approve(self, request: web.Request) -> web.Response:
        body = await request.json()
        self.brain.approve(bool(body.get("approve")), body.get("mode") or "canary")
        return web.json_response({"ok": True})

    async def ai_stop(self, request: web.Request) -> web.Response:
        await self.brain.stop()
        return web.json_response({"ok": True})

    async def ai_reset(self, request: web.Request) -> web.Response:
        await self.brain.new_chat()
        return web.json_response({"ok": True})

    async def ai_models(self, request: web.Request) -> web.Response:
        return web.json_response({"models": await self.brain.models(), "current": self.brain.model})

    async def ai_set_model(self, request: web.Request) -> web.Response:
        body = await request.json()
        model = str(body.get("model") or "").strip()
        if not model or any(c.isspace() for c in model):
            raise ValueError("invalid model id")
        await self.brain.set_model(model)
        return web.json_response({"ok": True, "model": self.brain.model})

    # ---- chat history --------------------------------------------------------------------

    async def list_chats(self, request: web.Request) -> web.Response:
        return web.json_response({"chats": self.db.list_chats(), "current": self.brain.chat, "days": CHAT_DAYS})

    async def open_chat(self, request: web.Request) -> web.Response:
        chat_id = int(request.match_info["id"])
        if self.brain.busy and chat_id != self.brain.chat:
            raise RuntimeError("The AI is still working — stop it or wait")
        return web.json_response(await self.brain.open_chat(chat_id))

    async def delete_chat(self, request: web.Request) -> web.Response:
        if self.brain.busy and int(request.match_info["id"]) == self.brain.chat:
            raise RuntimeError("The AI is still working in this chat — stop it or wait")
        await self.brain.delete_chat(int(request.match_info["id"]))
        return web.json_response({"ok": True})

    # ---- websockets ----------------------------------------------------------------------

    async def ws_events(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse(heartbeat=30)
        await ws.prepare(request)
        self.clients.add(ws)
        try:
            async for _ in ws:
                pass
        finally:
            self.clients.discard(ws)
        return ws

    async def ws_term(self, request: web.Request) -> web.WebSocketResponse:
        ws = web.WebSocketResponse(heartbeat=30, max_msg_size=4 * 1024 * 1024)
        await ws.prepare(request)
        server_id = int(request.query["server"])
        cols, rows = int(request.query.get("cols", 120)), int(request.query.get("rows", 32))
        try:
            proc = await self.ssh.open_shell(server_id, cols, rows)
        except (SSHError, Exception) as exc:
            await ws.send_json({"type": "error", "text": str(exc)})
            await ws.close()
            return ws
        await ws.send_json({"type": "ready"})
        server = self.db.get_server(server_id)
        if server and server.get("default_path"):
            proc.stdin.write(f"cd {json.dumps(server['default_path'])}\n".encode())

        async def pump() -> None:
            try:
                while True:
                    data = await proc.stdout.read(65536)
                    if not data:
                        break
                    await ws.send_bytes(data)
            except Exception:
                pass
            if not ws.closed:
                await ws.send_json({"type": "exit"})
                await ws.close()

        reader = asyncio.create_task(pump())
        try:
            async for msg in ws:
                if msg.type != WSMsgType.TEXT:
                    continue
                m = json.loads(msg.data)
                if m.get("type") == "in":
                    proc.stdin.write(m["data"].encode("utf-8"))
                elif m.get("type") == "resize":
                    proc.change_terminal_size(int(m["cols"]), int(m["rows"]))
        finally:
            reader.cancel()
            proc.close()
        return ws


def _target(body: dict) -> str | None:
    """The UI's label for who a message went to ("all servers", "web-1"), kept in the chat history."""
    return str(body.get("target") or "")[:200] or None
