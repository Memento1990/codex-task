import json
import os
from pathlib import Path
import subprocess
import sys

from floatboard.core import register_session


def test_stdio_is_utf8_despite_windows_codepage(tmp_path):
    reporter = Path(__file__).parents[1] / "reporter.py"
    binding = register_session(tmp_path, "encoding-test", "C:/isolated")
    arguments = dict(session_id="encoding-test", session_token=binding["session_token"], revision=0, title="中文进度", status="running", plan=[], blockers=[], evidence=[])
    requests = [dict(id=1, jsonrpc="2.0", method="initialize"), dict(id=2, jsonrpc="2.0", method="tools/list"), dict(id=3, jsonrpc="2.0", method="tools/call", params=dict(name="report_progress", arguments=arguments))]
    result = subprocess.run([sys.executable, str(reporter), "--mcp", "--data-dir", str(tmp_path)], input="".join(json.dumps(x, ensure_ascii=False) + "\n" for x in requests).encode("utf-8"), capture_output=True, timeout=10, env=dict(os.environ, PYTHONIOENCODING="cp1252", FLOATBOARD_PREFLIGHT="1"))
    assert result.returncode == 0
    replies = [json.loads(x) for x in result.stdout.decode("utf-8").splitlines()]
    assert "本地 Codex" in replies[1]["result"]["tools"][0]["description"]
    assert not replies[2]["result"]["isError"]
    queued = json.loads(next((tmp_path / "inbox").glob("*.json")).read_text(encoding="utf-8"))
    assert queued["report"]["title"] == "中文进度"
    assert not tmp_path.joinpath("events.sqlite3").exists()


def test_hook_fail_open_for_invalid_json(tmp_path):
    reporter = Path(__file__).parents[1] / "reporter.py"
    result = subprocess.run([sys.executable, str(reporter), "--hook", "--data-dir", str(tmp_path)], input=b"[invalid", capture_output=True, timeout=10)
    assert result.returncode == 0 and json.loads(result.stdout) == {}
