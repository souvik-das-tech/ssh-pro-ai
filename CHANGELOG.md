# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] - 2026-11-25

SSH Pro AI is now a native Rust app with a Windows installer. Your servers, keys, chats and
settings from 0.1.0 open as they are.

### Added
- **Windows installer**: `SSH-Pro-AI-Setup-0.2.0.exe` (~6 MB) installs per user without admin
  rights, adds Start menu and desktop shortcuts and an entry in Settings > Apps, installs the
  Microsoft Edge WebView2 Runtime when it is missing, and updates the app in place. Uninstalling
  keeps your data.
- **Light and dark mode**: light by default; the sun / moon button next to the logo switches it.
  The choice is remembered and the window title bar follows it. Terminals get a light palette.
- **Collapsible sidebar**: the button next to the logo (or `Ctrl+B`) folds the sidebar into a slim
  rail with a status dot per server (name on hover) and the footer tools as icons.
- **Server-to-server transfers and migrations** (GeoServer, PostgreSQL/PostGIS, MySQL, Docker
  volumes, app data). Two new AI step types:
  - `copy` moves a file or folder from one server into a folder on another. The app relays the
    data over its own SSH connections, so the servers need no SSH keys or network path to each
    other, and nothing is stored on this PC. It checks free space first, never overwrites an
    existing target unless asked to, keeps permissions, times and (with sudo) owners, and
    afterwards compares both sides (SHA-256 of every file by default). Any difference fails the
    step, and nothing is deleted.
  - `pipe` streams one server's command output into a command on another, for example
    `pg_dump -Fc` into `pg_restore` or `mysqldump` into `mysql`, with `pipefail` on both sides.
    The sudo password is sent ahead of the data, and only when sudo needs one, so it never ends
    up in the data.
  - Steps in a round run in the order the AI lists them, and a failed step stops the rest of that
    round. Both step types always need approval. The AI follows a no-data-loss procedure: inspect
    both servers, never delete or overwrite, copy a consistent state, then verify (checksums, row
    counts per table, service health) and report. Up to 10 rounds per request (was 6).
  - New starter prompts: *Move GeoServer* and *Move a database*.
- **Several chat windows at once**: open up to 6 AI chat windows as tabs (`+ New window`,
  `Alt+N`, `Alt+1…9` to switch, middle-click to close). Each window has its own AI session,
  conversation, targets and mode, so one window can upgrade the web servers while another checks
  the databases. Windows in the background keep updating; their tabs show a spinner while they
  work, an amber dot when a change waits for approval, and a green dot for new results. Windows,
  their chats and targets come back after a restart. In History a chat can be opened in a new
  window, and a chat that is already open switches to its window.
- **Favorite chats**: star a chat in History, in the **★ Favorites** menu, or for the open chat.
  Favorites are never deleted by the 15-day cleanup, are listed first in History, and can be
  renamed. In Favorites, **Ask again** opens a chat with its last request already in the box.
  History also has a search box.
- **Chats remember their servers**: every message stores the target that was picked (a folder, one
  server, all, or hand-picked servers). Opening a chat from History or Favorites selects those
  servers again; servers deleted since then are skipped.
- **Custom targets**: *Custom…* in the Target menu opens a picker to choose any servers from any
  folders, with whole-folder checkboxes, a filter, All / None / Online / Invert, and saved
  selections. `Ctrl`+click on folders in the Target menu adds or removes whole folders.
- **⚡ Auto model choice**: each request goes to a *Fast* or a *Smart* model, picked locally by a
  keyword classifier (English, Banglish, Hinglish) with no extra AI call. Quick checks and chat
  go to Fast. Troubleshooting, security, changes, analysis and long requests go to Smart. A chat
  that needed Smart stays on it. If Fast fails a round, Smart takes that round over with the chat
  so far. Both models can be picked in the toolbar, and the status line shows which model is
  working and why. `settings.json` stores Auto's two models.
- Command output is scanned for private keys, passwords, tokens, password hashes and URL
  credentials, which are shown to the AI as `[REDACTED]`.

### Changed
- **Rewritten in Rust**: one native `sshpro.exe` (WebView2 window, axum backend, russh, SQLite)
  with the same features and UI. No Python, virtual environment or Qt to install.
- **Compact AI toolbar**: the folder chips and the "One server" list became one **Target** button
  whose menu lists all servers, each folder, Custom and a searchable list of servers, so the
  toolbar fits on one line.
- The window opens inside the screen (without the taskbar) and centred; at 125 % scaling it used
  to be taller than a 1080p screen, hiding the prompt box.
- Fewer tokens per request: servers with identical output are sent to the AI as one entry, long
  output keeps its first lines and its end, colour codes and blank lines are dropped, and JSON is
  sent without padding.
- The AI gets today's date with each request, and keeps answering in the user's language *and*
  script (Banglish stays in Latin letters).
- When a dropped connection cuts a running command off, the command is reported as cut off
  instead of being run a second time.

### Security
- Hardened the read-only allowlist against commands that look harmless but write files, run
  programs or change the system, e.g. `env <cmd>`, `systemctl --failed restart x`,
  `git branch -D`, `git remote set-url`, `curl -sSLo file`, `sed 's/x/y/w file'` and `sed …e`,
  `awk '{print > f}'` / `print | "cmd"` / `getline`, `sort -o`, `uniq in out`, `tree -o`,
  `find -fprintf/-fls`, `ip netns exec` / `ip link set`, `ifconfig … down`, `hostname <name>`,
  `timedatectl set-*`, `date --set=`, `ss -K`, `dmesg -C`, `journalctl --vacuum-*`,
  `openssl … -out`, `nvidia-smi` setters, `iptables -Z/-F/-A`, `yum history undo`,
  `go env -w`, `php -v -r …`, `ionice -p`, `nohup`, `less`, `./cat`, `/dev/tcp`, and
  `LD_PRELOAD=` / `PATH=` assignments.
- Network commands (`curl`, `dig`, `nslookup`, `host`, `ping`, `openssl s_client`) with
  variables, command substitutions, `@file` arguments or piped data now need approval, so a
  "read-only" step cannot send server data out.
- The AI is instructed to treat command output and file contents as untrusted data
  (prompt-injection defence).
- Saved host keys are still enforced after the move to Rust: the app asks for the key type that
  was saved, and a changed key is refused before any password is sent.

### Fixed
- The prompt box is one tidy line when empty and resizes for drafts and suggestions, without a
  stray scrollbar.
- Long folder names in the sidebar stay on one line.
- `awk '{print $1 > 2}'` was treated as read-only although it writes a file named `2`.

### Removed
- The Python app (`sshpro/` package, `setup.cmd`, `run.cmd`, `requirements*.txt`). Use the
  installer, or build from source with `cargo`.

## [0.1.0] - 2026-11-01

First public release.

### Added
- Server manager: folders, tags, pins, online status and OS detection.
- SSH terminals with tabs, tiled view and broadcast typing (xterm.js).
- Direct mode: run one command on many servers in parallel.
- AI ops chat through the local AGY CLI: plan → read-only checks → analysis table; changes need
  approval and roll out canary-first. Optional web search for CVEs and release notes.
- Live model list from `agy models`, choice saved in `settings.json`.
- Local PowerShell steps, uploads to servers and downloads to this PC (folders sent as one archive,
  never overwrites local files).
- DPAPI-encrypted credentials, trust-on-first-use host keys, 5-day audit log, chat history.
- Backup and restore (`.sshpro`), Termix import.
- One-step `setup.cmd` with desktop shortcut.

[Unreleased]: https://github.com/souvik-das-tech/ssh-pro-ai/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/souvik-das-tech/ssh-pro-ai/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/souvik-das-tech/ssh-pro-ai/releases/tag/v0.1.0
