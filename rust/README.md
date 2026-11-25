# SSH Pro AI — Rust desktop app

The same app as the Python version, rewritten in Rust: one native `sshpro.exe` with no Python,
virtual environment or Qt to install. It has the same features, the same UI, and the same data.

- **Same UI**: the web UI in [`../sshpro/web`](../sshpro/web) is embedded in the exe unchanged and
  shown in a native WebView2 window (wry + tao) instead of Qt WebEngine.
- **Same data**: it uses the same folder (`%LOCALAPPDATA%\SSH-Pro-AI`), the same SQLite database,
  the same DPAPI-encrypted secrets, saved host keys, chats, settings and `.sshpro` backup format.
  The Python and Rust apps can open each other's data. Run only one of them at a time.
- **Same API**: the local backend serves the same REST routes and WebSocket events on 127.0.0.1,
  guarded by a random per-launch token.

## Build

Requirements: Windows 10/11, [Rust](https://rustup.rs) (the GNU toolchain is used here), and a
MinGW gcc for the C parts (ring, SQLite):

```bat
winget install BrechtSanders.WinLibs.POSIX.UCRT
rust\build.cmd
```

The result is `rust\target\release\sshpro.exe` with `WebView2Loader.dll` next to it (the GNU
toolchain links the WebView2 loader dynamically; the build copies it there). Keep the two files
together. The WebView2 Runtime ships with Windows 11 and current Windows 10.

```bat
rust\target\release\sshpro.exe              :: desktop window
rust\target\release\sshpro.exe --serve      :: backend only; prints the URL to open in a browser
```

## Installer (to share the app)

```bat
winget install JRSoftware.InnoSetup
rust\installer.cmd        :: -> rust\target\installer\SSH-Pro-AI-Setup-<version>.exe (~6 MB)
```

Share that one `Setup.exe`. Whoever runs it gets a normal Windows app:

- installs for the current user without admin rights (`%LOCALAPPDATA%\Programs\SSH Pro AI`), or
  for all users if they choose so in the setup;
- Start menu and desktop shortcuts, and an entry in **Settings > Apps** to uninstall;
- installs the Microsoft Edge WebView2 Runtime first if the PC lacks it (downloaded from Microsoft);
- running the setup of a newer version updates the app in place (it closes a running copy first).

Uninstalling removes the app but keeps the user's data (`%LOCALAPPDATA%\SSH-Pro-AI`). Every user
has their own servers, keys and chats; nothing of yours is inside the installer. The AI chat needs
the AGY CLI or the Codex CLI on each user's PC (the app shows how to install them).

The setup is not code-signed, so Windows SmartScreen shows "Windows protected your PC" the first
time: **More info > Run anyway**. Signing `sshpro.exe` and the setup with a code-signing
certificate removes that warning.

Tests: `cargo test` in `rust\` (with gcc on PATH). They cover the safety classifier (every case of
the Python suite), secret masking, the database, backups, the Termix import, the tar.gz folder
transfer, Codex reply parsing, the HTTP token guard, and the AI job loop with a scripted AI
(answers, approvals, rejections, Stop, results going back to the AI).

## Modules

Each Python module has a Rust counterpart with the same behaviour:

| Rust | Python | Responsibility |
|---|---|---|
| `main.rs` | `__main__.py` | Window (WebView2), single instance, dark title bar, backup save dialog, `--serve`. |
| `server.rs` | `server.py` | axum REST API + WebSockets (`/ws/events`, `/ws/term`), token guard, embedded UI. |
| `db.rs` | `db.py` | SQLite store (rusqlite): servers, audit log, chats. |
| `crypto.rs` | `crypto.py` | Windows DPAPI (`dpapi:` + base64, compatible). |
| `sshpool.rs` | `sshpool.py` | russh connection pool: commands, probe, shells, SFTP, tar.gz folder transfers, server-to-server copy/pipe, trust-on-first-use host keys. |
| `safety.rs`, `shlex.rs` | `safety.py` | Read-only allowlist, sudo wrapping, secret masking; Python-exact `shlex`. |
| `brain.rs` | `brain.py` | AI loop, approvals, canary rollout, chat windows, Auto routing. |
| `agy.rs`, `codex.rs` | `agy.py`, `codex.py` | AGY CLI and ChatGPT (Codex CLI) adapters; the AI only plans. |
| `router.rs` | `router.py` | Auto fast/smart model choice. |
| `localexec.rs` | `localexec.py` | PowerShell steps on this PC. |
| `backup.rs`, `importer.rs` | `backup.py`, `importer.py` | `.sshpro` backup and Termix import. |
| `instructions.txt` | `brain.INSTRUCTIONS` | The AI's standing instructions, byte for byte. |

## Differences from the Python app

- **Host keys**: a server has several host keys (RSA, Ed25519, ECDSA). The app asks for RSA first,
  which is what the Python app negotiated and saved, so existing fingerprints keep matching. If a
  saved key is of another type, the app asks for each other type in turn and still accepts only a
  key that matches the saved fingerprint; a changed key is refused before any password is sent.
- **Connection lost mid-command**: the Python app retried the whole command on a dropped
  connection; this app retries only when the command had not started, and reports a command that
  was cut off, so nothing runs twice.
- **Approval card**: when one round has several phases (e.g. commands, then a server-to-server
  copy, then more commands), the card lists every step for each server; the Python app showed only
  the last phase's steps for a server.
- Logs: `%LOCALAPPDATA%\SSH-Pro-AI\logs\sshpro-rust.*.log` (daily, last 5 kept). Set `SSHPRO_LOG`
  (e.g. `debug`) for more detail.
