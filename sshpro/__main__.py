"""Entry point: `pythonw -m sshpro` (desktop window) or `python -m sshpro --serve` (backend only,
prints the URL to open in a browser; for development)."""

from __future__ import annotations

import argparse
import asyncio
import ctypes
import getpass
import logging
import sys
import threading
from logging.handlers import RotatingFileHandler
from pathlib import Path

from sshpro.paths import WEB_DIR, db_path, logs_dir

log = logging.getLogger("sshpro")


def setup_logging() -> None:
    handler = RotatingFileHandler(logs_dir() / "sshpro.log", maxBytes=2_000_000, backupCount=3, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    root.addHandler(handler)
    if sys.stderr is not None:
        root.addHandler(logging.StreamHandler())
    logging.getLogger("aiohttp.access").setLevel(logging.WARNING)


class BackendThread(threading.Thread):
    """Runs the aiohttp backend (and every SSH/AGY task) on its own asyncio loop."""

    def __init__(self, port: int = 0):
        super().__init__(name="sshpro-backend", daemon=True)
        self.port = port
        self.ready = threading.Event()
        self.url = ""
        self.error: BaseException | None = None
        self.loop: asyncio.AbstractEventLoop | None = None
        self._runner = None

    def run(self) -> None:
        from aiohttp import web

        from sshpro.db import Database
        from sshpro.server import Backend

        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)
        try:
            backend = Backend(Database(db_path()))
            self._runner = web.AppRunner(backend.app(), access_log=None)
            self.loop.run_until_complete(self._runner.setup())
            site = web.TCPSite(self._runner, "127.0.0.1", self.port)
            self.loop.run_until_complete(site.start())
            port = site._server.sockets[0].getsockname()[1]  # type: ignore[union-attr]
            self.loop.run_until_complete(backend.startup())
            self.url = f"http://127.0.0.1:{port}/?t={backend.token}"
        except BaseException as exc:
            self.error = exc
            self.ready.set()
            raise
        self.ready.set()
        self.loop.run_forever()

    def shutdown(self) -> None:
        if self.loop is None or self._runner is None:
            return
        future = asyncio.run_coroutine_threadsafe(self._runner.cleanup(), self.loop)
        try:
            future.result(10)
        except Exception:
            log.exception("Backend shutdown")
        self.loop.call_soon_threadsafe(self.loop.stop)


def dark_title_bar(hwnd: int) -> None:
    try:  # Windows 11: DWMWA_USE_IMMERSIVE_DARK_MODE = 20, DWMWA_CAPTION_COLOR = 35
        dwm = ctypes.windll.dwmapi
        on = ctypes.c_int(1)
        dwm.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(on), ctypes.sizeof(on))
        color = ctypes.c_int(0x00170F0B)  # COLORREF 0x00BBGGRR -> #0b0f17
        dwm.DwmSetWindowAttribute(hwnd, 35, ctypes.byref(color), ctypes.sizeof(color))
    except Exception:
        pass


def run_window(backend: BackendThread) -> int:
    from PySide6.QtCore import QUrl
    from PySide6.QtGui import QColor, QIcon
    from PySide6.QtNetwork import QLocalServer, QLocalSocket
    from PySide6.QtWebEngineCore import QWebEngineSettings
    from PySide6.QtWebEngineWidgets import QWebEngineView
    from PySide6.QtWidgets import QApplication, QFileDialog, QMainWindow, QMessageBox

    try:  # own taskbar group + icon instead of python's
        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID("SSHProAI.Desktop")
    except Exception:
        pass
    app = QApplication(sys.argv)
    app.setApplicationName("SSH Pro AI")
    icon = QIcon(str(WEB_DIR / "icon.png"))
    app.setWindowIcon(icon)

    # One window at a time: a second double-click brings the first to the front.
    name = f"SSHProAI-{getpass.getuser()}"
    probe = QLocalSocket()
    probe.connectToServer(name)
    if probe.waitForConnected(400):
        probe.write(b"show")
        probe.flush()
        probe.waitForBytesWritten(400)
        return 0
    QLocalServer.removeServer(name)
    single = QLocalServer()
    single.listen(name)

    backend.start()
    backend.ready.wait(60)
    if backend.error or not backend.url:
        QMessageBox.critical(None, "SSH Pro AI", f"Backend failed to start:\n{backend.error}")
        return 1

    window = QMainWindow()
    window.setWindowTitle("SSH Pro AI")
    window.setWindowIcon(icon)
    window.resize(1480, 920)
    window.setMinimumSize(980, 620)
    view = QWebEngineView(window)
    view.page().setBackgroundColor(QColor("#0b0f17"))
    settings = view.settings()
    settings.setAttribute(QWebEngineSettings.WebAttribute.JavascriptCanAccessClipboard, True)
    settings.setAttribute(QWebEngineSettings.WebAttribute.JavascriptCanPaste, True)
    def save_download(download) -> None:
        """Downloads (the backup export) are cancelled unless accepted: ask where to save."""
        suggested = str(Path.home() / "Documents" / download.downloadFileName())
        path, _ = QFileDialog.getSaveFileName(window, "Save backup", suggested,
                                              "SSH Pro AI backup (*.sshpro);;All files (*)")
        if not path:
            download.cancel()
            return
        target = Path(path)
        download.setDownloadDirectory(str(target.parent))
        download.setDownloadFileName(target.name)
        download.accept()

    view.page().profile().downloadRequested.connect(save_download)
    view.setUrl(QUrl(backend.url))
    window.setCentralWidget(view)

    def bring_front() -> None:
        while single.hasPendingConnections():
            single.nextPendingConnection().close()
        window.showNormal() if window.isMinimized() else window.show()
        window.raise_()
        window.activateWindow()

    single.newConnection.connect(bring_front)
    window.show()
    dark_title_bar(int(window.winId()))
    app.aboutToQuit.connect(backend.shutdown)
    return app.exec()


def main() -> int:
    parser = argparse.ArgumentParser(prog="sshpro")
    parser.add_argument("--serve", action="store_true", help="backend only; print the URL")
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    setup_logging()
    backend = BackendThread(args.port)
    if args.serve:
        backend.start()
        backend.ready.wait(60)
        if backend.error:
            return 1
        print(backend.url, flush=True)
        try:
            backend.join()
        except KeyboardInterrupt:
            backend.shutdown()
        return 0
    return run_window(backend)


if __name__ == "__main__":
    sys.exit(main())
