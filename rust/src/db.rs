//! Local SQLite store: servers (secrets DPAPI-encrypted), the audit log and chat history.
//! Same schema and file as the Python app.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::crypto::{protect, unprotect};
use crate::util::{iso_days_ago, now_iso, tail_chars, truthy, val_int};

pub const SECRET_FIELDS: [&str; 4] = ["password", "private_key", "key_passphrase", "sudo_password"];
pub const EDITABLE_FIELDS: [&str; 10] =
    ["name", "host", "port", "username", "auth_type", "folder", "tags", "pinned", "notes", "default_path"];

const SCHEMA: &str = "
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
";

/// A server as the UI and the AI loop see it: no secrets, only whether each one is set.
#[derive(Debug, Clone, Serialize)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub folder: Option<String>,
    pub tags: Option<String>,
    pub pinned: bool,
    pub notes: Option<String>,
    pub default_path: Option<String>,
    pub host_key: Option<String>,
    pub facts: Option<Value>,
    pub last_status: Option<String>,
    pub last_seen: Option<String>,
    pub has_password: bool,
    pub has_private_key: bool,
    pub has_key_passphrase: bool,
    pub has_sudo_password: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Secrets {
    pub password: Option<String>,
    pub private_key: Option<String>,
    pub key_passphrase: Option<String>,
    pub sudo_password: Option<String>,
}

impl Secrets {
    pub fn get(&self, field: &str) -> Option<&String> {
        match field {
            "password" => self.password.as_ref(),
            "private_key" => self.private_key.as_ref(),
            "key_passphrase" => self.key_passphrase.as_ref(),
            "sudo_password" => self.sudo_password.as_ref(),
            _ => None,
        }
    }
}

fn server_from_row(row: &Row) -> rusqlite::Result<Server> {
    let facts: Option<String> = row.get("facts")?;
    let has = |field: &str| -> rusqlite::Result<bool> {
        Ok(row.get::<_, Option<String>>(field)?.is_some_and(|v| !v.is_empty()))
    };
    Ok(Server {
        id: row.get("id")?,
        name: row.get("name")?,
        host: row.get("host")?,
        port: row.get("port")?,
        username: row.get("username")?,
        auth_type: row.get("auth_type")?,
        folder: row.get("folder")?,
        tags: row.get("tags")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        notes: row.get("notes")?,
        default_path: row.get("default_path")?,
        host_key: row.get("host_key")?,
        facts: facts.and_then(|f| serde_json::from_str(&f).ok()),
        last_status: row.get("last_status")?,
        last_seen: row.get("last_seen")?,
        has_password: has("password")?,
        has_private_key: has("private_key")?,
        has_key_passphrase: has("key_passphrase")?,
        has_sudo_password: has("sudo_password")?,
    })
}

pub struct Database {
    conn: Mutex<Connection>,
}

/// A cleaned column value for INSERT / UPDATE.
#[derive(Debug, Clone)]
enum Col {
    Text(Option<String>),
    Int(i64),
}

impl rusqlite::ToSql for Col {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            Col::Text(t) => t.to_sql(),
            Col::Int(i) => i.to_sql(),
        }
    }
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        let has_favorite: bool = conn
            .prepare("PRAGMA table_info(chats)")?
            .query_map([], |r| r.get::<_, String>("name"))?
            .filter_map(|r| r.ok())
            .any(|n| n == "favorite");
        if !has_favorite {
            // favourite chats (never expire); added after 0.1
            conn.execute("ALTER TABLE chats ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0", [])?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        Self::open(Path::new(":memory:"))
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- servers -------------------------------------------------------------------------

    pub fn list_servers(&self) -> Vec<Server> {
        let conn = self.c();
        let mut stmt = conn
            .prepare("SELECT * FROM servers ORDER BY pinned DESC, COALESCE(folder, ''), name COLLATE NOCASE")
            .unwrap();
        stmt.query_map([], server_from_row).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_server(&self, id: i64) -> Option<Server> {
        self.c()
            .query_row("SELECT * FROM servers WHERE id = ?", [id], server_from_row)
            .optional()
            .ok()
            .flatten()
    }

    pub fn get_secrets(&self, id: i64) -> Result<Secrets> {
        let row: Option<[Option<String>; 4]> = self
            .c()
            .query_row(
                "SELECT password, private_key, key_passphrase, sudo_password FROM servers WHERE id = ?",
                [id],
                |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]),
            )
            .optional()?;
        let Some([password, private_key, key_passphrase, sudo_password]) = row else {
            bail!("server {id} not found");
        };
        Ok(Secrets {
            password: unprotect(password.as_deref())?,
            private_key: unprotect(private_key.as_deref())?,
            key_passphrase: unprotect(key_passphrase.as_deref())?,
            sudo_password: unprotect(sudo_password.as_deref())?,
        })
    }

    pub fn find_server(&self, host: &str, port: i64, username: &str) -> Option<i64> {
        self.c()
            .query_row(
                "SELECT id FROM servers WHERE host = ? AND port = ? AND username = ?",
                params![host, port, username],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn create_server(&self, data: &Map<String, Value>, source: Option<&str>, source_id: Option<i64>) -> Result<i64> {
        let mut values = clean(data, false)?;
        for field in SECRET_FIELDS {
            let secret = data.get(field).filter(|v| truthy(Some(v))).and_then(Value::as_str);
            values.push((field.to_string(), Col::Text(protect(secret)?)));
        }
        values.push(("source".into(), Col::Text(source.map(String::from))));
        values.push(("source_id".into(), source_id.map(Col::Int).unwrap_or(Col::Text(None))));
        values.push(("created_at".into(), Col::Text(Some(now_iso()))));
        values.push(("updated_at".into(), Col::Text(Some(now_iso()))));
        let columns: Vec<&str> = values.iter().map(|(k, _)| k.as_str()).collect();
        let marks = vec!["?"; values.len()].join(", ");
        let sql = format!("INSERT INTO servers ({}) VALUES ({marks})", columns.join(", "));
        let conn = self.c();
        conn.execute(&sql, rusqlite::params_from_iter(values.iter().map(|(_, v)| v)))?;
        Ok(conn.last_insert_rowid())
    }

    /// Secrets: a non-empty value replaces, `clear_<field>: true` removes, absent keeps.
    pub fn update_server(&self, id: i64, data: &Map<String, Value>) -> Result<()> {
        let mut values = clean(data, true)?;
        for field in SECRET_FIELDS {
            if truthy(data.get(&format!("clear_{field}"))) {
                values.push((field.to_string(), Col::Text(None)));
            } else if truthy(data.get(field)) {
                let secret = data.get(field).and_then(Value::as_str);
                values.push((field.to_string(), Col::Text(protect(secret)?)));
            }
        }
        if values.iter().any(|(k, _)| k == "host" || k == "port") {
            values.push(("host_key".into(), Col::Text(None))); // a different machine: trust its key afresh
        }
        values.push(("updated_at".into(), Col::Text(Some(now_iso()))));
        let assignments: Vec<String> = values.iter().map(|(k, _)| format!("{k} = ?")).collect();
        let sql = format!("UPDATE servers SET {} WHERE id = ?", assignments.join(", "));
        let mut args: Vec<Col> = values.into_iter().map(|(_, v)| v).collect();
        args.push(Col::Int(id));
        self.c().execute(&sql, rusqlite::params_from_iter(args.iter()))?;
        Ok(())
    }

    pub fn delete_server(&self, id: i64) {
        let _ = self.c().execute("DELETE FROM servers WHERE id = ?", [id]);
    }

    pub fn set_host_key(&self, id: i64, fingerprint: Option<&str>) {
        let _ = self.c().execute("UPDATE servers SET host_key = ? WHERE id = ?", params![fingerprint, id]);
    }

    pub fn set_status(&self, id: i64, status: &str, facts: Option<&Value>) {
        let conn = self.c();
        let _ = match facts {
            None => conn.execute("UPDATE servers SET last_status = ? WHERE id = ?", params![status, id]),
            Some(f) => conn.execute(
                "UPDATE servers SET last_status = ?, facts = ?, last_seen = ? WHERE id = ?",
                params![status, f.to_string(), now_iso(), id],
            ),
        };
    }

    // ---- audit ---------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn audit(&self, kind: &str, command: &str, server: Option<(Option<i64>, &str)>, prompt: Option<&str>,
                 exit_code: Option<i64>, duration: Option<f64>, output: &str) {
        let _ = self.c().execute(
            "INSERT INTO audit (ts, kind, prompt, server_id, server_name, command, exit_code, duration, output)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![now_iso(), kind, prompt, server.and_then(|s| s.0), server.map(|s| s.1), command,
                    exit_code, duration, tail_chars(output, 4000)],
        );
    }

    /// Delete audit rows older than `days`. Returns how many were removed.
    pub fn purge_audit(&self, days: i64) -> usize {
        self.c().execute("DELETE FROM audit WHERE ts < ?", [iso_days_ago(days)]).unwrap_or(0)
    }

    pub fn list_audit(&self, limit: i64) -> Vec<Value> {
        let conn = self.c();
        let mut stmt = conn.prepare("SELECT * FROM audit ORDER BY id DESC LIMIT ?").unwrap();
        stmt.query_map([limit], |r| {
            Ok(json!({
                "id": r.get::<_, i64>("id")?, "ts": r.get::<_, String>("ts")?, "kind": r.get::<_, String>("kind")?,
                "prompt": r.get::<_, Option<String>>("prompt")?, "server_id": r.get::<_, Option<i64>>("server_id")?,
                "server_name": r.get::<_, Option<String>>("server_name")?, "command": r.get::<_, String>("command")?,
                "exit_code": r.get::<_, Option<i64>>("exit_code")?, "duration": r.get::<_, Option<f64>>("duration")?,
                "output": r.get::<_, Option<String>>("output")?,
            }))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ---- chat history --------------------------------------------------------------------

    pub fn create_chat(&self, title: &str) -> i64 {
        let title: String = title.trim().chars().take(120).collect();
        let title = if title.is_empty() { "Chat".to_string() } else { title };
        let conn = self.c();
        let _ = conn.execute(
            "INSERT INTO chats (title, created_at, updated_at) VALUES (?, ?, ?)",
            params![title, now_iso(), now_iso()],
        );
        conn.last_insert_rowid()
    }

    pub fn add_chat_event(&self, chat_id: i64, event: &Value) {
        let conn = self.c();
        let _ = conn.execute(
            "INSERT INTO chat_events (chat_id, ts, event) VALUES (?, ?, ?)",
            params![chat_id, now_iso(), event.to_string()],
        );
        let _ = conn.execute("UPDATE chats SET updated_at = ? WHERE id = ?", params![now_iso(), chat_id]);
    }

    fn chat_row(r: &Row) -> rusqlite::Result<Map<String, Value>> {
        let mut m = Map::new();
        m.insert("id".into(), json!(r.get::<_, i64>("id")?));
        m.insert("title".into(), json!(r.get::<_, String>("title")?));
        m.insert("created_at".into(), json!(r.get::<_, String>("created_at")?));
        m.insert("updated_at".into(), json!(r.get::<_, String>("updated_at")?));
        m.insert("favorite".into(), json!(r.get::<_, i64>("favorite")? != 0));
        Ok(m)
    }

    pub fn get_chat(&self, chat_id: i64) -> Option<Value> {
        let conn = self.c();
        let mut chat = conn.query_row("SELECT * FROM chats WHERE id = ?", [chat_id], Self::chat_row).optional().ok()??;
        let mut stmt = conn.prepare("SELECT event FROM chat_events WHERE chat_id = ? ORDER BY id").unwrap();
        let events: Vec<Value> = stmt
            .query_map([chat_id], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|e| serde_json::from_str(&e).ok())
            .collect();
        chat.insert("events".into(), Value::Array(events));
        Some(Value::Object(chat))
    }

    /// Newest first, with the message count and the last request (its text, target label and the
    /// servers it went to) so a chat can be asked again straight from the list.
    pub fn list_chats(&self) -> Vec<Value> {
        let conn = self.c();
        let mut stmt = conn
            .prepare(
                "SELECT c.*, (SELECT COUNT(*) FROM chat_events e WHERE e.chat_id = c.id
                 AND json_extract(e.event, '$.type') = 'chat.user') AS messages,
                 (SELECT e.event FROM chat_events e WHERE e.chat_id = c.id
                 AND json_extract(e.event, '$.type') = 'chat.user' ORDER BY e.id DESC LIMIT 1) AS last
                 FROM chats c ORDER BY updated_at DESC, id DESC",
            )
            .unwrap();
        stmt.query_map([], |r| {
            let mut chat = Self::chat_row(r)?;
            chat.insert("messages".into(), json!(r.get::<_, i64>("messages")?));
            let last: Option<String> = r.get("last")?;
            let last: Value = last.and_then(|l| serde_json::from_str(&l).ok()).unwrap_or(Value::Null);
            let last = match last {
                Value::Object(o) if !o.is_empty() => {
                    let mut m = Map::new();
                    for k in ["text", "mode", "target", "scope"] {
                        m.insert(k.into(), o.get(k).cloned().unwrap_or(Value::Null));
                    }
                    Value::Object(m)
                }
                _ => Value::Null,
            };
            chat.insert("last".into(), last);
            Ok(Value::Object(chat))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn update_chat(&self, chat_id: i64, favorite: Option<bool>, title: Option<&str>) -> Result<()> {
        let conn = self.c();
        let exists = conn.query_row("SELECT 1 FROM chats WHERE id = ?", [chat_id], |_| Ok(())).optional()?;
        if exists.is_none() {
            bail!("chat not found");
        }
        if let Some(f) = favorite {
            conn.execute("UPDATE chats SET favorite = ? WHERE id = ?", params![f as i64, chat_id])?;
        }
        if let Some(t) = title {
            if t.trim().is_empty() {
                bail!("the name cannot be empty");
            }
            let t: String = t.trim().chars().take(120).collect();
            conn.execute("UPDATE chats SET title = ? WHERE id = ?", params![t, chat_id])?;
        }
        Ok(())
    }

    pub fn delete_chat(&self, chat_id: i64) {
        let conn = self.c();
        let _ = conn.execute("DELETE FROM chat_events WHERE chat_id = ?", [chat_id]);
        let _ = conn.execute("DELETE FROM chats WHERE id = ?", [chat_id]);
    }

    /// Delete chats with no activity for `days`, except favourites. Returns how many were removed.
    pub fn purge_chats(&self, days: i64) -> usize {
        let conn = self.c();
        let old: Vec<i64> = conn
            .prepare("SELECT id FROM chats WHERE updated_at < ? AND favorite = 0")
            .unwrap()
            .query_map([iso_days_ago(days)], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for id in &old {
            let _ = conn.execute("DELETE FROM chat_events WHERE chat_id = ?", [id]);
            let _ = conn.execute("DELETE FROM chats WHERE id = ?", [id]);
        }
        old.len()
    }
}

fn text_value(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        Some(Value::Bool(b)) => Some(if *b { "True" } else { "False" }.into()),
        Some(other) => Some(other.to_string()),
    }
}

fn clean(data: &Map<String, Value>, partial: bool) -> Result<Vec<(String, Col)>> {
    let mut values: Vec<(String, Col)> = Vec::new();
    let mut texts: std::collections::HashMap<&str, Option<String>> = Default::default();
    for field in EDITABLE_FIELDS {
        if partial && !data.contains_key(field) {
            continue;
        }
        let v = data.get(field);
        let col = match field {
            "port" => {
                let port = if truthy(v) { val_int(v).ok_or_else(|| anyhow!("invalid port"))? } else { 22 };
                Col::Int(if port == 0 { 22 } else { port })
            }
            "pinned" => Col::Int(truthy(v) as i64),
            "tags" => match v {
                Some(Value::Array(items)) => {
                    let tags: Vec<String> = items
                        .iter()
                        .filter_map(|t| text_value(Some(t)))
                        .collect();
                    Col::Text((!tags.is_empty()).then(|| tags.join(", ")))
                }
                other => Col::Text(text_value(other)),
            },
            "auth_type" => {
                let t = text_value(v);
                Col::Text(Some(match t.as_deref() {
                    Some("password") => "password".into(),
                    _ => "key".into(),
                }))
            }
            _ => Col::Text(text_value(v)),
        };
        if let Col::Text(t) = &col {
            texts.insert(field, t.clone());
        }
        values.push((field.to_string(), col));
    }
    for required in ["name", "host", "username"] {
        if let Some(value) = texts.get(required) {
            if value.is_none() {
                let host = texts.get("host").cloned().flatten();
                if required == "name" && host.is_some() {
                    for (k, v) in values.iter_mut() {
                        if k == "name" {
                            *v = Col::Text(host.clone());
                        }
                    }
                } else {
                    bail!("{required} is required");
                }
            }
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn servers_and_secrets() {
        let db = Database::open_memory().unwrap();
        let id = db
            .create_server(&obj(json!({"name": "", "host": "10.0.0.1", "port": "2222", "username": "deploy",
                                       "password": "pw", "tags": ["a", " b "], "pinned": true})), Some("manual"), None)
            .unwrap();
        let s = db.get_server(id).unwrap();
        assert_eq!(s.name, "10.0.0.1");
        assert_eq!(s.port, 2222);
        assert_eq!(s.tags.as_deref(), Some("a, b"));
        assert!(s.pinned && s.has_password && !s.has_private_key);
        assert_eq!(db.get_secrets(id).unwrap().password.as_deref(), Some("pw"));
        db.set_host_key(id, Some("SHA256:x"));
        db.update_server(id, &obj(json!({"port": 22, "clear_password": true}))).unwrap();
        let s = db.get_server(id).unwrap();
        assert!(s.host_key.is_none() && !s.has_password && s.port == 22);
        assert_eq!(db.find_server("10.0.0.1", 22, "deploy"), Some(id));
        assert!(db.create_server(&obj(json!({"host": "", "username": "x"})), None, None).is_err());
    }

    #[test]
    fn chats() {
        let db = Database::open_memory().unwrap();
        let chat = db.create_chat("  check disk  ");
        db.add_chat_event(chat, &json!({"type": "chat.user", "text": "check disk", "mode": "ai"}));
        db.add_chat_event(chat, &json!({"type": "ai.answer", "summary": "ok"}));
        let list = db.list_chats();
        assert_eq!(list[0]["title"], "check disk");
        assert_eq!(list[0]["messages"], 1);
        assert_eq!(list[0]["last"]["text"], "check disk");
        db.update_chat(chat, Some(true), Some("renamed")).unwrap();
        let got = db.get_chat(chat).unwrap();
        assert_eq!(got["favorite"], true);
        assert_eq!(got["events"].as_array().unwrap().len(), 2);
        assert!(db.update_chat(999, Some(true), None).is_err());
        db.delete_chat(chat);
        assert!(db.get_chat(chat).is_none());
    }
}
