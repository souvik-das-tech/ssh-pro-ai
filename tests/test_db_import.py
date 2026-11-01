import sqlite3

from sshpro.db import Database
from sshpro.importer import import_termix

KEY = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----\n"


def make_termix(path):
    c = sqlite3.connect(path)
    c.executescript("""
    CREATE TABLE ssh_data (id INTEGER PRIMARY KEY, user_id TEXT, name TEXT, ip TEXT, port INTEGER, username TEXT,
      folder TEXT, tags TEXT, pin INTEGER, auth_type TEXT, password TEXT, key TEXT, key_password TEXT, key_type TEXT,
      credential_id INTEGER, default_path TEXT);
    CREATE TABLE ssh_credentials (id INTEGER PRIMARY KEY, auth_type TEXT, password TEXT, key TEXT, private_key TEXT,
      key_password TEXT);
    """)
    c.execute("INSERT INTO ssh_data VALUES (1,'u','web','10.0.0.1',22,'root','Prod','[\"php\",\"web\"]',1,'key',NULL,?,NULL,'auto',NULL,'/var/www')", (KEY,))
    c.execute("INSERT INTO ssh_data VALUES (2,'u',NULL,'10.0.0.2',2222,'deploy',NULL,NULL,0,'password','pw',NULL,NULL,NULL,NULL,NULL)")
    c.commit()
    c.close()


def test_import_encrypts_and_is_idempotent(tmp_path):
    src = tmp_path / "ssh.sqlite"
    make_termix(src)
    db = Database(tmp_path / "local.sqlite3")
    assert import_termix(db, src) == {"imported": 2, "skipped": 0}
    assert import_termix(db, src) == {"imported": 0, "skipped": 2}

    servers = {s["host"]: s for s in db.list_servers()}
    web = servers["10.0.0.1"]
    assert web["name"] == "web" and web["tags"] == "php, web" and web["pinned"] and web["has_private_key"]
    assert "private_key" not in web and "password" not in web
    assert db.get_secrets(web["id"])["private_key"] == KEY
    raw = db.conn.execute("SELECT private_key, password FROM servers").fetchall()
    assert all(KEY not in (r[0] or "") and (r[1] or "") != "pw" for r in raw)  # encrypted at rest

    other = servers["10.0.0.2"]
    assert other["name"] == "10.0.0.2" and other["port"] == 2222 and other["auth_type"] == "password"
    assert db.get_secrets(other["id"])["password"] == "pw"


def test_update_keeps_secret_unless_given(tmp_path):
    db = Database(tmp_path / "local.sqlite3")
    sid = db.create_server({"host": "h", "username": "u", "auth_type": "password", "password": "one"})
    db.update_server(sid, {"name": "renamed"})
    assert db.get_secrets(sid)["password"] == "one"
    db.update_server(sid, {"password": "two", "sudo_password": "s"})
    assert db.get_secrets(sid)["password"] == "two" and db.get_secrets(sid)["sudo_password"] == "s"
    db.update_server(sid, {"clear_sudo_password": True})
    assert db.get_secrets(sid)["sudo_password"] is None


def test_purge_audit_keeps_last_five_days(tmp_path):
    db = Database(tmp_path / "local.sqlite3")
    db.audit("manual", "df -h")
    db.conn.execute("INSERT INTO audit (ts, kind, command) VALUES ('2000-01-01T00:00:00+00:00', 'ai', 'old')")
    db.conn.commit()
    assert db.purge_audit(5) == 1
    assert [r["command"] for r in db.list_audit()] == ["df -h"]


def test_purge_chats_drops_idle_ones(tmp_path):
    db = Database(tmp_path / "local.sqlite3")
    old, recent = db.create_chat("old one"), db.create_chat("recent one")
    db.add_chat_event(old, {"type": "chat.user", "text": "hi"})
    db.add_chat_event(recent, {"type": "chat.user", "text": "hello"})
    db.conn.execute("UPDATE chats SET updated_at = '2000-01-01T00:00:00+00:00' WHERE id = ?", (old,))
    db.conn.commit()
    assert db.purge_chats(15) == 1
    assert [c["title"] for c in db.list_chats()] == ["recent one"]
    assert db.get_chat(old) is None
    assert db.conn.execute("SELECT COUNT(*) FROM chat_events WHERE chat_id = ?", (old,)).fetchone()[0] == 0
