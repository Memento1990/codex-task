"""显式运行的双会话真机验收；只用无业务影响的固定测试。"""
import concurrent.futures
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).parents[1]
DATA = ROOT / "docs" / "evidence" / "live-data"
CODEX = Path(r"C:\Users\Memento\AppData\Local\OpenAI\Codex\bin\08d187884f2330d1\codex.exe")


def run(label):
    hook_cmd = subprocess.list2cmdline([sys.executable, str(ROOT / "reporter.py"), "--hook", "--data-dir", str(DATA)])
    args = [str(CODEX), "exec", "--skip-git-repo-check", "-s", "read-only", "--json", "--dangerously-bypass-hook-trust", "-C", str(ROOT)]
    config = tomllib.loads((Path.home() / ".codex" / "config.toml").read_text(encoding="utf-8-sig"))
    for name in config.get("mcp_servers", {}):
        args += ["-c", "mcp_servers." + name + ".enabled=false"]
    args += ["-c", "mcp_servers.codex_floatboard.command=" + json.dumps(sys.executable), "-c", "mcp_servers.codex_floatboard.args=" + json.dumps([str(ROOT / "reporter.py"), "--mcp", "--data-dir", str(DATA)]), "-c", 'mcp_servers.codex_floatboard.tools.report_progress.approval_mode="approve"']
    for name in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "SessionEnd"]:
        args += ["-c", "hooks." + name + '=[{hooks=[{type="command",command=' + json.dumps(hook_cmd) + ',timeout=5}]}]']
    plan = [dict(id="shell", title="执行无副作用检查", status="inProgress"), dict(id="fixture", title="核对验收夹具", status="pending")]
    reports = [dict(revision=0, title="链路验收 " + label, status="running", current_step="执行检查", plan=plan, blockers=[], evidence=[])]
    reports.append(dict(revision=1, title="链路验收 " + label, status="blocked", current_step="测试夹具暂未就绪（注入场景）", plan=[dict(id="shell", title="执行无副作用检查", status="completed"), dict(id="fixture", title="核对验收夹具", status="blocked")], blockers=[dict(id="synthetic", title="验收依赖未就绪（注入）", reason="用于验证受阻和恢复展示", impact="只影响此验收任务", owner="Codex", resolve_when="下一次报告解除", open=True)], evidence=[]))
    reports.append(dict(revision=2, title="链路验收 " + label, status="running", current_step="依赖恢复", plan=[dict(id="shell", title="执行无副作用检查", status="completed"), dict(id="fixture", title="核对验收夹具", status="inProgress")], blockers=[], evidence=[]))
    reports.append(dict(revision=3, title="链路验收 " + label, status="completed", current_step="验收收口", plan=[dict(id="shell", title="执行无副作用检查", status="completed"), dict(id="fixture", title="核对验收夹具", status="completed")], blockers=[], evidence=[dict(label="FLOATBOARD_TOOL_OK 无副作用命令成功；四次进度调用成功")]))
    prompt = (
        "This is a fixed local instrumentation acceptance test, not delegated project work. Do not create agents, access files or do any other work. "
        "Use the codex_floatboard report_progress tool to submit these FOUR reports IN ORDER, one at a time. Add session_id and session_token from your actual Hook additionalContext to every report. "
        "Immediately after the FIRST report, run one harmless Windows shell command: Write-Output FLOATBOARD_TOOL_OK . If it fails, do not claim completion. "
        "After the shell succeeds, submit reports 2,3,4. These blockers are injected fixtures. Never echo the binding token in your response. "
        "Reports: " + json.dumps(reports, ensure_ascii=False) + ". After all tools succeed reply exactly FLOATBOARD_ACCEPTANCE_OK."
    )
    begin = time.time()
    result = subprocess.run(args + [prompt], capture_output=True, timeout=150)
    events = []
    calls = []
    session = ""
    final_ok = False
    for line in result.stdout.decode("utf-8", "replace").splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        events.append(item.get("type"))
        if item.get("type") == "thread.started":
            session = item["thread_id"]
        nested = item.get("item", {})
        if item.get("type") == "item.completed" and nested.get("type") == "mcp_tool_call":
            # 不保存 arguments：其中有会话绑定码。
            calls.append({"server": nested.get("server"), "tool": nested.get("tool"), "status": nested.get("status"), "error": bool(nested.get("error")), "isError": bool((nested.get("result") or {}).get("isError"))})
        if nested.get("type") == "agent_message":
            final_ok |= "FLOATBOARD_ACCEPTANCE_OK" in nested.get("text", "")
    # 错误只记录类型信号，不保留潜在带凭据的 CLI 输出。
    diagnostic = result.stderr.decode("utf-8", "replace").lower()
    return {"label": label, "session_id": session, "exit_code": result.returncode, "duration_s": round(time.time() - begin, 2), "events": events, "calls": calls, "final_ok": final_ok, "stderr_bytes": len(result.stderr), "diagnostics": {key: key in diagnostic for key in ["invalid type", "parse", "configuration", "approval", "timed out", "unauthorized"]}}


if __name__ == "__main__":
    DATA.parent.mkdir(parents=True, exist_ok=True)
    begin = time.time()
    if "--recheck" in sys.argv:
        previous = json.loads((DATA.parent / "live-codex.json").read_text(encoding="utf-8"))
        results = previous["sessions"]
    else:
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(run, ["A", "B"]))
    sys.path.insert(0, str(ROOT))
    from floatboard.core import Store
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        try:
            store = Store(DATA, readonly=True)
            # 每次验收只判断本次会话；保留前次失败证据，支持原目录复验。
            tasks = [t for t in store.tasks() if t["session_id"] in {r["session_id"] for r in results}]
            store.close()
            if len(tasks) == 2 and all(t["revision"] == 3 for t in tasks):
                break
        except Exception:
            tasks = []
        time.sleep(.1)
    checks = {
        "two_distinct_sessions": len({r["session_id"] for r in results}) == 2,
        "eight_successful_mcp_calls": sum(sum(c["status"] == "completed" and not c["error"] and not c["isError"] for c in r["calls"]) for r in results) == 8,
        "final_responses_ok": all(r["exit_code"] == 0 and r["final_ok"] for r in results),
        "two_completed_tasks": len(tasks) == 2 and all(t["status"] == "completed" and t["revision"] == 3 for t in tasks),
        "task_identity_matches_hook": {t["session_id"] for t in tasks} == {r["session_id"] for r in results},
        "evidence_in_each_task": len(tasks) == 2 and all(len(t["plan"]) == 2 and t["evidence"] and not t["blockers"] for t in tasks),
        "runtime_stop_separate": len(tasks) == 2 and all(t["runtime"] == "idle" for t in tasks),
        "blocked_and_recovery_history": len(tasks) == 2 and all(any(e["status"] == "blocked" for e in t["history"]) for t in tasks),
    }
    record = {"at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "command": "python tests/live_codex.py" + (" --recheck" if "--recheck" in sys.argv else ""), "platform": sys.platform, "runtime": str(CODEX), "runtime_version": "0.165.0-alpha.1.2", "host": socket.gethostname(), "duration_s": previous["duration_s"] if "--recheck" in sys.argv else round(time.time() - begin, 2), "sessions": results, "checks": checks, "scope": "固定无业务副作用的真实 CLI 会话；隔离参数信任已审核测试 Hook；未改全局配置，未覆盖桌面 GUI 正常信任流程"}
    (DATA.parent / "live-codex.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"checks": checks, "duration_s": record["duration_s"]}, ensure_ascii=False))
    raise SystemExit(0 if all(checks.values()) else 1)
