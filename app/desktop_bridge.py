"""Tauri 的本地行协议适配；复用 Reporter 投影，不开放网络端口。"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time

from floatboard.core import Store, atomic_json, data_root
from floatboard.integration import connect, connection_state, preview, apply_preview
from floatboard.notifications import ChangeDetector
from floatboard import preferences
from floatboard.runtime import spawn_worker, command


class DesktopBridge:
    def __init__(self, root, gui_exe):
        self.root = Path(root)
        self.gui_exe = Path(gui_exe).resolve()
        if self.gui_exe.name != "CodexFloatboard.exe":
            raise ValueError("看板程序路径无效")
        self.store = None
        self.notices = ChangeDetector()
        self.connection = None
        self.checked_at = 0
        preferences.startup_command = lambda root: subprocess.list2cmdline([str(self.gui_exe), "--data-dir", str(Path(root).resolve())])

    def call(self, method, params):
        if method == "snapshot":
            if not self.store:
                self.store = Store(self.root, readonly=True)
            tasks = self.store.tasks()
            notices = self.notices.update(tasks) if params.get("notify", True) else []
            options = preferences.load(self.root)
            try:
                health = json.loads((self.root / "health.json").read_text(encoding="utf-8"))
                healthy = health.get("status") == "ok" and 0 <= time.time() - health["at"] < 5
            except (OSError, ValueError, TypeError, KeyError):
                healthy = False
            if self.connection is None or time.monotonic() - self.checked_at > 3:
                self.connection = connection_state(self.root)
                self.checked_at = time.monotonic()
            return dict(tasks=tasks, notices=notices if options["notifications"] else [], options=options, healthy=healthy, connection=self.connection, now=time.time())
        if method == "preferences":
            return preferences.save(self.root, params)
        if method == "preview":
            result = preview(self.root, remove=bool(params.get("remove")))
            return dict(changes=result.get("changes", []))
        if method == "connect":
            result = connect(self.root)
            self.connection = None
            return dict(success=True, changed=result.get("changed", False))
        if method == "connect_request":
            return "请帮我接入当前 Windows 用户的 Codex 任务看板，保留其他配置和正常 Hook 信任流程。请执行：" + subprocess.list2cmdline(command("--connect", self.root))
        if method == "disconnect":
            result = apply_preview(self.root, preview(self.root, remove=True))
            self.connection = None
            return dict(success=True, changed=bool(result))
        if method == "window_load":
            try:
                value = json.loads((self.root / "window.json").read_text(encoding="utf-8"))
                return value if isinstance(value, dict) else {}
            except (OSError, ValueError):
                return {}
        if method == "window_save":
            value = self.call("window_load", {})
            for key in ("x", "y", "width", "height", "pinned", "selected", "tab", "filter"):
                if key in params:
                    value[key] = params[key]
            atomic_json(self.root / "window.json", value)
            return True
        raise ValueError("未知看板操作")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir")
    parser.add_argument("--gui-exe", required=True)
    args = parser.parse_args()
    root = data_root(args.data_dir)
    spawn_worker(root)
    bridge = DesktopBridge(root, args.gui_exe)
    try:
        for line in sys.stdin:
            try:
                if len(line) > 1_000_000:
                    raise ValueError("请求过大")
                request = json.loads(line)
                result = dict(ok=True, result=bridge.call(request["method"], request.get("params") or {}))
            except Exception as error:
                # GUI 边界只返回简短错误，不输出载荷、凭据或工具日志。
                result = dict(ok=False, error=str(error)[:300])
            print(json.dumps(result, ensure_ascii=False), flush=True)
    finally:
        if bridge.store:
            bridge.store.close()


if __name__ == "__main__":
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    main()
