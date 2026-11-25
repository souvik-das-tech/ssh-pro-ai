//! AGY (Antigravity) CLI adapter. The AI chat depends on it; SSH, terminals and manual commands do not.
//!
//! Verified AGY 1.4.7 interfaces:
//!   * One-shot:   agy -p <prompt> --output-format json
//!                 -> one JSON object {status, response, structured_output?, ...}
//!   * Persistent: agy -p= --input-format stream-json --output-format stream-json
//!                 stdin  : one {"event":"user","message":{"role":"user","content":...}} per line
//!                 stdout : {"event":"init"|"step_update"|"result", ...} NDJSON
//!     Process start costs ~10 s; each later turn ~3 s. So the session is the primary path.
//!
//! AGY is itself an agent. The app never passes --dangerously-skip-permissions: in headless mode any
//! tool that needs approval is auto-denied, so AGY can only *plan*; the app executes.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::task::JoinHandle;

use crate::paths::{data_dir, which};
use crate::util::{command, kill_tree, tail_chars, KillGuard};

pub const AGY_INSTALL_URL: &str = "https://docs.flutter.dev/ai/antigravity-cli";
const AUTH_MARKERS: &[&str] = &["not authenticated", "[auth error]", "sign in", "login required", "unauthenticated"];
const AUTH_HINT: &str = "AGY is not signed in. Open a terminal, run `agy` and sign in.";

/// AI failures. Messages are safe to show to the user.
#[derive(Debug, Clone)]
pub enum AiError {
    NotInstalled(String),
    Auth(String),
    Timeout(String),
    /// The model/service returned an error result.
    Model(String),
    /// The AI process crashed or produced unreadable output.
    Process(String),
}

impl AiError {
    pub fn kind(&self) -> &'static str {
        match self {
            AiError::NotInstalled(_) => "AINotInstalled",
            AiError::Auth(_) => "AIAuthError",
            AiError::Timeout(_) => "AITimeout",
            AiError::Model(_) => "AIModelError",
            AiError::Process(_) => "AIProcessError",
        }
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (AiError::NotInstalled(m) | AiError::Auth(m) | AiError::Timeout(m) | AiError::Model(m) | AiError::Process(m)) =
            self;
        f.write_str(m)
    }
}

impl std::error::Error for AiError {}

pub fn looks_like_auth_error(texts: &[&str], markers: &[&str]) -> bool {
    let blob = texts.join(" ").to_lowercase();
    markers.iter().any(|m| blob.contains(m))
}

pub fn find_agy() -> Result<PathBuf, AiError> {
    let candidates = [
        std::env::var_os("SSHPRO_AGY").map(PathBuf::from),
        which("agy"),
        std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("agy").join("bin").join("agy.exe")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .ok_or_else(|| AiError::NotInstalled(format!("AGY CLI not found. Install it: {AGY_INSTALL_URL}")))
}

/// Path of the AGY CLI, or None when it is not installed.
pub fn agy_available() -> Option<String> {
    find_agy().ok().map(|p| p.to_string_lossy().into_owned())
}

/// An empty, app-owned working directory so AGY never sees project files.
pub fn agy_workspace() -> PathBuf {
    let path = data_dir().join("agy_workspace");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// Run a short-lived CLI (model lists, one-shot turns) with a timeout; the process tree is killed
/// on timeout or when the caller is cancelled. Returns (exit code, stdout, stderr).
pub async fn run_cli(args: &[String], stdin: Option<&[u8]>, timeout: f64, cwd: Option<&PathBuf>, what: &str)
                     -> Result<(i32, String, String), AiError> {
    let mut cmd = command(&args[0]);
    cmd.args(&args[1..])
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| AiError::Process(format!("could not start {}: {e}", args[0])))?;
    let mut guard = KillGuard(child.id());
    let input = stdin.map(|s| s.to_vec());
    let mut child_in = child.stdin.take();
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let work = async {
        let feed = async {
            if let (Some(mut w), Some(data)) = (child_in.take(), input) {
                let _ = w.write_all(&data).await;
                let _ = w.shutdown().await;
            }
        };
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let (_, _, _, status) = tokio::join!(feed, out.read_to_end(&mut o), err.read_to_end(&mut e), child.wait());
        (status, o, e)
    };
    match tokio::time::timeout(Duration::from_secs_f64(timeout), work).await {
        Ok((status, o, e)) => {
            guard.disarm();
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            Ok((code, String::from_utf8_lossy(&o).into_owned(), String::from_utf8_lossy(&e).into_owned()))
        }
        Err(_) => Err(AiError::Timeout(format!("{what} did not answer within {timeout:.0}s"))),
    }
}

/// Models the signed-in AGY account can use, straight from `agy models`, so new ones show up
/// without a code change. Each line is `<id>\t<label>`; the "Fetching..." banner has no tab.
pub async fn list_models() -> Result<Vec<Value>, AiError> {
    let agy = find_agy()?.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_cli(&[agy, "models".into()], None, 60.0, None, "agy models").await?;
    let mut models = Vec::new();
    for line in stdout.lines() {
        if let Some((id, label)) = line.trim().split_once('\t') {
            if !id.is_empty() && !id.contains(' ') {
                let label = label.trim();
                models.push(json!({"id": id, "label": if label.is_empty() { id } else { label }}));
            }
        }
    }
    if models.is_empty() {
        if looks_like_auth_error(&[&stdout, &stderr], AUTH_MARKERS) {
            return Err(AiError::Auth(AUTH_HINT.into()));
        }
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        return Err(AiError::Process(format!("agy models returned no models (exit {code}): {}", tail_chars(detail, 300))));
    }
    Ok(models)
}

struct Inner {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<Lines<BufReader<ChildStdout>>>,
    stderr_task: Option<JoinHandle<()>>,
    turns: usize,
    last_usage: HashMap<String, i64>,
    /// set while a turn is in flight: if the turn is cancelled the session's state is unknown
    in_turn: bool,
}

/// One long-running `agy` stream-json conversation. Turns are serialized.
pub struct AgySession {
    command: Vec<String>,
    pub model: String,
    workdir: PathBuf,
    start_timeout: f64,
    turn_timeout: f64,
    max_turns: usize,
    instructions: Mutex<String>,
    inner: tokio::sync::Mutex<Inner>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
    rewarm: Mutex<Option<JoinHandle<()>>>,
}

impl AgySession {
    pub fn new(command: Vec<String>, model: String, workdir: PathBuf, start_timeout: f64, turn_timeout: f64) -> Arc<Self> {
        Arc::new(Self {
            command,
            model,
            workdir,
            start_timeout,
            turn_timeout,
            max_turns: 40,
            instructions: Mutex::new(String::new()),
            inner: tokio::sync::Mutex::new(Inner {
                child: None,
                stdin: None,
                stdout: None,
                stderr_task: None,
                turns: 0,
                last_usage: HashMap::new(),
                in_turn: false,
            }),
            stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
            rewarm: Mutex::new(None),
        })
    }

    fn has_instructions(&self) -> bool {
        !self.instructions.lock().unwrap().is_empty()
    }

    fn set_instructions(&self, text: &str) {
        *self.instructions.lock().unwrap() = text.to_string();
    }

    pub async fn start(&self, instructions: &str) -> Result<(), AiError> {
        self.set_instructions(instructions);
        let mut inner = self.inner.lock().await;
        self.ensure_started(&mut inner).await
    }

    fn alive(inner: &mut Inner) -> bool {
        inner.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    async fn ensure_started(&self, inner: &mut Inner) -> Result<(), AiError> {
        if Self::alive(inner) && inner.turns < self.max_turns && !inner.in_turn {
            return Ok(());
        }
        self.kill(inner).await;
        tracing::info!("Starting AGY session (model={})", self.model);
        let mut cmd = command(&self.command[0]);
        cmd.args(&self.command[1..])
            .args(["-p=", "--input-format", "stream-json", "--output-format", "stream-json", "--model", &self.model,
                   "--disable-slash-commands"])
            .current_dir(&self.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| AiError::Process(format!("could not start AGY: {e}")))?;
        inner.stdin = child.stdin.take();
        inner.stdout = child.stdout.take().map(|o| BufReader::with_capacity(1 << 16, o).lines());
        let stderr = child.stderr.take().unwrap();
        self.stderr_tail.lock().unwrap().clear();
        let tail = self.stderr_tail.clone();
        inner.stderr_task = Some(tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim_end().to_string();
                if !line.is_empty() {
                    let mut t = tail.lock().unwrap();
                    t.push_back(line);
                    if t.len() > 50 {
                        t.pop_front();
                    }
                }
            }
        }));
        inner.child = Some(child);
        inner.turns = 0;
        inner.last_usage.clear();
        let instructions = self.instructions.lock().unwrap().clone();
        if !instructions.is_empty() {
            // The standing instructions are the first turn; the model acknowledges them.
            self.turn(inner, &instructions, self.start_timeout).await?;
        }
        Ok(())
    }

    pub async fn ask(self: &Arc<Self>, message: &str) -> Result<String, AiError> {
        let reply;
        let turns;
        {
            let mut inner = self.inner.lock().await;
            self.ensure_started(&mut inner).await?;
            reply = self.turn(&mut inner, message, self.turn_timeout).await?;
            turns = inner.turns;
        }
        if turns >= self.max_turns {
            let mut rewarm = self.rewarm.lock().unwrap();
            if rewarm.as_ref().is_none_or(|h| h.is_finished()) {
                // used up: start the fresh session now (~10 s), so the next command doesn't wait for it
                let me = self.clone();
                *rewarm = Some(tokio::spawn(async move {
                    let mut inner = me.inner.lock().await;
                    if let Err(e) = me.ensure_started(&mut inner).await {
                        tracing::warn!("AGY background restart failed: {e}");
                    }
                }));
            }
        }
        Ok(reply)
    }

    fn stderr_text(&self) -> String {
        self.stderr_tail.lock().unwrap().iter().cloned().collect::<Vec<_>>().join("\n")
    }

    async fn turn(&self, inner: &mut Inner, text: &str, timeout: f64) -> Result<String, AiError> {
        let line = json!({"event": "user", "message": {"role": "user", "content": text}}).to_string();
        let started = Instant::now();
        inner.in_turn = true;
        let write = async {
            let stdin = inner.stdin.as_mut().ok_or_else(|| std::io::Error::other("no stdin"))?;
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        };
        if let Err(e) = write.await {
            self.kill(inner).await;
            return Err(AiError::Process(format!("AGY session closed unexpectedly: {e}")));
        }
        let result = match tokio::time::timeout(Duration::from_secs_f64(timeout), self.read_result(inner)).await {
            Err(_) => {
                self.kill(inner).await;
                return Err(AiError::Timeout(format!("AGY did not answer within {timeout:.0}s")));
            }
            Ok(Err(e)) => {
                self.kill(inner).await;
                return Err(e);
            }
            Ok(Ok(r)) => r,
        };
        inner.in_turn = false;
        inner.turns += 1;
        // AGY reports usage cumulatively per session; log the per-turn delta.
        let usage: HashMap<String, i64> = result
            .get("usage")
            .and_then(Value::as_object)
            .map(|u| u.iter().filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f as i64))).collect())
            .unwrap_or_default();
        let delta = |k: &str| usage.get(k).copied().unwrap_or(0) - inner.last_usage.get(k).copied().unwrap_or(0);
        tracing::info!(
            "AGY turn {}: {:.1}s (in={} out={} think={})",
            inner.turns, started.elapsed().as_secs_f64(), delta("input_tokens"), delta("output_tokens"), delta("thinking_tokens")
        );
        inner.last_usage = usage;
        self.check_result(&result)
    }

    async fn read_result(&self, inner: &mut Inner) -> Result<Value, AiError> {
        loop {
            let line = match inner.stdout.as_mut() {
                Some(lines) => lines.next_line().await.ok().flatten(),
                None => None,
            };
            let Some(line) = line else {
                let code = match inner.child.as_mut() {
                    Some(c) => c.wait().await.ok().and_then(|s| s.code()),
                    None => None,
                };
                // give the stderr reader a moment to catch the last lines
                tokio::time::sleep(Duration::from_millis(100)).await;
                let tail = self.stderr_text();
                if looks_like_auth_error(&[&tail], AUTH_MARKERS) {
                    return Err(AiError::Auth(AUTH_HINT.into()));
                }
                let code = code.map(|c| c.to_string()).unwrap_or_else(|| "None".into());
                return Err(AiError::Process(format!("AGY exited (code {code}). {}", tail_chars(&tail, 400))));
            };
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match event.get("event").and_then(Value::as_str) {
                Some("result") => {
                    return Ok(event.get("result").filter(|r| r.is_object()).cloned().unwrap_or_else(|| json!({})))
                }
                _ => continue,
            }
        }
    }

    fn check_result(&self, result: &Value) -> Result<String, AiError> {
        if result.get("denied_actions").is_some_and(|d| crate::util::truthy(Some(d))) {
            tracing::info!("AGY tried tools that were denied (expected): {}", result["denied_actions"]);
        }
        if result.get("status").and_then(Value::as_str) != Some("SUCCESS") {
            let error = result.get("error").map(|e| crate::util::val_str(Some(e))).filter(|e| !e.is_empty())
                .unwrap_or_else(|| "unknown error".into());
            if looks_like_auth_error(&[&error, &self.stderr_text()], AUTH_MARKERS) {
                return Err(AiError::Auth(AUTH_HINT.into()));
            }
            let status = result.get("status").cloned().unwrap_or(Value::Null);
            let status = status.as_str().map(String::from).unwrap_or_else(|| "None".into());
            return Err(AiError::Model(format!("AGY returned {status}: {error}")));
        }
        Ok(crate::util::val_str(result.get("response")))
    }

    async fn kill(&self, inner: &mut Inner) {
        inner.stdin = None;
        inner.stdout = None;
        inner.in_turn = false;
        if let Some(mut child) = inner.child.take() {
            if matches!(child.try_wait(), Ok(None)) {
                if let Some(pid) = child.id() {
                    let _ = tokio::task::spawn_blocking(move || kill_tree(pid)).await;
                }
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    tracing::warn!("AGY process did not exit after kill");
                }
            }
        }
        if let Some(task) = inner.stderr_task.take() {
            task.abort();
        }
    }

    /// After a cancelled request: a turn that was cut off left the session in an unknown state.
    pub async fn abandon_if_in_turn(&self) {
        let mut inner = self.inner.lock().await;
        if inner.in_turn {
            self.kill(&mut inner).await;
        }
    }

    pub async fn close(&self) {
        if let Some(h) = self.rewarm.lock().unwrap().take() {
            h.abort();
        }
        let mut inner = self.inner.lock().await;
        self.kill(&mut inner).await;
    }
}

/// Fallback: one process per request.
pub struct AgyOneShot {
    command: Vec<String>,
    model: String,
    workdir: PathBuf,
    timeout: f64,
    instructions: Mutex<String>,
}

impl AgyOneShot {
    pub async fn ask(&self, message: &str) -> Result<String, AiError> {
        let instructions = self.instructions.lock().unwrap().clone();
        let prompt = if instructions.is_empty() { message.to_string() } else { format!("{instructions}\n\n{message}") };
        let mut args = self.command.clone();
        args.extend([
            "-p".into(), prompt, "--output-format".into(), "json".into(), "--print-timeout".into(),
            format!("{}s", self.timeout as i64), "--model".into(), self.model.clone(), "--disable-slash-commands".into(),
        ]);
        let (code, stdout, stderr) = run_cli(&args, None, self.timeout + 15.0, Some(&self.workdir), "AGY").await?;
        let result = stdout
            .lines()
            .rev()
            .map(str::trim)
            .filter(|l| l.starts_with('{'))
            .find_map(|l| serde_json::from_str::<Value>(l).ok());
        let Some(result) = result else {
            if looks_like_auth_error(&[&stdout, &stderr], AUTH_MARKERS) {
                return Err(AiError::Auth(AUTH_HINT.into()));
            }
            return Err(AiError::Process(format!("AGY produced no JSON (exit {code}): {}", tail_chars(&stderr, 400))));
        };
        if result.get("status").and_then(Value::as_str) != Some("SUCCESS") {
            let mut error = crate::util::val_str(result.get("error"));
            if error.is_empty() {
                error = tail_chars(&stderr, 300);
            }
            if error.is_empty() {
                error = "unknown error".into();
            }
            if looks_like_auth_error(&[&error, &stderr], AUTH_MARKERS) {
                return Err(AiError::Auth(AUTH_HINT.into()));
            }
            return Err(AiError::Model(format!("AGY returned {}: {error}", result.get("status").cloned().unwrap_or(Value::Null))));
        }
        if let Some(structured) = result.get("structured_output").filter(|s| !s.is_null()) {
            return Ok(structured.to_string());
        }
        Ok(crate::util::val_str(result.get("response")))
    }
}

/// AI provider for AGY: persistent session first, restart once, then one-shot fallback.
///
/// A spare session is kept warm in the background: a fresh session spends ~10 s on the standing
/// instructions, so reset() (new chat, reopening an old one) swaps the spare in instead of waiting.
pub struct AgyProvider {
    command: Vec<String>,
    pub model: String,
    workdir: PathBuf,
    start_timeout: f64,
    turn_timeout: f64,
    session: Mutex<Arc<AgySession>>,
    spare: Mutex<Option<Arc<AgySession>>>,
    oneshot: AgyOneShot,
    oneshot_mode: Mutex<bool>,
    instructions: Mutex<String>,
    bg: Mutex<Vec<JoinHandle<()>>>, // spare warm-ups and old sessions being closed
}

impl AgyProvider {
    pub fn new(model: &str, workdir: PathBuf) -> Result<Arc<Self>, AiError> {
        let command = vec![find_agy()?.to_string_lossy().into_owned()];
        let (start_timeout, turn_timeout) = (120.0, 150.0);
        let session = AgySession::new(command.clone(), model.into(), workdir.clone(), start_timeout, turn_timeout);
        Ok(Arc::new(Self {
            oneshot: AgyOneShot {
                command: command.clone(),
                model: model.into(),
                workdir: workdir.clone(),
                timeout: 150.0,
                instructions: Mutex::new(String::new()),
            },
            command,
            model: model.into(),
            workdir,
            start_timeout,
            turn_timeout,
            session: Mutex::new(session),
            spare: Mutex::new(None),
            oneshot_mode: Mutex::new(false),
            instructions: Mutex::new(String::new()),
            bg: Mutex::new(Vec::new()),
        }))
    }

    fn new_session(&self) -> Arc<AgySession> {
        AgySession::new(self.command.clone(), self.model.clone(), self.workdir.clone(), self.start_timeout, self.turn_timeout)
    }

    fn session(&self) -> Arc<AgySession> {
        self.session.lock().unwrap().clone()
    }

    fn background<F: std::future::Future<Output = ()> + Send + 'static>(&self, fut: F) {
        let mut bg = self.bg.lock().unwrap();
        bg.retain(|h| !h.is_finished());
        bg.push(tokio::spawn(fut));
    }

    fn warm_spare(&self) {
        if *self.oneshot_mode.lock().unwrap() {
            return;
        }
        let mut spare = self.spare.lock().unwrap();
        if spare.is_none() {
            let session = self.new_session();
            *spare = Some(session.clone());
            let instructions = self.instructions.lock().unwrap().clone();
            drop(spare);
            self.background(async move {
                if let Err(e) = session.start(&instructions).await {
                    tracing::warn!("AGY spare session warm-up failed: {e}");
                }
            });
        }
    }

    pub async fn start(&self, instructions: &str) -> Result<(), AiError> {
        *self.instructions.lock().unwrap() = instructions.to_string();
        *self.oneshot.instructions.lock().unwrap() = instructions.to_string();
        match self.session().start(instructions).await {
            Ok(()) => *self.oneshot_mode.lock().unwrap() = false,
            Err(e @ (AiError::Process(_) | AiError::Timeout(_) | AiError::Model(_))) => {
                tracing::warn!("AGY session failed to start ({e}); using one-shot mode");
                *self.oneshot_mode.lock().unwrap() = true;
            }
            Err(e) => return Err(e),
        }
        self.warm_spare();
        Ok(())
    }

    pub async fn ask(&self, message: &str) -> Result<String, AiError> {
        if !*self.oneshot_mode.lock().unwrap() {
            // A timeout is raised straight away (the killed session restarts on the next ask);
            // only a crashed process is retried, then we fall back to one-shot.
            for attempt in 1..=2 {
                match self.session().ask(message).await {
                    Ok(reply) => return Ok(reply),
                    Err(AiError::Process(e)) => {
                        tracing::warn!("AGY session turn failed (attempt {attempt}): {e}");
                        if attempt == 2 {
                            tracing::warn!("Falling back to one-shot mode");
                            *self.oneshot_mode.lock().unwrap() = true;
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        self.oneshot.ask(message).await
    }

    /// Fresh conversation without the wait: the warm spare (or one still warming up) takes over.
    pub async fn reset(&self) {
        let next = self.spare.lock().unwrap().take().unwrap_or_else(|| self.new_session());
        if !next.has_instructions() {
            next.set_instructions(&self.instructions.lock().unwrap()); // its first ask() sends them
        }
        let old = std::mem::replace(&mut *self.session.lock().unwrap(), next);
        self.background(async move { old.close().await });
        self.warm_spare();
    }

    pub async fn after_cancel(&self) {
        self.session().abandon_if_in_turn().await;
    }

    pub async fn close(&self) {
        self.session().close().await;
        let spare = self.spare.lock().unwrap().take();
        if let Some(spare) = spare {
            spare.close().await; // its warm-up then ends with "AGY exited"
        }
        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *self.bg.lock().unwrap());
        let _ = tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(handles)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_errors_are_recognised() {
        assert!(looks_like_auth_error(&["Error: Not Authenticated"], AUTH_MARKERS));
        assert!(!looks_like_auth_error(&["all good"], AUTH_MARKERS));
        assert_eq!(AiError::Timeout("x".into()).kind(), "AITimeout");
    }
}
