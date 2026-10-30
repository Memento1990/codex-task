import json
import tomllib
import tomlkit
from pathlib import Path

import pytest

from floatboard.integration import apply_preview, preview, connection_state, connect


def setup(tmp_path):
    home = tmp_path / "codex"
    home.mkdir()
    path = home / "config.toml"
    original = '# 用户注释\nmodel = "existing"\n\n[mcp_servers.other]\ncommand = "keep"\n\n[hooks]\nStop = [{hooks=[{type="command",command="existing-hook"}]}]\n'
    path.write_text(original, encoding="utf-8")
    return home, path, tmp_path / "board", original


def test_preview_does_not_write_and_preserves_existing(tmp_path):
    home, path, root, original = setup(tmp_path)
    plan = preview(root, home)
    assert path.read_text(encoding="utf-8") == original
    assert not root.exists()
    parsed = tomllib.loads(plan["updated"].decode())
    assert parsed["model"] == "existing" and parsed["mcp_servers"]["other"]["command"] == "keep"
    assert len(parsed["hooks"]["Stop"]) == 2
    assert "# 用户注释" in plan["updated"].decode()


def test_apply_idempotent_and_backup(tmp_path):
    home, path, root, original = setup(tmp_path)
    backup = apply_preview(root, preview(root, home))
    assert backup.read_text(encoding="utf-8") == original
    assert not preview(root, home)["changed"]
    assert json.loads((root / "integration.json").read_text())["status"] == "applied"


def test_existing_mcp_layout_is_byte_identical_on_reconnect(tmp_path):
    home = tmp_path / "codex"
    home.mkdir()
    path = home / "config.toml"
    path.write_text('[mcp_servers.existing]\ncommand = "preserve"\n', encoding="utf-8")
    root = tmp_path / "board"
    apply_preview(root, preview(root, home))
    before = path.read_bytes()
    assert not preview(root, home)["changed"]
    assert path.read_bytes() == before


def test_conflict_does_not_overwrite(tmp_path):
    home, path, root, original = setup(tmp_path)
    plan = preview(root, home)
    path.write_text(original + '\n# 并发修改\n', encoding="utf-8")
    with pytest.raises(ValueError, match="变化"):
        apply_preview(root, plan)
    assert "并发修改" in path.read_text(encoding="utf-8")


def test_remove_only_owned_entries_preserves_later_changes(tmp_path):
    home, path, root, original = setup(tmp_path)
    apply_preview(root, preview(root, home))
    with path.open("a", encoding="utf-8") as stream:
        stream.write('\n[mcp_servers.new_tool]\ncommand = "later"\n')
    apply_preview(root, preview(root, home, remove=True))
    parsed = tomllib.loads(path.read_text(encoding="utf-8"))
    assert "codex_floatboard" not in parsed["mcp_servers"]
    assert parsed["mcp_servers"]["new_tool"]["command"] == "later"
    assert parsed["hooks"]["Stop"] == [{"hooks": [{"type": "command", "command": "existing-hook"}]}]


def test_owned_server_modified_refuses_removal(tmp_path):
    home, path, root, original = setup(tmp_path)
    apply_preview(root, preview(root, home))
    text = path.read_text(encoding="utf-8").replace('approval_mode = "approve"', 'approval_mode = "prompt"')
    path.write_text(text, encoding="utf-8")
    with pytest.raises(ValueError, match="已被修改"):
        preview(root, home, remove=True)


def test_state_partial_ready_and_scoped_readable_diff(tmp_path):
    home, path, root, original = setup(tmp_path)
    path.write_text(original + '\n[secret]\napi_key = "fixture-private-value"\n', encoding="utf-8")
    assert connection_state(root, home)["state"] == "missing"
    plan = preview(root, home)
    assert len(plan["changes"]) == 10
    assert all(c["before"] == "未配置" and c["after"] for c in plan["changes"])
    assert not any("fixture-private-value" in c["diff"] for c in plan["changes"])
    assert any("report_progress" in c["diff"] for c in plan["changes"])
    apply_preview(root, plan)
    assert connection_state(root, home)["state"] == "ready"
    assert not preview(root, home)["changes"]
    document = tomlkit.parse(path.read_text(encoding="utf-8"))
    del document["hooks"]["SessionStart"]
    path.write_text(tomlkit.dumps(document), encoding="utf-8")
    assert connection_state(root, home)["state"] == "partial"


def test_connect_automatic_preflight_backup_and_idempotence(tmp_path):
    home, path, root, original = setup(tmp_path)
    result = connect(root, home)
    assert result["state"] == "ready" and result["changed"]
    assert Path(result["backup"]).read_text(encoding="utf-8") == original
    assert not connect(root, home)["changed"]


def test_connect_preflight_failure_keeps_config(tmp_path, monkeypatch):
    from floatboard import integration
    home, path, root, original = setup(tmp_path)
    def fail(*_):
        raise ValueError("测试预检失败")
    monkeypatch.setattr(integration, "preflight", fail)
    with pytest.raises(ValueError, match="预检失败"):
        connect(root, home)
    assert path.read_text(encoding="utf-8") == original


def test_disconnect_diff_is_scoped_removal(tmp_path):
    home, path, root, original = setup(tmp_path)
    connect(root, home)
    plan = preview(root, home, remove=True)
    assert len(plan["changes"]) == 10
    assert all(c["after"] == "未配置" and c["diff"].startswith('--- ') for c in plan["changes"])
    assert not any('existing-hook' in c["diff"] for c in plan["changes"])


def test_codex_can_use_complete_local_connect_command(tmp_path):
    import os
    import subprocess
    from floatboard.runtime import command
    home, path, root, original = setup(tmp_path)
    result = subprocess.run(command("--connect", root), capture_output=True, text=True, encoding="utf-8", env=dict(os.environ, CODEX_HOME=str(home)), timeout=15)
    assert result.returncode == 0
    response = json.loads(result.stdout)
    assert response["success"] and response["state"] == "ready"
    assert "首次同步" in response["message"]
    assert connection_state(root, home)["state"] == "ready"
    assert not (root / "events.sqlite3").exists()
