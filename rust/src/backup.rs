//! Portable backup of the server list, for safekeeping or moving to another PC / Windows user.
//!
//! DPAPI secrets only open for this Windows user, so a copy of the database file loses every
//! password and key elsewhere. A backup instead holds the servers with their secrets in PLAIN form
//! (JSON) — treat the file like the keys themselves. Restoring re-encrypts the secrets with DPAPI and
//! skips servers already present (same host + port + user), so it never overwrites or removes anything.
//! The file format is the same as the Python app's, so backups move between both.

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use crate::db::{Database, EDITABLE_FIELDS, SECRET_FIELDS};
use crate::util::{now_iso, val_int};

pub const FORMAT: &str = "sshpro-backup";
pub const VERSION: i64 = 1;

pub fn export_backup(db: &Database) -> Result<Value> {
    let mut servers = Vec::new();
    for s in db.list_servers() {
        let public = serde_json::to_value(&s)?;
        let mut server = Map::new();
        for k in EDITABLE_FIELDS {
            server.insert(k.into(), public.get(k).cloned().unwrap_or(Value::Null));
        }
        server.insert("host_key".into(), json!(s.host_key));
        let secrets = db.get_secrets(s.id)?;
        for f in SECRET_FIELDS {
            server.insert(f.into(), json!(secrets.get(f)));
        }
        servers.push(Value::Object(server));
    }
    Ok(json!({"format": FORMAT, "version": VERSION, "app_version": env!("CARGO_PKG_VERSION"),
              "created_at": now_iso(), "servers": servers}))
}

pub fn import_backup(db: &Database, blob: Option<&Value>) -> Result<Value> {
    let Some(Value::Object(blob)) = blob else {
        bail!("not an SSH Pro AI backup file");
    };
    if blob.get("format").and_then(Value::as_str) != Some(FORMAT) {
        bail!("not an SSH Pro AI backup file");
    }
    if blob.get("version").and_then(Value::as_i64) != Some(VERSION) {
        bail!("unsupported backup version {}", blob.get("version").cloned().unwrap_or(Value::Null));
    }
    let Some(Value::Array(servers)) = blob.get("servers") else {
        bail!("backup file is damaged");
    };
    let filled = |s: &Map<String, Value>, k: &str| s.get(k).and_then(Value::as_str).is_some_and(|v| !v.is_empty());
    let servers: Vec<&Map<String, Value>> = servers.iter().filter_map(Value::as_object).collect();
    if servers.len() != blob["servers"].as_array().map_or(0, Vec::len)
        || !servers.iter().all(|s| filled(s, "host") && filled(s, "username"))
    {
        bail!("backup file is damaged");
    }
    let (mut imported, mut skipped) = (0, 0);
    for server in servers {
        let host = server["host"].as_str().unwrap_or_default();
        let username = server["username"].as_str().unwrap_or_default();
        let port = val_int(server.get("port")).filter(|p| *p != 0).unwrap_or(22);
        if db.find_server(host, port, username).is_some() {
            skipped += 1;
            continue;
        }
        let mut data = Map::new();
        for k in EDITABLE_FIELDS.iter().chain(SECRET_FIELDS.iter()) {
            data.insert((*k).into(), server.get(*k).cloned().unwrap_or(Value::Null));
        }
        let id = db.create_server(&data, Some("backup"), None)?;
        if let Some(key) = server.get("host_key").and_then(Value::as_str).filter(|k| !k.is_empty()) {
            db.set_host_key(id, Some(key));
        }
        imported += 1;
    }
    Ok(json!({"imported": imported, "skipped": skipped}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_import_skips_existing() {
        let a = Database::open_memory().unwrap();
        let data = json!({"name": "web", "host": "h", "username": "u", "password": "pw", "tags": "x"});
        let id = a.create_server(data.as_object().unwrap(), None, None).unwrap();
        a.set_host_key(id, Some("SHA256:k"));
        let blob = export_backup(&a).unwrap();
        assert_eq!(blob["servers"][0]["password"], "pw");

        let b = Database::open_memory().unwrap();
        assert_eq!(import_backup(&b, Some(&blob)).unwrap(), json!({"imported": 1, "skipped": 0}));
        assert_eq!(import_backup(&b, Some(&blob)).unwrap(), json!({"imported": 0, "skipped": 1}));
        let s = &b.list_servers()[0];
        assert_eq!(s.host_key.as_deref(), Some("SHA256:k"));
        assert_eq!(b.get_secrets(s.id).unwrap().password.as_deref(), Some("pw"));
        assert!(import_backup(&b, Some(&json!({"format": "x"}))).is_err());
    }
}
