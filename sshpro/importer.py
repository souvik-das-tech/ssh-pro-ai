"""Import hosts from a Termix database (`ssh_data` + `ssh_credentials` tables).

Termix keeps keys and passwords in plain text; they are re-encrypted with DPAPI on import.
Hosts already present (same host + port + user) are skipped, so re-importing is safe.
"""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

from sshpro.db import Database


def _tags(raw: str | None) -> str | None:
    if not raw:
        return None
    raw = raw.strip()
    if raw.startswith("["):
        try:
            return ", ".join(str(t) for t in json.loads(raw)) or None
        except json.JSONDecodeError:
            pass
    return raw or None


def import_termix(db: Database, path: Path | str) -> dict[str, int]:
    path = Path(path)
    if not path.is_file():
        raise FileNotFoundError(f"Termix database not found: {path}")
    src = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    src.row_factory = sqlite3.Row
    try:
        credentials = {r["id"]: r for r in src.execute("SELECT * FROM ssh_credentials")}
        imported = skipped = 0
        for row in src.execute("SELECT * FROM ssh_data ORDER BY id"):
            host, port, username = row["ip"].strip(), int(row["port"] or 22), row["username"].strip()
            if db.find_server(host, port, username) is not None:
                skipped += 1
                continue
            password, key, passphrase = row["password"], row["key"], row["key_password"]
            auth_type = row["auth_type"]
            cred = credentials.get(row["credential_id"]) if row["credential_id"] else None
            if cred is not None and not (password or key):
                auth_type = cred["auth_type"]
                password = cred["password"]
                key = cred["private_key"] or cred["key"]
                passphrase = cred["key_password"]
            db.create_server(
                {
                    "name": row["name"] or host,
                    "host": host,
                    "port": port,
                    "username": username,
                    "auth_type": "password" if auth_type == "password" else "key",
                    "password": password,
                    "private_key": key,
                    "key_passphrase": passphrase,
                    "folder": row["folder"],
                    "tags": _tags(row["tags"]),
                    "pinned": bool(row["pin"]),
                    "default_path": row["default_path"],
                },
                source="termix",
                source_id=row["id"],
            )
            imported += 1
        return {"imported": imported, "skipped": skipped}
    finally:
        src.close()
