# Contributing to SSH Pro AI

Thanks for helping! Bug reports, ideas, docs fixes and code are all welcome.

## Ways to help

- **Report a bug**: open an [issue](https://github.com/souvik-das-tech/ssh-pro-ai/issues/new/choose)
  with steps to reproduce, your Windows version, and the relevant lines from
  `%LOCALAPPDATA%\SSH-Pro-AI\logs\sshpro-rust.*.log`. **Remove hostnames, IPs and anything secret first.**
- **Suggest a feature**: open an issue describing the problem you want solved.
- **Send a pull request**: for anything bigger than a small fix, open an issue first so we can
  agree on the approach.

## Development setup

You need [Rust](https://rustup.rs) and a MinGW gcc for the C parts (ring, SQLite):

```bat
winget install BrechtSanders.WinLibs.POSIX.UCRT
git clone https://github.com/<you>/ssh-pro-ai.git
cd ssh-pro-ai
cargo test
build.cmd
```

`cargo run -- --serve` starts only the backend and prints a URL you can open in a normal
browser, which is the quickest way to work on the UI (`web/`): a debug build reads the UI files
from disk, so a page reload shows your changes.

## Pull request checklist

- [ ] `cargo test` passes and `cargo build` shows no warnings.
- [ ] New behaviour has a test (the suite has a scripted AI provider, so no real servers or
      AGY account are needed).
- [ ] The code follows the existing style: small functions, short doc comments that say *why*.
- [ ] User-facing changes are described in `CHANGELOG.md` under *Unreleased*.
- [ ] No secrets, real hostnames or IP addresses in code, tests or screenshots.

## Safety rules for changes

SSH Pro AI runs commands on real servers, so a few rules are firm:

1. **The read-only allowlist in `src/safety.rs` is an allowlist.** Add a command only if it
   cannot change state with *any* arguments. Otherwise list only its read-only sub-commands. Add a
   test case in `src/safety.rs` for every addition.
2. **The AI never gets credentials or addresses.** Do not add hosts, IPs, keys or passwords to
   anything sent to the AI provider.
3. **Changes need approval.** Never add a path that runs a non-read-only command, upload,
   download or local command without the user approving it.
4. **Read-only also means nothing leaves the server.** A network tool counts as read-only only
   when its request is fixed text: no variables, substitutions, `@file` or piped data.
5. **Output for the AI goes through `redact_secrets`.** Any new place that sends command output
   or file contents to the AI must mask it first, and needs a test.

## Code of conduct

Be kind and constructive. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
