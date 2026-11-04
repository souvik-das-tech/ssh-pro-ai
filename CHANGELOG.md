# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **ChatGPT as an AI brain** through the OpenAI Codex CLI, signed in with a ChatGPT account (no
  API key). The bundled `codex.exe` of the ChatGPT/Codex desktop app or the VS Code extension is
  found on its own. ChatGPT models appear in the model dropdown next to the AGY models
  (`ChatGPT · …`, live from the Codex catalog). Codex only plans: read-only sandbox, approvals off,
  its shell, sub-agent, plugin and browser tools disabled.
- **⚡ Auto model choice**: each request goes to a *Fast* or a *Smart* model, picked locally by a
  keyword classifier (English, Banglish, Hinglish) with no extra AI call. Quick checks and chat
  go to Fast. Troubleshooting, security, changes, analysis and long requests go to Smart. A chat
  that needed Smart stays on it. If Fast fails a round, Smart takes that round over with the chat
  so far. Both models can be picked in the toolbar, and the status line shows which model is
  working and why.
- Command output is scanned for private keys, passwords, tokens, password hashes and URL
  credentials, which are shown to the AI as `[REDACTED]`.
- New settings: `SSHPRO_CODEX` (path to `codex.exe`), `SSHPRO_CODEX_EFFORT` (reasoning effort,
  default `low`). `settings.json` also stores Auto's two models.

### Changed
- Fewer tokens per request: servers with identical output are sent to the AI as one entry,
  long output keeps its first lines and its end, colour codes and blank lines are dropped, and
  JSON is sent without padding. On ChatGPT the app's instructions replace Codex's own system
  prompt, cutting uncached input tokens by about 78% in a two-round test.
- The AI gets today's date with each request, and keeps answering in the user's language *and*
  script (Banglish stays in Latin letters).
- Wording in the UI and errors is no longer AGY-only ("AI brain", the active model's name).

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

### Fixed
- A new ChatGPT chat could answer only "ready" to the first request (the instructions'
  acknowledgement line was sent together with it).
- `awk '{print $1 > 2}'` was treated as read-only although it writes a file named `2`.

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
