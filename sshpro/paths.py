"""Filesystem locations. Runtime data (db, logs) lives outside the repo, in %LOCALAPPDATA%/SSH-Pro-AI."""

from __future__ import annotations

import os
from pathlib import Path

APP_NAME = "SSH-Pro-AI"
APP_ROOT = Path(__file__).resolve().parents[1]   # repo root
WEB_DIR = Path(__file__).resolve().parent / "web"
TERMIX_DB = APP_ROOT / "ssh.sqlite"


def data_dir() -> Path:
    """Override with SSHPRO_DATA_DIR (used by tests)."""
    override = os.environ.get("SSHPRO_DATA_DIR")
    if override:
        base = Path(override)
    else:
        local = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        base = Path(local) / APP_NAME
    base.mkdir(parents=True, exist_ok=True)
    return base


def logs_dir() -> Path:
    path = data_dir() / "logs"
    path.mkdir(parents=True, exist_ok=True)
    return path


def db_path() -> Path:
    return data_dir() / "sshpro.sqlite3"
