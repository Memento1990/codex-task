"""进程边界：采集器独立于 GUI，不依赖网络端口。"""
import json
import os
from pathlib import Path
import subprocess
import sys
import sqlite3
import time

from .core import Store, atomic_json


def command(mode, root):
    if getattr(sys, "frozen", False):
        return [str(Path(sys.executable).with_name("FloatboardReporter.exe")), mode, "--data-dir", str(root)]
    return [sys.executable, str(Path(__file__).parents[1] / "reporter.py"), mode, "--data-dir", str(root)]


def spawn_worker(root):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    probe = ProcessLock(root / "collector.lock")
    if not probe.acquire():
        return
    probe.close()
    subprocess.Popen(command("--worker", root), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)


class ProcessLock:
    def __init__(self, path):
        self.path = Path(path)
        self.stream = None

    def acquire(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.stream = self.path.open("a+b")
        self.stream.seek(0)
        self.stream.write(b"0")
        self.stream.flush()
        self.stream.seek(0)
        try:
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(self.stream.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return True
        except OSError:
            self.stream.close()
            self.stream = None
            return False

    def close(self):
        if self.stream:
            self.stream.close()
            self.stream = None


def drain(root, store):
    root = Path(root)
    inbox = root / "inbox"
    inbox.mkdir(parents=True, exist_ok=True)
    accepted = rejected = 0
    for path in sorted(inbox.glob("*.json"), key=lambda p: p.stat().st_mtime_ns)[:500]:
        try:
            if path.stat().st_size > 1_000_000:
                raise ValueError("事件过大")
            event = json.loads(path.read_text(encoding="utf-8"))
            store.ingest(event)
            path.unlink()
            accepted += 1
        except (ValueError, KeyError, TypeError):
            quarantine = root / "rejected"
            quarantine.mkdir(exist_ok=True)
            os.replace(path, quarantine / path.name)
            rejected += 1
    return accepted, rejected


def worker(root):
    root = Path(root)
    lock = ProcessLock(root / "collector.lock")
    if not lock.acquire():
        return
    store = Store(root)
    rejected = len(list((root / "rejected").glob("*.json")))
    try:
        while True:
            try:
                _, count = drain(root, store)
                rejected += count
                atomic_json(root / "health.json", {"pid": os.getpid(), "at": time.time(), "rejected": rejected, "status": "ok"})
            except (OSError, sqlite3.Error):
                # 只写健康信息，不记录原始载荷；异常不会删除未入库事件。
                try:
                    atomic_json(root / "health.json", {"pid": os.getpid(), "at": time.time(), "rejected": rejected, "status": "storage_error"})
                except OSError:
                    pass
            time.sleep(.35)
    finally:
        store.close()
        lock.close()
