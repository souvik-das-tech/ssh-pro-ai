# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
