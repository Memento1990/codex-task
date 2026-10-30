"""Windows 悬浮看板入口。"""
import argparse
from pathlib import Path
import sys

from PySide6.QtWidgets import QApplication

from floatboard import __version__
from floatboard.core import atomic_json, data_root
from floatboard.gui import Board
from floatboard.runtime import ProcessLock, spawn_worker


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir")
    parser.add_argument("--version", action="version", version=__version__)
    args = parser.parse_args()
    root = data_root(args.data_dir)
    root.mkdir(parents=True, exist_ok=True)
    lock = ProcessLock(root / "gui.lock")
    if not lock.acquire():
        atomic_json(root / "show.request", {})
        return 0
    app = QApplication(sys.argv[:1])
    app.setApplicationName("CodexFloatboard")
    app.setApplicationVersion(__version__)
    app.setQuitOnLastWindowClosed(False)
    spawn_worker(root)
    board = Board(root)
    board.show()
    try:
        return app.exec()
    finally:
        lock.close()


if __name__ == "__main__":
    raise SystemExit(main())
