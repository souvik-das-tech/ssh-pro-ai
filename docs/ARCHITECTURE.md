# Architecture

SSH Pro AI is a Python app with a web UI shown in a Qt window. Everything runs locally on the
user's Windows PC.

```
 Desktop icon ──► pythonw -m sshpro
                    │
        ┌───────────┴─────────────┐
        ▼                         ▼
  Qt window (PySide6)       Backend thread (asyncio)
  QWebEngineView  ──HTTP/WS──► aiohttp on 127.0.0.1:<random>  (token-protected)
  shows sshpro/web/               │
                                  ├── Database (SQLite, DPAPI secrets)
                                  ├── SSHManager (asyncssh pool: run / probe / shell / SFTP)
                                  └── Brain (AI loop) ──► AGY CLI or Codex CLI (ChatGPT) subprocess (planner only)
```

## Modules

| File | Responsibility |
|---|---|
| `sshpro/__main__.py` | Entry point. Starts the backend thread and the Qt window (single instance, dark title bar, save dialog for backups). `--serve` runs the backend only. |
| `sshpro/server.py` | aiohttp app: REST API (`/api/...`) and WebSockets (`/ws/events` for app events, `/ws/term` for terminals). Every request is checked against a random per-launch token. |
| `sshpro/db.py` | SQLite store for servers, audit log and chat history. Secret columns go through `crypto.py`. |
| `sshpro/crypto.py` | Windows DPAPI protect/unprotect. |
| `sshpro/sshpool.py` | Connection pool on asyncssh: run commands, probe OS and status, interactive shells, SFTP upload/download (folders as one tar.gz). Trust-on-first-use host keys. |
| `sshpro/safety.py` | Classifies shell and PowerShell commands as read-only or changing (allowlist with per-tool argument checks, quoting-, substitution- and pipe-aware; network tools with dynamic or piped data count as changes), masks secrets in output for the AI (`redact_secrets`), and wraps `sudo` so the password goes through stdin. |
| `sshpro/brain.py` | The AI loop: send the request and a redacted server list to the AI, parse its JSON plan, run read-only steps, wait for approval on others, feed results back (secrets masked, servers with the same output grouped, long output cut to head + tail), at most 6 rounds. |
| `sshpro/agy.py` | Adapter for the AGY CLI: install detection, persistent session with one-shot fallback, model list, timeouts. |
| `sshpro/codex.py` | Adapter for ChatGPT via the Codex CLI: `codex exec --json` per turn, later turns `exec resume` the same thread; tools disabled, read-only sandbox. Model ids are `chatgpt:<slug>`, and `Brain` picks the adapter by that prefix. |
| `sshpro/router.py` | Auto model choice: a keyword classifier (English, Banglish, Hinglish) sends each request to the fast or the smart model; default model picks. `Brain` switches sessions and hands over the chat. |
| `sshpro/localexec.py` | Runs PowerShell steps on this PC. |
| `sshpro/backup.py` | `.sshpro` export and import (plain JSON, re-encrypted on import). |
| `sshpro/importer.py` | Imports hosts from a Termix `ssh.sqlite`. |
| `sshpro/paths.py` | Data, log and database locations (`%LOCALAPPDATA%\SSH-Pro-AI`, or `SSHPRO_DATA_DIR`). |
| `sshpro/web/` | Single-page UI (vanilla JS + xterm.js): sidebar, overview, AI Ops (AI / Direct), terminals, audit log, backup. |

## The AI round trip

1. `POST /api/ai/ask` reaches `Brain`, which sends the user's message to AGY. AGY knows only server
   ids, names, folders, tags, user and OS.
2. AGY returns one JSON object: `say`, `action` (`run` or `answer`), `risk`, `rollout`, `steps`,
   `answer`.
3. Each step is checked with `safety.is_read_only` / `is_local_read_only`. The AI's `risk` label
   is never trusted on its own.
4. Read-only rounds run at once (up to 16 servers in parallel). Other rounds emit an `ai.plan`
   event and wait for `POST /api/ai/approve` (canary or all at once).
5. Outputs (trimmed) go back to AGY, which plans the next round or answers with a table.
6. Every step is written to the audit log, and chat events are saved so a chat can be reopened.

## Tests

`tests/` covers the safety classifier, the database and import, backups, the SSH engine against an
in-process fake SSH server, the full AI job flow with a fake provider, and running without AGY.
Run them with `python -m pytest`.
