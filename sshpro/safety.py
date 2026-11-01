"""Command safety: decide whether a shell command only *reads* (runs without asking) or may
*change* a server (needs the user's approval). Unknown commands count as changes.

This is an allowlist on purpose: the AI's own risk label is never trusted alone.
"""

from __future__ import annotations

import re
import shlex

# Commands that only read state, whatever their arguments.
_READ_ONLY = {
    "cat", "head", "tail", "less", "more", "grep", "egrep", "fgrep", "zgrep", "wc", "sort", "uniq",
    "cut", "tr", "column", "echo", "printf", "true", "false", "test", "[", "[[", "basename",
    "dirname", "readlink", "realpath", "stat", "file", "ls", "du", "df", "free", "uptime", "uname",
    "hostname", "hostnamectl", "whoami", "id", "groups", "who", "w", "last", "date", "timedatectl",
    "nproc", "lscpu", "lsblk", "lsmem", "lsof", "lsb_release", "ps", "pgrep", "pstree", "top",
    "vmstat", "iostat", "mpstat", "sar", "netstat", "ss", "ip", "ifconfig", "ping", "dig",
    "nslookup", "host", "which", "whereis", "type", "command", "env", "printenv", "getent",
    "md5sum", "sha256sum", "sha1sum", "diff", "cmp", "jq", "nvidia-smi", "sensors", "dmesg",
    "journalctl", "openssl", "certbot-certificates", "locale", "ulimit", "tree", "seq", "sleep",
    "export", "awk", "gawk", "mawk", "sed", "find", "xargs", "curl", "zcat", "base64", "hexdump",
    "lsmod", "blkid", "findmnt", "mount", "getconf", "arch",
}

# Commands that read only with some sub-commands / flags.
_READ_ONLY_SUB = {
    "systemctl": {"status", "is-active", "is-enabled", "is-failed", "list-units", "list-unit-files",
                  "show", "cat", "list-timers", "--version", "--failed"},
    "service": {"--status-all"},
    "docker": {"ps", "images", "version", "info", "inspect", "logs", "stats", "top", "port",
               "--version", "-v", "network", "volume", "compose"},
    "apt": {"list", "show", "policy", "search", "depends", "rdepends", "changelog"},
    "apt-cache": None,
    "apt-mark": {"showhold", "showmanual", "showauto"},
    "dpkg": {"-l", "-s", "-L", "--list", "--status", "--get-selections", "-S", "--print-architecture"},
    "dpkg-query": None,
    "rpm": {"-q", "-qa", "-qi", "-ql", "--query"},
    "yum": {"list", "info", "check-update", "history", "repolist", "--version"},
    "dnf": {"list", "info", "check-update", "history", "repolist", "--version"},
    "snap": {"list", "info", "version"},
    "npm": {"-v", "--version", "ls", "list", "view", "outdated", "root", "config", "doctor"},
    "nvm": {"ls", "list", "current", "version", "ls-remote", "--version", "which"},
    "n": {"--version", "ls", "ls-remote"},
    "node": {"-v", "--version"},
    "nodejs": {"-v", "--version"},
    "php": {"-v", "--version", "-m", "-i", "--ini", "--modules"},
    "php-fpm": {"-v", "--version", "-t"},
    "composer": {"--version", "-V", "show", "diagnose"},
    "python": {"--version", "-V"},
    "python3": {"--version", "-V"},
    "pip": {"--version", "-V", "list", "show", "freeze"},
    "pip3": {"--version", "-V", "list", "show", "freeze"},
    "pm2": {"list", "ls", "status", "jlist", "describe", "info", "show", "--version", "-v", "logs"},
    "git": {"status", "log", "rev-parse", "branch", "remote", "show", "diff", "describe", "--version"},
    "nginx": {"-v", "-V", "-t", "-T"},
    "apache2": {"-v", "-V"},
    "apache2ctl": {"-v", "-V", "-t", "-S", "-M", "configtest"},
    "httpd": {"-v", "-V", "-t"},
    "mysql": {"--version", "-V"},
    "mysqld": {"--version", "-V"},
    "mariadb": {"--version", "-V"},
    "psql": {"--version", "-V"},
    "redis-cli": {"--version", "-v", "info", "ping"},
    "redis-server": {"--version", "-v"},
    "java": {"-version", "--version"},
    "go": {"version", "env"},
    "ufw": {"status"},
    "iptables": {"-L", "-S", "--list"},
    "crontab": {"-l"},
    "certbot": {"certificates", "--version"},
    "kubectl": {"get", "describe", "version", "logs", "top"},
    "timeout": None,  # handled as a prefix
}

_PREFIXES = {"sudo", "time", "nice", "ionice", "nohup", "command", "builtin", "exec"}
_KEYWORDS = {"if", "then", "else", "elif", "fi", "do", "done", "while", "until", "!", "{", "}"}
_SAFE_REDIRECTS = re.compile(r"(?:\d?>&\d|&?\d?>\s*/dev/null|<\s*/dev/null)")
_ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")


def _strip_quoted(text: str, double: bool = True) -> str:
    """Blank out quoted text so it is not mistaken for operators. With double=False only
    single-quoted text is blanked (double quotes still expand $(...) and backticks)."""
    out, quote, escaped = [], None, False
    for ch in text:
        if escaped:
            escaped = False
            out.append(" " if quote else ch)
            continue
        if ch == "\\" and quote != "'":
            escaped = True
            out.append(ch)
            continue
        if quote:
            if ch == quote:
                quote = None
                out.append(ch)
            elif quote == '"' and not double:
                out.append(ch)
            else:
                out.append(" ")
        elif ch in "'\"":
            quote = ch
            out.append(ch)
        else:
            out.append(ch)
    return "".join(out)


def _flatten(command: str) -> list[str]:
    """Pull $(...) and `...` substitutions out as commands of their own (innermost first)."""
    parts = []
    for _ in range(50):
        masked = _strip_quoted(command, double=False)
        m = re.search(r"\$\(([^()]*)\)", masked) or re.search(r"`([^`]*)`", masked)
        if not m:
            break
        parts.append(command[m.start(1):m.end(1)])
        command = command[:m.start()] + "SUBST" + command[m.end():]
    else:
        parts.append("unparseable >")
    return [command, *parts]


def _segments(command: str) -> list[str]:
    """Split on ; && || | & newlines and ( ), quote-aware."""
    masked = _strip_quoted(command)
    cuts = [0]
    for m in re.finditer(r"&&|\|\||[;|&\n()]", masked):
        cuts += [m.start(), m.end()]
    cuts.append(len(command))
    parts = [command[cuts[i]:cuts[i + 1]] for i in range(0, len(cuts) - 1, 2)]
    return [p.strip() for p in parts if p.strip()]


def _segment_is_read_only(segment: str) -> bool:
    try:
        words = shlex.split(segment, comments=True)
    except ValueError:
        return False
    while words and (words[0] in _KEYWORDS or _ASSIGNMENT.match(words[0])):
        words = words[1:]
    if not words:
        return True
    if words[0] == "for":  # `for x in a b c` header
        return True
    if words[0] in ("command", "type") and len(words) > 1 and words[1] in ("-v", "-V", "-a", "-t"):
        return True
    while words and words[0] in _PREFIXES:
        words = words[1:]
        while words and words[0].startswith("-"):  # sudo -n, nice -n 5 …
            words = words[1:]
            if words and words[0].isdigit():
                words = words[1:]
    if words and words[0] == "timeout":
        words = [w for w in words[1:] if not w.startswith("-")][1:]
    if not words:
        return True
    name = words[0].rsplit("/", 1)[-1]
    args = words[1:]
    if name in _READ_ONLY:
        if name == "sed" and any(a == "-i" or a.startswith("-i") or a.startswith("--in-place") for a in args):
            return False
        if name == "find" and any(a in ("-delete", "-exec", "-execdir", "-ok", "-fprint") for a in args):
            return False
        if name in ("awk", "gawk", "mawk") and any("system(" in a or "print >" in a for a in args):
            return False
        if name == "curl" and any(a in ("-o", "-O", "--output", "--remote-name", "-T", "--upload-file",
                                        "-X", "--request", "-d", "--data") for a in args):
            return False
        if name == "xargs":
            rest = [a for a in args if not a.startswith("-")]
            return bool(rest) and _segment_is_read_only(shlex.join(rest))
        if name == "export":
            return True
        if name == "mount":
            return not args
        if name == "date":
            return not any(a in ("-s", "--set") for a in args)
        if name == "hostnamectl":
            return not args or args[0] == "status"
        if name == "ip":
            return not any(a in ("add", "del", "delete", "set", "flush", "change", "replace") for a in args)
        return True
    if name in _READ_ONLY_SUB:
        allowed = _READ_ONLY_SUB[name]
        if allowed is None:
            return True
        if name == "docker" and args and args[0] == "compose":
            return len(args) > 1 and args[1] in ("ps", "ls", "logs", "config", "version", "images")
        if name in ("docker",) and args and args[0] in ("network", "volume"):
            return len(args) > 1 and args[1] in ("ls", "inspect")
        if name in ("npm", "pip", "pip3") and args and args[0] == "config":
            return len(args) > 1 and args[1] in ("get", "list", "ls")
        first = next((a for a in args if a in allowed), None)
        positional = [a for a in args if not a.startswith("-")]
        if name in ("dpkg", "rpm", "node", "nodejs", "php", "php-fpm", "nginx", "httpd", "apache2",
                    "mysql", "mysqld", "mariadb", "psql", "java", "python", "python3", "redis-server",
                    "crontab", "iptables"):
            return bool(args) and args[0] in allowed and not (name == "crontab" and len(args) > 1)
        return first is not None and (not positional or positional[0] == first or first.startswith("-"))
    return False


def is_read_only(command: str) -> bool:
    for part in _flatten(command.strip()):
        part = _SAFE_REDIRECTS.sub(" ", part)
        masked = _strip_quoted(part)
        if ">" in masked or "<<" in masked:
            return False
        if not all(_segment_is_read_only(s) for s in _segments(part)):
            return False
    return True


# ---- execution wrapping ------------------------------------------------------------------

_SUDO = re.compile(r"(?<![\w./-])sudo(?=\s)")

# Non-interactive shells skip ~/.bashrc, where nvm is usually loaded; load it so `node`/`npm` resolve.
_PRELUDE = (
    'export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"; '
    '[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh" >/dev/null 2>&1; '
    'export PATH="$PATH:/usr/local/bin:/usr/sbin:/sbin"; '
)


def prepare(command: str, username: str, sudo_password: str | None) -> tuple[str, str | None]:
    """Return (remote command line, stdin text) for running `command` in a login bash.

    root: `sudo` is dropped. With a sudo password it is fed on stdin (`sudo -S`), never put on
    the command line; without one `sudo -n` fails fast instead of hanging on a prompt.
    """
    stdin = None
    count = len(_SUDO.findall(command))
    if count:
        if username == "root":
            command = _SUDO.sub("", command)
        elif sudo_password:
            command = _SUDO.sub("sudo -S -p ''", command)
            stdin = (sudo_password + "\n") * count
        else:
            command = _SUDO.sub("sudo -n", command)
    script = _PRELUDE + command
    return f"bash -lc {shlex.quote(script)}", stdin


# ---- local (this Windows PC, PowerShell) -----------------------------------------------------

# Cmdlets / aliases that only read. Everything else on this PC needs the user's approval.
_LOCAL_READ_ONLY = {
    "get-childitem", "gci", "dir", "ls", "get-item", "gi", "get-itemproperty", "test-path", "resolve-path",
    "split-path", "join-path", "get-content", "gc", "cat", "type", "get-filehash", "get-location", "gl", "pwd",
    "measure-object", "measure", "select-object", "select", "where-object", "where", "?", "sort-object", "sort",
    "group-object", "format-table", "ft", "format-list", "fl", "out-string", "select-string", "sls",
    "write-output", "echo", "get-command", "gcm", "get-psdrive", "get-volume", "get-date", "get-process", "gps",
    "where.exe", "hostname", "whoami",
}
# Programs checked with the server rules (e.g. `git status`, `node -v`, `pm2 list`).
_LOCAL_TOOLS = {"node", "npm", "git", "python", "pip", "composer", "pm2", "java", "go", "docker", "php"}
# Redirects, call operator, escapes, static calls, assignments and .Method( calls can all change things.
_LOCAL_UNSAFE = re.compile(r"[>&`=]|::|\.\s*[A-Za-z_]\w*\s*\(")


def _mask_ps(command: str) -> str | None:
    """Blank out PowerShell string contents; None if a double-quoted string runs code ($(...))."""
    out, quote = [], None
    for i, ch in enumerate(command):
        if quote:
            if ch == quote:
                quote = None
                out.append(ch)
            else:
                if quote == '"' and ch == "$" and command[i + 1:i + 2] == "(":
                    return None
                out.append(" ")
        else:
            if ch in "'\"":
                quote = ch
            out.append(ch)
    return "".join(out)


def is_local_read_only(command: str) -> bool:
    masked = _mask_ps(command.strip())
    if masked is None or _LOCAL_UNSAFE.search(masked):
        return False
    for segment in re.split(r"[|;{}()\n]", masked):
        words = segment.split()
        if not words:
            continue
        first = words[0].lower()
        if first.startswith(("$", "-")) or first[0].isdigit() or re.match(r"\.[a-z_]", first):
            continue  # expression: $_.Length -gt 1GB, -not …, (Get-Item x).Length
        name = first[:-4] if first.endswith(".exe") and first != "where.exe" else first
        if name in _LOCAL_READ_ONLY:
            continue
        if name in _LOCAL_TOOLS and _segment_is_read_only(" ".join([name, *words[1:]])):
            continue
        return False
    return True
