# Architecture

SSH Pro AI is one Rust program with a web UI shown in a native window. Everything runs locally on
the user's Windows PC.

```
 Start menu icon ──► sshpro.exe
                        │
        ┌───────────────┴────────────────┐
        ▼                                ▼
  Window (tao + wry / WebView2)    Backend (tokio runtime)
  shows the UI from web/ ──HTTP/WS──► axum on 127.0.0.1:<random>  (token-protected)
                                        │
                                        ├── Database (SQLite via rusqlite, DPAPI secrets)
                                        ├── SshManager (russh pool: run / probe / shell / SFTP / copy / pipe)
                                        └── Brain (AI loop) ──► AGY CLI subprocess (planner only)
```

The UI files in `web/` are embedded into the exe at build time (`rust-embed`); a debug build reads
them from disk, so UI changes show up on a reload without rebuilding.

## Modules

| File | Responsibility |
|---|---|
| `src/main.rs` | Entry point. Starts the backend and the window: single instance per data folder, window sized to the screen, title bar following light/dark, save dialog for backups, links opened in the browser. `--serve` runs the backend only. |
| `src/server.rs` | axum REST API (`/api/...`) and WebSockets (`/ws/events` for app events, `/ws/term` for terminals). Every request is checked against a random per-launch token. Serves the embedded UI. |
| `src/db.rs` | SQLite store for servers, audit log and chat history. Secret columns go through `crypto.rs`. |
| `src/crypto.rs` | Windows DPAPI protect/unprotect (`dpapi:` + base64). |
| `src/sshpool.rs` | Connection pool on russh: run commands, probe OS and status, interactive shells, SFTP upload/download (folders as one tar.gz), server-to-server `copy` and `pipe` relayed through the app. Trust-on-first-use host keys. |
| `src/safety.rs`, `src/shlex.rs` | Classifies shell and PowerShell commands as read-only or changing (allowlist with per-tool argument checks, quoting-, substitution- and pipe-aware; network tools with dynamic or piped data count as changes), masks secrets in output for the AI (`redact_secrets`), and wraps `sudo` so the password goes through stdin. `shlex.rs` splits and quotes words exactly like a POSIX shell. |
| `src/brain.rs` | The AI loop: send the request and a redacted server list to the AI, parse its JSON plan, run read-only steps, wait for approval on others, feed results back (secrets masked, servers with the same output grouped, long output cut to head + tail), at most 10 rounds. Also the parallel chat windows. |
| `src/instructions.txt` | The AI's standing instructions. |
| `src/agy.rs` | Adapter for the AGY CLI: install detection, persistent stream-json session with a warm spare and one-shot fallback, model list, timeouts, process clean-up. |
| `src/router.rs` | Auto model choice: a keyword classifier (English, Banglish, Hinglish) sends each request to the fast or the smart model. |
| `src/localexec.rs` | Runs PowerShell steps on this PC. |
| `src/backup.rs` | `.sshpro` export and import (plain JSON, re-encrypted on import). |
| `src/importer.rs` | Imports hosts from a Termix `ssh.sqlite`. |
| `src/paths.rs` | Data, log and database locations (`%LOCALAPPDATA%\SSH-Pro-AI`, or `SSHPRO_DATA_DIR`). |
| `src/util.rs` | Small shared helpers: command results, sizes, path expansion, child-process clean-up. |
| `web/` | Single-page UI (vanilla JS + xterm.js): sidebar, overview, AI Ops (AI / Direct), terminals, audit log, backup, light/dark themes. |
| `build.rs` | Embeds the icon and version info in the exe and puts `WebView2Loader.dll` next to it. |
| `installer/sshpro.iss` | Inno Setup script for the Windows installer (`installer.cmd`). |

## The AI round trip

1. `POST /api/ai/ask` reaches `Brain`, which sends the user's message to AGY. AGY knows only server
   ids, names, folders, tags, user and OS.
2. AGY returns one JSON object: `say`, `action` (`run` or `answer`), `risk`, `rollout`, `steps`,
   `answer`.
3. Each step is checked with `safety::is_read_only` / `is_local_read_only`. The AI's `risk` label
   is never trusted on its own.
4. Read-only rounds run at once (up to 16 servers in parallel). Other rounds emit an `ai.plan`
   event and wait for `POST /api/ai/approve` (canary or all at once).
5. Outputs (trimmed) go back to AGY, which plans the next round or answers with a table.
6. Every step is written to the audit log, and chat events are saved so a chat can be reopened.

## Host keys

A server has several host keys (RSA, Ed25519, ECDSA). The first key seen is saved and pinned. The
app asks for RSA first (the type the earlier Python version saved), and if the saved key is of
another type it asks for each other type in turn, accepting only a key that matches the saved
fingerprint. A changed key is refused before any password is sent, until the user chooses
**Forget host key**.

## Toolchain notes

The project builds with the GNU Rust toolchain plus a MinGW gcc (WinLibs) for the C code in ring
and SQLite. With the GNU toolchain, wry links `WebView2Loader.dll` dynamically, so `build.rs`
copies it next to `sshpro.exe` and the installer ships both. `env.sh` sets up `PATH` for Git Bash.

## Tests

`cargo test` covers the safety classifier, secret masking, `shlex`, the database, backups and the
Termix import, tar.gz folder transfers, probe parsing, the HTTP token guard, and the full AI job
flow with a scripted AI provider (answers, approvals, rejections, Stop, results going back to the
AI). No real servers or AGY account are needed.
