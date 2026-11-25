//! The AI bot: the local AGY CLI (or ChatGPT through Codex) plans, this app executes on the servers
//! (and on this PC: local PowerShell steps, uploads and downloads), the AI analyses the results.
//!
//! The AI never runs anything itself. Every round it returns one JSON object; read-only steps run at
//! once, anything that may change a server waits for the user's approval (see safety::is_read_only).

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use futures::future::{BoxFuture, FutureExt, Shared};
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Map, Value};
use tokio::sync::{oneshot, Semaphore};
use tokio::task::{AbortHandle, JoinHandle};

use crate::agy::{agy_available, AgyProvider, AiError, AGY_INSTALL_URL};
use crate::codex::{self, CodexProvider};
use crate::db::{Database, Server};
use crate::localexec::run_local;
use crate::paths::{data_dir, home_dir};
use crate::router::{classify, pick_default, AUTO, FAST_PREFERENCE, SMART_PREFERENCE};
use crate::safety::{is_local_read_only, is_read_only, redact_secrets};
use crate::sshpool::{PipeProgress, Progress, SshManager};
use crate::util::{head_chars, human_size, positive_number, tail_chars, truthy, val_int, val_str, win_join, ExecResult};

/// Sends one event to the UI (already tagged with its chat window).
pub type Emit = Arc<dyn Fn(Value) + Send + Sync>;

pub const FAST_FALLBACK: &str = "gemini-3.8-flash-low"; // Auto's fast model before the model list has loaded
const SETTINGS_FILE: &str = "settings.json"; // in data_dir(); the model picked in the UI and Auto's two models
const MAX_ROUNDS: usize = 10; // migrations need room: inspect, prepare, copy, restore, verify
const PARALLEL: usize = 16;
const OUTPUT_TO_AI: usize = 2000; // chars of one step's output the AI sees: the head (headers) and the tail
const OUTPUT_HEAD: usize = 600;
const LOCAL_ID: i64 = 0; // "server" id of this PC in plan/result events
const LOCAL_NAME: &str = "This PC";
const HISTORY_TO_AI: usize = 8000; // chars of an earlier chat handed to a fresh AI session when it is reopened
/// events kept in the chat history (replayed by the UI when a chat is reopened)
const CHAT_EVENTS: &[&str] =
    &["chat.user", "ai.plan", "ai.decision", "ai.result", "ai.answer", "ai.error", "exec.start", "exec.result"];

pub const INSTRUCTIONS: &str = include_str!("instructions.txt");

pub fn default_model() -> String {
    std::env::var("SSHPRO_AGY_MODEL").unwrap_or_else(|_| "gemini-3.8-flash-medium".into())
}

pub fn no_brain() -> String {
    format!(
        "AI chat needs the AGY CLI ({AGY_INSTALL_URL}) or the Codex CLI signed in with ChatGPT ({}). \
         Install one, then restart the app.",
        codex::CODEX_INSTALL_URL
    )
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07|\r").unwrap());
static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)```(?:json)?\s*(.*?)```").unwrap());
static WINDOW_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{1,32}$").unwrap());
static UNSAFE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap());

/// job / batch ids, shared by every chat window and unique across restarts (they live on in chat history)
static IDS: LazyLock<AtomicI64> = LazyLock::new(|| {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    AtomicI64::new(secs as i64 * 1000)
});

fn next_id() -> i64 {
    IDS.fetch_add(1, Ordering::SeqCst)
}

/// Errors of the AI loop and the chat API; each maps to the Python exception class it replaces.
#[derive(Debug, Clone)]
pub enum BrainError {
    Ai(AiError),
    Value(String),
    Runtime(String),
    Key(String),
}

impl BrainError {
    pub fn kind(&self) -> &'static str {
        match self {
            BrainError::Ai(e) => e.kind(),
            BrainError::Value(_) => "ValueError",
            BrainError::Runtime(_) => "RuntimeError",
            BrainError::Key(_) => "KeyError",
        }
    }
}

impl std::fmt::Display for BrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrainError::Ai(e) => write!(f, "{e}"),
            BrainError::Value(m) | BrainError::Runtime(m) | BrainError::Key(m) => f.write_str(m),
        }
    }
}

impl From<AiError> for BrainError {
    fn from(e: AiError) -> Self {
        BrainError::Ai(e)
    }
}

/// A server name usable as a Windows folder name.
fn safe_name(name: &str) -> String {
    let s = UNSAFE_NAME.replace_all(name, "_");
    let s = s.trim_matches(|c| c == ' ' || c == '.');
    if s.is_empty() { "server".into() } else { s.to_string() }
}

pub fn extract_json(text: &str) -> Result<Value, BrainError> {
    let mut candidates: Vec<&str> = FENCE.captures_iter(text).map(|c| c.get(1).unwrap().as_str()).collect();
    candidates.push(text);
    for candidate in candidates {
        for (start, _) in candidate.match_indices('{') {
            let mut stream = serde_json::Deserializer::from_str(&candidate[start..]).into_iter::<Value>();
            if let Some(Ok(obj @ Value::Object(_))) = stream.next() {
                return Ok(obj);
            }
        }
    }
    Err(BrainError::Value("no JSON object in AI reply".into()))
}

/// What the AI sees of one output: no colour codes or blank lines, secrets masked, and when it
/// is long the first lines (headers) plus the end.
pub fn output_for_ai(output: &str) -> String {
    let text = redact_secrets(&ANSI.replace_all(output, ""));
    let text = text.lines().filter(|l| !l.trim().is_empty()).map(str::trim_end).collect::<Vec<_>>().join("\n");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= OUTPUT_TO_AI {
        return text;
    }
    let tail = OUTPUT_TO_AI - OUTPUT_HEAD;
    let cut = chars[OUTPUT_HEAD..chars.len() - tail].iter().filter(|c| **c == '\n').count();
    let head: String = chars[..OUTPUT_HEAD].iter().collect();
    let end: String = chars[chars.len() - tail..].iter().collect();
    format!("{head}\n…[{cut} lines cut]…\n{end}")
}

/// One entry per distinct (command, exit, error, output): twenty servers printing the same
/// version cost the AI one entry, not twenty.
pub fn group_results(results: &[Value]) -> Vec<Value> {
    let mut groups: IndexMap<String, Map<String, Value>> = IndexMap::new();
    for r in results {
        let get = |k: &str| r.get(k).cloned().unwrap_or(Value::Null);
        let key = json!([get("command"), get("exit"), get("error"), get("output")]).to_string();
        let entry = groups.entry(key).or_insert_with(|| {
            let mut e = Map::new();
            for (k, v) in [("cmd", get("command")), ("on", json!([])), ("exit", get("exit")), ("error", get("error")),
                           ("out", get("output"))] {
                if k == "exit" || !(v.is_null() || v == "") {
                    e.insert(k.into(), v);
                }
            }
            e
        });
        if let Some(Value::Array(on)) = entry.get_mut("on") {
            on.push(json!([get("server_id"), get("name")]));
        }
    }
    groups.into_values().map(Value::Object).collect()
}

fn compact(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

// ---- AI providers --------------------------------------------------------------------------

#[derive(Clone)]
pub enum Provider {
    Agy(Arc<AgyProvider>),
    Codex(Arc<CodexProvider>),
    #[cfg(test)]
    Script(Arc<tests::ScriptProvider>),
}

impl Provider {
    async fn start(&self, instructions: &str) -> Result<(), AiError> {
        match self {
            Provider::Agy(p) => p.start(instructions).await,
            Provider::Codex(p) => p.start(instructions).await,
            #[cfg(test)]
            Provider::Script(_) => Ok(()),
        }
    }

    async fn ask(&self, message: &str) -> Result<String, AiError> {
        match self {
            Provider::Agy(p) => p.ask(message).await,
            Provider::Codex(p) => p.ask(message).await,
            #[cfg(test)]
            Provider::Script(p) => p.ask(message),
        }
    }

    async fn reset(&self) {
        match self {
            Provider::Agy(p) => p.reset().await,
            Provider::Codex(p) => p.reset().await,
            #[cfg(test)]
            Provider::Script(_) => {}
        }
    }

    async fn close(&self) {
        match self {
            Provider::Agy(p) => p.close().await,
            Provider::Codex(p) => p.close().await,
            #[cfg(test)]
            Provider::Script(_) => {}
        }
    }

    async fn after_cancel(&self) {
        if let Provider::Agy(p) = self {
            p.after_cancel().await;
        }
    }
}

type Factory = Arc<dyn Fn(&str) -> Provider + Send + Sync>;

#[derive(Clone)]
struct StartTask {
    fut: Shared<BoxFuture<'static, Result<(), AiError>>>,
    abort: AbortHandle,
}

// ---- steps of one round ----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Local,
    Ssh,
    Upload,
    Download,
}

#[derive(Clone, Debug)]
struct Step {
    kind: Kind,
    command: String,
    cwd: Option<String>,
    timeout: Option<f64>,
    label: String,
    from: String,
    to: String,
}

#[derive(Clone, Debug)]
struct Cross {
    copy: bool, // copy, else pipe
    src: i64,
    dst: i64,
    timeout: f64,
    key: String,
    name: String,
    label: String,
    from: String,
    into: String,
    verify: String,
    sudo: bool,
    overwrite: bool,
    command: String,
    into_command: String,
}

enum Phase {
    Servers(IndexMap<i64, Vec<Step>>),
    Cross(Cross),
}

fn reads_only(step: &Step) -> bool {
    match step.kind {
        Kind::Local => is_local_read_only(&step.command),
        Kind::Ssh => is_read_only(&step.command),
        _ => false,
    }
}

/// Split the AI's steps into local (this PC) steps and per-server steps, keeping their order.
fn expand(steps: &[&Value], by_id: &IndexMap<i64, Server>) -> (Vec<Step>, IndexMap<i64, Vec<Step>>) {
    let mut local = Vec::new();
    let mut tasks: IndexMap<i64, Vec<Step>> = IndexMap::new();
    for step in steps {
        let timeout = positive_number(step.get("timeout"));
        if truthy(step.get("local")) {
            let command = if step["local"].is_string() { val_str(step.get("local")) } else { val_str(step.get("command")) };
            let command = command.trim().to_string();
            let cwd = Some(val_str(step.get("cwd")).trim().to_string()).filter(|c| !c.is_empty());
            if !command.is_empty() {
                let label = match &cwd {
                    Some(c) => format!("PS {c}> {command}"),
                    None => format!("PS> {command}"),
                };
                local.push(Step { kind: Kind::Local, command, cwd, timeout, label, from: String::new(), to: String::new() });
            }
            continue;
        }
        let (upload, download) = (step.get("upload"), step.get("download"));
        let entry = if let Some(Value::Object(d)) = download {
            let src = val_str(d.get("from")).trim().to_string();
            let mut dst = val_str(d.get("to")).trim().to_string();
            if dst.is_empty() {
                dst = win_join(&win_join(&home_dir().to_string_lossy(), "Downloads"), "");
            }
            if src.is_empty() {
                continue;
            }
            let label = format!("download {src} -> {dst}");
            Step { kind: Kind::Download, command: String::new(), cwd: None, timeout: None, label, from: src, to: dst }
        } else if let Some(Value::Object(u)) = upload {
            let (src, dst) = (val_str(u.get("from")).trim().to_string(), val_str(u.get("to")).trim().to_string());
            if src.is_empty() || dst.is_empty() {
                continue;
            }
            let label = format!("upload {src} -> {dst}");
            Step { kind: Kind::Upload, command: String::new(), cwd: None, timeout: None, label, from: src, to: dst }
        } else {
            let command = val_str(step.get("command")).trim().to_string();
            if command.is_empty() {
                continue;
            }
            Step { kind: Kind::Ssh, label: command.clone(), command, cwd: None, timeout, from: String::new(), to: String::new() }
        };
        let targets: Vec<Value> = match step.get("servers") {
            Some(Value::Array(t)) if !t.is_empty() => t.clone(),
            Some(v) if truthy(Some(v)) && !v.is_array() => vec![v.clone()],
            _ => vec![json!("all")],
        };
        let mut ids: Vec<i64> = Vec::new();
        if targets.iter().any(|t| val_str(Some(t)).to_lowercase() == "all") {
            ids = by_id.keys().copied().collect();
        } else {
            for t in &targets {
                match val_int(Some(t)) {
                    Some(i) if !t.is_boolean() => ids.push(i),
                    _ => ids.extend(by_id.iter().filter(|(_, s)| Some(s.name.as_str()) == t.as_str()).map(|(i, _)| *i)),
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let ids: Vec<i64> = ids.into_iter().filter(|i| seen.insert(*i) && by_id.contains_key(i)).collect();
        for sid in &ids {
            let mut e = entry.clone();
            if e.kind == Kind::Download && ids.len() > 1 {
                // a folder per server: copies never collide
                let folder = win_join(&win_join(&entry.to, &safe_name(&by_id[sid].name)), "");
                e.label = format!("download {} -> {folder}", entry.from);
                e.to = folder;
            }
            tasks.entry(*sid).or_default().push(e);
        }
    }
    (local, tasks)
}

/// A copy / pipe step between two of the target servers, or None if it is not valid.
fn cross(step: &Value, by_id: &IndexMap<i64, Server>, n: usize) -> Option<Cross> {
    let copy = step.get("copy").is_some_and(Value::is_object);
    let spec = if copy { &step["copy"] } else { &step["pipe"] };
    let server = |v: Option<&Value>| -> Option<i64> {
        let v = v?;
        let sid = match val_int(Some(v)) {
            Some(i) if !v.is_boolean() => Some(i),
            _ => by_id.iter().find(|(_, s)| Some(s.name.as_str()) == v.as_str()).map(|(i, _)| *i),
        }?;
        by_id.contains_key(&sid).then_some(sid)
    };
    let (src, dst) = (server(spec.get("from_server"))?, server(spec.get("to_server"))?);
    if src == dst {
        return None;
    }
    let timeout = positive_number(spec.get("timeout")).or_else(|| positive_number(step.get("timeout")));
    let timeout = timeout.map(|t| t.min(86400.0)).unwrap_or(6.0 * 3600.0);
    let (a, b) = (by_id[&src].name.clone(), by_id[&dst].name.clone());
    let mut entry = Cross {
        copy, src, dst, timeout, key: format!("x{n}"), name: format!("{a} → {b}"), label: String::new(),
        from: String::new(), into: String::new(), verify: String::new(), sudo: false, overwrite: false,
        command: String::new(), into_command: String::new(),
    };
    if copy {
        let from = val_str(spec.get("from")).trim().to_string();
        let mut into = val_str(spec.get("into")).trim().to_string();
        if into.is_empty() {
            into = val_str(spec.get("to")).trim().to_string();
        }
        if from.is_empty() || into.is_empty() {
            return None;
        }
        let verify = spec.get("verify").and_then(Value::as_str).filter(|v| ["checksum", "size", "none"].contains(v));
        let verify = verify.unwrap_or("checksum").to_string();
        entry.sudo = truthy(spec.get("sudo"));
        entry.overwrite = truthy(spec.get("overwrite"));
        let flags: String = [("sudo", entry.sudo), ("overwrite", entry.overwrite)]
            .iter()
            .filter(|(_, on)| *on)
            .map(|(f, _)| format!(", {f}"))
            .collect();
        entry.label = format!("copy {a}:{from} -> {b}:{into} (verify: {verify}{flags})");
        (entry.from, entry.into, entry.verify) = (from, into, verify);
    } else {
        let cmd = val_str(spec.get("command")).trim().to_string();
        let into_cmd = val_str(spec.get("into_command")).trim().to_string();
        if cmd.is_empty() || into_cmd.is_empty() {
            return None;
        }
        entry.label = format!("{a}$ {cmd}  ==>  {b}$ {into_cmd}");
        (entry.command, entry.into_command) = (cmd, into_cmd);
    }
    Some(entry)
}

/// Local steps, then the round in the AI's order: runs of ordinary server steps (each a
/// {server id: steps} map run in parallel) split by server-to-server copy / pipe steps.
fn phases(steps: &[Value], by_id: &IndexMap<i64, Server>) -> (Vec<Step>, Vec<Phase>) {
    let mut local = Vec::new();
    let mut phases = Vec::new();
    let mut chunk: Vec<&Value> = Vec::new();
    let flush = |chunk: &mut Vec<&Value>, local: &mut Vec<Step>, phases: &mut Vec<Phase>| {
        if !chunk.is_empty() {
            let (loc, tasks) = expand(chunk, by_id);
            local.extend(loc);
            if !tasks.is_empty() {
                phases.push(Phase::Servers(tasks));
            }
            chunk.clear();
        }
    };
    for step in steps {
        if step.get("copy").is_some_and(Value::is_object) || step.get("pipe").is_some_and(Value::is_object) {
            flush(&mut chunk, &mut local, &mut phases);
            if let Some(c) = cross(step, by_id, phases.len()) {
                phases.push(Phase::Cross(c));
            }
        } else {
            chunk.push(step);
        }
    }
    flush(&mut chunk, &mut local, &mut phases);
    (local, phases)
}

// ---- one chat window's brain -----------------------------------------------------------------

struct State {
    chat: Option<i64>,         // chat history id of the current conversation (created on its first message)
    owner: HashMap<i64, i64>,  // job / batch id -> chat id
    note: String,              // carried into the next request (e.g. "the last change was rejected")
    model: String,             // the dropdown choice; may be "auto"
    auto_fast: Option<String>,
    auto_smart: Option<String>,
    active: Option<String>,    // the model the running provider uses (under Auto it changes)
    tier: &'static str,        // Auto: a chat that needed the smart model stays on it
    labels: IndexMap<String, String>, // model id -> label, from the last models() call
    job_id: Option<i64>,
}

pub struct Brain {
    db: Arc<Database>,
    ssh: Arc<SshManager>,
    send: Emit,
    st: Mutex<State>,
    provider: Mutex<Option<Provider>>,
    start: Mutex<Option<StartTask>>,
    job: Mutex<Option<JoinHandle<()>>>,
    approval: Mutex<Option<oneshot::Sender<(bool, String)>>>,
    factory: Option<Factory>,
}

fn load_settings() -> Map<String, Value> {
    std::fs::read_to_string(data_dir().join(SETTINGS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_settings(changes: &[(&str, Value)]) {
    let mut data = load_settings();
    for (k, v) in changes {
        data.insert((*k).into(), v.clone());
    }
    let _ = std::fs::write(data_dir().join(SETTINGS_FILE), serde_json::to_string_pretty(&Value::Object(data)).unwrap());
}

fn setting(settings: &Map<String, Value>, key: &str) -> Option<String> {
    settings.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
}

impl Brain {
    fn with_factory(db: Arc<Database>, ssh: Arc<SshManager>, send: Emit, factory: Option<Factory>) -> Arc<Self> {
        let settings = load_settings();
        Arc::new(Self {
            db,
            ssh,
            send,
            st: Mutex::new(State {
                chat: None,
                owner: HashMap::new(),
                note: String::new(),
                model: setting(&settings, "model").unwrap_or_else(default_model),
                auto_fast: setting(&settings, "auto_fast"),
                auto_smart: setting(&settings, "auto_smart"),
                active: None,
                tier: "fast",
                labels: IndexMap::new(),
                job_id: None,
            }),
            provider: Mutex::new(None),
            start: Mutex::new(None),
            job: Mutex::new(None),
            approval: Mutex::new(None),
            factory,
        })
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn chat(&self) -> Option<i64> {
        self.st().chat
    }

    pub fn model(&self) -> String {
        self.st().model.clone()
    }

    pub fn auto(&self) -> Value {
        let st = self.st();
        json!({"fast": st.auto_fast, "smart": st.auto_smart})
    }

    // ---- chat history --------------------------------------------------------------------

    /// Send an event to the UI, tagged with its chat; history events are also stored.
    pub fn emit(&self, mut event: Value) {
        let key = val_int(event.get("job")).or_else(|| val_int(event.get("batch")));
        let chat = key.and_then(|k| self.st().owner.get(&k).copied());
        if let Some(chat) = chat {
            event["chat"] = json!(chat);
            if CHAT_EVENTS.contains(&event["type"].as_str().unwrap_or("")) {
                self.db.add_chat_event(chat, &event);
            }
        }
        (self.send)(event);
    }

    /// Record the user's message in the current chat (started now if needed); returns the job id.
    /// `scope` is the UI's target choice ({"spec": "folder:web", "ids": [...]}), so reopening the chat
    /// selects the same servers again.
    fn begin(&self, mode: &str, text: &str, target: Option<&str>, scope: Option<&Value>) -> i64 {
        let chat = {
            let mut st = self.st();
            if st.chat.is_none() {
                st.chat = Some(self.db.create_chat(text));
            }
            st.chat.unwrap()
        };
        let mut event = json!({"type": "chat.user", "mode": mode, "text": text, "target": target});
        if let Some(s) = scope {
            event["scope"] = s.clone();
        }
        self.db.add_chat_event(chat, &event);
        let job_id = next_id();
        self.st().owner.insert(job_id, chat);
        job_id
    }

    pub async fn new_chat(self: &Arc<Self>) {
        self.st().tier = "fast"; // Auto starts every chat on the fast model again
        let fast = if self.model() == AUTO { self.auto_target("fast").await } else { None };
        if let Some(fast) = fast.filter(|f| *f != self.current()) {
            // switch now (not on the first message) so the fast model is warm by then
            self.stop().await;
            {
                let mut st = self.st();
                st.note.clear();
                st.chat = None;
            }
            self.use_model(&fast).await;
            return;
        }
        self.reset().await;
        self.st().chat = None;
    }

    /// Continue an earlier chat: the AI starts fresh and gets its messages as context.
    pub async fn open_chat(self: &Arc<Self>, chat_id: i64) -> Result<Value, BrainError> {
        let chat = self.db.get_chat(chat_id).ok_or_else(|| BrainError::Key("chat not found".into()))?;
        if Some(chat_id) != self.chat() {
            self.reset().await;
            let mut st = self.st();
            st.chat = Some(chat_id);
            st.tier = "fast";
            st.note = history_note(chat["events"].as_array().map(Vec::as_slice).unwrap_or_default());
        }
        Ok(chat)
    }

    // ---- model choice --------------------------------------------------------------------

    /// Live lists from `agy models` and the Codex (ChatGPT) catalog; new models appear without a
    /// code change. A brain that is missing or fails only drops out of the list.
    pub async fn models(&self) -> Result<Vec<Value>, BrainError> {
        let mut sources: Vec<BoxFuture<'static, Result<Vec<Value>, AiError>>> = Vec::new();
        if agy_available().is_some() {
            sources.push(crate::agy::list_models().boxed());
        }
        if codex::codex_available().is_some() {
            sources.push(codex::list_models().boxed());
        }
        if sources.is_empty() {
            return Err(BrainError::Runtime(no_brain()));
        }
        let results = futures::future::join_all(sources).await;
        let models: Vec<Value> = results.iter().filter_map(|r| r.as_ref().ok()).flatten().cloned().collect();
        if models.is_empty() {
            let err = results.into_iter().find_map(|r| r.err()).unwrap();
            return Err(err.into());
        }
        for r in &results {
            if let Err(e) = r {
                tracing::warn!("Model list failed: {e}");
            }
        }
        self.st().labels = models
            .iter()
            .map(|m| (val_str(m.get("id")), val_str(m.get("label"))))
            .collect();
        Ok(models)
    }

    /// Switch model: the running AI session is closed and a new one starts with it.
    pub async fn set_model(self: &Arc<Self>, model: &str) {
        if model == self.model() {
            return;
        }
        self.close_provider().await;
        {
            let mut st = self.st();
            st.note.clear();
            st.chat = None;
            st.tier = "fast";
            if model == AUTO && st.model != AUTO && st.auto_smart.is_none() {
                st.auto_smart = Some(st.model.clone()); // the model the user trusted so far becomes the smart one
                save_settings(&[("auto_smart", json!(st.model))]);
            }
            st.model = model.to_string();
        }
        save_settings(&[("model", json!(model))]);
        tracing::info!("AI model changed to {model}");
        self.warm_up();
    }

    /// Models Auto uses; the next request switches to them.
    pub fn set_auto(&self, fast: &str, smart: &str) {
        let mut st = self.st();
        st.auto_fast = Some(fast.into());
        st.auto_smart = Some(smart.into());
        save_settings(&[("auto_fast", json!(fast)), ("auto_smart", json!(smart))]);
    }

    /// The fast and smart models Auto will use (the user's picks, else sensible defaults).
    pub async fn auto_models(&self) -> Value {
        json!({"fast": self.auto_target("fast").await, "smart": self.auto_target("smart").await})
    }

    async fn auto_target(&self, tier: &str) -> Option<String> {
        let picked = {
            let st = self.st();
            if tier == "fast" { st.auto_fast.clone() } else { st.auto_smart.clone() }
        };
        if picked.is_some() {
            return picked;
        }
        if self.st().labels.is_empty() {
            if let Err(e) = self.models().await {
                tracing::warn!("Auto could not list models: {e}"); // no list: fall back to the model that is running
            }
        }
        let st = self.st();
        let other = if tier == "fast" { st.auto_smart.clone() } else { st.auto_fast.clone() };
        let preference = if tier == "smart" { SMART_PREFERENCE } else { FAST_PREFERENCE };
        let ids: Vec<String> = st.labels.keys().cloned().collect();
        drop(st);
        pick_default(&ids, preference, other.as_deref()).or_else(|| Some(self.current()))
    }

    /// The model the provider runs (or will run when it starts).
    fn current(&self) -> String {
        let st = self.st();
        if let Some(a) = &st.active {
            return a.clone();
        }
        if st.model != AUTO {
            return st.model.clone();
        }
        st.auto_fast.clone().unwrap_or_else(|| {
            if agy_available().is_some() { FAST_FALLBACK.into() } else { format!("{}gpt-5.6-luna", codex::PREFIX) }
        })
    }

    fn label(&self) -> String {
        let model = self.current();
        if let Some(l) = self.st().labels.get(&model) {
            return l.clone();
        }
        match model.strip_prefix(codex::PREFIX) {
            Some(slug) => format!("ChatGPT · {slug}"),
            None => model,
        }
    }

    async fn close_provider(self: &Arc<Self>) {
        self.stop().await;
        if let Some(t) = self.start.lock().unwrap().take() {
            t.abort.abort();
        }
        let provider = self.provider.lock().unwrap().take();
        if let Some(p) = provider {
            p.close().await;
        }
        self.st().active = None;
    }

    /// Auto: run the next turn on `model`. A new session starts and gets this chat so far (up to
    /// the request being answered) as context, the same way a reopened chat does.
    async fn use_model(self: &Arc<Self>, model: &str) {
        let running = self.provider.lock().unwrap().is_some() || self.start.lock().unwrap().is_some();
        if model == self.current() && running {
            return;
        }
        if running {
            if let Some(t) = self.start.lock().unwrap().take() {
                t.abort.abort();
            }
            let provider = self.provider.lock().unwrap().take();
            if let Some(p) = provider {
                p.close().await;
            }
            if let Some(chat) = self.chat() {
                let events = self.db.get_chat(chat).and_then(|c| c["events"].as_array().cloned()).unwrap_or_default();
                let last_user = events.iter().rposition(|e| e["type"] == "chat.user").unwrap_or(events.len());
                let note = history_note(&events[..last_user]);
                let mut st = self.st();
                st.note = [st.note.as_str(), note.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join("\n");
            }
        }
        self.st().active = Some(model.to_string());
        tracing::info!("Auto: using {model}");
        self.warm_up();
    }

    // ---- AI lifecycle --------------------------------------------------------------------

    fn make_provider(&self) -> Result<Provider, AiError> {
        let model = self.current();
        self.st().active = Some(model.clone());
        if let Some(f) = &self.factory {
            return Ok(f(&model));
        }
        if codex::is_chatgpt(&model) {
            return Ok(Provider::Codex(Arc::new(CodexProvider::new(&model, codex::codex_workspace())?)));
        }
        Ok(Provider::Agy(AgyProvider::new(&model, crate::agy::agy_workspace())?))
    }

    /// The AI chat needs the local AGY CLI or the Codex CLI (ChatGPT); everything else in the app
    /// works without them.
    pub fn available(&self) -> bool {
        self.factory.is_some()
            || self.provider.lock().unwrap().is_some()
            || agy_available().is_some()
            || codex::codex_available().is_some()
    }

    pub fn warm_up(self: &Arc<Self>) {
        let mut start = self.start.lock().unwrap();
        if start.is_some() || !self.available() {
            return;
        }
        let me = self.clone();
        let handle = tokio::spawn(async move {
            let existing = me.provider.lock().unwrap().clone();
            let provider = match existing {
                Some(p) => p,
                None => {
                    let p = me.make_provider()?;
                    *me.provider.lock().unwrap() = Some(p.clone());
                    p
                }
            };
            provider.start(INSTRUCTIONS).await
        });
        let abort = handle.abort_handle();
        let fut = async move {
            match handle.await {
                Ok(r) => r,
                Err(_) => Err(AiError::Process("the AI session start was cancelled".into())),
            }
        }
        .boxed()
        .shared();
        *start = Some(StartTask { fut, abort });
    }

    async fn ready(self: &Arc<Self>) -> Result<Provider, BrainError> {
        self.warm_up();
        let task = self.start.lock().unwrap().clone();
        let Some(task) = task else {
            return Err(BrainError::Runtime(no_brain()));
        };
        if let Err(e) = task.fut.clone().await {
            *self.start.lock().unwrap() = None; // retry next time
            return Err(e.into());
        }
        self.provider.lock().unwrap().clone().ok_or_else(|| BrainError::Runtime(no_brain()))
    }

    pub async fn reset(self: &Arc<Self>) {
        self.stop().await;
        self.st().note.clear();
        let provider = self.provider.lock().unwrap().clone();
        if let Some(p) = provider {
            p.reset().await;
            *self.start.lock().unwrap() = None;
            self.warm_up();
        }
    }

    pub async fn close(self: &Arc<Self>) {
        self.stop().await;
        if let Some(t) = self.start.lock().unwrap().take() {
            t.abort.abort();
        }
        let provider = self.provider.lock().unwrap().clone();
        if let Some(p) = provider {
            p.close().await;
        }
    }

    // ---- jobs ----------------------------------------------------------------------------

    pub fn busy(&self) -> bool {
        self.job.lock().unwrap().as_ref().is_some_and(|h| !h.is_finished())
    }

    /// A planned change is waiting for the user's approval.
    pub fn waiting(&self) -> bool {
        self.approval.lock().unwrap().as_ref().is_some_and(|s| !s.is_closed())
    }

    pub fn ask(self: &Arc<Self>, prompt: &str, server_ids: Option<Vec<i64>>, target: Option<&str>,
               scope: Option<&Value>) -> Result<i64, BrainError> {
        if !self.available() {
            return Err(BrainError::Runtime(no_brain()));
        }
        if self.busy() {
            return Err(BrainError::Runtime("The AI is still working on the previous request.".into()));
        }
        let mut servers = self.db.list_servers();
        if let Some(ids) = server_ids.filter(|i| !i.is_empty()) {
            servers.retain(|s| ids.contains(&s.id));
        }
        if servers.is_empty() {
            return Err(BrainError::Value("No target servers.".into()));
        }
        let job_id = self.begin("ai", prompt, target, scope);
        self.st().job_id = Some(job_id);
        let me = self.clone();
        let prompt = prompt.to_string();
        *self.job.lock().unwrap() = Some(tokio::spawn(async move { me.run_job(job_id, prompt, servers).await }));
        Ok(job_id)
    }

    pub fn approve(&self, approved: bool, mode: &str) {
        if let Some(tx) = self.approval.lock().unwrap().take() {
            let _ = tx.send((approved, mode.to_string()));
        }
    }

    pub async fn stop(self: &Arc<Self>) {
        let handle = {
            let mut job = self.job.lock().unwrap();
            match job.as_ref() {
                Some(h) if !h.is_finished() => job.take(),
                _ => None,
            }
        };
        let Some(handle) = handle else {
            return;
        };
        handle.abort();
        let cancelled = matches!(handle.await, Err(e) if e.is_cancelled());
        self.approval.lock().unwrap().take();
        let job_id = self.st().job_id;
        if let (true, Some(job)) = (cancelled, job_id) {
            self.emit(json!({"type": "ai.status", "job": job, "state": "cancelled"}));
        }
        let provider = self.provider.lock().unwrap().clone();
        if let Some(p) = provider {
            p.after_cancel().await; // a turn cut off mid-way leaves the AI session unusable
        }
    }

    async fn ask_ai(self: &Arc<Self>, job_id: i64, message: &str, context: &str) -> Result<Value, BrainError> {
        match self.ask_once(job_id, message, context).await {
            Ok(v) => Ok(v),
            Err(e @ (BrainError::Ai(_) | BrainError::Value(_))) => {
                // Auto: when the fast model fails (timeout, crash, no valid JSON) the smart one takes the round.
                if self.model() != AUTO || self.st().tier == "smart" {
                    return Err(e);
                }
                let smart = self.auto_target("smart").await;
                let Some(smart) = smart.filter(|s| *s != self.current()) else {
                    return Err(e);
                };
                let failed = self.label();
                tracing::warn!("Auto: {failed} failed ({e}); switching to {smart}");
                self.st().tier = "smart";
                self.use_model(&smart).await;
                self.emit(json!({"type": "ai.status", "job": job_id, "state": "thinking", "model": self.label(),
                                 "text": format!("{failed} failed — {} is taking over…", self.label())}));
                let note = std::mem::take(&mut self.st().note);
                let first = if note.is_empty() { context.to_string() } else { format!("NOTE: {note}\n{context}") };
                let retry = if message == context { first } else { format!("{first}\n\n{message}") };
                self.ask_once(job_id, &retry, "").await
            }
            Err(e) => Err(e),
        }
    }

    async fn ask_once(self: &Arc<Self>, job_id: i64, message: &str, context: &str) -> Result<Value, BrainError> {
        let provider = self.ready().await?;
        let reply = match provider.ask(message).await {
            Ok(r) => r,
            Err(e @ (AiError::Timeout(_) | AiError::Process(_))) => {
                // The session was restarted and lost this conversation: ask again with the full context.
                tracing::warn!("AI turn failed ({e}); retrying with context");
                self.emit(json!({"type": "ai.status", "job": job_id, "state": "thinking", "text": "The AI was slow — retrying…"}));
                let again = if message.starts_with("NOTE") || message.starts_with("USER REQUEST") {
                    message.to_string()
                } else {
                    format!("{context}\n\n{message}")
                };
                provider.ask(&again).await?
            }
            Err(e) => return Err(e.into()),
        };
        match extract_json(&reply) {
            Ok(v) => Ok(v),
            Err(_) => {
                tracing::warn!("AI reply was not JSON: {}", head_chars(&reply, 300));
                let reply = provider.ask("Your last reply was not valid JSON. Reply again with ONLY the JSON object.").await?;
                extract_json(&reply)
            }
        }
    }

    /// Auto: switch to the fast or smart model for this request; returns the reason shown.
    async fn route(self: &Arc<Self>, prompt: &str) -> String {
        let (mut tier, mut why) = classify(prompt);
        if tier == "fast" && self.st().tier == "smart" {
            (tier, why) = ("smart", "this chat already needed it");
        }
        self.st().tier = tier;
        if let Some(model) = self.auto_target(tier).await {
            self.use_model(&model).await;
        }
        format!("Auto · {tier}: {why}")
    }

    async fn run_job(self: Arc<Self>, job_id: i64, prompt: String, servers: Vec<Server>) {
        if let Err(e) = self.run_job_inner(job_id, &prompt, servers).await {
            tracing::error!("AI job {job_id} failed: {e}");
            self.emit(json!({"type": "ai.error", "job": job_id, "text": format!("{}: {e}", e.kind())}));
            self.emit(json!({"type": "ai.status", "job": job_id, "state": "error"}));
        }
    }

    async fn run_job_inner(self: &Arc<Self>, job_id: i64, prompt: &str, servers: Vec<Server>) -> Result<(), BrainError> {
        let by_id: IndexMap<i64, Server> = servers.into_iter().map(|s| (s.id, s)).collect();
        let why = if self.model() == AUTO { self.route(prompt).await } else { String::new() };
        let inventory: Vec<Value> = by_id
            .values()
            .map(|s| {
                let mut m = Map::new();
                let os = s.facts.as_ref().and_then(|f| f.get("os")).cloned().unwrap_or(Value::Null);
                for (k, v) in [("id", json!(s.id)), ("name", json!(s.name)), ("folder", json!(s.folder)),
                               ("tags", json!(s.tags)), ("user", json!(s.username)), ("os", os),
                               ("status", json!(s.last_status))] {
                    if !(v.is_null() || v == "" || v == json!([])) {
                        m.insert(k.into(), v);
                    }
                }
                Value::Object(m)
            })
            .collect();
        let note = std::mem::take(&mut self.st().note);
        let mut message = format!(
            "{}USER REQUEST: {prompt}\n\nTODAY: {}\nTHIS PC: Windows, home folder {}\n\nTARGET SERVERS (use only these ids):\n{}",
            if note.is_empty() { String::new() } else { format!("NOTE: {note}\n") },
            chrono::Local::now().format("%Y-%m-%d"),
            home_dir().display(),
            compact(&Value::Array(inventory)),
        );
        let context = message.clone();
        for round_no in 1..=MAX_ROUNDS + 1 {
            self.emit(json!({"type": "ai.status", "job": job_id, "state": "thinking", "model": self.label(),
                             "why": if round_no == 1 { why.as_str() } else { "" }}));
            let plan = self.ask_ai(job_id, &message, &context).await?;
            let steps: Vec<Value> = plan.get("steps").and_then(Value::as_array).cloned().unwrap_or_default();
            if plan.get("action").and_then(Value::as_str) != Some("run") || steps.is_empty() || round_no > MAX_ROUNDS {
                let answer = plan.get("answer").cloned().unwrap_or(json!({}));
                let say = val_str(plan.get("say"));
                let summary = val_str(answer.get("summary"));
                self.emit(json!({"type": "ai.answer", "job": job_id, "say": say,
                                 "summary": if summary.is_empty() { say.clone() } else { summary },
                                 "table": answer.get("table").cloned().unwrap_or(Value::Null)}));
                self.emit(json!({"type": "ai.status", "job": job_id, "state": "done"}));
                return Ok(());
            }

            let (local, phases) = phases(&steps, &by_id);
            let has_servers = phases.iter().any(|p| matches!(p, Phase::Servers(t) if !t.is_empty()));
            let has_cross = phases.iter().any(|p| matches!(p, Phase::Cross(_)));
            if local.is_empty() && !has_servers && !has_cross {
                message = "None of your steps targeted a valid server id. Use only the given ids.".into();
                continue;
            }
            let all_read = local.iter().all(reads_only)
                && phases.iter().all(|p| match p {
                    Phase::Servers(t) => t.values().flatten().all(reads_only),
                    Phase::Cross(_) => false,
                });
            let changes = plan.get("risk").and_then(Value::as_str) == Some("change") || !all_read;
            let rollout = plan.get("rollout").and_then(Value::as_str).filter(|r| *r == "canary" || *r == "parallel");
            let mut rollout = rollout.unwrap_or("canary").to_string();
            let mut tasks_view: Vec<Value> = Vec::new();
            if !local.is_empty() {
                let cmds: Vec<&str> = local.iter().map(|s| s.label.as_str()).collect();
                tasks_view.push(json!({"server_id": LOCAL_ID, "name": LOCAL_NAME, "commands": cmds}));
            }
            // one entry per server (its steps from every phase, in order), then the cross steps
            let mut per_server: IndexMap<i64, Vec<String>> = IndexMap::new();
            for p in &phases {
                if let Phase::Servers(t) = p {
                    for (sid, steps) in t {
                        per_server.entry(*sid).or_default().extend(steps.iter().map(|s| s.label.clone()));
                    }
                }
            }
            for (sid, cmds) in &per_server {
                tasks_view.push(json!({"server_id": sid, "name": by_id[sid].name, "commands": cmds}));
            }
            for p in &phases {
                if let Phase::Cross(c) = p {
                    tasks_view.push(json!({"server_id": c.key, "name": c.name, "commands": [c.label]}));
                }
            }
            self.emit(json!({
                "type": "ai.plan", "job": job_id, "round": round_no, "say": val_str(plan.get("say")),
                "risk": if changes { "change" } else { "read" }, "needs_approval": changes, "rollout": rollout,
                "tasks": tasks_view,
            }));
            if changes {
                self.emit(json!({"type": "ai.status", "job": job_id, "state": "waiting"}));
                let (tx, rx) = oneshot::channel();
                *self.approval.lock().unwrap() = Some(tx);
                let (approved, mode) = rx.await.unwrap_or((false, "canary".into()));
                rollout = mode;
                self.emit(json!({"type": "ai.decision", "job": job_id, "round": round_no, "approve": approved, "mode": rollout}));
                if !approved {
                    self.st().note = "The user REJECTED your last proposed change; it was not run.".into();
                    self.emit(json!({"type": "ai.answer", "job": job_id, "say": "",
                                     "summary": "Change cancelled — nothing was run.", "table": null}));
                    self.emit(json!({"type": "ai.status", "job": job_id, "state": "cancelled"}));
                    return Ok(());
                }
            }

            self.emit(json!({"type": "ai.status", "job": job_id, "state": "running"}));
            let results = self
                .execute(job_id, round_no, prompt, &local, &phases, &by_id, changes && rollout == "canary")
                .await;
            message = format!(
                "RESULTS of round {round_no} (exit null = not run or unreachable):\n{}\n\nReply with the next JSON (more steps, or action \"answer\").",
                compact(&Value::Array(group_results(&results)))
            );
        }
        Ok(())
    }

    /// SFTP progress callback: at most one status update per second per server.
    fn progress(self: &Arc<Self>, job_id: i64, name: &str, verb: &'static str, where_: &'static str) -> Progress {
        let me = self.clone();
        let name = name.to_string();
        let last: Mutex<Option<Instant>> = Mutex::new(None);
        Arc::new(move |src: &str, _dst: &str, done: u64, total: u64| {
            {
                let mut last = last.lock().unwrap();
                if last.is_some_and(|l| l.elapsed().as_secs_f64() < 1.0) && done < total {
                    return;
                }
                *last = Some(Instant::now());
            }
            let file = src.rsplit(['/', '\\']).next().unwrap_or(src);
            let pct = if total > 0 { format!("{:.0}% ", 100.0 * done as f64 / total as f64) } else { String::new() };
            let text = format!("{verb} {file} {where_} {name}:{pct}({} / {})", human_size(done as f64), human_size(total as f64));
            me.emit(json!({"type": "ai.status", "job": job_id, "state": "running", "text": text}));
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute(self: &Arc<Self>, job_id: i64, round_no: usize, prompt: &str, local: &[Step], phases: &[Phase],
                     by_id: &IndexMap<i64, Server>, canary: bool) -> Vec<Value> {
        let results: Mutex<Vec<Value>> = Mutex::new(Vec::new());
        let sem = Semaphore::new(PARALLEL);

        // Audit, show and collect one step's result; false if the steps after it must not run.
        let record = |sid: Value, audit_id: Option<i64>, name: &str, label: &str, read: bool, res: &ExecResult| -> bool {
            let output = res.output();
            self.db.audit("ai", label, Some((audit_id, name)), Some(prompt), res.exit, Some(res.duration), &output);
            self.emit(json!({"type": "ai.result", "job": job_id, "round": round_no, "server_id": sid, "name": name,
                             "command": label, "exit": res.exit, "error": res.error, "duration": res.duration,
                             "output": tail_chars(&output, 20000)}));
            let shown_id = if sid == json!(LOCAL_ID) { json!("local") } else { sid };
            results.lock().unwrap().push(json!({"server_id": shown_id, "name": name, "command": label, "exit": res.exit,
                                                "error": res.error, "output": output_for_ai(&output)}));
            // a read step may legitimately exit non-zero (grep found nothing); a failed change stops
            !(res.error.is_some() || (res.exit != Some(0) && !read))
        };
        let skip = |sids: &[i64], why: &str| {
            let mut r = results.lock().unwrap();
            for sid in sids {
                r.push(json!({"server_id": sid, "name": by_id[sid].name, "exit": null, "error": format!("skipped: {why}"), "output": ""}));
            }
        };
        let skip_phases = |rest: &[Phase], why: &str| {
            for p in rest {
                match p {
                    Phase::Servers(t) => skip(&t.keys().copied().collect::<Vec<_>>(), why),
                    Phase::Cross(c) => results.lock().unwrap().push(json!({
                        "server_id": c.key, "name": c.name, "command": c.label, "exit": null,
                        "error": format!("skipped: {why}"), "output": ""})),
                }
            }
        };

        for step in local {
            let read = reads_only(step);
            let timeout = step.timeout.unwrap_or(if read { 60.0 } else { 1800.0 });
            self.emit(json!({"type": "ai.status", "job": job_id, "state": "running",
                             "text": format!("Running on this PC: {}", head_chars(&step.command, 80))}));
            let res = run_local(&step.command, step.cwd.as_deref(), timeout.min(3600.0)).await;
            if !record(json!(LOCAL_ID), None, LOCAL_NAME, &step.label, read, &res) {
                skip_phases(phases, "a local step failed");
                return results.into_inner().unwrap();
            }
        }

        let run_server = |tasks: &IndexMap<i64, Vec<Step>>, sid: i64| {
            let steps = tasks[&sid].clone();
            let server = by_id[&sid].clone();
            let (sem, record) = (&sem, &record);
            async move {
                let _permit = sem.acquire().await.unwrap();
                for step in &steps {
                    let res = match step.kind {
                        Kind::Upload => {
                            let p = self.progress(job_id, &server.name, "Uploading", "to");
                            self.ssh.upload(sid, &step.from, &step.to, Some(p)).await
                        }
                        Kind::Download => {
                            let p = self.progress(job_id, &server.name, "Downloading", "from");
                            self.ssh.download(sid, &step.from, &step.to, Some(p)).await
                        }
                        _ => {
                            let t = step.timeout.unwrap_or(if canary || !is_read_only(&step.command) { 900.0 } else { 90.0 });
                            self.ssh.run(sid, &step.command, t.min(1800.0), false).await
                        }
                    };
                    if !record(json!(sid), Some(sid), &server.name, &step.label, reads_only(step), &res) {
                        return false;
                    }
                }
                true
            }
        };

        for (i, phase) in phases.iter().enumerate() {
            let ok = match phase {
                Phase::Servers(tasks) => {
                    let mut order: Vec<i64> = tasks.keys().copied().collect();
                    let mut ok = true;
                    if canary && order.len() > 1 {
                        let first = order.remove(0);
                        self.emit(json!({"type": "ai.status", "job": job_id, "state": "running",
                                         "text": format!("Canary: {} first", by_id[&first].name)}));
                        if !run_server(tasks, first).await {
                            skip(&order, "canary server failed");
                            order.clear();
                            ok = false;
                        }
                    }
                    if ok {
                        let all = futures::future::join_all(order.iter().map(|sid| run_server(tasks, *sid))).await;
                        all.into_iter().all(|x| x)
                    } else {
                        false
                    }
                }
                Phase::Cross(step) => {
                    let verb = if step.copy { "Copying" } else { "Streaming" };
                    let me = self.clone();
                    let name = step.name.clone();
                    let last = Mutex::new(0.0f64);
                    let report: PipeProgress = Arc::new(move |sent: u64, elapsed: f64| {
                        {
                            let mut last = last.lock().unwrap();
                            if elapsed - *last < 1.0 {
                                return;
                            }
                            *last = elapsed;
                        }
                        let rate = if elapsed > 0.0 { sent as f64 / elapsed } else { 0.0 };
                        let text = format!("{verb} {name}: {} sent ({}/s)", human_size(sent as f64), human_size(rate));
                        me.emit(json!({"type": "ai.status", "job": job_id, "state": "running", "text": text}));
                    });
                    self.emit(json!({"type": "ai.status", "job": job_id, "state": "running",
                                     "text": format!("{} {}…", if step.copy { "Checking and copying" } else { "Streaming" }, step.name)}));
                    let res = if step.copy {
                        self.ssh.copy(step.src, &step.from, step.dst, &step.into, &step.verify, step.sudo, step.overwrite,
                                      step.timeout, Some(report)).await
                    } else {
                        self.ssh.pipe(step.src, &step.command, step.dst, &step.into_command, step.timeout, Some(report)).await
                    };
                    record(json!(step.key), Some(step.dst), &step.name, &step.label, false, &res)
                }
            };
            if !ok && i + 1 < phases.len() {
                skip_phases(&phases[i + 1..], "an earlier step in this round failed");
                break;
            }
        }
        results.into_inner().unwrap()
    }

    // ---- direct (manual) multi-server command ---------------------------------------------

    /// Record the command in the chat now (so it keeps its place), then run it in the background.
    pub fn direct(self: &Arc<Self>, command: &str, server_ids: Vec<i64>, target: Option<&str>, scope: Option<&Value>) {
        let batch = self.begin("direct", command, target, scope);
        let me = self.clone();
        let command = command.to_string();
        tokio::spawn(async move { me.run_direct(&command, &server_ids, batch).await });
    }

    pub async fn run_direct(self: &Arc<Self>, command: &str, server_ids: &[i64], batch: i64) {
        let servers: IndexMap<i64, Server> =
            self.db.list_servers().into_iter().filter(|s| server_ids.contains(&s.id)).map(|s| (s.id, s)).collect();
        let sem = Semaphore::new(PARALLEL);
        let list: Vec<Value> = servers.values().map(|s| json!({"server_id": s.id, "name": s.name})).collect();
        self.emit(json!({"type": "exec.start", "batch": batch, "command": command, "servers": list}));
        futures::future::join_all(servers.values().map(|s| {
            let sem = &sem;
            async move {
                let res = {
                    let _permit = sem.acquire().await.unwrap();
                    self.ssh.run(s.id, command, 600.0, false).await
                };
                let output = res.output();
                self.db.audit("manual", command, Some((Some(s.id), &s.name)), None, res.exit, Some(res.duration), &output);
                self.emit(json!({"type": "exec.result", "batch": batch, "server_id": s.id, "name": s.name,
                                 "command": command, "exit": res.exit, "error": res.error, "duration": res.duration,
                                 "output": tail_chars(&output, 20000)}));
            }
        }))
        .await;
        self.emit(json!({"type": "exec.done", "batch": batch}));
    }
}

fn history_note(events: &[Value]) -> String {
    let mut lines = Vec::new();
    for ev in events {
        match ev["type"].as_str() {
            Some("chat.user") => {
                let direct = if ev.get("mode").and_then(Value::as_str) == Some("direct") { " (ran directly)" } else { "" };
                lines.push(format!("USER{direct}: {}", val_str(ev.get("text"))));
            }
            Some("ai.answer") => {
                let table = ev.get("table").cloned().unwrap_or(Value::Null);
                let rows = match (&table, table.get("rows")) {
                    (Value::Object(_), Some(Value::Array(r))) if !r.is_empty() => {
                        let mut all = vec![table.get("columns").cloned().unwrap_or(Value::Null)];
                        all.extend(r.iter().cloned());
                        format!(" TABLE {}", compact(&Value::Array(all)))
                    }
                    _ => String::new(),
                };
                let mut said = val_str(ev.get("summary"));
                if said.is_empty() {
                    said = val_str(ev.get("say"));
                }
                lines.push(format!("YOU: {said}{rows}"));
            }
            _ => {}
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "This chat continues an earlier conversation (the AI session was restarted). Earlier messages:\n{}",
        tail_chars(&lines.join("\n"), HISTORY_TO_AI)
    )
}

// ---- several chat windows at once -----------------------------------------------------------

pub const MAIN: &str = "main";
pub const MAX_WINDOWS: usize = 6;

/// Chat windows that work at the same time. Each window is a Brain of its own (its own AI
/// session, running job, pending approval and chat), so one window can upgrade the web servers
/// while another checks the databases. The model choice is shared by all of them.
pub struct Windows {
    db: Arc<Database>,
    ssh: Arc<SshManager>,
    emit: Emit,
    pub main: Arc<Brain>,
    all: Mutex<IndexMap<String, Arc<Brain>>>,
    factory: Option<Factory>,
}

fn tagged(emit: &Emit, win: &str) -> Emit {
    let emit = emit.clone();
    let win = win.to_string();
    Arc::new(move |mut event: Value| {
        event["win"] = json!(win);
        emit(event)
    })
}

impl Windows {
    pub fn new(db: Arc<Database>, ssh: Arc<SshManager>, emit: Emit) -> Self {
        Self::with_factory(db, ssh, emit, None)
    }

    fn with_factory(db: Arc<Database>, ssh: Arc<SshManager>, emit: Emit, factory: Option<Factory>) -> Self {
        let main = Brain::with_factory(db.clone(), ssh.clone(), tagged(&emit, MAIN), factory.clone());
        let mut all = IndexMap::new();
        all.insert(MAIN.to_string(), main.clone());
        Self { db, ssh, emit, main, all: Mutex::new(all), factory }
    }

    pub fn list(&self) -> Vec<(String, Arc<Brain>)> {
        self.all.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn get(&self, win: Option<&str>, create: bool) -> Result<Arc<Brain>, BrainError> {
        let win = win.filter(|w| !w.is_empty()).unwrap_or(MAIN);
        let mut all = self.all.lock().unwrap();
        if let Some(b) = all.get(win) {
            return Ok(b.clone());
        }
        if !WINDOW_ID.is_match(win) {
            return Err(BrainError::Value("invalid chat window id".into()));
        }
        if !create {
            return Err(BrainError::Key("chat window not found".into()));
        }
        if all.len() >= MAX_WINDOWS {
            return Err(BrainError::Runtime(format!("At most {MAX_WINDOWS} chat windows can work at once — close one first")));
        }
        let brain = Brain::with_factory(self.db.clone(), self.ssh.clone(), tagged(&self.emit, win), self.factory.clone());
        {
            let main = self.main.st();
            let mut st = brain.st();
            st.model = main.model.clone();
            st.auto_fast = main.auto_fast.clone();
            st.auto_smart = main.auto_smart.clone();
            st.labels = main.labels.clone();
        }
        all.insert(win.to_string(), brain.clone());
        tracing::info!("Chat window {win} opened ({} open)", all.len());
        Ok(brain)
    }

    /// The window that has this chat open, if any.
    pub fn holder(&self, chat_id: i64) -> Option<String> {
        self.list().into_iter().find(|(_, b)| b.chat() == Some(chat_id)).map(|(w, _)| w)
    }

    pub fn state(&self) -> Value {
        let mut m = Map::new();
        for (win, b) in self.list() {
            m.insert(win, json!({"chat": b.chat(), "busy": b.busy(), "waiting": b.waiting()}));
        }
        Value::Object(m)
    }

    pub async fn open_chat(&self, win: Option<&str>, chat_id: i64) -> Result<Value, BrainError> {
        let brain = self.get(win, true)?;
        if let Some(other) = self.holder(chat_id) {
            let holder = self.all.lock().unwrap().get(&other).cloned();
            if holder.is_some_and(|h| !Arc::ptr_eq(&h, &brain)) {
                return Err(BrainError::Runtime("This chat is already open in another window".into()));
            }
        }
        if brain.busy() && Some(chat_id) != brain.chat() {
            return Err(BrainError::Runtime("The AI is still working in this window — stop it or wait".into()));
        }
        brain.open_chat(chat_id).await
    }

    pub async fn delete_chat(&self, chat_id: i64) -> Result<(), BrainError> {
        if let Some(win) = self.holder(chat_id) {
            let brain = self.all.lock().unwrap().get(&win).cloned();
            if let Some(brain) = brain {
                if brain.busy() {
                    return Err(BrainError::Runtime("The AI is still working in this chat — stop it or wait".into()));
                }
                brain.new_chat().await;
            }
        }
        self.db.delete_chat(chat_id);
        Ok(())
    }

    pub async fn close_window(&self, win: &str) {
        let win = if win.is_empty() { MAIN } else { win };
        if win == MAIN {
            self.main.new_chat().await; // the main window's session stays warm for the next chat
            return;
        }
        let brain = self.all.lock().unwrap().shift_remove(win);
        if let Some(b) = brain {
            b.close().await;
            tracing::info!("Chat window {win} closed ({} open)", self.all.lock().unwrap().len());
        }
    }

    pub async fn models(&self) -> Result<Vec<Value>, BrainError> {
        let models = self.main.models().await?;
        let labels = self.main.st().labels.clone();
        for (_, b) in self.list() {
            b.st().labels = labels.clone();
        }
        Ok(models)
    }

    /// The model applies to every window; each starts a fresh chat with it.
    pub async fn set_model(&self, model: &str) {
        for (_, b) in self.list() {
            b.set_model(model).await;
        }
    }

    pub fn set_auto(&self, fast: &str, smart: &str) {
        for (_, b) in self.list() {
            b.set_auto(fast, smart);
        }
    }

    pub async fn close(&self) {
        for (_, b) in self.list() {
            b.close().await;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A provider that answers with scripted replies and records what it was asked.
    pub struct ScriptProvider {
        pub replies: Mutex<VecDeque<String>>,
        pub asked: Mutex<Vec<String>>,
    }

    impl ScriptProvider {
        pub fn ask(&self, message: &str) -> Result<String, AiError> {
            self.asked.lock().unwrap().push(message.to_string());
            self.replies.lock().unwrap().pop_front().ok_or_else(|| AiError::Process("no more replies".into()))
        }
    }

    #[test]
    fn extracts_json_from_replies() {
        assert_eq!(extract_json("sure: ```json\n{\"a\": 1}\n``` done").unwrap(), json!({"a": 1}));
        assert_eq!(extract_json("x {not json} {\"b\": [1]} trailing").unwrap(), json!({"b": [1]}));
        assert!(extract_json("nothing here").is_err());
    }

    #[test]
    fn output_for_ai_keeps_head_and_tail_and_masks() {
        let lines: Vec<String> = (0..2000).map(|i| format!("line {i}")).collect();
        let out = format!("\x1b[32mHEADER\x1b[0m\n\n{}\npassword=topsecret", lines.join("\n"));
        let text = output_for_ai(&out);
        assert!(text.starts_with("HEADER\nline 0") && text.ends_with("password=[REDACTED]"), "{text}");
        assert!(text.contains("lines cut]") && text.chars().count() < 2100 && !text.contains('\x1b'));
    }

    #[test]
    fn group_results_merges_identical_outputs() {
        let mut rows: Vec<Value> = (1..=3)
            .map(|i| json!({"server_id": i, "name": format!("s{i}"), "command": "node -v", "exit": 0, "error": null, "output": "v22"}))
            .collect();
        rows.push(json!({"server_id": 4, "name": "s4", "command": "node -v", "exit": 127, "error": null, "output": ""}));
        assert_eq!(
            Value::Array(group_results(&rows)),
            json!([
                {"cmd": "node -v", "on": [[1, "s1"], [2, "s2"], [3, "s3"]], "exit": 0, "out": "v22"},
                {"cmd": "node -v", "on": [[4, "s4"]], "exit": 127},
            ])
        );
    }

    fn setup(replies: &[&str]) -> (Arc<Database>, Windows, Arc<Mutex<Vec<Value>>>, Arc<ScriptProvider>) {
        let dir = std::env::temp_dir().join(format!("sshpro-test-{}-{}", std::process::id(), next_id()));
        std::env::set_var("SSHPRO_DATA_DIR", &dir);
        let db = Arc::new(Database::open_memory().unwrap());
        for name in ["web-1", "web-2"] {
            let data = json!({"name": name, "host": "127.0.0.1", "port": 1, "username": "u"});
            db.create_server(data.as_object().unwrap(), None, None).unwrap();
        }
        let ssh = Arc::new(SshManager::new(db.clone()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let emit: Emit = Arc::new(move |e| sink.lock().unwrap().push(e));
        let script = Arc::new(ScriptProvider {
            replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
            asked: Mutex::new(Vec::new()),
        });
        let p = script.clone();
        let factory: Factory = Arc::new(move |_| Provider::Script(p.clone()));
        let windows = Windows::with_factory(db.clone(), ssh, emit, Some(factory));
        windows.main.st().model = "test-model".into();
        (db, windows, events, script)
    }

    async fn wait_done(brain: &Arc<Brain>) {
        for _ in 0..500 {
            if !brain.busy() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("job did not finish");
    }

    fn of_type<'a>(events: &'a [Value], t: &str) -> Vec<&'a Value> {
        events.iter().filter(|e| e["type"] == t).collect()
    }

    #[tokio::test]
    async fn answers_without_steps_and_records_the_chat() {
        let (db, windows, events, script) = setup(&[r#"{"say":"hi","action":"answer","answer":{"summary":"Hello!"}}"#]);
        let brain = windows.main.clone();
        let job = brain.ask("hello", None, Some("all servers"), None).unwrap();
        wait_done(&brain).await;
        let events = events.lock().unwrap().clone();
        let answer = of_type(&events, "ai.answer");
        assert_eq!(answer[0]["summary"], "Hello!");
        assert_eq!(answer[0]["job"], job);
        assert_eq!(answer[0]["win"], "main");
        assert_eq!(of_type(&events, "ai.status").last().unwrap()["state"], "done");
        let asked = script.asked.lock().unwrap();
        assert!(asked[0].starts_with("USER REQUEST: hello") && asked[0].contains("\"name\":\"web-1\""));
        let chat = db.get_chat(brain.chat().unwrap()).unwrap();
        let kinds: Vec<&str> = chat["events"].as_array().unwrap().iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["chat.user", "ai.answer"]);
    }

    #[tokio::test]
    async fn changes_wait_for_approval_and_rejection_runs_nothing() {
        let (_db, windows, events, script) = setup(&[
            r#"{"say":"restarting","action":"run","risk":"read","rollout":"parallel","steps":[{"servers":["all"],"command":"systemctl restart nginx"}]}"#,
            r#"{"say":"ok","action":"answer","answer":{"summary":"done"}}"#,
        ]);
        let brain = windows.main.clone();
        brain.ask("restart nginx", None, None, None).unwrap();
        for _ in 0..200 {
            if brain.waiting() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(brain.waiting(), "a change must wait for approval even if the AI calls it a read");
        let plan = of_type(&events.lock().unwrap(), "ai.plan")[0].clone();
        assert_eq!(plan["needs_approval"], true);
        assert_eq!(plan["risk"], "change");
        assert_eq!(plan["tasks"].as_array().unwrap().len(), 2);
        brain.approve(false, "canary");
        wait_done(&brain).await;
        let events = events.lock().unwrap().clone();
        assert!(of_type(&events, "ai.result").is_empty());
        assert_eq!(of_type(&events, "ai.answer")[0]["summary"], "Change cancelled — nothing was run.");
        assert_eq!(script.asked.lock().unwrap().len(), 1);
        assert!(brain.st().note.contains("REJECTED"));
    }

    #[tokio::test]
    async fn read_steps_run_at_once_and_results_go_back_grouped() {
        let (_db, windows, events, script) = setup(&[
            r#"{"say":"checking","action":"run","risk":"read","steps":[{"servers":["all"],"command":"df -h"}]}"#,
            r#"{"say":"ok","action":"answer","answer":{"summary":"both unreachable"}}"#,
        ]);
        let brain = windows.main.clone();
        brain.ask("disk?", None, None, None).unwrap();
        wait_done(&brain).await;
        let events = events.lock().unwrap().clone();
        let results = of_type(&events, "ai.result");
        assert_eq!(results.len(), 2); // port 1 refuses: an error result per server, no approval asked
        assert!(results.iter().all(|r| r["error"].is_string()));
        let asked = script.asked.lock().unwrap();
        assert!(asked[1].starts_with("RESULTS of round 1"), "{}", asked[1]);
        assert!(asked[1].contains("\"cmd\":\"df -h\""));
    }

    #[tokio::test]
    async fn stop_cancels_a_waiting_job() {
        let (_db, windows, events, _script) = setup(&[
            r#"{"action":"run","risk":"change","steps":[{"servers":["all"],"command":"reboot"}]}"#,
        ]);
        let brain = windows.main.clone();
        let job = brain.ask("reboot all", None, None, None).unwrap();
        for _ in 0..200 {
            if brain.waiting() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        brain.stop().await;
        assert!(!brain.busy());
        let events = events.lock().unwrap().clone();
        let last = of_type(&events, "ai.status").last().unwrap().clone();
        assert_eq!((last["state"].as_str(), last["job"].as_i64()), (Some("cancelled"), Some(job)));
    }

    #[test]
    fn expands_steps_per_server() {
        let db = Database::open_memory().unwrap();
        for name in ["a", "b"] {
            db.create_server(json!({"name": name, "host": name, "username": "u"}).as_object().unwrap(), None, None).unwrap();
        }
        let by_id: IndexMap<i64, Server> = db.list_servers().into_iter().map(|s| (s.id, s)).collect();
        let steps = vec![
            json!({"local": "npm run build", "cwd": "D:\\app"}),
            json!({"servers": [1], "command": "uptime"}),
            json!({"servers": ["all"], "download": {"from": "/tmp/x", "to": "C:\\out"}}),
            json!({"copy": {"from_server": 1, "from": "/a/data", "to_server": "b", "into": "/b/"}}),
            json!({"servers": [99], "command": "ignored"}),
        ];
        let (local, phases) = phases(&steps, &by_id);
        assert_eq!(local[0].label, "PS D:\\app> npm run build");
        assert_eq!(phases.len(), 2);
        let Phase::Servers(t) = &phases[0] else { panic!() };
        assert_eq!(t[&1].len(), 2);
        assert_eq!(t[&2][0].to, "C:\\out\\b\\");
        let Phase::Cross(c) = &phases[1] else { panic!() };
        assert_eq!((c.src, c.dst, c.key.as_str()), (1, 2, "x1"));
        assert_eq!(c.label, "copy a:/a/data -> b:/b/ (verify: checksum)");
        assert!(!reads_only(&local[0]));
    }
}
