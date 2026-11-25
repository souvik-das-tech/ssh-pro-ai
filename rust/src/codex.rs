//! ChatGPT brain through the OpenAI Codex CLI, signed in with the user's ChatGPT account.
//!
//! Verified codex-cli 0.166 interfaces:
//!   * Models:  codex debug models  -> {"models": [{"slug", "display_name", "visibility", "priority", ...}]}
//!   * Turn 1:  codex exec --json [flags] -          (prompt on stdin)
//!   * Turn n:  codex exec resume --json [flags] <thread_id> -
//!     stdout : JSONL {"type": "thread.started"|"item.completed"|"turn.completed"|"turn.failed"|"error", ...}
//!     The reply is the last item of type "agent_message".
//!
//! Like AGY, Codex only plans: its shell, sub-agent, plugin and browser tools are disabled and the
//! sandbox is read-only with approvals off, so it cannot run anything. The app executes every step.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use serde_json::{json, Value};

use crate::agy::{looks_like_auth_error, run_cli, AiError};
use crate::paths::{data_dir, home_dir, which};
use crate::util::tail_chars;

pub const PREFIX: &str = "chatgpt:"; // model ids in the app's dropdown: "chatgpt:<codex slug>"
pub const CODEX_INSTALL_URL: &str = "https://developers.openai.com/codex/cli";
const AUTH_MARKERS: &[&str] = &["not logged in", "codex login", "unauthorized", "401", "please sign in", "login required"];
const AUTH_HINT: &str = "ChatGPT is not signed in. Open a terminal, run `codex login` and sign in.";
/// Tools Codex could otherwise use on this PC. The app must stay the only one that executes anything.
const DISABLED: &[&str] = &[
    "shell_tool", "unified_exec", "multi_agent", "image_generation", "computer_use", "browser_use",
    "apps", "plugins", "hooks", "goals", "view_image", "skill_search", "tool_suggest",
];

pub fn is_chatgpt(model: &str) -> bool {
    model.starts_with(PREFIX)
}

pub fn find_codex() -> Result<PathBuf, AiError> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SSHPRO_CODEX") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(p) = which("codex") {
        candidates.push(p);
    }
    // the ChatGPT/Codex desktop app and the VS Code extension ship their own copy; newest first
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let patterns = [
            PathBuf::from(&local).join("OpenAI").join("Codex").join("bin").join("*").join("codex.exe"),
            home_dir().join(".vscode").join("extensions").join("openai.chatgpt-*").join("bin").join("*").join("codex.exe"),
        ];
        let mut bundled: Vec<PathBuf> = patterns
            .iter()
            .filter_map(|p| glob::glob(&p.to_string_lossy()).ok())
            .flatten()
            .flatten()
            .collect();
        bundled.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
        candidates.extend(bundled);
    }
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        AiError::NotInstalled(format!("Codex CLI not found. Install it and run `codex login`: {CODEX_INSTALL_URL}"))
    })
}

pub fn codex_available() -> Option<String> {
    find_codex().ok().map(|p| p.to_string_lossy().into_owned())
}

/// An empty, app-owned working directory so Codex never sees project files.
pub fn codex_workspace() -> PathBuf {
    let path = data_dir().join("codex_workspace");
    let _ = std::fs::create_dir_all(&path);
    path
}

/// ChatGPT models the signed-in account can use in Codex, from its live catalog.
pub async fn list_models() -> Result<Vec<Value>, AiError> {
    let codex = find_codex()?.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_cli(&[codex, "debug".into(), "models".into()], None, 60.0, None, "ChatGPT").await?;
    let catalog: Value = serde_json::from_str(&stdout).map_err(|_| {
        if looks_like_auth_error(&[&stdout, &stderr], AUTH_MARKERS) {
            return AiError::Auth(AUTH_HINT.into());
        }
        let detail = if stderr.is_empty() { &stdout } else { &stderr };
        AiError::Process(format!("codex debug models returned no JSON (exit {code}): {}", tail_chars(detail, 300)))
    })?;
    let mut listed: Vec<&Value> = catalog
        .get("models")
        .and_then(Value::as_array)
        .map(|m| {
            m.iter()
                .filter(|m| {
                    m.get("slug").and_then(Value::as_str).is_some_and(|s| !s.is_empty())
                        && m.get("visibility").and_then(Value::as_str) == Some("list")
                })
                .collect()
        })
        .unwrap_or_default();
    listed.sort_by_key(|m| m.get("priority").and_then(Value::as_i64).unwrap_or(999));
    Ok(listed
        .into_iter()
        .map(|m| {
            let slug = m["slug"].as_str().unwrap_or_default();
            let name = m.get("display_name").and_then(Value::as_str).filter(|d| !d.is_empty()).unwrap_or(slug);
            json!({"id": format!("{PREFIX}{slug}"), "label": format!("ChatGPT · {name}")})
        })
        .collect())
}

/// AI provider for ChatGPT via `codex exec`. One process per turn; the conversation lives in a
/// Codex thread that later turns resume, so nothing has to stay running between requests.
pub struct CodexProvider {
    command: Vec<String>,
    pub model: String,
    workdir: PathBuf,
    timeout: f64,
    effort: String,
    instructions_file: Mutex<Option<PathBuf>>,
    thread: Mutex<Option<String>>,
    lock: tokio::sync::Mutex<()>,
}

impl CodexProvider {
    pub fn new(model: &str, workdir: PathBuf) -> Result<Self, AiError> {
        Ok(Self {
            command: vec![find_codex()?.to_string_lossy().into_owned()],
            model: model.strip_prefix(PREFIX).unwrap_or(model).to_string(),
            workdir,
            timeout: 150.0,
            effort: std::env::var("SSHPRO_CODEX_EFFORT").unwrap_or_else(|_| "low".into()),
            instructions_file: Mutex::new(None),
            thread: Mutex::new(None),
            lock: tokio::sync::Mutex::new(()),
        })
    }

    fn flags(&self) -> Vec<String> {
        let mut flags: Vec<String> = [
            "--json", "--skip-git-repo-check", "--ignore-user-config", "--ignore-rules",
            "-c", "sandbox_mode=read-only", "-c", "approval_policy=never",
            "-c", "skills.bundled.enabled=false", "-c", "include_environment_context=false",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(path) = self.instructions_file.lock().unwrap().as_ref() {
            let posix = path.to_string_lossy().replace('\\', "/");
            flags.extend(["-c".into(), format!("model_instructions_file={posix}")]);
        }
        for feature in DISABLED {
            flags.extend(["--disable".into(), feature.to_string()]);
        }
        if !self.model.is_empty() {
            flags.extend(["-m".into(), self.model.clone()]);
        }
        if !self.effort.is_empty() {
            flags.extend(["-c".into(), format!("model_reasoning_effort={}", self.effort)]);
        }
        flags
    }

    /// Nothing to warm up: the instructions become the model's system prompt (in place of
    /// Codex's own), so they cost no extra turn and are cached across conversations.
    pub async fn start(&self, instructions: &str) -> Result<(), AiError> {
        let lines: Vec<&str> =
            instructions.lines().filter(|l| !l.starts_with("Acknowledge these instructions")).collect();
        let path = self.workdir.join("instructions.md");
        std::fs::write(&path, lines.join("\n")).map_err(|e| AiError::Process(format!("cannot write {}: {e}", path.display())))?;
        *self.instructions_file.lock().unwrap() = Some(path);
        Ok(())
    }

    pub async fn ask(&self, message: &str) -> Result<String, AiError> {
        let _turn = self.lock.lock().await;
        let thread = self.thread.lock().unwrap().clone();
        let mut args = self.command.clone();
        args.push("exec".into());
        if let Some(t) = &thread {
            args.push("resume".into());
            args.extend(self.flags());
            args.push(t.clone());
        } else {
            args.extend(self.flags());
        }
        args.push("-".into());
        let started = Instant::now();
        let (code, stdout, stderr) =
            run_cli(&args, Some(message.as_bytes()), self.timeout, Some(&self.workdir), "ChatGPT").await?;
        let reply = self.parse(code, &stdout, &stderr);
        tracing::info!("ChatGPT turn ({}, thread={:?}): {:.1}s", self.model, thread, started.elapsed().as_secs_f64());
        reply
    }

    fn parse(&self, code: i32, stdout: &str, stderr: &str) -> Result<String, AiError> {
        let (mut reply, mut error): (Option<String>, Option<String>) = (None, None);
        for line in stdout.lines().map(str::trim).filter(|l| l.starts_with('{')) {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            match event.get("type").and_then(Value::as_str) {
                Some("thread.started") => {
                    if let Some(t) = event.get("thread_id").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                        *self.thread.lock().unwrap() = Some(t.to_string());
                    }
                }
                Some("item.completed") if event["item"]["type"] == "agent_message" => {
                    reply = Some(crate::util::val_str(event["item"].get("text")));
                }
                Some("turn.failed") => {
                    let m = crate::util::val_str(event["error"].get("message"));
                    error = Some(if m.is_empty() { "turn failed".into() } else { m });
                }
                Some("error") => {
                    let m = crate::util::val_str(event.get("message"));
                    error = Some(if m.is_empty() { "error".into() } else { m });
                }
                _ => {}
            }
        }
        if let (Some(r), None) = (&reply, &error) {
            return Ok(r.clone());
        }
        let tail = tail_chars(stderr.trim(), 400);
        let detail = error.clone().unwrap_or_else(|| if tail.is_empty() { format!("exit {code}, no reply") } else { tail });
        if looks_like_auth_error(&[&detail, stderr], AUTH_MARKERS) {
            return Err(AiError::Auth(AUTH_HINT.into()));
        }
        if let Some(e) = error {
            return Err(AiError::Model(format!("ChatGPT error: {e}")));
        }
        Err(AiError::Process(format!("Codex produced no reply (exit {code}): {detail}")))
    }

    pub async fn reset(&self) {
        *self.thread.lock().unwrap() = None; // the next ask() starts a new conversation
    }

    pub async fn close(&self) {
        *self.thread.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> CodexProvider {
        CodexProvider {
            command: vec!["codex".into()],
            model: "gpt-x".into(),
            workdir: std::env::temp_dir(),
            timeout: 1.0,
            effort: "low".into(),
            instructions_file: Mutex::new(None),
            thread: Mutex::new(None),
            lock: tokio::sync::Mutex::new(()),
        }
    }

    #[test]
    fn parses_codex_events() {
        let p = provider();
        let out = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"item.completed","item":{"type":"reasoning","text":"hmm"}}
{"type":"item.completed","item":{"type":"agent_message","text":"{\"say\":\"hi\"}"}}"#;
        assert_eq!(p.parse(0, out, "").unwrap(), r#"{"say":"hi"}"#);
        assert_eq!(p.thread.lock().unwrap().as_deref(), Some("t1"));
        let err = p.parse(1, r#"{"type":"turn.failed","error":{"message":"boom"}}"#, "").unwrap_err();
        assert_eq!(err.to_string(), "ChatGPT error: boom");
        assert!(matches!(p.parse(1, "", "Error: not logged in"), Err(AiError::Auth(_))));
        let flags = p.flags().join(" ");
        assert!(flags.contains("sandbox_mode=read-only") && flags.contains("--disable shell_tool") && flags.contains("-m gpt-x"));
    }
}
