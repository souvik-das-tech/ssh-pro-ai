//! SSH engine: one cached russh connection per server, shared by commands and terminals.
//!
//! Host keys are trust-on-first-use: the first fingerprint seen is stored; a later mismatch refuses
//! the connection *before* any password is sent.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh::client::{self, Handle, Handler, Msg};
use russh::keys::{decode_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg, Disconnect, Sig};
use russh_sftp::client::SftpSession;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::db::{Database, Secrets, Server};
use crate::safety::{prepare, sudo_count};
use crate::shlex::quote as q;
use crate::util::{expand, human_size, posix_basename, posix_join, posix_split, round_to, ExecResult, MAX_OUTPUT};

pub const PROBE: &str = concat!(
    r#". /etc/os-release 2>/dev/null; echo "os=${PRETTY_NAME:-$(uname -s)}"; "#,
    r#"echo "kernel=$(uname -r)"; echo "uptime=$(cut -d. -f1 /proc/uptime 2>/dev/null)"; "#,
    r#"echo "load=$(cut -d" " -f1-3 /proc/loadavg 2>/dev/null)"; echo "cpus=$(nproc 2>/dev/null)"; "#,
    r#"free -m 2>/dev/null | awk '/^Mem:/{print "mem="$2" "$7}'; "#,
    r#"df -Pk / 2>/dev/null | awk 'NR==2{print "disk="$2" "$4}'"#,
);

const CHUNK: usize = 256 * 1024; // local file reads per SFTP write
const COPY_MARGIN: u64 = 64 * 1024 * 1024; // free space a copy must leave on the destination

const HAS_TAR: &str = "command -v tar >/dev/null 2>&1 && command -v gzip >/dev/null 2>&1";
/// `sudo` is dropped for root and gets the stored sudo password otherwise (safety::prepare).
const INSTALL_TAR: &str = concat!(
    "if command -v apt-get >/dev/null 2>&1; then",
    " sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tar gzip",
    " || { sudo apt-get update -qq && sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq tar gzip; };",
    " elif command -v dnf >/dev/null 2>&1; then sudo dnf install -y -q tar gzip;",
    " elif command -v yum >/dev/null 2>&1; then sudo yum install -y -q tar gzip;",
    " elif command -v apk >/dev/null 2>&1; then sudo apk add -q tar gzip;",
    " elif command -v zypper >/dev/null 2>&1; then sudo zypper -n -q install tar gzip;",
    " else echo 'no known package manager' >&2; exit 3; fi",
);

/// SFTP progress: (source path, destination path, bytes done, bytes total).
pub type Progress = Arc<dyn Fn(&str, &str, u64, u64) + Send + Sync>;
/// Server-to-server progress: (bytes sent, seconds elapsed).
pub type PipeProgress = Arc<dyn Fn(u64, f64) + Send + Sync>;

pub type Conn = Arc<Handle<Client>>;
pub type ShellChannel = Channel<Msg>;

/// Checks the host key against the saved fingerprint (trust on first use).
pub struct Client {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
}

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let fingerprint = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key().fingerprint(HashAlg::Sha256).to_string(),
        };
        *self.seen.lock().unwrap() = Some(fingerprint.clone());
        Ok(self.expected.as_deref().is_none_or(|e| e == fingerprint))
    }
}

/// What one connection attempt found out about the server's host key.
#[derive(Clone, Copy, PartialEq)]
enum KeyCheck {
    NotSeen,  // failed before the key exchange (unreachable, no common key type)
    Mismatch, // a key that does not match the saved fingerprint (refused before logging in)
    Matched,  // the saved key, or the first key of a new server
}

enum ConnectError {
    Ssh(russh::Error),
    AuthFailed,
    Key(String),
}

impl From<russh::Error> for ConnectError {
    fn from(e: russh::Error) -> Self {
        ConnectError::Ssh(e)
    }
}

fn friendly_io(e: &std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match (e.kind(), e.raw_os_error()) {
        (_, Some(11001)) | (_, Some(11004)) => "Host name not found.".into(),
        (ConnectionRefused, _) | (_, Some(10061)) => "Connection refused (is SSH running on that port?).".into(),
        (TimedOut, _) | (_, Some(10060)) => "Connection timed out.".into(),
        (ConnectionReset, _) | (ConnectionAborted, _) | (BrokenPipe, _) | (UnexpectedEof, _) => {
            format!("Connection lost: {e}")
        }
        _ => format!("OSError: {e}"),
    }
}

/// A connection problem, phrased for the user.
pub fn friendly(e: &russh::Error) -> String {
    match e {
        russh::Error::IO(io) => friendly_io(io),
        russh::Error::ConnectionTimeout | russh::Error::KeepaliveTimeout | russh::Error::InactivityTimeout => {
            "Connection timed out.".into()
        }
        russh::Error::NotAuthenticated => "Authentication failed (wrong key / password or user).".into(),
        russh::Error::Disconnect | russh::Error::HUP | russh::Error::SendError | russh::Error::RecvError => {
            "Connection lost.".into()
        }
        other => format!("SSHError: {other}"),
    }
}

fn friendly_connect(e: ConnectError, expected: &Option<String>, seen: &Option<String>) -> String {
    if let (Some(exp), Some(got)) = (expected, seen) {
        if exp != got {
            return format!(
                "HOST KEY CHANGED — the server's identity does not match the saved one (saved {exp}, got {got}). \
                 Not connecting. If the server was reinstalled, use 'Forget host key' in Edit."
            );
        }
    }
    match e {
        ConnectError::AuthFailed => "Authentication failed (wrong key / password or user).".into(),
        ConnectError::Key(msg) => format!("Private key could not be read: {msg}"),
        ConnectError::Ssh(e) => friendly(&e),
    }
}

pub fn parse_facts(text: &str) -> Value {
    let raw: HashMap<&str, &str> = text.lines().filter_map(|l| l.split_once('=')).collect();
    let get = |k: &str| raw.get(k).copied().unwrap_or("");
    let non_empty = |k: &str| (!get(k).is_empty()).then(|| get(k).to_string());
    let mut facts = Map::new();
    facts.insert("os".into(), json!(non_empty("os")));
    facts.insert("kernel".into(), json!(non_empty("kernel")));
    let uptime = get("uptime");
    facts.insert("uptime".into(), if uptime.is_empty() { json!(0) } else { json!(uptime.trim().parse::<i64>().ok()) });
    let load: Vec<&str> = get("load").split_whitespace().collect();
    let load_vals: Option<Vec<f64>> = load.iter().map(|x| x.parse().ok()).collect();
    facts.insert("load".into(), if load.len() == 3 { json!(load_vals) } else { Value::Null });
    let cpus = get("cpus");
    facts.insert(
        "cpus".into(),
        if !cpus.is_empty() && cpus.chars().all(|c| c.is_ascii_digit()) { json!(cpus.parse::<i64>().ok()) } else { Value::Null },
    );
    let digits = |v: &[&str]| v.len() == 2 && v.iter().all(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit()));
    let mem: Vec<&str> = get("mem").split_whitespace().collect();
    if digits(&mem) {
        let (total, avail): (f64, f64) = (mem[0].parse().unwrap(), mem[1].parse().unwrap());
        let used = if total > 0.0 { round_to(100.0 * (total - avail) / total, 1) } else { 0.0 };
        facts.insert("mem".into(), json!({"total_mb": total as i64, "used_pct": used}));
    }
    let disk: Vec<&str> = get("disk").split_whitespace().collect();
    if digits(&disk) {
        let (total, avail): (f64, f64) = (disk[0].parse().unwrap(), disk[1].parse().unwrap());
        let used = if total > 0.0 { round_to(100.0 * (total - avail) / total, 1) } else { 0.0 };
        facts.insert(
            "disk".into(),
            json!({"total_gb": round_to(total / 1048576.0, 1), "free_gb": round_to(avail / 1048576.0, 1), "used_pct": used}),
        );
    }
    Value::Object(facts)
}

fn signal_number(sig: &Sig) -> i64 {
    match sig {
        Sig::HUP => 1,
        Sig::INT => 2,
        Sig::QUIT => 3,
        Sig::ILL => 4,
        Sig::ABRT => 6,
        Sig::FPE => 8,
        Sig::KILL => 9,
        Sig::USR1 => 10,
        Sig::SEGV => 11,
        Sig::PIPE => 13,
        Sig::ALRM => 14,
        Sig::TERM => 15,
        _ => 0,
    }
}

/// Keeps only the last bytes of a growing output buffer.
fn push_bounded(buf: &mut Vec<u8>, data: &[u8]) {
    buf.extend_from_slice(data);
    if buf.len() > 8 * MAX_OUTPUT {
        let cut = buf.len() - 4 * MAX_OUTPUT;
        buf.drain(..cut);
    }
}

fn text(buf: &[u8]) -> String {
    String::from_utf8_lossy(buf).into_owned()
}

enum ExecError {
    /// The cached connection is gone (the server rebooted, the network dropped): reconnect.
    Lost(russh::Error),
    Timeout { stdout: Vec<u8>, stderr: Vec<u8> },
}

struct Output {
    exit: Option<i64>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    lost: bool,
}

/// Run one command on an open connection.
async fn exec_on(conn: &Conn, line: &str, stdin: Option<&str>, timeout: f64) -> Result<Output, ExecError> {
    let mut ch = conn.channel_open_session().await.map_err(ExecError::Lost)?;
    ch.exec(true, line.as_bytes()).await.map_err(ExecError::Lost)?;
    if let Some(input) = stdin {
        let _ = ch.data(input.as_bytes()).await;
    }
    let _ = ch.eof().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs_f64(timeout.max(1.0));
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let (mut status, mut signal, mut finished) = (None, None, false);
    loop {
        match tokio::time::timeout_at(deadline, ch.wait()).await {
            Err(_) => {
                let _ = ch.close().await;
                return Err(ExecError::Timeout { stdout, stderr });
            }
            Ok(None) => break,
            Ok(Some(msg)) => match msg {
                ChannelMsg::Data { data } => push_bounded(&mut stdout, &data),
                ChannelMsg::ExtendedData { data, .. } => push_bounded(&mut stderr, &data),
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status as i64),
                ChannelMsg::ExitSignal { signal_name, .. } => signal = Some(-signal_number(&signal_name)),
                ChannelMsg::Eof | ChannelMsg::Close => finished = true,
                _ => {}
            },
        }
    }
    let exit = status.or(signal);
    Ok(Output { exit, stdout, stderr, lost: exit.is_none() && !finished })
}

/// An SFTP session on its own channel of the connection.
async fn open_sftp(conn: &Conn) -> Result<SftpSession, russh::Error> {
    let ch = conn.channel_open_session().await?;
    ch.request_subsystem(true, "sftp").await?;
    SftpSession::new(ch.into_stream()).await.map_err(|e| russh::Error::IO(std::io::Error::other(e.to_string())))
}

async fn sftp_isdir(sftp: &SftpSession, path: &str) -> bool {
    sftp.metadata(path.to_string()).await.map(|m| m.file_type().is_dir()).unwrap_or(false)
}

async fn sftp_isfile(sftp: &SftpSession, path: &str) -> bool {
    sftp.metadata(path.to_string()).await.map(|m| m.file_type().is_file()).unwrap_or(false)
}

async fn sftp_makedirs(sftp: &SftpSession, path: &str) -> Result<(), String> {
    let absolute = path.starts_with('/');
    let mut current = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        current = if current.is_empty() && absolute { format!("/{part}") } else if current.is_empty() { part.to_string() } else { format!("{current}/{part}") };
        if !sftp_isdir(sftp, &current).await {
            sftp.create_dir(current.clone()).await.map_err(|e| format!("cannot create {current}: {e}"))?;
        }
    }
    Ok(())
}

async fn sftp_put_file(sftp: &SftpSession, local: &Path, remote: &str, progress: &Option<Progress>) -> Result<(), String> {
    let mut file = tokio::fs::File::open(local).await.map_err(|e| format!("{}: {e}", local.display()))?;
    let total = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let mut out = sftp.create(remote.to_string()).await.map_err(|e| format!("{remote}: {e}"))?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    let src = local.to_string_lossy().into_owned();
    loop {
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).await.map_err(|e| format!("{remote}: {e}"))?;
        done += n as u64;
        if let Some(p) = progress {
            p(&src, remote, done, total);
        }
    }
    out.shutdown().await.map_err(|e| format!("{remote}: {e}"))?;
    if total == 0 {
        if let Some(p) = progress {
            p(&src, remote, 0, 0);
        }
    }
    Ok(())
}

/// `sftp.put(dir, remote, recurse=True)`: the folder's contents end up in `remote`.
fn sftp_put_dir<'a>(
    sftp: &'a SftpSession,
    local: &'a Path,
    remote: &'a str,
    progress: &'a Option<Progress>,
) -> futures::future::BoxFuture<'a, Result<(), String>> {
    Box::pin(async move {
        sftp_makedirs(sftp, remote).await?;
        let mut entries: Vec<_> = std::fs::read_dir(local).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let target = posix_join(remote, &name);
            let path = entry.path();
            if path.is_dir() {
                sftp_put_dir(sftp, &path, &target, progress).await?;
            } else {
                sftp_put_file(sftp, &path, &target, progress).await?;
            }
        }
        Ok(())
    })
}

async fn sftp_get_file(sftp: &SftpSession, remote: &str, local: &Path, progress: &Option<Progress>) -> Result<(), String> {
    let total = sftp.metadata(remote.to_string()).await.map(|m| m.len()).unwrap_or(0);
    let mut file = sftp.open(remote.to_string()).await.map_err(|e| format!("{remote}: {e}"))?;
    let mut out = tokio::fs::File::create(local).await.map_err(|e| format!("{}: {e}", local.display()))?;
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0u64;
    let dst = local.to_string_lossy().into_owned();
    loop {
        let n = file.read(&mut buf).await.map_err(|e| format!("{remote}: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).await.map_err(|e| e.to_string())?;
        done += n as u64;
        if let Some(p) = progress {
            p(remote, &dst, done, total);
        }
    }
    out.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

fn sftp_get_dir<'a>(
    sftp: &'a SftpSession,
    remote: &'a str,
    local: &'a Path,
    progress: &'a Option<Progress>,
) -> futures::future::BoxFuture<'a, Result<(), String>> {
    Box::pin(async move {
        std::fs::create_dir_all(local).map_err(|e| e.to_string())?;
        let entries = sftp.read_dir(remote.to_string()).await.map_err(|e| format!("{remote}: {e}"))?;
        for entry in entries {
            let name = entry.file_name().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let path = posix_join(remote, &name);
            let target = local.join(&name);
            if sftp_isdir(sftp, &path).await {
                sftp_get_dir(sftp, &path, &target, progress).await?;
            } else if sftp_isfile(sftp, &path).await {
                sftp_get_file(sftp, &path, &target, progress).await?;
            }
        }
        Ok(())
    })
}

/// Pack the folder's contents into a .tar.gz with Linux-friendly owners and modes (Windows has
/// none: files 644, directories and *.sh 755). Returns (bytes, files) packed.
fn pack(src: &Path, archive: &Path) -> std::io::Result<(u64, u64)> {
    let file = std::fs::File::create(archive)?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::new(6));
    let mut tar = tar::Builder::new(gz);
    let (mut size, mut count) = (0u64, 0u64);

    fn add(
        tar: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
        path: &Path,
        name: &str,
        size: &mut u64,
        count: &mut u64,
    ) -> std::io::Result<()> {
        let meta = std::fs::metadata(path)?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut header = tar::Header::new_gnu();
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(mtime);
        if meta.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            tar.append_data(&mut header, format!("{name}/"), std::io::empty())?;
            let mut children: Vec<_> = std::fs::read_dir(path)?.filter_map(|e| e.ok()).collect();
            children.sort_by_key(|e| e.file_name());
            for child in children {
                let child_name = format!("{name}/{}", child.file_name().to_string_lossy());
                add(tar, &child.path(), &child_name, size, count)?;
            }
        } else {
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if name.ends_with(".sh") { 0o755 } else { 0o644 });
            header.set_size(meta.len());
            tar.append_data(&mut header, name, std::fs::File::open(path)?)?;
            *size += meta.len();
            *count += 1;
        }
        Ok(())
    }

    let mut children: Vec<_> = std::fs::read_dir(src)?.filter_map(|e| e.ok()).collect();
    children.sort_by_key(|e| e.file_name());
    for child in children {
        let name = child.file_name().to_string_lossy().into_owned();
        add(&mut tar, &child.path(), &name, &mut size, &mut count)?;
    }
    tar.into_inner()?.finish()?;
    Ok((size, count))
}

/// Unpack a downloaded .tar.gz into dest. Entries that would land outside dest, links and device
/// files are skipped (like Python's "data" filter). Returns (bytes, files) unpacked.
fn unpack(archive: &Path, dest: &Path) -> std::io::Result<(u64, u64)> {
    std::fs::create_dir_all(dest)?;
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
    let mut tar = tar::Archive::new(gz);
    tar.set_preserve_permissions(false);
    let (mut size, mut count) = (0u64, 0u64);
    for entry in tar.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            continue; // symlinks, hard links, devices, fifos
        }
        let len = entry.header().size().unwrap_or(0);
        // unpack_in refuses absolute paths and ".." components
        if entry.unpack_in(dest)? && kind.is_file() {
            size += len;
            count += 1;
        }
    }
    Ok((size, count))
}

/// path, or "name (2).sql.gz", "name (3).sql.gz"… if it already exists (downloads never overwrite).
pub fn free_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // whole extension: db.sql.gz -> "db (2).sql.gz"
    let dot = if path.is_dir() { None } else { name.char_indices().skip(1).find(|(_, c)| *c == '.').map(|(i, _)| i) };
    let (stem, suffix) = match dot {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    let mut n = 2;
    loop {
        let candidate = path.with_file_name(format!("{stem} ({n}){suffix}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

pub struct SshManager {
    db: Arc<Database>,
    connect_timeout: Duration,
    conns: Mutex<HashMap<i64, Conn>>,
    locks: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
    /// where folder uploads land as an archive before unpacking
    pub remote_tmp: String,
    has_tar: Mutex<HashSet<i64>>, // servers known to have tar + gzip
}

impl SshManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            connect_timeout: Duration::from_secs(12),
            conns: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            remote_tmp: "/tmp".into(),
            has_tar: Mutex::new(HashSet::new()),
        }
    }

    pub async fn connection(&self, id: i64) -> Result<Conn, String> {
        let lock = self.locks.lock().unwrap().entry(id).or_default().clone();
        let _held = lock.lock().await;
        if let Some(conn) = self.conns.lock().unwrap().get(&id) {
            if !conn.is_closed() {
                return Ok(conn.clone());
            }
        }
        let conn = self.connect(id).await?;
        self.conns.lock().unwrap().insert(id, conn.clone());
        Ok(conn)
    }

    async fn connect(&self, id: i64) -> Result<Conn, String> {
        let server = self.db.get_server(id).ok_or("Server not found.")?;
        let secrets = self.db.get_secrets(id).map_err(|e| e.to_string())?;
        let expected = server.host_key.clone();
        let mut first_error = None;
        // A server has several host keys (RSA, Ed25519, ECDSA) and the saved fingerprint belongs to
        // the one negotiated back then. Ask for that kind first; if the key does not match, ask for
        // each other kind on its own, accepting only a key that matches the saved fingerprint.
        for algorithms in host_key_orders() {
            match self.connect_with(&server, &secrets, &expected, algorithms).await {
                Ok(handle) => return Ok(handle),
                // the saved key was presented and accepted, so this is the real failure (e.g. login)
                Err((e, KeyCheck::Matched)) => return Err(e),
                Err((e, check)) if expected.is_some() && (check == KeyCheck::Mismatch || first_error.is_some()) => {
                    // keep the first failure; key types the server lacks just fail to negotiate
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                }
                Err((e, _)) => return Err(e),
            }
        }
        Err(first_error.unwrap_or_else(|| "Connection failed.".into()))
    }

    async fn connect_with(&self, server: &Server, secrets: &Secrets, expected: &Option<String>,
                          host_keys: Vec<russh::keys::Algorithm>) -> Result<Conn, (String, KeyCheck)> {
        let id = server.id;
        let seen = Arc::new(Mutex::new(None));
        let handler = Client { expected: expected.clone(), seen: seen.clone() };
        let mut preferred = russh::Preferred::default();
        preferred.key = host_keys.into();
        let config = Arc::new(client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            inactivity_timeout: None,
            nodelay: true,
            preferred,
            ..Default::default()
        });
        let attempt = async {
            let key = match secrets.private_key.as_deref() {
                Some(pem) => Some(
                    decode_secret_key(pem, secrets.key_passphrase.as_deref()).map_err(|e| ConnectError::Key(e.to_string()))?,
                ),
                None => None,
            };
            let port = u16::try_from(server.port).unwrap_or(22);
            let mut handle = client::connect(config, (server.host.as_str(), port), handler).await?;
            let user = server.username.clone();
            let mut ok = false;
            if let Some(key) = key {
                let hash = if key.algorithm().is_rsa() { handle.best_supported_rsa_hash().await?.flatten() } else { None };
                ok = handle
                    .authenticate_publickey(user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                    .await?
                    .success();
            }
            if let (false, Some(password)) = (ok, secrets.password.as_deref()) {
                ok = handle.authenticate_password(user.clone(), password).await?.success();
                if !ok {
                    ok = keyboard_interactive(&mut handle, &user, password).await?;
                }
            }
            if !ok && secrets.private_key.is_none() && secrets.password.is_none() {
                ok = handle.authenticate_none(user.clone()).await?.success();
            }
            if !ok {
                return Err(ConnectError::AuthFailed);
            }
            Ok(handle)
        };
        let result = tokio::time::timeout(self.connect_timeout, attempt).await;
        let check = match (expected, seen.lock().unwrap().as_ref()) {
            (_, None) => KeyCheck::NotSeen,
            (Some(e), Some(s)) if e != s => KeyCheck::Mismatch,
            _ => KeyCheck::Matched,
        };
        let handle = match result {
            Err(_) => return Err(("Connection timed out.".into(), check)),
            Ok(Err(e)) => {
                let seen = seen.lock().unwrap().clone();
                return Err((friendly_connect(e, expected, &seen), check));
            }
            Ok(Ok(h)) => h,
        };
        let seen = seen.lock().unwrap().clone();
        if server.host_key.is_none() {
            if let Some(fp) = seen {
                self.db.set_host_key(id, Some(&fp));
            }
        }
        tracing::info!("Connected to {} ({id})", server.name);
        Ok(Arc::new(handle))
    }

    pub fn drop_conn(&self, id: i64) {
        self.has_tar.lock().unwrap().remove(&id); // the server may have been edited to point elsewhere
        if let Some(conn) = self.conns.lock().unwrap().remove(&id) {
            tokio::spawn(async move {
                let _ = conn.disconnect(Disconnect::ByApplication, "", "en").await;
            });
        }
    }

    pub async fn close(&self) {
        let ids: Vec<i64> = self.conns.lock().unwrap().keys().copied().collect();
        for id in ids {
            if let Some(conn) = self.conns.lock().unwrap().remove(&id) {
                let _ = tokio::time::timeout(Duration::from_secs(2), conn.disconnect(Disconnect::ByApplication, "", "en")).await;
            }
        }
    }

    pub fn sudo_password(server: &Server, secrets: &Secrets) -> Option<String> {
        if let Some(p) = secrets.sudo_password.clone().filter(|p| !p.is_empty()) {
            return Some(p);
        }
        if server.auth_type == "password" {
            secrets.password.clone()
        } else {
            None
        }
    }

    fn prepared(&self, server: &Server, command: &str) -> (String, Option<String>) {
        let secrets = self.db.get_secrets(server.id).unwrap_or_default();
        prepare(command, &server.username, Self::sudo_password(server, &secrets).as_deref())
    }

    /// Run one command. Never fails for remote failures: they come back in the result.
    pub async fn run(&self, id: i64, command: &str, timeout: f64, raw: bool) -> ExecResult {
        let started = Instant::now();
        let Some(server) = self.db.get_server(id) else {
            return ExecResult::error(started, "Server not found.");
        };
        let (line, stdin) = if raw { (command.to_string(), None) } else { self.prepared(&server, command) };
        for attempt in 1..=2 {
            let conn = match self.connection(id).await {
                Ok(c) => c,
                Err(e) => return ExecResult::error(started, e),
            };
            match exec_on(&conn, &line, stdin.as_deref(), timeout).await {
                Ok(out) => {
                    if out.lost {
                        self.drop_conn(id);
                        return ExecResult::new(started, None, &text(&out.stdout), &text(&out.stderr),
                                               Some("Connection lost while the command was running.".into()));
                    }
                    return ExecResult::new(started, out.exit, &text(&out.stdout), &text(&out.stderr), None);
                }
                Err(ExecError::Timeout { stdout, stderr }) => {
                    return ExecResult::new(started, None, &text(&stdout), &text(&stderr),
                                           Some(format!("Timed out after {timeout:.0}s")));
                }
                Err(ExecError::Lost(e)) => {
                    self.drop_conn(id);
                    if attempt == 2 {
                        return ExecResult::error(started, friendly(&e));
                    }
                }
            }
        }
        unreachable!()
    }

    /// Folder uploads need tar + gzip on the server; install them when missing.
    /// Returns a note for the result ("" when they were already there), None if unavailable.
    async fn ensure_tar(&self, id: i64) -> Option<String> {
        if self.has_tar.lock().unwrap().contains(&id) {
            return Some(String::new());
        }
        let res = self.run(id, HAS_TAR, 30.0, true).await;
        if res.error.is_some() {
            return None;
        }
        let mut note = String::new();
        if res.exit != Some(0) {
            let res = self.run(id, &format!("{INSTALL_TAR}\n{HAS_TAR}"), 600.0, false).await;
            if res.error.is_some() || res.exit != Some(0) {
                tracing::warn!("Could not install tar/gzip on server {id}: {} {}", res.error.unwrap_or_default(),
                               crate::util::tail_chars(&res.stderr, 300));
                return None;
            }
            note = "installed missing tar/gzip on the server; ".into();
        }
        self.has_tar.lock().unwrap().insert(id);
        Some(note)
    }

    /// Copy a local file or folder to the server, as the SSH user (like `scp -r`: a `remote`
    /// ending in "/" or naming an existing directory receives it inside).
    ///
    /// A folder (a build output can be thousands of small files) is packed into one .tar.gz here,
    /// sent in one transfer, unpacked into the destination with `tar`, and the archive is deleted
    /// on both sides.
    pub async fn upload(&self, id: i64, local: &str, remote: &str, progress: Option<Progress>) -> ExecResult {
        let started = Instant::now();
        let src = PathBuf::from(expand(local));
        if !src.exists() {
            return ExecResult::error(started, format!("Not found on this PC: {}", src.display()));
        }
        let is_dir = src.is_dir();
        let tools = if is_dir { self.ensure_tar(id).await } else { Some(String::new()) };
        let mut archive: Option<tempfile::TempPath> = None;
        let note;
        if let (Some(tools), true) = (&tools, is_dir) {
            let tmp = match tempfile::Builder::new().prefix("sshpro-upload-").suffix(".tar.gz").tempfile() {
                Ok(t) => t.into_temp_path(),
                Err(e) => return ExecResult::new(started, Some(1), "", &format!("Upload failed: {e}"), None),
            };
            let (s, a) = (src.clone(), tmp.to_path_buf());
            let (size, count) = match tokio::task::spawn_blocking(move || pack(&s, &a)).await {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return ExecResult::new(started, Some(1), "", &format!("Upload failed: {e}"), None),
                Err(e) => return ExecResult::error(started, e.to_string()),
            };
            let packed = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
            note = format!("{tools}{count} files, {}, sent as one {} archive", human_size(size as f64), human_size(packed as f64));
            archive = Some(tmp);
        } else if is_dir {
            note = "tar/gzip missing on the server and could not be installed: sent file by file (slower)".into();
        } else {
            note = human_size(std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0) as f64);
        }
        let archive_name = archive.as_ref().and_then(|a| a.file_name()).map(|n| n.to_string_lossy().into_owned());
        let mut target = remote.to_string();
        let mut sent = target.clone();
        for attempt in 1..=2 {
            let conn = match self.connection(id).await {
                Ok(c) => c,
                Err(e) => return ExecResult::error(started, e),
            };
            let sftp = match open_sftp(&conn).await {
                Ok(s) => s,
                Err(e) => {
                    self.drop_conn(id);
                    if attempt == 2 {
                        return ExecResult::error(started, friendly(&e));
                    }
                    continue;
                }
            };
            let result: Result<(), String> = async {
                if remote.ends_with('/') {
                    sftp_makedirs(&sftp, remote).await?;
                }
                if remote.ends_with('/') || sftp_isdir(&sftp, remote).await {
                    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    target = posix_join(remote, &name);
                }
                sent = match &archive_name {
                    Some(name) => posix_join(&self.remote_tmp, name),
                    None => target.clone(),
                };
                match &archive {
                    Some(a) => sftp_put_file(&sftp, a, &sent, &progress).await,
                    None if is_dir => sftp_put_dir(&sftp, &src, &sent, &progress).await,
                    None => sftp_put_file(&sftp, &src, &sent, &progress).await,
                }
            }
            .await;
            let _ = sftp.close().await;
            if let Err(e) = result {
                return ExecResult::new(started, Some(1), "", &format!("Upload failed: {e}"), None);
            }
            break;
        }
        if archive.is_some() {
            let (t, a) = (q(&target), q(&sent));
            let res = self
                .run(id, &format!("mkdir -p {t} && tar -xzf {a} -C {t}; rc=$?; rm -f {a}; exit $rc"), 1800.0, true)
                .await;
            if res.error.is_some() || res.exit != Some(0) {
                return ExecResult::new(started, res.exit, &res.stdout,
                                       &format!("Uploaded, but unpacking into {target} failed: {}", res.stderr), res.error);
            }
        }
        drop(archive);
        ExecResult::new(started, Some(0), &format!("uploaded {} -> {target} ({note})", src.display()), "", None)
    }

    /// Copy a server file or folder to this PC, as the SSH user (like `scp -r`: a `local` ending
    /// in a slash or naming an existing folder receives it inside). Never overwrites: an existing
    /// target gets a " (2)" style name instead.
    ///
    /// A folder is packed into one .tar.gz on the server, sent in one transfer, unpacked here, and
    /// the archive is deleted on both sides.
    pub async fn download(&self, id: i64, remote: &str, local: &str, progress: Option<Progress>) -> ExecResult {
        let started = Instant::now();
        let mut packed: Option<String> = None; // archive path on the server
        let res = self.download_inner(id, remote, local, progress, started, &mut packed).await;
        if let Some(p) = packed {
            self.run(id, &format!("rm -f {}", q(&p)), 30.0, true).await;
        }
        res
    }

    async fn download_inner(&self, id: i64, remote: &str, local: &str, progress: Option<Progress>, started: Instant,
                            packed: &mut Option<String>) -> ExecResult {
        let mut dest = PathBuf::from(expand(local));
        let mut is_dir = false;
        for attempt in 1..=2 {
            let conn = match self.connection(id).await {
                Ok(c) => c,
                Err(e) => return ExecResult::error(started, e),
            };
            let sftp = match open_sftp(&conn).await {
                Ok(s) => s,
                Err(e) => {
                    self.drop_conn(id);
                    if attempt == 2 {
                        return ExecResult::error(started, friendly(&e));
                    }
                    continue;
                }
            };
            is_dir = sftp_isdir(&sftp, remote).await;
            let is_file = !is_dir && sftp_isfile(&sftp, remote).await;
            let _ = sftp.close().await;
            if !is_dir && !is_file {
                return ExecResult::new(started, Some(1), "", &format!("Not found on the server: {remote}"), None);
            }
            break;
        }
        let name = posix_basename(remote.trim_end_matches('/'));
        let name = if name.is_empty() { "download".to_string() } else { name };
        let local_trim = local.trim_end();
        if local_trim.ends_with('/') || local_trim.ends_with('\\') || dest.is_dir() {
            dest = dest.join(&name);
        }
        let dest = free_path(&dest);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tools = if is_dir { self.ensure_tar(id).await } else { Some(String::new()) };
        let mut archive: Option<tempfile::TempPath> = None;
        if let (true, Some(_)) = (is_dir, &tools) {
            let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
            let path = posix_join(&self.remote_tmp, &format!("sshpro-download-{millis}.tar.gz"));
            *packed = Some(path.clone());
            let res = self.run(id, &format!("tar -czf {} -C {} .", q(&path), q(remote)), 1800.0, true).await;
            if res.error.is_some() || res.exit != Some(0) {
                return ExecResult::new(started, res.exit, &res.stdout,
                                       &format!("Packing {remote} on the server failed: {}", res.stderr), res.error);
            }
            match tempfile::Builder::new().prefix("sshpro-download-").suffix(".tar.gz").tempfile() {
                Ok(t) => archive = Some(t.into_temp_path()),
                Err(e) => return ExecResult::new(started, Some(1), "", &format!("Download failed: {e}"), None),
            }
        }
        let conn = match self.connection(id).await {
            Ok(c) => c,
            Err(e) => return ExecResult::error(started, e),
        };
        let fetched: Result<(), String> = async {
            let sftp = open_sftp(&conn).await.map_err(|e| friendly(&e))?;
            let r = match (&archive, packed.as_deref()) {
                (Some(a), Some(p)) => sftp_get_file(&sftp, p, a, &progress).await,
                _ if is_dir => sftp_get_dir(&sftp, remote, &dest, &progress).await,
                _ => sftp_get_file(&sftp, remote, &dest, &progress).await,
            };
            let _ = sftp.close().await;
            r
        }
        .await;
        if let Err(e) = fetched {
            return ExecResult::new(started, Some(1), "", &format!("Download failed: {e}"), None);
        }
        let note = if let Some(a) = &archive {
            let sent = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
            let (a2, d2) = (a.to_path_buf(), dest.clone());
            match tokio::task::spawn_blocking(move || unpack(&a2, &d2)).await {
                Ok(Ok((size, count))) => format!(
                    "{}{count} files, {}, sent as one {} archive",
                    tools.clone().unwrap_or_default(), human_size(size as f64), human_size(sent as f64)
                ),
                Ok(Err(e)) => return ExecResult::new(started, Some(1), "", &format!("Download failed: {e}"), None),
                Err(e) => return ExecResult::error(started, e.to_string()),
            }
        } else if is_dir {
            "tar/gzip missing on the server and could not be installed: sent file by file (slower)".into()
        } else {
            human_size(std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) as f64)
        };
        ExecResult::new(started, Some(0), &format!("downloaded {remote} -> {} ({note})", dest.display()), "", None)
    }

    // ---- server to server --------------------------------------------------------------------

    /// Command line + stdin prefix for a process that reads streamed data on stdin.
    ///
    /// A sudo password goes in front of the data (sudo -S reads it byte by byte, so what follows
    /// reaches the command intact). When sudo needs no password (NOPASSWD) it must get none, or
    /// that line would end up inside the data, so that is checked first.
    async fn receiver(&self, id: i64, command: &str) -> Result<(String, Vec<u8>), String> {
        let server = self.db.get_server(id).ok_or("Server not found.")?;
        let mut password = None;
        if sudo_count(command) > 0 && server.username != "root" {
            let check = self.run(id, "sudo -n true", 20.0, true).await;
            if let Some(e) = check.error {
                return Err(e);
            }
            if check.exit != Some(0) {
                if sudo_count(command) > 1 {
                    return Err("The receiving command may use sudo only once; wrap it: sudo bash -c '…'".into());
                }
                let secrets = self.db.get_secrets(id).unwrap_or_default();
                password = Self::sudo_password(&server, &secrets);
            }
        }
        let (line, stdin) = prepare(&format!("set -o pipefail; {command}"), &server.username, password.as_deref());
        Ok((line, stdin.unwrap_or_default().into_bytes()))
    }

    async fn open_exec(&self, id: i64, line: &str) -> Result<Channel<Msg>, String> {
        let conn = self.connection(id).await?;
        let ch = conn.channel_open_session().await.map_err(|e| friendly(&e))?;
        ch.exec(true, line.as_bytes()).await.map_err(|e| friendly(&e))?;
        Ok(ch)
    }

    /// Stream the output of `src_command` on one server into `dst_command` on another, e.g.
    /// `pg_dump -Fc db` into `pg_restore -d db`, or `tar -cf - dir` into `tar -xf -`.
    ///
    /// The data is relayed through this app's own SSH connections (nothing is stored on this PC),
    /// so the servers need no network path or SSH keys to each other. Both sides run with
    /// `pipefail`; the result fails if either side fails.
    pub async fn pipe(&self, src_id: i64, src_command: &str, dst_id: i64, dst_command: &str, timeout: f64,
                      progress: Option<PipeProgress>) -> ExecResult {
        let started = Instant::now();
        let (Some(src_server), Some(dst_server)) = (self.db.get_server(src_id), self.db.get_server(dst_id)) else {
            return ExecResult::error(started, "Server not found.");
        };
        let opened: Result<_, String> = async {
            let (dst_line, dst_prefix) = self.receiver(dst_id, dst_command).await?;
            let (src_line, src_stdin) = self.prepared(&src_server, &format!("set -o pipefail; {src_command}"));
            let dst = self.open_exec(dst_id, &dst_line).await?;
            let src = match self.open_exec(src_id, &src_line).await {
                Ok(s) => s,
                Err(e) => {
                    let _ = dst.close().await;
                    return Err(e);
                }
            };
            Ok((src, src_stdin, dst, dst_prefix))
        }
        .await;
        let (src, src_stdin, dst, dst_prefix) = match opened {
            Ok(x) => x,
            Err(e) => return ExecResult::error(started, e),
        };
        let (mut src_r, src_w) = src.split();
        let (mut dst_r, dst_w) = dst.split();

        let pump = async {
            if let Some(input) = &src_stdin {
                src_w.data(input.as_bytes()).await.map_err(|e| e.to_string())?;
            }
            src_w.eof().await.map_err(|e| e.to_string())?;
            if !dst_prefix.is_empty() {
                dst_w.data(&dst_prefix[..]).await.map_err(|e| e.to_string())?;
            }
            let (mut sent, mut err, mut rc, mut eof_sent) = (0u64, Vec::new(), None, false);
            let mut failure: Option<String> = None;
            while let Some(msg) = src_r.wait().await {
                match msg {
                    ChannelMsg::Data { data } => {
                        if failure.is_none() {
                            if let Err(e) = dst_w.data(&data[..]).await {
                                failure = Some(e.to_string());
                                let _ = src_w.close().await;
                                continue;
                            }
                            sent += data.len() as u64;
                            if let Some(p) = &progress {
                                p(sent, started.elapsed().as_secs_f64());
                            }
                        }
                    }
                    ChannelMsg::ExtendedData { data, .. } => push_bounded(&mut err, &data),
                    ChannelMsg::ExitStatus { exit_status } => rc = Some(exit_status as i64),
                    ChannelMsg::ExitSignal { signal_name, .. } => rc = rc.or(Some(-signal_number(&signal_name))),
                    ChannelMsg::Eof => {
                        if !eof_sent {
                            let _ = dst_w.eof().await;
                            eof_sent = true;
                        }
                    }
                    _ => {}
                }
            }
            if !eof_sent {
                let _ = dst_w.eof().await;
            }
            Ok::<_, String>((sent, err, rc, failure))
        };
        let collect = async {
            let (mut out, mut err, mut rc) = (Vec::new(), Vec::new(), None);
            while let Some(msg) = dst_r.wait().await {
                match msg {
                    ChannelMsg::Data { data } => push_bounded(&mut out, &data),
                    ChannelMsg::ExtendedData { data, .. } => push_bounded(&mut err, &data),
                    ChannelMsg::ExitStatus { exit_status } => rc = Some(exit_status as i64),
                    ChannelMsg::ExitSignal { signal_name, .. } => rc = rc.or(Some(-signal_number(&signal_name))),
                    _ => {}
                }
            }
            (out, err, rc)
        };
        let both = async { tokio::join!(pump, collect) };
        let mut error: Option<String> = None;
        let (mut sent, mut src_err, mut src_rc) = (0, Vec::new(), None);
        let (mut dst_out, mut dst_err, mut dst_rc) = (Vec::new(), Vec::new(), None);
        match tokio::time::timeout(Duration::from_secs_f64(timeout.max(1.0)), both).await {
            Err(_) => {
                error = Some(format!("Timed out after {timeout:.0}s"));
            }
            Ok((pumped, collected)) => {
                (dst_out, dst_err, dst_rc) = collected;
                match pumped {
                    Ok((s, e, rc, failure)) => {
                        (sent, src_err, src_rc) = (s, e, rc);
                        if let Some(f) = failure {
                            error = Some(format!("The transfer broke off: {f}"));
                        }
                    }
                    Err(e) => error = Some(format!("The transfer broke off: {e}")),
                }
            }
        }
        let _ = src_w.close().await;
        let _ = dst_w.close().await;
        let (src_err, dst_out, dst_err) = (text(&src_err), text(&dst_out), text(&dst_err));
        if error.is_some() && dst_err.is_empty() && dst_rc.is_some_and(|c| c != 0) {
            error = None; // the receiver failed: its own exit code and stderr say why
        }
        let took = started.elapsed().as_secs_f64();
        let rate = if took > 0.0 { sent as f64 / took } else { 0.0 };
        let summary = format!(
            "streamed {} from {} to {} in {took:.0}s ({}/s)",
            human_size(sent as f64), src_server.name, dst_server.name, human_size(rate)
        );
        let mut stderr = [
            (!src_err.is_empty()).then(|| format!("[{}] {}", src_server.name, src_err.trim())),
            (!dst_err.is_empty()).then(|| format!("[{}] {}", dst_server.name, dst_err.trim())),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
        let failed = |rc: Option<i64>| rc.is_some_and(|c| c != 0);
        let exit = if failed(src_rc) || failed(dst_rc) {
            let (code, side) = if failed(src_rc) { (src_rc, &src_server.name) } else { (dst_rc, &dst_server.name) };
            stderr = format!("failed on {side} (exit {})\n{stderr}", code.unwrap()).trim().to_string();
            code
        } else if error.is_some() {
            None
        } else {
            Some(0)
        };
        ExecResult::new(started, exit, format!("{summary}\n{dst_out}").trim(), &stderr, error)
    }

    /// Copy a file or folder from one server into a folder on another (`into` receives it,
    /// like `cp -r`), keeping permissions, times and owners (owners need `sudo`).
    ///
    /// Built to lose nothing: the source is only read; the destination's free space is checked
    /// first; an existing target is never overwritten unless `overwrite`; afterwards both sides
    /// are compared (file count, bytes and, with "checksum", a SHA-256 over every file) and any
    /// difference fails the step.
    #[allow(clippy::too_many_arguments)]
    pub async fn copy(&self, src_id: i64, src_path: &str, dst_id: i64, into: &str, verify: &str, sudo: bool,
                      overwrite: bool, timeout: f64, progress: Option<PipeProgress>) -> ExecResult {
        let started = Instant::now();
        let src_path = src_path.trim_end_matches('/');
        let (parent, name) = posix_split(src_path);
        if !src_path.starts_with('/') || !into.starts_with('/') || name.is_empty() || name == "." || name == ".." {
            return ExecResult::error(started, "copy needs absolute paths, and the source must be a file or folder (not /)");
        }
        let parent = if parent.is_empty() { "/".to_string() } else { parent };
        let into = into.trim_end_matches('/');
        let into = if into.is_empty() { "/" } else { into };
        let target = posix_join(into, &name);
        let sudo_ = if sudo { "sudo " } else { "" };
        let as_root = |script: &str| {
            if sudo { format!("sudo bash -c {}", q(&format!("set -o pipefail; {script}"))) } else { script.to_string() }
        };

        for sid in [src_id, dst_id] {
            if self.ensure_tar(sid).await.is_none() {
                return ExecResult::error(started, format!("tar/gzip are missing on server {sid} and could not be installed"));
            }
        }

        // 1. before: the source exists; the destination has room and no target in the way
        let (sp, ip, tp) = (q(src_path), q(into), q(&target));
        let src_cmd = format!("{sudo_}test -e {sp} || {{ echo NOT_FOUND; exit 3; }}; {sudo_}du -sk {sp} | cut -f1");
        let dst_cmd = format!(
            "p={ip}; while [ ! -d \"$p\" ]; do p=$(dirname \"$p\"); done; df -Pk \"$p\" | awk 'NR==2{{print $4}}'; \
             if {sudo_}test -e {tp}; then echo EXISTS; fi");
        let (src_check, dst_check) =
            tokio::join!(self.run(src_id, &src_cmd, 1800.0, false), self.run(dst_id, &dst_cmd, 120.0, false));
        for (res, where_) in [(&src_check, "source"), (&dst_check, "destination")] {
            if let Some(e) = &res.error {
                return ExecResult::error(started, format!("{where_}: {e}"));
            }
        }
        if src_check.stdout.contains("NOT_FOUND") {
            return ExecResult::new(started, Some(3), "", &format!("Not found on the source server: {src_path}"), None);
        }
        if src_check.exit != Some(0) {
            return ExecResult::new(started, src_check.exit, "", &format!("Could not read the source: {}", src_check.stderr), None);
        }
        let size = src_check.stdout.split_whitespace().last().and_then(|x| x.parse::<u64>().ok()).map(|k| k * 1024);
        let free = dst_check.stdout.split_whitespace().next().and_then(|x| x.parse::<u64>().ok()).map(|k| k * 1024);
        let (Some(size), Some(free)) = (size, free) else {
            return ExecResult::new(started, Some(1), "",
                                   "Could not measure the source size or the free space on the destination", None);
        };
        if dst_check.stdout.contains("EXISTS") && !overwrite {
            return ExecResult::new(started, Some(1), "", &format!(
                "{target} already exists on the destination and was left untouched. Move it aside first \
                 (e.g. mv {target} {target}.old-$(date +%F)) or set overwrite: true to merge into it."), None);
        }
        if size + COPY_MARGIN > free {
            return ExecResult::new(started, Some(1), "", &format!(
                "Not enough space on the destination: {} needed, {} free in {into}",
                human_size(size as f64), human_size(free as f64)), None);
        }

        // 2. stream it: tar | gzip on the source, gunzip | tar on the destination
        let (pp, np) = (q(&parent), q(&name));
        let mut res = self
            .pipe(src_id, &as_root(&format!("tar -C {pp} -cf - {np} | gzip -1")), dst_id,
                  &as_root(&format!("mkdir -p {ip} && gzip -dc | tar -C {ip} -xpf -")), timeout, progress)
            .await;
        if res.error.is_some() || res.exit != Some(0) {
            res.stderr = format!("Copy failed — the source is unchanged.\n{}", res.stderr).trim().to_string();
            return res;
        }
        let note = res.stdout.lines().next().unwrap_or_default().to_string();

        // 3. after: compare both sides
        if verify == "none" {
            return ExecResult::new(started, Some(0), &format!("copied {src_path} -> {target} ({note}); not verified"), "", None);
        }
        let script = |dir: &str| {
            let digest = if verify == "checksum" {
                format!("find {np} -type f -print0 | LC_ALL=C sort -z | xargs -0 -r sha256sum | sha256sum | cut -d' ' -f1")
            } else {
                "echo -".to_string()
            };
            format!(
                "cd {dir} && {{ {digest}; find {np} -type f | wc -l; \
                 find {np} -type f -exec stat -c %s {{}} + | awk '{{s+=$1}} END {{print s+0}}'; }}"
            )
        };
        let (sa, sb) = (as_root(&script(&pp)), as_root(&script(&ip)));
        let (a, b) = tokio::join!(self.run(src_id, &sa, timeout, false), self.run(dst_id, &sb, timeout, false));
        let copied = format!("copied {src_path} -> {target} ({note})");
        if a.error.is_some() || b.error.is_some() || a.exit != Some(0) || b.exit != Some(0) {
            let why = |r: &ExecResult| r.error.clone().unwrap_or_else(|| r.stderr.clone());
            return ExecResult::new(started, Some(1), &copied,
                                   &format!("Copied, but the check could not run: {} {}", why(&a), why(&b)), None);
        }
        let left: Vec<&str> = a.stdout.split_whitespace().collect();
        let right: Vec<&str> = b.stdout.split_whitespace().collect();
        if left.len() != 3 || left != right {
            return ExecResult::new(started, Some(1), &copied, &format!(
                "VERIFY FAILED: the copy differs from the source (source: {}; destination: {}). Nothing was deleted. \
                 If the source was being written to, stop that service and copy again.", left.join(" "), right.join(" ")), None);
        }
        let files: u64 = left[1].parse().unwrap_or(0);
        let total: f64 = left[2].parse().unwrap_or(0.0);
        let check = if verify == "checksum" { "SHA-256 of every file matches" } else { "file count and sizes match" };
        ExecResult::new(started, Some(0), &format!(
            "copied {src_path} -> {target}: {files} files, {} ({note}); verified: {check}", human_size(total)), "", None)
    }

    pub async fn probe(&self, id: i64) -> Value {
        let result = self.run(id, PROBE, 20.0, true).await;
        if let Some(error) = result.error {
            self.db.set_status(id, "offline", None);
            return json!({"status": "offline", "error": error});
        }
        let facts = parse_facts(&result.stdout);
        self.db.set_status(id, "online", Some(&facts));
        json!({"status": "online", "facts": facts})
    }

    pub async fn open_shell(&self, id: i64, cols: u32, rows: u32) -> Result<ShellChannel, String> {
        for attempt in 1..=2 {
            let conn = self.connection(id).await?;
            let opened = async {
                let ch = conn.channel_open_session().await?;
                ch.request_pty(true, "xterm-256color", cols, rows, 0, 0, &[]).await?;
                ch.request_shell(true).await?;
                Ok::<_, russh::Error>(ch)
            }
            .await;
            match opened {
                Ok(ch) => return Ok(ch),
                Err(e) => {
                    self.drop_conn(id);
                    if attempt == 2 {
                        return Err(friendly(&e));
                    }
                }
            }
        }
        unreachable!()
    }
}

/// Host key algorithms to offer, one list per connection attempt: RSA first (what the earlier Python version
/// negotiated, so its saved fingerprints match and new ones stay readable by both), then each
/// other key type on its own for servers whose saved key is of that type.
fn host_key_orders() -> Vec<Vec<russh::keys::Algorithm>> {
    use russh::keys::{Algorithm, EcdsaCurve};
    let rsa = vec![
        Algorithm::Rsa { hash: Some(HashAlg::Sha512) },
        Algorithm::Rsa { hash: Some(HashAlg::Sha256) },
        Algorithm::Rsa { hash: None },
    ];
    let ed25519 = Algorithm::Ed25519;
    let ecdsa = [EcdsaCurve::NistP256, EcdsaCurve::NistP384, EcdsaCurve::NistP521].map(|curve| Algorithm::Ecdsa { curve });
    let mut first = rsa;
    first.push(ed25519.clone());
    first.extend(ecdsa.iter().cloned());
    let mut orders = vec![first, vec![ed25519]];
    orders.extend(ecdsa.into_iter().map(|a| vec![a]));
    orders
}

/// Password through keyboard-interactive (servers with PasswordAuthentication off but PAM on).
async fn keyboard_interactive(handle: &mut Handle<Client>, user: &str, password: &str) -> Result<bool, russh::Error> {
    use russh::client::KeyboardInteractiveAuthResponse as R;
    let mut reply = handle.authenticate_keyboard_interactive_start(user.to_string(), None).await?;
    for _ in 0..5 {
        match reply {
            R::Success => return Ok(true),
            R::Failure { .. } => return Ok(false),
            R::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|_| password.to_string()).collect();
                reply = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_output() {
        let facts = parse_facts("os=Ubuntu 24.04 LTS\nkernel=6.8\nuptime=3600\nload=0.10 0.20 0.30\ncpus=4\nmem=7900 3950\ndisk=104857600 52428800\n");
        assert_eq!(facts["os"], "Ubuntu 24.04 LTS");
        assert_eq!(facts["uptime"], 3600);
        assert_eq!(facts["load"], json!([0.1, 0.2, 0.3]));
        assert_eq!(facts["cpus"], 4);
        assert_eq!(facts["mem"], json!({"total_mb": 7900, "used_pct": 50.0}));
        assert_eq!(facts["disk"], json!({"total_gb": 100.0, "free_gb": 50.0, "used_pct": 50.0}));
        let empty = parse_facts("");
        assert!(empty["os"].is_null() && empty["load"].is_null() && empty.get("mem").is_none());
    }

    #[test]
    fn free_path_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("db.sql.gz");
        assert_eq!(free_path(&p), p);
        std::fs::write(&p, "x").unwrap();
        assert_eq!(free_path(&p), dir.path().join("db (2).sql.gz"));
        std::fs::write(dir.path().join("db (2).sql.gz"), "x").unwrap();
        assert_eq!(free_path(&p), dir.path().join("db (3).sql.gz"));
    }

    #[test]
    fn pack_and_unpack_round_trip() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("sub")).unwrap();
        std::fs::write(src.path().join("a.txt"), "hello").unwrap();
        std::fs::write(src.path().join("sub").join("run.sh"), "echo hi").unwrap();
        let archive = src.path().join("..").join(format!("sshpro-test-{}.tar.gz", std::process::id()));
        assert_eq!(pack(src.path(), &archive).unwrap(), (12, 2));
        let dest = tempfile::tempdir().unwrap();
        assert_eq!(unpack(&archive, dest.path()).unwrap(), (12, 2));
        assert_eq!(std::fs::read_to_string(dest.path().join("sub").join("run.sh")).unwrap(), "echo hi");
        std::fs::remove_file(archive).unwrap();
    }
}
