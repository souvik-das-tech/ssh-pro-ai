//! Small helpers shared by the modules: Python-style truthiness and str() for JSON values,
//! sizes, timestamps, path expansion and child-process clean-up.

use serde_json::Value;

/// Python truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `str(value or "")`.
pub fn val_str(v: Option<&Value>) -> String {
    if !truthy(v) {
        return String::new();
    }
    match v.unwrap() {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        other => other.to_string(),
    }
}

/// `int(value)` for JSON numbers and digit strings.
pub fn val_int(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

/// A positive number (bools excluded), like `isinstance(x, (int, float)) and x > 0`.
pub fn positive_number(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().filter(|f| *f > 0.0),
        _ => None,
    }
}

pub fn human_size(n: f64) -> String {
    let mut n = n;
    for unit in ["B", "KB", "MB", "GB"] {
        if n < 1024.0 || unit == "GB" {
            return if unit == "B" { format!("{n:.0} {unit}") } else { format!("{n:.1} {unit}") };
        }
        n /= 1024.0;
    }
    String::new()
}

pub fn round_to(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

/// UTC ISO timestamp with seconds, like `datetime.now(timezone.utc).isoformat(timespec="seconds")`.
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

pub fn iso_days_ago(days: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::days(days)).format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

/// The last `max` characters of a string.
pub fn tail_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    s.chars().skip(count - max).collect()
}

pub fn head_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// `os.path.expanduser(os.path.expandvars(path.strip()))` on Windows: %VAR%, $VAR, ${VAR}, ~.
pub fn expand(path: &str) -> String {
    let path = path.trim();
    let chars: Vec<char> = path.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == '%') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if !name.is_empty() {
                    if let Ok(val) = std::env::var(&name) {
                        out.push_str(&val);
                        i += end + 2;
                        continue;
                    }
                }
            }
        } else if c == '$' {
            if chars.get(i + 1) == Some(&'{') {
                if let Some(end) = chars[i + 2..].iter().position(|&x| x == '}') {
                    let name: String = chars[i + 2..i + 2 + end].iter().collect();
                    if let Ok(val) = std::env::var(&name) {
                        out.push_str(&val);
                        i += end + 3;
                        continue;
                    }
                }
            } else {
                let name: String = chars[i + 1..]
                    .iter()
                    .take_while(|x| x.is_alphanumeric() || **x == '_' || **x == '-')
                    .collect();
                if !name.is_empty() {
                    if let Ok(val) = std::env::var(&name) {
                        out.push_str(&val);
                        i += 1 + name.chars().count();
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    if out == "~" || out.starts_with("~\\") || out.starts_with("~/") {
        let home = crate::paths::home_dir().to_string_lossy().into_owned();
        out = format!("{home}{}", &out[1..]);
    }
    out
}

/// `os.path.join(a, b)` with Windows separators.
pub fn win_join(a: &str, b: &str) -> String {
    if a.is_empty() {
        return b.to_string();
    }
    if a.ends_with('\\') || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}\\{b}")
    }
}

/// `posixpath.join(a, b)`.
pub fn posix_join(a: &str, b: &str) -> String {
    if b.starts_with('/') || a.is_empty() {
        return b.to_string();
    }
    if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `posixpath.split(p)` -> (head, tail).
pub fn posix_split(p: &str) -> (String, String) {
    match p.rfind('/') {
        None => (String::new(), p.to_string()),
        Some(i) => {
            let head = &p[..=i];
            let tail = &p[i + 1..];
            let trimmed = head.trim_end_matches('/');
            (if trimmed.is_empty() { head.to_string() } else { trimmed.to_string() }, tail.to_string())
        }
    }
}

pub fn posix_basename(p: &str) -> String {
    posix_split(p).1
}

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A tokio command that never flashes a console window.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Kill a process and all its children (agy.exe and codex.exe may spawn helpers).
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

/// Kills the process tree when dropped unless disarmed: a request that is cancelled (the user
/// pressed Stop) must not leave its AI process running.
pub struct KillGuard(pub Option<u32>);

impl KillGuard {
    pub fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for KillGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            std::thread::spawn(move || kill_tree(pid));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_match_python() {
        assert_eq!(human_size(512.0), "512 B");
        assert_eq!(human_size(2048.0), "2.0 KB");
        assert_eq!(human_size(5.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0), "5120.0 GB");
        assert_eq!(posix_split("/a/b"), ("/a".into(), "b".into()));
        assert_eq!(posix_split("/b"), ("/".into(), "b".into()));
        assert_eq!(posix_split("b"), ("".into(), "b".into()));
        assert_eq!(posix_join("/tmp", "x"), "/tmp/x");
        assert_eq!(posix_join("/tmp/", "x"), "/tmp/x");
        assert_eq!(win_join("C:\\a", "b"), "C:\\a\\b");
        std::env::set_var("SSHPRO_TEST_VAR", "V");
        assert_eq!(expand("%SSHPRO_TEST_VAR%\\x"), "V\\x");
        assert_eq!(expand("$SSHPRO_TEST_VAR/x"), "V/x");
        assert_eq!(expand("%NOPE_NOT_SET%"), "%NOPE_NOT_SET%");
    }
}

pub const MAX_OUTPUT: usize = 200_000;

/// One command's result, the same shape for servers and this PC.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExecResult {
    pub exit: Option<i64>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub duration: f64,
}

impl ExecResult {
    pub fn new(started: std::time::Instant, exit: Option<i64>, stdout: &str, stderr: &str, error: Option<String>) -> Self {
        Self {
            exit,
            stdout: tail_chars(stdout, MAX_OUTPUT),
            stderr: tail_chars(stderr, MAX_OUTPUT),
            error,
            duration: round_to(started.elapsed().as_secs_f64(), 2),
        }
    }

    pub fn error(started: std::time::Instant, error: impl Into<String>) -> Self {
        Self::new(started, None, "", "", Some(error.into()))
    }

    /// stdout and stderr together, as the audit log and the UI show them.
    pub fn output(&self) -> String {
        let mut out = self.stdout.clone();
        if !self.stderr.is_empty() {
            out.push('\n');
            out.push_str(&self.stderr);
        }
        out.trim().to_string()
    }
}
