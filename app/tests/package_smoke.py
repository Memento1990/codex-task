"""不依赖开发 Python 环境的构建产物烟测。"""
import hashlib
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time

ROOT = Path(__file__).parents[1]
DIST = ROOT / "dist" / "CodexFloatboard"
OUTPUT = ROOT / "docs" / "evidence"
DATA = ROOT / "docs" / "evidence" / ("package-data-" + time.strftime("%H%M%S"))
ENV = {k: v for k, v in os.environ.items() if not k.startswith(("PYTHON", "CONDA", "QT_"))}
ENV["PATH"] = r"C:\Windows\System32;C:\Windows"
FLAGS = subprocess.CREATE_NO_WINDOW


def run(args, payload):
    return subprocess.run([str(DIST / "FloatboardReporter.exe"), *args, "--data-dir", str(DATA)], input=payload.encode("utf-8"), capture_output=True, env=ENV, timeout=15, creationflags=FLAGS)


def task(session):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            connection = sqlite3.connect((DATA / "events.sqlite3").as_uri() + "?mode=ro", uri=True)
            tasks = [json.loads(x[0]) for x in connection.execute("select payload from tasks")]
            connection.close()
            found = next((t for t in tasks if t["session_id"] == session and t["revision"] >= 0), None)
            if found:
                return found
        except sqlite3.Error:
            pass
        time.sleep(.1)
    return None


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", type=Path, default=DIST)
    parser.add_argument("--evidence-dir", type=Path, default=OUTPUT)
    options = parser.parse_args()
    DIST, OUTPUT = options.dist.resolve(), options.evidence_dir.resolve()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    DATA = OUTPUT / ("package-data-" + time.strftime("%H%M%S"))
    begin = time.time()
    home = DATA / "codex-home"
    home.mkdir(parents=True, exist_ok=True)
    if not (home / "config.toml").exists():
        (home / "config.toml").write_text('# 原生配置流程专用夹具\nmodel = "fixture-only"\n\n[mcp_servers.existing]\ncommand = "preserve-this-fixture"\n', encoding="utf-8")
    ENV["CODEX_HOME"] = str(home)
    checks = {}
    connected = run(["--connect"], "")
    checks["packaged_one_click_connect"] = connected.returncode == 0 and json.loads(connected.stdout)["success"]
    again = run(["--connect"], "")
    checks["packaged_connect_idempotent"] = again.returncode == 0 and not json.loads(again.stdout)["changed"]
    checks["packaged_connect_preserves_other_settings"] = 'preserve-this-fixture' in (home / "config.toml").read_text(encoding="utf-8")
    pre = subprocess.run([str(DIST / "FloatboardReporter.exe"), "--mcp", "--data-dir", str(DATA)], input=b'{"jsonrpc":"2.0","id":1,"method":"tools/list"}\n', capture_output=True, env={**ENV, "FLOATBOARD_PREFLIGHT": "1"}, timeout=15, creationflags=FLAGS)
    checks["reporter_without_python_path"] = pre.returncode == 0 and json.loads(pre.stdout)["result"]["tools"][0]["name"] == "report_progress"
    payload = {"hook_event_name": "SessionStart", "session_id": "package-smoke", "cwd": str(ROOT)}
    receipt = run(["--hook"], json.dumps(payload, ensure_ascii=False))
    checks["packaged_hook_context"] = receipt.returncode == 0 and "additionalContext" in json.loads(receipt.stdout)["hookSpecificOutput"]
    binding_file = DATA / "sessions" / (hashlib.sha256(b"package-smoke").hexdigest() + ".json")
    binding = json.loads(binding_file.read_text(encoding="utf-8"))
    arguments = dict(session_id="package-smoke", session_token=binding["session_token"], revision=0, title="打包验收 · 专用测试", status="blocked", current_step="核实原生窗口与采集独立", plan=[dict(id="pipe", title="打包程序上报中文进度", status="completed"), dict(id="native", title="核实窗口、托盘与恢复", status="blocked")], blockers=[dict(id="check", title="等待窗口交互验收（测试）", reason="专用回归场景，用于核实卡点展示", impact="仅影响此验收清单", owner="Codex", resolve_when="原生交互检查通过", open=True)], evidence=[dict(label="打包 Hook → MCP → 本地邮箱", reference="")])
    requests = [dict(id=1, jsonrpc="2.0", method="initialize"), dict(id=2, jsonrpc="2.0", method="tools/call", params=dict(name="report_progress", arguments=arguments))]
    result = run(["--mcp"], "".join(json.dumps(x, ensure_ascii=False) + "\n" for x in requests))
    response = [json.loads(x) for x in result.stdout.decode("utf-8").splitlines()]
    checks["packaged_mcp_utf8"] = result.returncode == 0 and not response[-1]["result"]["isError"]
    projected = task("package-smoke")
    checks["packaged_collector_persisted"] = projected is not None and projected["title"] == arguments["title"] and projected["status"] == "blocked"
    health = json.loads((DATA / "health.json").read_text(encoding="utf-8"))
    checks["collector_is_independent_process"] = health["pid"] != os.getpid() and time.time() - health["at"] < 5
    gui = subprocess.Popen([str(DIST / "CodexFloatboard.exe"), "--data-dir", str(DATA)], env=ENV, creationflags=FLAGS)
    (OUTPUT / "package-gui.pid").write_text(str(gui.pid))
    record = {"at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "command": "python tests/package_smoke.py", "duration_s": round(time.time() - begin, 2), "environment": "PATH only Windows; PYTHON, CONDA and QT variables removed", "data_dir": str(DATA), "codex_home": str(home), "gui_pid": gui.pid, "collector_pid": health["pid"], "checks": checks}
    (OUTPUT / "package-smoke.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(record, ensure_ascii=False))
    raise SystemExit(0 if all(checks.values()) else 1)
