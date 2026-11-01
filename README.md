<div align="center">

<img src="sshpro/web/icon.png" width="96" alt="SSH Pro AI icon">

# SSH Pro AI

**Your whole Linux server fleet in one Windows app: real SSH terminals, parallel commands,
and an AI ops assistant that plans the work while *you* stay in control.**

[![Tests](https://github.com/souvik-das-tech/ssh-pro-ai/actions/workflows/tests.yml/badge.svg)](https://github.com/souvik-das-tech/ssh-pro-ai/actions/workflows/tests.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
![Python 3.12+](https://img.shields.io/badge/python-3.12%2B-blue.svg)
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
- 🛡️ **Safe by design**: an allowlist decides what counts as read-only. Everything else waits for
  your approval and rolls out **canary-first** (one server, then the rest).
- 🔐 **Secrets never leave your PC**: SSH keys and passwords are encrypted with Windows DPAPI.
  The AI only sees server names, folders, tags, user and OS: **never IPs, passwords or keys**.
- 📦 **Uploads and downloads**: send a file or folder from your PC to many servers (folders travel
  as one archive), or pull backups down into Downloads.
- 🧾 **Audit log**: every prompt, command, server and result is recorded.
- 💾 **Backup and restore**: move your whole server list to another PC in one file.
- 📥 **Termix import**: bring over existing connections from a Termix `ssh.sqlite` export.

## Quick start

**Requirements:** Windows 10/11 and [Python 3.12+](https://www.python.org/downloads/).

```bat
git clone https://github.com/souvik-das-tech/ssh-pro-ai.git
cd ssh-pro-ai
setup.cmd
```

`setup.cmd` creates a virtual environment, installs the dependencies and puts an
**SSH Pro AI** icon on your desktop. Double-click the icon (or run `run.cmd`) and add your first
server with **Add server**.

SSH, terminals and Direct mode work right away. Only the AI chat needs the AGY CLI (next section).

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

Pick a model from the dropdown in the chat toolbar. The list comes live from `agy models`, so new
models show up without an app update.

## How the AI works

```
 You: "update Node.js where it's installed"
        │
        ▼
 ┌──────────────┐  server list (names, folders, tags, OS; no IPs, no secrets)
 │  AGY CLI     │◄──────────────────────────────────────────────┐
 │  (planner)   │                                               │
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
2. AGY replies with a JSON plan: which servers, which commands, how risky.
3. SSH Pro AI checks every command against its own **read-only allowlist**. It never trusts the
   AI's own risk label on its own.
4. Read-only steps run immediately in parallel. Anything else (install, restart, delete, upload,
   local commands) is shown to you first and needs **Approve**.
5. Results go back to AGY for analysis (up to 6 rounds), and you get a summary and a table.

AGY never executes anything itself. The app runs every command, and AGY's own tools stay disabled.

## Working on your PC too

The AI can also plan steps on the Windows PC running the app:

- **Local PowerShell**: find a file, `git status`, `npm run build`, pack a folder.
- **Upload**: local file or folder → servers over SFTP. A folder is packed into one `.tar.gz`,
  sent in one transfer, unpacked on the server, and the archive is removed on both sides.
- **Download**: server file or folder → this PC (Downloads by default, one subfolder per server).
  Existing files are never overwritten: a copy becomes `name (2).sql.gz`. For root-owned files the
  AI first copies them to `/tmp` with `sudo`, then cleans up.

Example: *"upload the backup from my Downloads to the db server, import it, then clean up"* or
*"download yesterday's DB backup from the db server"*.

Read-only local checks (`Get-ChildItem`, `git status`, `node -v`) run at once. Every other local
command, upload and download needs your approval.

## Your data

| What | Where |
|---|---|
| Servers, audit log (kept 5 days), chat history | `%LOCALAPPDATA%\SSH-Pro-AI\sshpro.sqlite3` |
| Logs | `%LOCALAPPDATA%\SSH-Pro-AI\logs\` |
| Chosen AI model | `%LOCALAPPDATA%\SSH-Pro-AI\settings.json` |

Passwords, private keys, key passphrases and sudo passwords are encrypted with **Windows DPAPI**,
so they can only be decrypted by your Windows user on this PC.

### Backup and restore

Because DPAPI is tied to your Windows account, copying the database file to another PC loses all
credentials. Use **Backup** in the sidebar instead. It exports a `.sshpro` file with all servers
and their credentials **in plain text**, so treat it like the keys themselves. Restoring it (on
any PC) adds the servers and skips ones already present (same host, port and user). Nothing is
overwritten or deleted.

### Import from Termix

Put a Termix export named `ssh.sqlite` in the project folder and it is imported on first start,
or use **Load from file…**. That file contains plain-text secrets and is git-ignored.

## Configuration

| Environment variable | Purpose |
|---|---|
| `SSHPRO_AGY` | Path to `agy.exe` if it is installed somewhere non-standard |
| `SSHPRO_AGY_MODEL` | Default AI model (the dropdown choice overrides it) |
| `SSHPRO_DATA_DIR` | Use a different data folder (handy for testing) |

## Development

```bat
setup.cmd
.venv\Scripts\python -m pip install -r requirements-dev.txt
.venv\Scripts\python -m pytest
```

Run only the backend and open the printed URL in a browser:

```bat
.venv\Scripts\python -m sshpro --serve
```

The tests include a fake SSH server and a fake AI provider, so no real servers or AGY account are
needed. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for a tour of the code.

## Roadmap

- [ ] SFTP file manager in the UI
- [ ] SSH port tunnels
- [ ] Scheduled health checks (daily disk/RAM/updates report)
- [ ] Packaged installer (no Python needed)
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
