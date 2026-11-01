# Security policy

SSH Pro AI holds SSH credentials and runs commands on servers, so security reports are taken
seriously.

## Reporting a vulnerability

**Please do not open a public issue.** Instead use GitHub's private
[Report a vulnerability](https://github.com/souvik-das-tech/ssh-pro-ai/security/advisories/new)
form. Include:

- what the problem is and its impact (e.g. a command that bypasses approval, a secret leaking to
  the AI or the logs),
- steps or a proof of concept,
- the version or commit you tested.

You can expect a first reply within a few days. Please give a reasonable time for a fix before
disclosing publicly.

## Supported versions

Only the latest release on `main` gets security fixes.

## Security model in short

- The backend listens on `127.0.0.1` only and every request needs a random per-launch token.
- Credentials are encrypted at rest with Windows DPAPI (bound to the current Windows user).
- The AI provider receives server names, folders, tags, user and OS only, never hosts, IPs or
  credentials.
- Commands are classified by an allowlist (`sshpro/safety.py`). Anything not provably read-only
  needs the user's approval and rolls out canary-first.
- Host keys are trusted on first use and pinned afterwards. A changed key blocks the connection
  until you choose **Forget host key**.
- `.sshpro` backup files contain credentials in **plain text** by design. Store them safely.
