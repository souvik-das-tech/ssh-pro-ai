"""Secrets at rest are encrypted with Windows DPAPI (bound to the current Windows user), so the
database file is useless if copied to another account or machine. No master password needed."""

from __future__ import annotations

import base64

import win32crypt

_PREFIX = "dpapi:"
_DESCRIPTION = "SSH-Pro-AI"


def protect(value: str | None) -> str | None:
    if not value:
        return None
    blob = win32crypt.CryptProtectData(value.encode("utf-8"), _DESCRIPTION, None, None, None, 0)
    return _PREFIX + base64.b64encode(blob).decode("ascii")


def unprotect(value: str | None) -> str | None:
    if not value:
        return None
    if not value.startswith(_PREFIX):
        raise ValueError("secret is not DPAPI-protected")
    blob = base64.b64decode(value[len(_PREFIX):])
    return win32crypt.CryptUnprotectData(blob, None, None, None, 0)[1].decode("utf-8")
