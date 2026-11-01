import json

import pytest

from sshpro.backup import export_backup, import_backup
from sshpro.db import Database

KEY = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n"


def make_db(path):
    db = Database(path)
    a = db.create_server({"name": "web", "host": "10.0.0.1", "username": "root", "auth_type": "key",
                          "private_key": KEY, "key_passphrase": "kp", "sudo_password": "sp", "folder": "Prod",
                          "tags": "php, web", "pinned": True, "notes": "main box", "default_path": "/var/www"})
    db.set_host_key(a, "SHA256:abc")
    db.create_server({"host": "10.0.0.2", "port": 2222, "username": "deploy", "auth_type": "password",
                      "password": "pw"})
    return db


def test_round_trip_keeps_everything(tmp_path):
    src = make_db(tmp_path / "a.sqlite3")
    blob = json.loads(json.dumps(export_backup(src)))  # as written to / read from a file
    assert len(blob["servers"]) == 2

    dst = Database(tmp_path / "b.sqlite3")
    assert import_backup(dst, blob) == {"imported": 2, "skipped": 0}
    servers = {s["host"]: s for s in dst.list_servers()}
    web = servers["10.0.0.1"]
    assert (web["name"], web["folder"], web["tags"], web["pinned"], web["notes"], web["default_path"], web["host_key"]) \
        == ("web", "Prod", "php, web", True, "main box", "/var/www", "SHA256:abc")
    assert dst.get_secrets(web["id"]) == {"password": None, "private_key": KEY, "key_passphrase": "kp",
                                          "sudo_password": "sp"}
    other = servers["10.0.0.2"]
    assert other["port"] == 2222 and other["auth_type"] == "password"
    assert dst.get_secrets(other["id"])["password"] == "pw"


def test_restore_never_overwrites(tmp_path):
    src = make_db(tmp_path / "a.sqlite3")
    blob = export_backup(src)
    dst = Database(tmp_path / "b.sqlite3")
    sid = dst.create_server({"name": "mine", "host": "10.0.0.1", "username": "root", "password": "local"})
    assert import_backup(dst, blob) == {"imported": 1, "skipped": 1}
    assert dst.get_server(sid)["name"] == "mine" and dst.get_secrets(sid)["password"] == "local"
    assert import_backup(dst, blob) == {"imported": 0, "skipped": 2}


def test_bad_file_is_rejected(tmp_path):
    dst = Database(tmp_path / "b.sqlite3")
    with pytest.raises(ValueError, match="not an SSH Pro AI backup"):
        import_backup(dst, {"hello": 1})
    with pytest.raises(ValueError, match="damaged"):
        import_backup(dst, {"format": "sshpro-backup", "version": 1, "servers": [{"name": "x"}]})
    assert dst.list_servers() == []
