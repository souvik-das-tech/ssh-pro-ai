import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


def pytest_configure(config):
    os.environ.setdefault("SSHPRO_DATA_DIR", str(Path(config.rootpath) / ".pytest-data"))
