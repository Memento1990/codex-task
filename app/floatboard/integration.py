"""保留注释和其他工具配置；只增删本工具拥有的条目。"""
import copy
import difflib
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time
import uuid

import tomlkit

from .core import atomic_json, text
from .runtime import command, ProcessLock

SERVER = "codex_floatboard"
HOOKS = ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "SessionEnd", "SubagentStart", "SubagentStop")


def codex_home():
    return Path(os.environ.get("CODEX_HOME") or Path.home() / ".codex").resolve()


def digest(value):
    return hashlib.sha256(value).hexdigest()


def contribution(root):
    args = command("--mcp", root)
    hook_args = command("--hook", root)
    hook_command = subprocess.list2cmdline(hook_args) if os.name == "nt" else shlex.join(hook_args)
    # 只为本地、无业务操作的上报工具设置许可，不变更 Codex 全局策略。
    mcp = {"command": args[0], "args": args[1:], "tools": {"report_progress": {"approval_mode": "approve"}}}
    return {"mcp": mcp, "hook": {"hooks": [{"type": "command", "command": hook_command, "timeout": 5}]}}


def read_config(home):
    path = Path(home) / "config.toml"
    original = path.read_bytes() if path.exists() else b""
    try:
        document = tomlkit.parse(original.decode("utf-8-sig"))
    except (ValueError, UnicodeError, tomlkit.exceptions.TOMLKitError):
        raise ValueError("Codex 配置无法解析；未修改文件") from None
    return path, original, document


def preview(root, home=None, remove=False):
    root, home = Path(root), Path(home or codex_home())
    config_path, original, document = read_config(home)
    receipt_path = root / "integration.json"
    owned = contribution(root)
    if receipt_path.exists():
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        if receipt.get("home") != str(home.resolve()):
            raise ValueError("此数据目录已绑定另一处 Codex 配置")
        owned = receipt["contribution"]
    mcp = document.get("mcp_servers", {})
    installed = mcp.get(SERVER)
    if installed is not None and installed != owned["mcp"]:
        raise ValueError("同名 MCP 配置已被修改；请先核对，工具不会覆盖")
    hooks = document.get("hooks", {})
    if not isinstance(hooks, dict):
        raise ValueError("现有 hooks 格式不兼容")
    for name in HOOKS:
        if name in hooks and not isinstance(hooks[name], list):
            raise ValueError("现有 Hook 配置格式不兼容：" + name)
    present = int(installed is not None) + sum(owned["hook"] in hooks.get(name, []) for name in HOOKS)
    changes = []
    entries = [("本地进度连接", "mcp_servers." + SERVER, installed is not None, owned["mcp"], "任务、清单、卡点与证据上报")]
    names = ("会话开始", "收到请求", "工具调用前", "工具调用后", "等待审批", "本轮结束", "会话结束", "子任务开始", "子任务结束")
    entries += [(title, "hooks." + name, owned["hook"] in hooks.get(name, []), [owned["hook"]], "自动同步任务事件") for name, title in zip(HOOKS, names)]
    for title, key, exists, value, description in entries:
        target_exists = not remove
        if exists == target_exists:
            continue
        # 只展示本工具的配置块，避免把其他连接和相邻密钥带入高级预览。
        nested = value
        for part in reversed(key.split(".")):
            nested = {part: nested}
        snippet = tomlkit.dumps(nested)
        before, after = (snippet, "") if remove else ("", snippet)
        scoped_diff = "\n".join(difflib.unified_diff(before.splitlines(), after.splitlines(), fromfile="当前", tofile="断开后" if remove else "连接后", n=2))
        changes.append({"title": title, "key": key, "before": description if exists else "未配置", "after": description if target_exists else "未配置", "diff": text(scoped_diff, 30000)})
    if remove:
        if installed is not None:
            del document["mcp_servers"][SERVER]
        for name in HOOKS:
            if name in hooks:
                remaining = [x for x in hooks[name] if x != owned["hook"]]
                if remaining:
                    document["hooks"][name] = remaining
                else:
                    del document["hooks"][name]
        if "hooks" in document and not document["hooks"]:
            del document["hooks"]
        if "mcp_servers" in document and not document["mcp_servers"]:
            del document["mcp_servers"]
    else:
        if "mcp_servers" not in document:
            document["mcp_servers"] = tomlkit.table()
        if installed is None:
            document["mcp_servers"][SERVER] = owned["mcp"]
        if "hooks" not in document:
            document["hooks"] = tomlkit.table()
        for name in HOOKS:
            current = list(document["hooks"].get(name, []))
            if owned["hook"] not in current:
                current.append(copy.deepcopy(owned["hook"]))
                document["hooks"][name] = current
    updated = tomlkit.dumps(document).encode("utf-8")
    tomlkit.parse(updated.decode("utf-8"))
    if original and original.startswith(b"\xef\xbb\xbf"):
        updated = b"\xef\xbb\xbf" + updated
    # 差异仅留在本地预览；显示前过滤常见密钥形式。
    diff = "\n".join(difflib.unified_diff(original.decode("utf-8-sig").splitlines(), updated.decode("utf-8-sig").splitlines(), fromfile="当前 config.toml", tofile="预览 config.toml", n=0))
    return {"path": str(config_path), "home": str(home.resolve()), "before_hash": digest(original), "after_hash": digest(updated), "changed": original != updated, "remove": remove, "contribution": owned, "diff": text(diff, 100000), "changes": changes, "present": present, "updated": updated}


def connection_state(root, home=None):
    try:
        plan = preview(root, home)
        state = "ready" if plan["present"] == len(HOOKS) + 1 else "partial" if plan["present"] else "missing"
        return {"state": state, "error": ""}
    except (OSError, ValueError, KeyError, TypeError) as error:
        return {"state": "error", "error": text(str(error), 500)}


def connect(root, home=None):
    plan = preflight(root, home)
    backup = apply_preview(root, plan)
    return {"state": "ready", "changed": plan["changed"], "backup": str(backup) if backup else None}


def apply_preview(root, plan):
    root = Path(root)
    lock = ProcessLock(root / "integration.lock")
    if not lock.acquire():
        raise ValueError("另一个接入操作正在进行")
    try:
        path = Path(plan["path"])
        current = path.read_bytes() if path.exists() else b""
        if digest(current) != plan["before_hash"]:
            raise ValueError("配置在预览后发生变化，请重新预览")
        if not plan["changed"]:
            return None
        backup = root / "backups" / (time.strftime("%Y%m%d-%H%M%S") + "-" + uuid.uuid4().hex[:8] + ".toml")
        backup.parent.mkdir(parents=True, exist_ok=True)
        backup.write_bytes(current)
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(".floatboard-" + uuid.uuid4().hex + ".tmp")
        try:
            with temporary.open("wb") as stream:
                stream.write(plan["updated"])
                stream.flush()
                os.fsync(stream.fileno())
            if digest(path.read_bytes() if path.exists() else b"") != plan["before_hash"]:
                raise ValueError("配置出现并发修改，请重新预览")
            receipt = {"home": plan["home"], "contribution": plan["contribution"], "backup": str(backup), "before_hash": plan["before_hash"], "after_hash": plan["after_hash"], "status": "prepared"}
            atomic_json(root / "integration.json", receipt)
            os.replace(temporary, path)
            receipt["status"] = "removed" if plan["remove"] else "applied"
            atomic_json(root / "integration.json", receipt)
        finally:
            temporary.unlink(missing_ok=True)
        return backup
    finally:
        lock.close()


def preflight(root, home=None):
    plan = preview(root, home)
    args = [plan["contribution"]["mcp"]["command"], *plan["contribution"]["mcp"]["args"]]
    if not Path(args[0]).is_file() or (args[0] == sys.executable and not getattr(sys, "frozen", False) and not Path(args[1]).is_file()):
        raise ValueError("上报程序不存在，不能应用配置")
    requests = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05"}}, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}]
    # 此处仅检测协议，不启动采集器，不写日常任务数据。
    isolated = dict(os.environ, FLOATBOARD_PREFLIGHT="1")
    result = subprocess.run(args, input="".join(json.dumps(x) + "\n" for x in requests), capture_output=True, text=True, encoding="utf-8", timeout=12, env=isolated, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        responses = [json.loads(x) for x in result.stdout.splitlines()]
        tools = next(x["result"]["tools"] for x in responses if x.get("id") == 2)
        if result.returncode or not any(x["name"] == "report_progress" for x in tools):
            raise ValueError()
    except (ValueError, KeyError, StopIteration):
        raise ValueError("本地 MCP 预检失败，未改动 Codex 配置") from None
    return plan
