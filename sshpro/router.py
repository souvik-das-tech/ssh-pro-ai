"""Auto model choice: pick a fast or a smart model for each request, locally and instantly.

No AI call decides this (that would cost the tokens and seconds Auto is meant to save): a keyword
classifier that knows English, Banglish and Hinglish sends quick look-ups and chat to the fast
model, and troubleshooting, security, changes and long multi-step requests to the smart one.
"""

from __future__ import annotations

import re
from collections.abc import Iterable

AUTO = "auto"

# (reason shown to the user, pattern). Checked in order; the first hit wins.
_SMART = [
    ("security check", r"\b(cve-?\d*|vulnerab\w*|secur\w*|exploit\w*|malware|hack\w*|audit\w*|breach|"
                       r"rootkit|suspicious|patch(ed|es)?|harden\w*|brute.?force|intrusion)\b"),
    ("troubleshooting", r"\b(why|keno|kyu|kyon|kyun|debug\w*|troubleshoot\w*|diagnos\w*|investigat\w*|"
                        r"root.?cause|error\w*|fail\w*|crash\w*|broken|not working|down|slow\w*|hang\w*|"
                        r"timeout|502|503|504|500|oom|leak\w*|problem|somossa|issue|kharap|"
                        r"chol(che|ch?e) ?na|kaj kor(che|ch?e) ?na|nahi? chal|kaam nahi?|thik ?(koro|kar)|fix\w*)\b"),
    ("change to servers", r"\b(install\w*|uninstall\w*|upgrad\w*|updat\w*|deploy\w*|migrat\w*|restor\w*|"
                          r"backup\w*|configur\w*|setup|set up|enable|disable|restart\w*|reboot|delete|"
                          r"remov\w*|clean\w*|kill|rollback|renew\w*|rotate|creat\w*|add|chang\w*|edit|"
                          r"upload\w*|download\w*|import\w*|dump|kor[eo] dao|kar ?do|bana(o|do)|hatao|muchh?o)\b"),
    ("analysis", r"\b(analy[sz]\w*|compar\w*|optimi[sz]\w*|recommend\w*|plan|strategy|explain|best way|"
                 r"tune|tuning|capacity|bottleneck|review)\b"),
    ("complex setup", r"\b(nginx|apache|ssl|tls|certbot|firewall|ufw|iptables|docker.?compose|kubernetes|"
                      r"k8s|mysql|mariadb|postgres\w*|redis|replication|cron\w*|systemd|selinux)\b.*\b"
                      r"(config\w*|rule\w*|set\w*|chang\w*|fix\w*|move|switch)\b"),
]
_SMART_RE = [(why, re.compile(p, re.IGNORECASE)) for why, p in _SMART]
LONG_REQUEST = 220  # characters; long requests usually carry several steps or conditions

# Defaults when the user has not picked models for Auto: the first available match wins.
FAST_PREFERENCE = (r"gemini.*flash.*low", r"gemini.*flash.*medium", r"chatgpt:gpt-5\.6-luna", r"flash", r"luna")
SMART_PREFERENCE = (r"chatgpt:gpt-6", r"claude-sonnet", r"claude-opus", r"gemini.*flash.*high", r"terra")


def classify(prompt: str) -> tuple[str, str]:
    """("fast" | "smart", reason) for one user request."""
    text = prompt.strip()
    for why, pattern in _SMART_RE:
        if pattern.search(text):
            return "smart", why
    if len(text) > LONG_REQUEST or text.count("\n") >= 3:
        return "smart", "long request"
    return "fast", "quick check"


def pick_default(available: Iterable[str], preference: Iterable[str], avoid: str | None = None) -> str | None:
    ids = [m for m in available if m != AUTO]
    for pattern in preference:
        for model in ids:
            if model != avoid and re.search(pattern, model, re.IGNORECASE):
                return model
    return next((m for m in ids if m != avoid), ids[0] if ids else None)
