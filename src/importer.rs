//! Import hosts from a Termix database (`ssh_data` + `ssh_credentials` tables).
//!
//! Termix keeps keys and passwords in plain text; they are re-encrypted with DPAPI on import.
//! Hosts already present (same host + port + user) are skipped, so re-importing is safe.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Result};
use rusqlite::{Connection, OpenFlags, Row};
use serde_json::{json, Map, Value};

use crate::db::Database;

fn tags(raw: Option<String>) -> Option<String> {
    let raw = raw?.trim().to_string();
    if raw.starts_with('[') {
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) {
            let joined = items
                .iter()
                .map(|t| t.as_str().map(String::from).unwrap_or_else(|| t.to_string()))
                .collect::<Vec<_>>()
                .join(", ");
            return (!joined.is_empty()).then_some(joined);
        }
    }
    (!raw.is_empty()).then_some(raw)
}

/// A column as text, whatever its SQLite type (Termix stores some numbers as text).
fn text(row: &Row, col: &str) -> Option<String> {
    use rusqlite::types::ValueRef;
    match row.get_ref(col).ok()? {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) | ValueRef::Blob(t) => Some(String::from_utf8_lossy(t).into_owned()),
    }
}

fn int(row: &Row, col: &str) -> Option<i64> {
    text(row, col).and_then(|t| t.trim().parse::<f64>().ok()).map(|f| f as i64)
}

fn present(v: &Option<String>) -> bool {
    v.as_ref().is_some_and(|s| !s.is_empty())
}

pub fn import_termix(db: &Database, path: &Path) -> Result<Value> {
    if !path.is_file() {
        bail!("Termix database not found: {}", path.display());
    }
    let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut credentials: HashMap<i64, HashMap<&'static str, Option<String>>> = HashMap::new();
    {
        let mut stmt = src.prepare("SELECT * FROM ssh_credentials")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let mut c = HashMap::new();
            for k in ["auth_type", "password", "private_key", "key", "key_password"] {
                c.insert(k, text(r, k));
            }
            if let Some(id) = int(r, "id") {
                credentials.insert(id, c);
            }
        }
    }
    let (mut imported, mut skipped) = (0, 0);
    let mut stmt = src.prepare("SELECT * FROM ssh_data ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let host = text(row, "ip").unwrap_or_default().trim().to_string();
        let port = int(row, "port").filter(|p| *p != 0).unwrap_or(22);
        let username = text(row, "username").unwrap_or_default().trim().to_string();
        if db.find_server(&host, port, &username).is_some() {
            skipped += 1;
            continue;
        }
        let (mut password, mut key, mut passphrase) = (text(row, "password"), text(row, "key"), text(row, "key_password"));
        let mut auth_type = text(row, "auth_type");
        let cred = int(row, "credential_id").filter(|c| *c != 0).and_then(|c| credentials.get(&c));
        if let Some(cred) = cred {
            if !(present(&password) || present(&key)) {
                auth_type = cred["auth_type"].clone();
                password = cred["password"].clone();
                key = cred["private_key"].clone().filter(|k| !k.is_empty()).or_else(|| cred["key"].clone());
                passphrase = cred["key_password"].clone();
            }
        }
        let name = text(row, "name").filter(|n| !n.is_empty()).unwrap_or_else(|| host.clone());
        let data: Map<String, Value> = json!({
            "name": name, "host": host, "port": port, "username": username,
            "auth_type": if auth_type.as_deref() == Some("password") { "password" } else { "key" },
            "password": password, "private_key": key, "key_passphrase": passphrase,
            "folder": text(row, "folder"), "tags": tags(text(row, "tags")),
            "pinned": int(row, "pin").unwrap_or(0) != 0, "default_path": text(row, "default_path"),
        })
        .as_object()
        .unwrap()
        .clone();
        db.create_server(&data, Some("termix"), int(row, "id"))?;
        imported += 1;
    }
    Ok(json!({"imported": imported, "skipped": skipped}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_termix_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh.sqlite");
        let src = Connection::open(&path).unwrap();
        src.execute_batch(
            "CREATE TABLE ssh_credentials (id INTEGER, auth_type TEXT, password TEXT, private_key TEXT, key TEXT, key_password TEXT);
             CREATE TABLE ssh_data (id INTEGER, name TEXT, ip TEXT, port INTEGER, username TEXT, auth_type TEXT,
                 password TEXT, key TEXT, key_password TEXT, credential_id INTEGER, folder TEXT, tags TEXT, pin INTEGER,
                 default_path TEXT);
             INSERT INTO ssh_credentials VALUES (7, 'password', 'credpw', NULL, NULL, NULL);
             INSERT INTO ssh_data VALUES (1, 'web', ' 10.0.0.1 ', 22, 'root', 'key', NULL, NULL, NULL, 7, 'prod', '[\"a\",\"b\"]', 1, '/var/www');",
        )
        .unwrap();
        drop(src);
        let db = Database::open_memory().unwrap();
        assert_eq!(import_termix(&db, &path).unwrap(), json!({"imported": 1, "skipped": 0}));
        assert_eq!(import_termix(&db, &path).unwrap(), json!({"imported": 0, "skipped": 1}));
        let s = &db.list_servers()[0];
        assert_eq!((s.host.as_str(), s.auth_type.as_str(), s.tags.as_deref()), ("10.0.0.1", "password", Some("a, b")));
        assert_eq!(db.get_secrets(s.id).unwrap().password.as_deref(), Some("credpw"));
    }
}
