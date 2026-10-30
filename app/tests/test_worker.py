import json
from pathlib import Path
import subprocess
import sys
import time

from floatboard.core import Store, enqueue
from floatboard.runtime import ProcessLock, spawn_worker
from test_core import event
import pytest


def wait_for(predicate, seconds=8):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError("采集器未在限定时间完成")


def test_worker_restart_drains_backlog_and_retains_singleton(tmp_path):
    reporter = Path(__file__).parents[1] / "reporter.py"
    args = [sys.executable, str(reporter), "--worker", "--data-dir", str(tmp_path)]
    first = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    second = None
    restarted = None
    try:
        wait_for(lambda: (tmp_path / "health.json").exists())
        second = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        assert second.wait(timeout=5) == 0 and first.poll() is None
        first.terminate()
        first.wait(timeout=5)
        identifier = enqueue(tmp_path, event("Stop", session="offline"))
        assert (tmp_path / "inbox" / (identifier + ".json")).exists()
        # 刚退出时健康时间仍新鲜；以进程锁判断存活，不能漏掉立即恢复。
        spawn_worker(tmp_path)
        def projected():
            try:
                store = Store(tmp_path, readonly=True)
                tasks = store.tasks()
                store.close()
                return tasks and tasks[0]["runtime"] == "idle"
            except Exception:
                return False
        wait_for(projected)
        health = json.loads((tmp_path / "health.json").read_text())
        assert health["pid"] != first.pid
        restarted = health["pid"]
        assert not (tmp_path / "inbox" / (identifier + ".json")).exists()
    finally:
        for process in [first, second]:
            if process and process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
        if restarted:
            if sys.platform == "win32":
                import ctypes
                handle = ctypes.windll.kernel32.OpenProcess(1, False, restarted)
                if handle:
                    ctypes.windll.kernel32.TerminateProcess(handle, 0)
                    ctypes.windll.kernel32.CloseHandle(handle)
            else:
                import os, signal
                os.kill(restarted, signal.SIGTERM)


@pytest.mark.parametrize("unavailable", [False, True])
def test_worker_health_reports_storage_error_and_releases_lock(tmp_path, monkeypatch, unavailable):
    import floatboard.runtime as runtime
    import sqlite3
    identifier = enqueue(tmp_path, event())
    if unavailable:
        monkeypatch.setattr(runtime, "drain", lambda *_: (_ for _ in ()).throw(sqlite3.OperationalError("fixture")))
    monkeypatch.setattr(runtime.time, "sleep", lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    with pytest.raises(KeyboardInterrupt):
        runtime.worker(tmp_path)
    health = json.loads((tmp_path / "health.json").read_text())
    assert health["status"] == ("storage_error" if unavailable else "ok")
    assert (tmp_path / "inbox" / (identifier + ".json")).exists() == unavailable
    lock = ProcessLock(tmp_path / "collector.lock")
    assert lock.acquire()
    lock.close()
