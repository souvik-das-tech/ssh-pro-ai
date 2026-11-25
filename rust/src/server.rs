//! Local backend for the UI: REST + WebSockets on 127.0.0.1 with a random per-launch token.
//!
//! The token keeps other local programs and web pages from driving the API (it can run commands
//! on every server), so every /api and /ws request must carry it. The routes and event shapes are
//! the same as the Python backend's, so the web UI in sshpro/web runs unchanged.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use russh::ChannelMsg;
use rust_embed::RustEmbed;
use serde_json::{json, Map, Value};
use tokio::sync::{broadcast, Semaphore};

use crate::agy::{agy_available, AGY_INSTALL_URL};
use crate::backup::{export_backup, import_backup};
use crate::brain::{BrainError, Emit, Windows};
use crate::codex::codex_available;
use crate::db::Database;
use crate::importer::import_termix;
use crate::paths::{data_dir, termix_db, which};
use crate::router::AUTO;
use crate::sshpool::SshManager;
use crate::util::{truthy, val_int, val_str};

pub const AUDIT_DAYS: i64 = 5;
pub const CHAT_DAYS: i64 = 15;

#[derive(RustEmbed)]
#[folder = "../sshpro/web/"]
struct Web;

pub struct Backend {
    pub db: Arc<Database>,
    pub token: String,
    pub ssh: Arc<SshManager>,
    pub windows: Windows,
    events: broadcast::Sender<String>,
    errors: Mutex<HashMap<i64, String>>, // last connection error per server (not persisted)
}

/// `{"error": ...}` with status 400 (403 for a bad token), like the Python backend.
#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<BrainError> for ApiError {
    fn from(e: BrainError) -> Self {
        ApiError(StatusCode::BAD_REQUEST, e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, e.to_string())
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

type ApiResult = Result<Json<Value>, ApiError>;
type App = State<Arc<Backend>>;

/// The request body as a JSON object; an empty body is `{}`.
fn body(bytes: &Bytes) -> Result<Map<String, Value>, ApiError> {
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(bad("expected a JSON object")),
        Err(e) => Err(bad(format!("invalid JSON: {e}"))),
    }
}

fn ids(v: Option<&Value>) -> Result<Vec<i64>, ApiError> {
    match v {
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| val_int(Some(i)).ok_or_else(|| bad(format!("invalid server id: {i}"))))
            .collect(),
        Some(other) if truthy(Some(other)) => Err(bad("ids must be a list")),
        _ => Ok(Vec::new()),
    }
}

fn win(b: &Map<String, Value>) -> Option<String> {
    let w = val_str(b.get("win"));
    (!w.is_empty()).then_some(w)
}

/// The UI's target choice (a folder, hand-picked servers, ...), kept with the message so
/// reopening the chat selects the same servers.
fn scope(b: &Map<String, Value>) -> Option<Value> {
    let Some(Value::Object(scope)) = b.get("scope") else {
        return None;
    };
    let spec: String = val_str(scope.get("spec")).chars().take(200).collect();
    let ids: Vec<i64> = scope
        .get("ids")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(1000)
                .filter_map(|i| {
                    let s = val_str(Some(i));
                    (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    (!spec.is_empty() || !ids.is_empty()).then(|| json!({"spec": spec, "ids": ids}))
}

/// The UI's label for who a message went to ("all servers", "web-1"), kept in the chat history.
fn target(b: &Map<String, Value>) -> Option<String> {
    let t: String = val_str(b.get("target")).chars().take(200).collect();
    (!t.is_empty()).then_some(t)
}

fn model_id(v: Option<&Value>) -> Result<String, ApiError> {
    let model = val_str(v).trim().to_string();
    if model.is_empty() || model.chars().any(char::is_whitespace) {
        return Err(bad("invalid model id"));
    }
    Ok(model)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn guard(State(app): App, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") || path.starts_with("/ws/") {
        let header = request.headers().get("X-Token").and_then(|v| v.to_str().ok()).map(String::from);
        let query = request.uri().query().and_then(|q| {
            url_param(q, "t")
        });
        let given = header.or(query).unwrap_or_default();
        if given.is_empty() || !constant_time_eq(&given, &app.token) {
            return ApiError(StatusCode::FORBIDDEN, "forbidden".into()).into_response();
        }
    }
    next.run(request).await
}

fn url_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Backend {
    pub fn new(db: Arc<Database>, token: Option<String>) -> Arc<Self> {
        let token = token.unwrap_or_else(|| {
            let bytes: [u8; 24] = rand::rng().random();
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
        });
        let ssh = Arc::new(SshManager::new(db.clone()));
        let (events, _) = broadcast::channel(4096);
        let sender = events.clone();
        let emit: Emit = Arc::new(move |event: Value| {
            let _ = sender.send(event.to_string());
        });
        let windows = Windows::new(db.clone(), ssh.clone(), emit); // chat windows that work at the same time
        Arc::new(Self { db, token, ssh, windows, events, errors: Mutex::new(HashMap::new()) })
    }

    fn emit(&self, event: Value) {
        let _ = self.events.send(event.to_string());
    }

    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/", get(index))
            .route("/static/{*path}", get(static_file))
            .route("/api/meta", get(meta))
            .route("/api/servers", get(list_servers).post(create_server))
            .route("/api/servers/{id}", put(update_server).delete(delete_server))
            .route("/api/servers/{id}/forget-key", post(forget_key))
            .route("/api/probe", post(probe))
            .route("/api/import", post(import_db))
            .route("/api/backup/export", post(backup_export))
            .route("/api/backup/import", post(backup_import))
            .route("/api/audit", get(audit))
            .route("/api/open", post(open_url))
            .route("/api/exec", post(exec_direct))
            .route("/api/ai/ask", post(ai_ask))
            .route("/api/ai/approve", post(ai_approve))
            .route("/api/ai/stop", post(ai_stop))
            .route("/api/ai/reset", post(ai_reset))
            .route("/api/ai/windows", post(ai_open_window))
            .route("/api/ai/windows/{win}", delete(ai_close_window))
            .route("/api/ai/models", get(ai_models))
            .route("/api/ai/model", post(ai_set_model))
            .route("/api/ai/auto", post(ai_set_auto))
            .route("/api/chats", get(list_chats))
            .route("/api/chats/{id}/open", post(open_chat))
            .route("/api/chats/{id}", delete(delete_chat).patch(update_chat))
            .route("/ws/events", get(ws_events))
            .route("/ws/term", get(ws_term))
            .layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024))
            .layer(middleware::from_fn_with_state(self.clone(), guard))
            .with_state(self.clone())
    }

    /// First launch: pull in the Termix servers, then check every server's health.
    pub fn startup(self: &Arc<Self>) {
        let termix = termix_db();
        if self.db.list_servers().is_empty() && termix.is_file() {
            match import_termix(&self.db, &termix) {
                Ok(r) => tracing::info!("Termix import: {r}"),
                Err(e) => tracing::error!("Termix import failed: {e}"),
            }
        }
        self.windows.main.warm_up();
        self.spawn_probe(self.db.list_servers().iter().map(|s| s.id).collect());
        let me = self.clone();
        tokio::spawn(async move {
            // Audit rows are kept for AUDIT_DAYS, chats for CHAT_DAYS after their last message;
            // checked at start and every hour after.
            loop {
                let removed = me.db.purge_audit(AUDIT_DAYS);
                if removed > 0 {
                    tracing::info!("Audit: removed {removed} entries older than {AUDIT_DAYS} days");
                }
                let removed = me.db.purge_chats(CHAT_DAYS);
                if removed > 0 {
                    tracing::info!("Chats: removed {removed} idle for more than {CHAT_DAYS} days");
                }
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
    }

    pub async fn shutdown(&self) {
        self.windows.close().await;
        self.ssh.close().await;
    }

    fn spawn_probe(self: &Arc<Self>, ids: Vec<i64>) {
        let me = self.clone();
        tokio::spawn(async move { me.probe_all(ids).await });
    }

    async fn probe_all(self: &Arc<Self>, ids: Vec<i64>) {
        let sem = Arc::new(Semaphore::new(16));
        futures::future::join_all(ids.into_iter().map(|id| {
            let (me, sem) = (self.clone(), sem.clone());
            async move {
                me.emit(json!({"type": "probe", "server_id": id, "status": "checking"}));
                let result = {
                    let _permit = sem.acquire().await.unwrap();
                    me.ssh.probe(id).await
                };
                match result.get("error").and_then(Value::as_str) {
                    Some(e) => me.errors.lock().unwrap().insert(id, e.to_string()),
                    None => me.errors.lock().unwrap().remove(&id),
                };
                let mut event = json!({"type": "probe", "server_id": id});
                if let Value::Object(r) = result {
                    for (k, v) in r {
                        event[k] = v;
                    }
                }
                me.emit(event);
            }
        }))
        .await;
    }

    fn new_server_ids(&self) -> Vec<i64> {
        self.db.list_servers().into_iter().filter(|s| s.last_status.is_none()).map(|s| s.id).collect()
    }
}

// ---- pages / meta --------------------------------------------------------------------------

fn asset(path: &str, no_store: bool) -> Response {
    match Web::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, mime.as_ref().parse().unwrap());
            if no_store {
                headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            }
            (headers, file.data.into_owned()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn index() -> Response {
    asset("index.html", true)
}

async fn static_file(Path(path): Path<String>) -> Response {
    if path.split('/').any(|p| p == "..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    asset(&path, false)
}

async fn meta(State(app): App) -> ApiResult {
    let termix = termix_db();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"), "model": app.windows.main.model(), "agy": agy_available(),
        "codex": codex_available(), "agy_install_url": AGY_INSTALL_URL, "data_dir": data_dir().to_string_lossy(),
        "termix_db": termix.is_file().then(|| termix.to_string_lossy().into_owned()),
        "ssh_exe": which("ssh").map(|p| p.to_string_lossy().into_owned()),
        "chat": app.windows.main.chat(), "chat_days": CHAT_DAYS, "windows": app.windows.state(),
    })))
}

// ---- servers -------------------------------------------------------------------------------

async fn list_servers(State(app): App) -> ApiResult {
    let errors = app.errors.lock().unwrap().clone();
    let servers: Vec<Value> = app
        .db
        .list_servers()
        .into_iter()
        .map(|s| {
            let id = s.id;
            let mut v = serde_json::to_value(s).unwrap();
            v["last_error"] = json!(errors.get(&id));
            v
        })
        .collect();
    Ok(Json(Value::Array(servers)))
}

async fn create_server(State(app): App, raw: Bytes) -> ApiResult {
    let id = app.db.create_server(&body(&raw)?, Some("manual"), None)?;
    app.spawn_probe(vec![id]);
    Ok(Json(json!(app.db.get_server(id))))
}

async fn update_server(State(app): App, Path(id): Path<i64>, raw: Bytes) -> ApiResult {
    app.db.update_server(id, &body(&raw)?)?;
    app.ssh.drop_conn(id); // reconnect with the new settings
    app.spawn_probe(vec![id]);
    Ok(Json(json!(app.db.get_server(id))))
}

async fn delete_server(State(app): App, Path(id): Path<i64>) -> ApiResult {
    app.ssh.drop_conn(id);
    app.db.delete_server(id);
    Ok(Json(json!({"ok": true})))
}

async fn forget_key(State(app): App, Path(id): Path<i64>) -> ApiResult {
    app.db.set_host_key(id, None);
    app.ssh.drop_conn(id);
    Ok(Json(json!({"ok": true})))
}

async fn probe(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let mut list = ids(b.get("ids"))?;
    if list.is_empty() {
        list = app.db.list_servers().iter().map(|s| s.id).collect();
    }
    app.spawn_probe(list);
    Ok(Json(json!({"ok": true})))
}

async fn import_db(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let path = val_str(b.get("path"));
    let path = if path.is_empty() { termix_db() } else { std::path::PathBuf::from(path) };
    let result = import_termix(&app.db, &path)?;
    app.spawn_probe(app.new_server_ids());
    Ok(Json(result))
}

async fn backup_export(State(app): App) -> Response {
    match export_backup(&app.db) {
        Ok(blob) => ([(header::CACHE_CONTROL, "no-store")], Json(blob)).into_response(),
        Err(e) => bad(e.to_string()).into_response(),
    }
}

async fn backup_import(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let result = import_backup(&app.db, b.get("backup"))?;
    app.spawn_probe(app.new_server_ids());
    Ok(Json(result))
}

async fn audit(State(app): App, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let limit = q.get("limit").map(|l| l.parse::<i64>().map_err(|_| bad("invalid limit"))).transpose()?.unwrap_or(300);
    Ok(Json(Value::Array(app.db.list_audit(limit))))
}

/// Open an http(s) link in the default browser.
pub fn open_in_browser(url: &str) -> bool {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no shell: the URL is one argument, so it cannot smuggle in a command
        let _ = std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(crate::util::CREATE_NO_WINDOW)
            .spawn();
    }
    true
}

async fn open_url(raw: Bytes) -> ApiResult {
    let url = val_str(body(&raw)?.get("url"));
    if !open_in_browser(&url) {
        return Err(bad("only http(s) links can be opened"));
    }
    Ok(Json(json!({"ok": true})))
}

// ---- commands / AI -------------------------------------------------------------------------

async fn exec_direct(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let command = val_str(b.get("command")).trim().to_string();
    let list = ids(b.get("ids"))?;
    if command.is_empty() || list.is_empty() {
        return Err(bad("command and servers are required"));
    }
    let brain = app.windows.get(win(&b).as_deref(), true)?;
    brain.direct(&command, list, target(&b).as_deref(), scope(&b).as_ref());
    Ok(Json(json!({"ok": true})))
}

async fn ai_ask(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let prompt = val_str(b.get("prompt")).trim().to_string();
    if prompt.is_empty() {
        return Err(bad("empty prompt"));
    }
    let list = ids(b.get("ids"))?;
    let brain = app.windows.get(win(&b).as_deref(), true)?;
    let job = brain.ask(&prompt, (!list.is_empty()).then_some(list), target(&b).as_deref(), scope(&b).as_ref())?;
    Ok(Json(json!({"job": job})))
}

async fn ai_approve(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let mode = val_str(b.get("mode"));
    app.windows
        .get(win(&b).as_deref(), false)?
        .approve(truthy(b.get("approve")), if mode.is_empty() { "canary" } else { &mode });
    Ok(Json(json!({"ok": true})))
}

async fn ai_stop(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    app.windows.get(win(&b).as_deref(), false)?.stop().await;
    Ok(Json(json!({"ok": true})))
}

async fn ai_reset(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    app.windows.get(win(&b).as_deref(), true)?.new_chat().await;
    Ok(Json(json!({"ok": true})))
}

/// A new chat window: its AI session starts warming up now, so its first answer is quick.
async fn ai_open_window(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    app.windows.get(win(&b).as_deref(), true)?.warm_up();
    Ok(Json(json!({"ok": true, "windows": app.windows.state()})))
}

async fn ai_close_window(State(app): App, Path(w): Path<String>) -> ApiResult {
    app.windows.close_window(&w).await;
    Ok(Json(json!({"ok": true})))
}

async fn ai_models(State(app): App) -> ApiResult {
    let models = app.windows.models().await?;
    Ok(Json(json!({"models": models, "current": app.windows.main.model(), "auto": app.windows.main.auto_models().await})))
}

async fn ai_set_model(State(app): App, raw: Bytes) -> ApiResult {
    let model = model_id(body(&raw)?.get("model"))?;
    app.windows.set_model(&model).await;
    Ok(Json(json!({"ok": true, "model": app.windows.main.model()})))
}

async fn ai_set_auto(State(app): App, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let (fast, smart) = (model_id(b.get("fast"))?, model_id(b.get("smart"))?);
    if fast == AUTO || smart == AUTO {
        return Err(bad("Auto needs real models"));
    }
    app.windows.set_auto(&fast, &smart);
    Ok(Json(json!({"ok": true, "auto": app.windows.main.auto()})))
}

// ---- chat history --------------------------------------------------------------------------

async fn list_chats(State(app): App) -> ApiResult {
    let mut open = Map::new(); // chat id -> window showing it
    for (w, b) in app.windows.list() {
        if let Some(chat) = b.chat() {
            open.insert(chat.to_string(), json!(w));
        }
    }
    Ok(Json(json!({"chats": app.db.list_chats(), "current": app.windows.main.chat(), "open": open, "days": CHAT_DAYS})))
}

async fn open_chat(State(app): App, Path(id): Path<i64>, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    Ok(Json(app.windows.open_chat(win(&b).as_deref(), id).await?))
}

/// Star / unstar a chat (favourites never expire) or rename it.
async fn update_chat(State(app): App, Path(id): Path<i64>, raw: Bytes) -> ApiResult {
    let b = body(&raw)?;
    let favorite = b.get("favorite").filter(|f| !f.is_null()).map(|f| truthy(Some(f)));
    let title = b.get("title").filter(|t| !t.is_null()).map(|t| val_str(Some(t)));
    app.db.update_chat(id, favorite, title.as_deref())?;
    Ok(Json(json!({"ok": true})))
}

async fn delete_chat(State(app): App, Path(id): Path<i64>) -> ApiResult {
    app.windows.delete_chat(id).await?;
    Ok(Json(json!({"ok": true})))
}

// ---- websockets ----------------------------------------------------------------------------

async fn ws_events(State(app): App, ws: WebSocketUpgrade) -> Response {
    let rx = app.events.subscribe();
    ws.on_upgrade(move |socket| events_loop(socket, rx))
}

async fn events_loop(socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    let (mut tx, mut incoming) = socket.split();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            event = rx.recv() => match event {
                Ok(text) => if tx.send(Message::Text(text.into())).await.is_err() { break },
                Err(broadcast::error::RecvError::Lagged(n)) => tracing::warn!("UI missed {n} events"),
                Err(_) => break,
            },
            msg = incoming.next() => match msg {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = ping.tick() => if tx.send(Message::Ping(Vec::new().into())).await.is_err() { break },
        }
    }
}

async fn ws_term(State(app): App, Query(q): Query<HashMap<String, String>>, ws: WebSocketUpgrade) -> Response {
    let server: Option<i64> = q.get("server").and_then(|s| s.parse().ok());
    let cols: u32 = q.get("cols").and_then(|c| c.parse().ok()).unwrap_or(120);
    let rows: u32 = q.get("rows").and_then(|r| r.parse().ok()).unwrap_or(32);
    ws.max_message_size(4 * 1024 * 1024).on_upgrade(move |socket| term_loop(app, socket, server, cols, rows))
}

async fn term_loop(app: Arc<Backend>, socket: WebSocket, server: Option<i64>, cols: u32, rows: u32) {
    let (mut tx, mut incoming) = socket.split();
    let send_json = |v: Value| Message::Text(v.to_string().into());
    let Some(server_id) = server else {
        let _ = tx.send(send_json(json!({"type": "error", "text": "server is required"}))).await;
        let _ = tx.close().await;
        return;
    };
    let channel = match app.ssh.open_shell(server_id, cols, rows).await {
        Ok(ch) => ch,
        Err(e) => {
            let _ = tx.send(send_json(json!({"type": "error", "text": e}))).await;
            let _ = tx.close().await;
            return;
        }
    };
    if tx.send(send_json(json!({"type": "ready"}))).await.is_err() {
        return;
    }
    let (mut reader, writer) = channel.split();
    if let Some(path) = app.db.get_server(server_id).and_then(|s| s.default_path).filter(|p| !p.is_empty()) {
        let line = format!("cd {}\n", serde_json::to_string(&path).unwrap());
        let _ = writer.data(line.as_bytes()).await;
    }
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    if tx.send(Message::Binary(data.to_vec().into())).await.is_err() { break }
                }
                Some(_) => {}
                None => {
                    let _ = tx.send(send_json(json!({"type": "exit"}))).await;
                    let _ = tx.close().await;
                    break;
                }
            },
            msg = incoming.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let Ok(m) = serde_json::from_str::<Value>(&text) else { continue };
                    match m.get("type").and_then(Value::as_str) {
                        Some("in") => {
                            let data = val_str(m.get("data"));
                            if writer.data(data.as_bytes()).await.is_err() { break }
                        }
                        Some("resize") => {
                            let c = val_int(m.get("cols")).unwrap_or(120).clamp(1, 1000) as u32;
                            let r = val_int(m.get("rows")).unwrap_or(32).clamp(1, 1000) as u32;
                            let _ = writer.window_change(c, r, 0, 0).await;
                        }
                        _ => {}
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = ping.tick() => if tx.send(Message::Ping(Vec::new().into())).await.is_err() { break },
        }
    }
    let _ = writer.close().await;
}

/// Bind on 127.0.0.1 (port 0 = any free port), start serving and return the UI's URL.
pub async fn serve(backend: Arc<Backend>, port: u16) -> anyhow::Result<String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    let router = backend.router();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!("HTTP server stopped: {e}");
        }
    });
    backend.startup();
    Ok(format!("http://127.0.0.1:{port}/?t={}", backend.token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(url_param("server=3&t=ab%2Bc", "t").as_deref(), Some("ab+c"));
        assert_eq!(url_param("t=a-b_c", "t").as_deref(), Some("a-b_c"));
        assert!(constant_time_eq("abc", "abc") && !constant_time_eq("abc", "abd") && !constant_time_eq("a", "ab"));
        let b = body(&Bytes::from_static(br#"{"scope": {"spec": "folder:web", "ids": [1, "2", "x"]}, "target": "web"}"#)).unwrap();
        assert_eq!(scope(&b), Some(json!({"spec": "folder:web", "ids": [1, 2]})));
        assert_eq!(target(&b).as_deref(), Some("web"));
        assert!(body(&Bytes::new()).unwrap().is_empty());
        assert!(model_id(Some(&json!("a b"))).is_err());
    }

    #[tokio::test]
    async fn token_guard_and_api() {
        std::env::set_var("SSHPRO_DATA_DIR", std::env::temp_dir().join(format!("sshpro-srv-{}", std::process::id())));
        let db = Arc::new(Database::open_memory().unwrap());
        let backend = Backend::new(db, Some("tok".into()));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let router = backend.router();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let get = |path: &str| {
            let path = path.to_string();
            async move {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                let mut out = String::new();
                stream.read_to_string(&mut out).await.unwrap();
                out
            }
        };
        assert!(get("/api/servers").await.starts_with("HTTP/1.1 403"));
        assert!(get("/api/servers?t=wrong").await.starts_with("HTTP/1.1 403"));
        let ok = get("/api/servers?t=tok").await;
        assert!(ok.starts_with("HTTP/1.1 200") && ok.ends_with("[]"), "{ok}");
        let page = get("/").await;
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("no-store") && page.contains("<html"), "index");
        assert!(get("/static/app.js").await.contains("TOKEN"));
        assert!(get("/static/../Cargo.toml").await.starts_with("HTTP/1.1 404"));
    }
}
