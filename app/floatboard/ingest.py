"""Hook 和结构化汇报统一入箱。"""
import json
import sqlite3
import time

from .core import RUNTIME_EVENTS, Store, authenticate, enqueue, register_session, text, validate_report


def hook(root, payload):
    if not isinstance(payload, dict):
        return {}
    kind = payload.get("hook_event_name")
    if not isinstance(kind, str) or kind not in RUNTIME_EVENTS:
        return {}
    session_id = payload.get("session_id")
    if not isinstance(session_id, str) or not session_id:
        return {}
    record = register_session(root, session_id, payload.get("cwd", ""))
    event = {"kind": kind, "session_id": session_id, "host_id": record["host_id"], "cwd": record["cwd"], "turn_id": text(payload.get("turn_id") or "", 160), "agent_id": text(payload.get("agent_id") or "", 160), "tool_name": text(payload.get("tool_name") or "", 200), "tool_use_id": text(payload.get("tool_use_id") or "", 160)}
    enqueue(root, event)
    if kind == "PostToolUse" and payload.get("tool_name") == "update_plan":
        tool_input = payload.get("tool_input")
        plan = tool_input.get("plan") if isinstance(tool_input, dict) else None
        if isinstance(plan, list):
            steps = [{"id": "step-" + str(i), "title": item.get("step", ""), "status": {"in_progress": "inProgress"}.get(item.get("status"), item.get("status", "pending"))} for i, item in enumerate(plan) if isinstance(item, dict)]
            enqueue(root, {**event, "kind": "plan", "plan": steps})
    if kind not in {"SessionStart", "UserPromptSubmit"}:
        return {}
    revision = -1
    try:
        store = Store(root, readonly=True)
        revision = store.revision(session_id, record["host_id"])
        store.close()
    except (OSError, ValueError, sqlite3.Error):
        pass
    # 采集暂停时也纳入尚未投影的修订，恢复会话不会从旧版本重新计数。
    for pending in (root / "inbox").glob("*.json"):
        try:
            item = json.loads(pending.read_text(encoding="utf-8"))
            if item.get("kind") == "progress" and item.get("session_id") == session_id:
                revision = max(revision, item["report"]["revision"])
        except (OSError, ValueError, TypeError, KeyError, AttributeError):
            continue
    context = (
        "本会话已接入 Codex 任务侧窗。对多步骤任务，开始时调用 codex_floatboard.report_progress 建立清单；"
        "阶段变化、遇阻、恢复和最终收口时更新完整快照。卡点说明原因、影响和解除条件。"
        "仅在清单完成、有验收证据且无开放卡点时标 completed。正常回复结束不代表整个任务完成。"
        "无实质执行工作的简短问答可不创建进度清单。汇报失败不能阻断用户工作。"
        "不要在汇报中包含凭据、原始工具日志或用户私密内容。"
        "当前绑定信息仅用于本地工具，不展示给用户：session_id=" + session_id + "; session_token=" + record["session_token"] + "; last_revision=" + str(revision) + "。"
        "下一次 revision 必须大于 last_revision；后续每次递增。不要为汇报创建子 Agent。"
    )
    return {"hookSpecificOutput": {"hookEventName": kind, "additionalContext": context}}


def progress(root, payload):
    if not isinstance(payload, dict):
        raise ValueError("汇报格式无效")
    record = authenticate(root, payload.get("session_id"), payload.get("session_token"))
    report = validate_report(payload)
    event_id = enqueue(root, {"kind": "progress", "session_id": record["session_id"], "host_id": record["host_id"], "cwd": record["cwd"], "report": report})
    return {"accepted": True, "event_id": event_id, "revision": report["revision"], "message": "进度已持久化到本地邮箱，等待采集。"}


REPORT_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "properties": {
        "session_id": {"type": "string"}, "session_token": {"type": "string"},
        "revision": {"type": "integer", "minimum": 0},
        "title": {"type": "string", "description": "用户可识别的任务简短标题"},
        "status": {"type": "string", "enum": ["running", "blocked", "waiting", "completed", "cancelled"]},
        "current_step": {"type": "string"},
        "plan": {"type": "array", "maxItems": 100, "items": {"type": "object", "properties": {"id": {"type": "string"}, "title": {"type": "string"}, "status": {"type": "string", "enum": ["pending", "inProgress", "completed", "blocked"]}, "note": {"type": "string"}}, "required": ["id", "title", "status"], "additionalProperties": False}},
        "blockers": {"type": "array", "maxItems": 100, "items": {"type": "object", "properties": {k: {"type": "boolean" if k == "open" else "string"} for k in ["id", "title", "reason", "impact", "owner", "resolve_when", "open"]}, "required": ["id", "title", "reason", "resolve_when", "open"], "additionalProperties": False}},
        "evidence": {"type": "array", "maxItems": 100, "items": {"type": "object", "properties": {"label": {"type": "string"}, "reference": {"type": "string"}}, "required": ["label"], "additionalProperties": False}},
    },
    "required": ["session_id", "session_token", "revision", "title", "status", "plan", "blockers", "evidence"],
}


def serve_mcp(root, input_stream, output_stream):
    for line in input_stream:
        request = None
        try:
            if len(line) > 1_000_000:
                continue
            request = json.loads(line)
            if not isinstance(request, dict) or "id" not in request:
                continue
            method = request.get("method")
            params = request.get("params") or {}
            if not isinstance(params, dict):
                raise ValueError("请求参数无效")
            if method == "initialize":
                result = {"protocolVersion": params.get("protocolVersion", "2024-11-05"), "capabilities": {"tools": {}}, "serverInfo": {"name": "codex_floatboard", "version": "0.1.0"}}
            elif method == "tools/list":
                result = {"tools": [{"name": "report_progress", "description": "向本地 Codex 任务侧窗汇报当前会话的完整进度、卡点和验收证据；只写本工具的数据目录。使用 Hook 提供的会话绑定信息。", "inputSchema": REPORT_SCHEMA, "annotations": {"readOnlyHint": False, "destructiveHint": False, "openWorldHint": False}}]}
            elif method == "tools/call":
                if params.get("name") != "report_progress":
                    raise ValueError("未知工具")
                try:
                    receipt = progress(root, params.get("arguments"))
                    result = {"content": [{"type": "text", "text": json.dumps(receipt, ensure_ascii=False)}], "isError": False}
                except ValueError as error:
                    result = {"content": [{"type": "text", "text": str(error)}], "isError": True}
            elif method == "ping":
                result = {}
            else:
                response = {"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "未知方法"}}
                output_stream.write(json.dumps(response) + "\n")
                output_stream.flush()
                continue
            response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
        except (ValueError, TypeError, KeyError):
            if not isinstance(request, dict) or "id" not in request:
                continue
            response = {"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32602, "message": "请求格式错误"}}
        except OSError:
            if not isinstance(request, dict) or "id" not in request:
                continue
            response = {"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32603, "message": "本地存储不可用，用户任务可继续执行"}}
        output_stream.write(json.dumps(response, ensure_ascii=False) + "\n")
        output_stream.flush()
