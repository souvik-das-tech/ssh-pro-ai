"""Portable backup of the server list, for safekeeping or moving to another PC / Windows user.

DPAPI secrets only open for this Windows user, so a copy of the database file loses every
password and key elsewhere. A backup instead holds the servers with their secrets in PLAIN form
(JSON) — treat the file like the keys themselves. Restoring re-encrypts the secrets with DPAPI and
skips servers already present (same host + port + user), so it never overwrites or removes anything.
"""

from __future__ import annotations

from typing import Any

from sshpro import __version__
from sshpro.db import EDITABLE_FIELDS, SECRET_FIELDS, Database, now

FORMAT = "sshpro-backup"
VERSION = 1


def export_backup(db: Database) -> dict[str, Any]:
    servers = []
    for public in db.list_servers():
        server = {k: public[k] for k in EDITABLE_FIELDS}
        server["host_key"] = public["host_key"]
        server.update(db.get_secrets(public["id"]))
        servers.append(server)
    return {"format": FORMAT, "version": VERSION, "app_version": __version__, "created_at": now(),
            "servers": servers}


def import_backup(db: Database, blob: dict[str, Any]) -> dict[str, int]:
    if not isinstance(blob, dict) or blob.get("format") != FORMAT:
        raise ValueError("not an SSH Pro AI backup file")
    if blob.get("version") != VERSION:
        raise ValueError(f"unsupported backup version {blob.get('version')}")
    servers = blob.get("servers")
    if not isinstance(servers, list) or not all(isinstance(s, dict) and s.get("host") and s.get("username")
                                                for s in servers):
        raise ValueError("backup file is damaged")
    imported = skipped = 0
    for server in servers:
        host, port, username = server["host"], int(server.get("port") or 22), server["username"]
        if db.find_server(host, port, username) is not None:
            skipped += 1
            continue
        data = {k: server.get(k) for k in (*EDITABLE_FIELDS, *SECRET_FIELDS)}
        server_id = db.create_server(data, source="backup")
        if server.get("host_key"):
            db.set_host_key(server_id, server["host_key"])
        imported += 1
    return {"imported": imported, "skipped": skipped}
