"""Command safety: decide whether a shell command only *reads* (runs without asking) or may
*change* a server (needs the user's approval). Unknown commands count as changes.

This is an allowlist on purpose: the AI's own risk label is never trusted alone. "Read-only" also
means nothing leaves the server on its own: network tools with variable or piped data need approval.

redact_secrets() masks keys, passwords and tokens in command output before it goes to the AI.
"""

from __future__ import annotations

import re
import shlex

# Commands that only read state, whatever their arguments.
_READ_ONLY = {
    "cat", "head", "tail", "grep", "egrep", "fgrep", "zgrep", "wc", "sort", "uniq",
    "cut", "tr", "column", "echo", "printf", "true", "false", "test", "[", "[[", "basename",
    "dirname", "readlink", "realpath", "stat", "file", "ls", "du", "df", "free", "uptime", "uname",
    "hostname", "hostnamectl", "whoami", "id", "groups", "who", "w", "last", "date", "timedatectl",
    "nproc", "lscpu", "lsblk", "lsmem", "lsof", "lsb_release", "ps", "pgrep", "pstree", "top",
    "vmstat", "iostat", "mpstat", "sar", "netstat", "ss", "ip", "ifconfig", "ping", "dig",
    "nslookup", "host", "which", "whereis", "type", "command", "printenv", "getent",
    "md5sum", "sha256sum", "sha1sum", "diff", "cmp", "jq", "nvidia-smi", "sensors", "dmesg",
    "journalctl", "openssl", "certbot-certificates", "locale", "ulimit", "tree", "seq", "sleep",
    "export", "awk", "gawk", "mawk", "sed", "find", "xargs", "curl", "zcat", "base64", "hexdump", "sar",
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
    "iptables": set(),  # these four: see _segment_is_read_only
    "ip6tables": set(),
    "iptables-save": set(),
    "nft": set(),
    "crontab": {"-l"},
    "certbot": {"certificates", "--version"},
    "kubectl": {"get", "describe", "version", "logs", "top"},
    "timeout": None,  # handled as a prefix
}

_PREFIXES = {"sudo", "time", "nice", "ionice", "command", "builtin", "exec", "env"}  # not nohup: writes nohup.out
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


_SUBST = "__SUBST__"  # stands in for a $(...) / `...` substitution that was pulled out


def _flatten(command: str) -> list[str]:
    """Pull $(...) and `...` substitutions out as commands of their own (innermost first)."""
    parts = []
    for _ in range(50):
        masked = _strip_quoted(command, double=False)
        m = re.search(r"\$\(([^()]*)\)", masked) or re.search(r"`([^`]*)`", masked)
        if not m:
            break
        parts.append(command[m.start(1):m.end(1)])
        command = command[:m.start()] + _SUBST + command[m.end():]
    else:
        parts.append("unparseable >")
    return [command, *parts]


def _segments(command: str) -> list[tuple[str, str]]:
    """Split on ; && || | & newlines and ( ), quote-aware. Each part comes with the operator
    before it ("" for the first), so the reading end of a pipe can be told apart."""
    masked = _strip_quoted(command)
    parts, start, op = [], 0, ""
    for m in re.finditer(r"&&|\|\||[;|&\n()]", masked):
        parts.append((op, command[start:m.start()]))
        op, start = m.group(), m.end()
    parts.append((op, command[start:]))
    return [(o, p.strip()) for o, p in parts if p.strip()]


# Variables that change what later commands run or load: setting them is not "reading".
_RISKY_VAR = re.compile(r"^(?:LD_\w+|BASH_ENV|ENV|PATH|PS4|SHELLOPTS|BASHOPTS|PROMPT_COMMAND|IFS|"
                        r"GIT_\w+|PAGER|LESSOPEN|LESSCLOSE|EDITOR|VISUAL|PYTHON\w*|NODE_OPTIONS|PERL5\w*)=")
# Commands that talk to other hosts: a variable or substitution in their arguments (or data piped
# into them) could carry server data out, so those need approval.
_NETWORK = {"curl", "ping", "dig", "nslookup", "host", "openssl"}
# short curl options that take a value (to find where a cluster like -sSLo ends)
_CURL_VALUE = set("AbcCdDeEFHKmoPQrTuUwxXyYzt")
_CURL_BAD_LONG = {
    "--output", "--remote-name", "--remote-name-all", "--upload-file", "--request", "--data", "--data-binary",
    "--data-raw", "--data-urlencode", "--data-ascii", "--json", "--form", "--form-string", "--config",
    "--cookie-jar", "--dump-header", "--output-dir", "--create-dirs", "--trace", "--trace-ascii", "--libcurl",
    "--etag-save", "--hsts", "--alt-svc", "--stderr", "--upload", "--mail-rcpt", "--unix-socket",
}
# a sed script made only of these commands cannot write files or run programs
_SED_ADDR = r"(?:\d+|\$|/(?:[^/\\]|\\.)*/)"
_SED_CMD = re.compile(
    rf"^\s*(?:{_SED_ADDR}(?:\s*,\s*(?:{_SED_ADDR}|\+\d+))?)?\s*!?\s*"
    r"(?:[pdqQ=nNlgGhHx]|s(.)(?:(?!\1)[^\\]|\\.)*\1(?:(?!\1)[^\\]|\\.)*\1[gpiImM0-9]*)?\s*$"
)
_AWK_BAD = re.compile(r"\bsystem\s*\(|\bgetline\b|\|&|@load|@include|\bprintf?\b[^;}]*[>|]")
_GIT_BRANCH_BAD = {"-d", "-D", "-m", "-M", "-c", "-C", "-f", "-u", "--delete", "--move", "--copy", "--force",
                   "--set-upstream-to", "--unset-upstream", "--edit-description", "--track", "--no-track"}
_NVIDIA_SET = {
    "-pm", "--persistence-mode", "-e", "--ecc-config", "-p", "--reset-ecc-errors", "-c", "--compute-mode", "-r",
    "--gpu-reset", "-ac", "--applications-clocks", "-rac", "--reset-applications-clocks", "-acp",
    "--applications-clocks-permission", "-pl", "--power-limit", "-cc", "--cuda-clocks", "-am", "--accounting-mode",
    "-caa", "--clear-accounted-apps", "--auto-boost-default", "--auto-boost-permission", "-lgc",
    "--lock-gpu-clocks", "-rgc", "--reset-gpu-clocks", "-lmc", "--lock-memory-clocks", "-rmc",
    "--reset-memory-clocks", "-mig", "--multi-instance-gpu", "-dm", "--driver-model", "-fdm",
    "--force-driver-model", "-f", "--filename", "-gom", "--gom",
}
_NVIDIA_SET_SUB = {"mig", "drain", "clocks", "conf-compute", "boost-slider", "power-hint", "vgpu", "nvlink", "c2c"}
# runtimes that can execute code: only the exact version/info flags, nothing else on the line
_EXACT = {"node", "nodejs", "php", "php-fpm", "python", "python3", "java", "mysql", "mysqld", "mariadb", "psql",
          "redis-server", "nginx", "httpd", "apache2"}


def _long_name(arg: str) -> str:
    return arg.split("=", 1)[0]


def _curl_ok(args: list[str]) -> bool:
    for i, a in enumerate(args):
        nxt = args[i + 1] if i + 1 < len(args) else ""
        if a.startswith("@"):
            return False  # @file reads a local file into the request
        if a.startswith("--"):
            name = _long_name(a)
            value = a.split("=", 1)[1] if "=" in a else nxt
            if name in ("--output", "--dump-header"):
                if value not in ("/dev/null", "-"):
                    return False
            elif name in _CURL_BAD_LONG or (name == "--write-out" and "output{" in value):
                return False
            if value.startswith("@"):
                return False
        elif a.startswith("-") and len(a) > 1:
            for j, ch in enumerate(a[1:], 1):
                if ch in "OJ":
                    return False
                if ch in _CURL_VALUE:
                    value = a[j + 1:] or nxt
                    if ch in "oD":
                        if value not in ("/dev/null", "-"):
                            return False
                    elif ch in "TXdFKcy" or (ch == "w" and "output{" in value) or value.startswith("@"):
                        return False
                    break
    return True


def _sed_ok(args: list[str]) -> bool:
    scripts, positional, i = [], [], 0
    while i < len(args):
        a = args[i]
        if a in ("-e", "--expression"):
            scripts.append(args[i + 1] if i + 1 < len(args) else "")
            i += 2
            continue
        if a.startswith("--expression="):
            scripts.append(a.split("=", 1)[1])
        elif a in ("-f", "--file") or a.startswith(("--file=", "--in-place")) or (
                a.startswith("-") and not a.startswith("--") and any(c in a[1:] for c in "if")):
            return False  # -i edits in place, -f loads a script file
        elif a in ("-l", "--line-length"):
            i += 1
        elif not a.startswith("-"):
            positional.append(a)
        i += 1
    if not scripts:
        if not positional:
            return False
        scripts = [positional[0]]
    return all(_SED_CMD.match(cmd) for script in scripts for cmd in re.split(r"[;\n]", script))


def _awk_ok(args: list[str]) -> bool:
    for a in args:
        if a in ("-f", "--file", "-i", "--include", "-l", "--load", "-E", "--exec") or a.startswith(
                ("--file=", "--include=", "--load=", "--exec=")):
            return False
        if _AWK_BAD.search(a):
            return False  # system(), getline, print > file, print | "cmd"
    return True


def _flags_ok(args: list[str], bad_short: str = "", bad_long: tuple[str, ...] = (), value_short: str = "") -> bool:
    """False if a short cluster holds a letter of `bad_short` (scanning stops at a value-taking
    letter) or a long option starts with one of `bad_long`."""
    for a in args:
        if a.startswith("--"):
            if bad_long and _long_name(a).startswith(bad_long):
                return False
        elif a.startswith("-") and len(a) > 1:
            for ch in a[1:]:
                if ch in bad_short:
                    return False
                if ch in value_short:
                    break
    return True


def _positional(args: list[str], value_flags: tuple[str, ...] = ()) -> list[str]:
    out, skip = [], False
    for a in args:
        if skip:
            skip = False
        elif a in value_flags:
            skip = True
        elif not a.startswith("-"):
            out.append(a)
    return out


def _ip_ok(args: list[str]) -> bool:
    words, i = [], 0
    while i < len(args):
        a = args[i]
        if a in ("-batch", "-b", "-force"):
            return False
        if a in ("-f", "-family", "-n", "-netns", "-rc", "-rcvbuf", "-l", "-loops"):
            i += 2
            continue
        if not a.startswith("-"):
            words.append(a)
        i += 1
    # ip [options] OBJECT [COMMAND ...]: only the listing commands read
    return len(words) < 2 or words[1] in ("show", "list", "lst", "sh", "ls", "l", "get", "monitor")


def _iptables_ok(args: list[str]) -> bool:
    listed, i = False, 0
    while i < len(args):
        a = args[i]
        if a in ("-t", "--table"):
            i += 2
            continue
        if a.startswith("--"):
            if a not in ("--list", "--list-rules", "--numeric", "--verbose", "--exact", "--line-numbers"):
                return False
            listed |= a.startswith("--list")
        elif a.startswith("-"):
            if not set(a[1:]) <= set("LSnvx"):
                return False  # -A -D -I -F -Z -P … change the rules or counters
            listed |= bool(set(a[1:]) & set("LS"))
        i += 1
    return listed


def _command_ok(name: str, args: list[str]) -> bool:
    """Argument checks for commands in _READ_ONLY that can also write or change things."""
    if name == "sed":
        return _sed_ok(args)
    if name == "find":
        return not any(a in ("-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf",
                             "-fls") for a in args)
    if name in ("awk", "gawk", "mawk"):
        return _awk_ok(args)
    if name == "curl":
        return _curl_ok(args)
    if name == "export":
        return not any(_RISKY_VAR.match(a) for a in args)
    if name == "mount":
        return not args
    if name == "date":
        for a in args:
            if a.startswith("--set"):
                return False
            if a.startswith("-") and not a.startswith(("--", "-I")):
                for ch in a[1:]:
                    if ch == "s":
                        return False
                    if ch in "dfr":
                        break
        return True
    if name == "hostnamectl":
        return not args or args[0] == "status"
    if name == "hostname":
        return all(a.startswith("-") for a in args) and not any(a in ("-F", "--file", "-b", "--boot") for a in args)
    if name == "ifconfig":
        return len(_positional(args)) <= 1 and all(a in ("-a", "-s", "-v") for a in args if a.startswith("-"))
    if name == "timedatectl":
        pos = _positional(args)
        return not pos or pos[0] in ("status", "show", "list-timezones", "timesync-status", "show-timesync")
    if name == "ip":
        return _ip_ok(args)
    if name == "ss":
        return "--kill" not in args and _flags_ok(args, "K", value_short="fAFN")
    if name == "sensors":
        return not any(a in ("-s", "--set") for a in args)
    if name == "file":
        return not any(a in ("-C", "--compile") for a in args)
    if name == "dmesg":
        return _flags_ok(args, "cCDEn", ("--clear", "--read-clear", "--console"), value_short="lfFsS")
    if name == "journalctl":
        return _flags_ok(args, bad_long=("--vacuum", "--rotate", "--flush", "--sync", "--relinquish-var",
                                         "--smart-relinquish-var", "--setup-keys", "--update-catalog",
                                         "--cursor-file"))
    if name == "sort":
        return _flags_ok(args, "o", ("--output",), value_short="ktST")
    if name == "uniq":
        return len(_positional(args, ("-f", "-s", "-w"))) <= 1  # a second file name is the output
    if name == "tree":
        return not any(a == "-R" or a.startswith("-o") for a in args)
    if name == "sar":
        return not any(a.startswith("-o") for a in args)
    if name == "openssl":
        pos = _positional(args)
        return bool(pos) and pos[0] in (
            "version", "x509", "s_client", "verify", "crl", "req", "rsa", "ec", "pkey", "pkcs7", "ciphers", "list",
            "dgst", "asn1parse", "sess_id", "errstr", "crl2pkcs7",
        ) and not any(_long_name(a) in ("-out", "-keyout", "-writerand", "-sess_out", "-keylogfile",
                                        "-CAcreateserial", "-msgfile") for a in args)
    if name == "nvidia-smi":
        return not any(_long_name(a) in _NVIDIA_SET for a in args) and not (set(_positional(args)) & _NVIDIA_SET_SUB)
    if name == "dig":
        return not any(a.startswith("-f") for a in args)  # -f reads queries from a file
    return True


def _is_literal_echo(segment: str) -> bool:
    """`echo` / `printf` / `true` with fixed text: safe to pipe into a network command."""
    try:
        words = shlex.split(segment)
    except ValueError:
        return False
    return bool(words) and words[0] in ("echo", "printf", "true") and not any("$" in w or _SUBST in w for w in words)


def _segment_is_read_only(segment: str, piped: bool = False, upstream: str = "") -> bool:
    try:
        words = shlex.split(segment, comments=True)
    except ValueError:
        return False
    if any("/dev/tcp/" in w or "/dev/udp/" in w for w in words):
        return False
    while words and (words[0] in _KEYWORDS or _ASSIGNMENT.match(words[0])):
        if _RISKY_VAR.match(words[0]):
            return False
        words = words[1:]
    if not words:
        return True
    if words[0] == "for":  # `for x in a b c` header
        return True
    if words[0] in ("command", "type") and len(words) > 1 and words[1] in ("-v", "-V", "-a", "-t"):
        return True
    while words and words[0] in _PREFIXES:
        prefix, words = words[0], words[1:]
        while words and (words[0].startswith("-") or (prefix == "env" and _ASSIGNMENT.match(words[0]))):
            if prefix == "ionice" and words[0] in ("-p", "-P", "-u"):
                return False  # changes the priority of a running process
            if _RISKY_VAR.match(words[0]):
                return False
            words = words[1:]
            if words and words[0].isdigit():
                words = words[1:]
    if words and words[0] == "timeout":
        words = [w for w in words[1:] if not w.startswith("-")][1:]
    if not words:
        return True
    if "/" in words[0] and not words[0].startswith(("/usr/bin/", "/bin/", "/usr/sbin/", "/sbin/", "/usr/local/bin/")):
        return False  # ./cat or /tmp/x/ls is not the system tool
    name = words[0].rsplit("/", 1)[-1]
    args = words[1:]
    if name in _NETWORK:
        if any("$" in a or _SUBST in a for a in args):
            return False  # the request itself could carry data from the server
        sends_stdin = name in ("nslookup", "dig", "host", "ping") or (name == "openssl" and "s_client" in args)
        if piped and sends_stdin and not _is_literal_echo(upstream):
            return False  # piped data would be sent on (openssl s_client, nslookup …)
    if name in _READ_ONLY:
        if name == "xargs":
            rest = [a for a in args if not a.startswith("-")]
            return bool(rest) and rest[0] not in _NETWORK and _segment_is_read_only(shlex.join(rest))
        return _command_ok(name, args)
    if name in _READ_ONLY_SUB:
        allowed = _READ_ONLY_SUB[name]
        if allowed is None:
            return True
        if name == "docker" and args and args[0] == "compose":
            return len(args) > 1 and args[1] in ("ps", "ls", "logs", "config", "version", "images")
        if name == "docker" and args and args[0] in ("network", "volume"):
            return len(args) > 1 and args[1] in ("ls", "inspect")
        if name in ("npm", "pip", "pip3") and args and args[0] == "config":
            return len(args) > 1 and args[1] in ("get", "list", "ls")
        if name in ("iptables", "ip6tables"):
            return _iptables_ok(args)
        if name == "iptables-save":
            return not any(a == "-f" or a.startswith("--file") for a in args)
        if name == "nft":
            pos = _positional(args)
            return bool(pos) and pos[0] == "list" and "-f" not in args
        if name in _EXACT:
            return bool(args) and all(a in allowed for a in args)
        if name == "git":
            if any(a.startswith("--output") for a in args):
                return False
            while len(args) > 1 and args[0] == "-C":  # git -C <dir> status
                args = args[2:]
            pos = _positional(args)
            if pos and pos[0] == "remote":
                sub = args[args.index("remote") + 1:]
                return not sub or sub[0] in ("show", "get-url") or (sub[0] in ("-v", "--verbose") and len(sub) == 1)
            if pos and pos[0] == "branch":
                sub = args[args.index("branch") + 1:]
                if any(_long_name(a) in _GIT_BRANCH_BAD for a in sub):
                    return False
                listing = {"--list", "-l", "--contains", "--merged", "--no-merged", "--points-at"}
                return not _positional(sub) or bool(listing & {_long_name(a) for a in sub})
        if name == "go" and args and args[0] == "env":
            return not any(a in ("-w", "-u") for a in args)
        if name in ("yum", "dnf") and "history" in args:
            sub = _positional(args[args.index("history") + 1:])
            return not sub or sub[0] in ("list", "info", "summary", "userinstalled", "stats")
        if name in ("dpkg", "rpm", "crontab"):
            return bool(args) and args[0] in allowed and not (name == "crontab" and len(args) > 1)
        first = next((a for a in args if a in allowed), None)
        positional = [a for a in args if not a.startswith("-")]
        if first is None:
            return False
        if first.startswith("-"):
            # a flag like `systemctl --failed` only reads on its own: `--failed restart x` restarts
            return not positional
        return positional[0] == first
    return False


def is_read_only(command: str) -> bool:
    for part in _flatten(command.strip()):
        part = _SAFE_REDIRECTS.sub(" ", part)
        masked = _strip_quoted(part)
        if ">" in masked or "<<" in masked:
            return False
        segments = _segments(part)
        for i, (op, segment) in enumerate(segments):
            upstream = segments[i - 1][1] if op == "|" and i else ""
            if not _segment_is_read_only(segment, piped=op == "|", upstream=upstream):
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


# ---- secrets in output ------------------------------------------------------------------------

_REDACTED = "[REDACTED]"
_PEM = re.compile(r"-----BEGIN ([A-Z0-9 ]*PRIVATE KEY)-----.*?(?:-----END \1-----|\Z)", re.S)
_TOKENS = re.compile(
    r"\bAKIA[0-9A-Z]{16}\b|\bgh[pousr]_[A-Za-z0-9]{30,}|\bgithub_pat_\w{30,}|\bsk-[A-Za-z0-9_-]{20,}"
    r"|\bxox[abprs]-[A-Za-z0-9-]{10,}|\bAIza[0-9A-Za-z_-]{35}|\bglpat-[A-Za-z0-9_-]{20,}"
    r"|\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"
)
_BEARER = re.compile(r"(?i)\b(bearer\s+)[A-Za-z0-9._~+/=-]{16,}")
_URL_CREDS = re.compile(r"(?i)\b([a-z][a-z0-9+.-]*://[^/\s:@]+):([^@\s/]+)@")
_SHADOW = re.compile(r"(?m)^([^:\s]+):(\$[^:\s]+):")
# key = value / key: value / "key": "value" where the key names a secret (not /etc/passwd paths)
_SECRET_KV = re.compile(
    r"(?i)((?<![/\w.-])[\w.-]*(?:pass(?:word|wd|phrase)?|pwd|secret|token|api[_-]?key|access[_-]?key"
    r"|private[_-]?key|credentials?|auth[_-]?key|salt)[\w.-]*[\"']?\s*[:=]\s*)([\"']?)([^\s\"',;]{3,})"
)


def redact_secrets(text: str) -> str:
    """Mask private keys, passwords, tokens and password hashes so they never reach the AI."""
    if not text:
        return text
    text = _PEM.sub(f"-----{_REDACTED} PRIVATE KEY-----", text)
    text = _TOKENS.sub(_REDACTED, text)
    text = _BEARER.sub(lambda m: m.group(1) + _REDACTED, text)
    text = _URL_CREDS.sub(lambda m: f"{m.group(1)}:{_REDACTED}@", text)
    text = _SHADOW.sub(lambda m: f"{m.group(1)}:{_REDACTED}:", text)
    return _SECRET_KV.sub(lambda m: m.group(1) + m.group(2) + _REDACTED, text)
