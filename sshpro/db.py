"""Local SQLite store: servers (secrets DPAPI-encrypted), the audit log and chat history.

Used only from the backend's asyncio thread, so one connection is enough.
"""

from __future__ import annotations

import json
import sqlite3
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from sshpro.crypto import protect, unprotect

SECRET_FIELDS = ("password", "private_key", "key_passphrase", "sudo_password")
PUBLIC_FIELDS = (
    "id", "name", "host", "port", "username", "auth_type", "folder", "tags", "pinned",
    "notes", "default_path", "host_key", "facts", "last_status", "last_seen",
)
EDITABLE_FIELDS = ("name", "host", "port", "username", "auth_type", "folder", "tags", "pinned",
                   "notes", "default_path")

_SCHEMA = """
CREATE TABLE IF NOT EXISTS servers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL DEFAULT 'key',
    password TEXT,
    private_key TEXT,
    key_passphrase TEXT,
    sudo_password TEXT,
    folder TEXT,
    tags TEXT,
    pinned INTEGER NOT NULL DEFAULT 0,
    notes TEXT,
    default_path TEXT,
    host_key TEXT,
    facts TEXT,
    last_status TEXT,
    last_seen TEXT,
    source TEXT,
    source_id INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    prompt TEXT,
    server_id INTEGER,
    server_name TEXT,
    command TEXT NOT NULL,
    exit_code INTEGER,
    duration REAL,
    output TEXT
);
CREATE INDEX IF NOT EXISTS audit_ts ON audit(ts);
CREATE TABLE IF NOT EXISTS chats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chat_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id INTEGER NOT NULL,
    ts TEXT NOT NULL,
    event TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS chat_events_chat ON chat_events(chat_id, id);
"""


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


class Database:
    def __init__(self, path: Path | str):
        self.conn = sqlite3.connect(str(path), check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self.conn.executescript(_SCHEMA)
        self.conn.commit()

    def close(self) -> None:
        self.conn.close()

    # ---- servers -------------------------------------------------------------------------

    @staticmethod
    def _public(row: sqlite3.Row) -> dict[str, Any]:
        server = {k: row[k] for k in PUBLIC_FIELDS}
        server["pinned"] = bool(server["pinned"])
        server["facts"] = json.loads(server["facts"]) if server["facts"] else None
        for field in SECRET_FIELDS:
            server[f"has_{field}"] = bool(row[field])
        return server

    def list_servers(self) -> list[dict[str, Any]]:
        rows = self.conn.execute(
            "SELECT * FROM servers ORDER BY pinned DESC, COALESCE(folder, ''), name COLLATE NOCASE"
        )
        return [self._public(r) for r in rows]

    def get_server(self, server_id: int) -> dict[str, Any] | None:
        row = self.conn.execute("SELECT * FROM servers WHERE id = ?", (server_id,)).fetchone()
        return self._public(row) if row else None

    def get_secrets(self, server_id: int) -> dict[str, str | None]:
        row = self.conn.execute(
            f"SELECT {', '.join(SECRET_FIELDS)} FROM servers WHERE id = ?", (server_id,)
        ).fetchone()
        if row is None:
            raise KeyError(server_id)
        return {field: unprotect(row[field]) for field in SECRET_FIELDS}

    def find_server(self, host: str, port: int, username: str) -> int | None:
        row = self.conn.execute(
            "SELECT id FROM servers WHERE host = ? AND port = ? AND username = ?", (host, port, username)
        ).fetchone()
        return row["id"] if row else None

    def create_server(self, data: dict[str, Any], source: str | None = None,
                      source_id: int | None = None) -> int:
        values = _clean(data)
        for field in SECRET_FIELDS:
            values[field] = protect(data.get(field) or None)
        values.update(source=source, source_id=source_id, created_at=now(), updated_at=now())
        columns = ", ".join(values)
        marks = ", ".join("?" for _ in values)
        cur = self.conn.execute(f"INSERT INTO servers ({columns}) VALUES ({marks})", tuple(values.values()))
        self.conn.commit()
        return int(cur.lastrowid)

    def update_server(self, server_id: int, data: dict[str, Any]) -> None:
        """Secrets: a non-empty value replaces, `clear_<field>: true` removes, absent keeps."""
        values = _clean(data, partial=True)
        for field in SECRET_FIELDS:
            if data.get(f"clear_{field}"):
                values[field] = None
            elif data.get(field):
                values[field] = protect(data[field])
        if "host" in values or "port" in values:
            values["host_key"] = None  # a different machine: trust its key afresh
        values["updated_at"] = now()
        assignments = ", ".join(f"{k} = ?" for k in values)
        self.conn.execute(f"UPDATE servers SET {assignments} WHERE id = ?", (*values.values(), server_id))
        self.conn.commit()

    def delete_server(self, server_id: int) -> None:
        self.conn.execute("DELETE FROM servers WHERE id = ?", (server_id,))
        self.conn.commit()

    def set_host_key(self, server_id: int, fingerprint: str) -> None:
        self.conn.execute("UPDATE servers SET host_key = ? WHERE id = ?", (fingerprint, server_id))
        self.conn.commit()

    def set_status(self, server_id: int, status: str, facts: dict | None = None) -> None:
        if facts is None:
            self.conn.execute("UPDATE servers SET last_status = ? WHERE id = ?", (status, server_id))
        else:
            self.conn.execute(
                "UPDATE servers SET last_status = ?, facts = ?, last_seen = ? WHERE id = ?",
                (status, json.dumps(facts), now(), server_id),
            )
        self.conn.commit()

    # ---- audit ---------------------------------------------------------------------------

    def audit(self, kind: str, command: str, server: dict | None = None, prompt: str | None = None,
              exit_code: int | None = None, duration: float | None = None, output: str | None = None) -> None:
        self.conn.execute(
            "INSERT INTO audit (ts, kind, prompt, server_id, server_name, command, exit_code, duration, output)"
            " VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (now(), kind, prompt, server and server["id"], server and server["name"], command,
             exit_code, duration, (output or "")[-4000:]),
        )
        self.conn.commit()

    def purge_audit(self, days: int = 5) -> int:
        """Delete audit rows older than `days`. Returns how many were removed."""
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat(timespec="seconds")
        cur = self.conn.execute("DELETE FROM audit WHERE ts < ?", (cutoff,))
        self.conn.commit()
        return cur.rowcount

    def list_audit(self, limit: int = 200) -> list[dict[str, Any]]:
        rows = self.conn.execute("SELECT * FROM audit ORDER BY id DESC LIMIT ?", (limit,))
        return [dict(r) for r in rows]

    # ---- chat history --------------------------------------------------------------------

    def create_chat(self, title: str) -> int:
        cur = self.conn.execute("INSERT INTO chats (title, created_at, updated_at) VALUES (?, ?, ?)",
                                (title.strip()[:120] or "Chat", now(), now()))
        self.conn.commit()
        return int(cur.lastrowid)

    def add_chat_event(self, chat_id: int, event: dict[str, Any]) -> None:
        self.conn.execute("INSERT INTO chat_events (chat_id, ts, event) VALUES (?, ?, ?)",
                          (chat_id, now(), json.dumps(event, ensure_ascii=False)))
        self.conn.execute("UPDATE chats SET updated_at = ? WHERE id = ?", (now(), chat_id))
        self.conn.commit()

    def get_chat(self, chat_id: int) -> dict[str, Any] | None:
        row = self.conn.execute("SELECT * FROM chats WHERE id = ?", (chat_id,)).fetchone()
        if row is None:
            return None
        events = self.conn.execute("SELECT event FROM chat_events WHERE chat_id = ? ORDER BY id", (chat_id,))
        return {**dict(row), "events": [json.loads(r["event"]) for r in events]}

    def list_chats(self) -> list[dict[str, Any]]:
        rows = self.conn.execute(
            "SELECT c.*, (SELECT COUNT(*) FROM chat_events e WHERE e.chat_id = c.id"
            " AND json_extract(e.event, '$.type') = 'chat.user') AS messages"
            " FROM chats c ORDER BY updated_at DESC, id DESC"
        )
        return [dict(r) for r in rows]

    def delete_chat(self, chat_id: int) -> None:
        self.conn.execute("DELETE FROM chat_events WHERE chat_id = ?", (chat_id,))
        self.conn.execute("DELETE FROM chats WHERE id = ?", (chat_id,))
        self.conn.commit()

    def purge_chats(self, days: int = 15) -> int:
        """Delete chats with no activity for `days`. Returns how many were removed."""
        cutoff = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat(timespec="seconds")
        old = [r["id"] for r in self.conn.execute("SELECT id FROM chats WHERE updated_at < ?", (cutoff,))]
        for chat_id in old:
            self.conn.execute("DELETE FROM chat_events WHERE chat_id = ?", (chat_id,))
        self.conn.execute("DELETE FROM chats WHERE updated_at < ?", (cutoff,))
        self.conn.commit()
        return len(old)


def _clean(data: dict[str, Any], partial: bool = False) -> dict[str, Any]:
    values: dict[str, Any] = {}
    for field in EDITABLE_FIELDS:
        if partial and field not in data:
            continue
        value = data.get(field)
        if isinstance(value, str):
            value = value.strip() or None
        values[field] = value
    if "tags" in values and isinstance(values["tags"], list):
        values["tags"] = ", ".join(t.strip() for t in values["tags"] if t.strip()) or None
    if "port" in values:
        values["port"] = int(values["port"] or 22)
    if "pinned" in values:
        values["pinned"] = 1 if values["pinned"] else 0
    if "auth_type" in values and values["auth_type"] not in ("key", "password"):
        values["auth_type"] = "key"
    for required in ("name", "host", "username"):
        if required in values and not values[required]:
            if required == "name" and values.get("host"):
                values["name"] = values["host"]
            else:
                raise ValueError(f"{required} is required")
    return values
