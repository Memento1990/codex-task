"""白名单事件与本地持久化；不依赖 GUI。"""
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import secrets
import socket
import sqlite3
import time
import uuid

from .timing import TimingIndex

RUNTIME_EVENTS = {"SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt", "SubagentStart", "SubagentStop"}
BUSINESS_STATES = {"running", "blocked", "waiting", "completed", "cancelled"}
STEP_STATES = {"pending", "inProgress", "completed", "blocked"}
SECRET = re.compile(r"(?i)(?:sk-[a-z0-9_-]{16,}|(?:password|passwd|token|api[_-]?key)\s*[:=]\s*[^\s,;]{6,})")


def text(value, limit=500):
    if not isinstance(value, str):
        raise ValueError("文本字段类型错误")
    return SECRET.sub("[敏感值已过滤]", value.strip())[:limit]


def data_root(explicit=None):
    return Path(explicit or os.environ.get("CODEX_FLOATBOARD_DATA") or Path(os.environ.get("LOCALAPPDATA", str(Path.home()))) / "CodexFloatboard" / "data").resolve()


def atomic_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with temporary.open("w", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def session_path(root, session_id):
    return Path(root) / "sessions" / (hashlib.sha256(session_id.encode()).hexdigest() + ".json")


def register_session(root, session_id, cwd):
    if not isinstance(session_id, str) or not 1 <= len(session_id) <= 160:
        raise ValueError("会话标识无效")
    path = session_path(root, session_id)
    if not path.exists():
        record = {"session_id": session_id, "session_token": secrets.token_urlsafe(32), "host_id": socket.gethostname(), "cwd": text(cwd, 2000), "created_at": time.time()}
        temporary = path.with_name(uuid.uuid4().hex + ".tmp")
        try:
            atomic_json(temporary, record)
            # 链接仅在目标不存在时成功，避免多个 Hook 覆盖首次绑定。
            try:
                os.link(temporary, path)
            except FileExistsError:
                pass
        finally:
            temporary.unlink(missing_ok=True)
    record = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(record, dict) or record.get("session_id") != session_id:
        raise ValueError("会话标识不匹配")
    return record


def authenticate(root, session_id, token):
    if not isinstance(session_id, str) or not isinstance(token, str):
        raise ValueError("缺少会话绑定信息")
    try:
        record = json.loads(session_path(root, session_id).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise ValueError("会话尚未登记，请先启用 Hook") from None
    if not isinstance(record, dict) or record.get("session_id") != session_id or not isinstance(record.get("session_token"), str) or not hmac.compare_digest(record["session_token"], token):
        raise ValueError("会话绑定信息不匹配")
    return record


def validate_report(value):
    if not isinstance(value, dict):
        raise ValueError("进度必须是对象")
    revision = value.get("revision")
    if type(revision) is not int or revision < 0:
        raise ValueError("revision 必须是非负整数")
    status = value.get("status")
    if not isinstance(status, str) or status not in BUSINESS_STATES:
        raise ValueError("任务状态无效")
    raw_plan = value.get("plan", [])
    raw_blockers = value.get("blockers", [])
    raw_evidence = value.get("evidence", [])
    if not isinstance(raw_plan, list) or not isinstance(raw_blockers, list) or not isinstance(raw_evidence, list) or max(len(raw_plan), len(raw_blockers), len(raw_evidence)) > 100:
        raise ValueError("清单格式或数量无效")
    plan, blockers, evidence = [], [], []
    for step in raw_plan:
        if not isinstance(step, dict) or not isinstance(step.get("status"), str) or step["status"] not in STEP_STATES:
            raise ValueError("步骤状态无效")
        plan.append({"id": text(step.get("id"), 120), "title": text(step.get("title"), 300), "status": step["status"], "note": text(step.get("note", ""), 400)})
    if len({x["id"] for x in plan}) != len(plan) or any(not x["id"] or not x["title"] for x in plan):
        raise ValueError("步骤标识须非空且唯一")
    for item in raw_blockers:
        if not isinstance(item, dict) or type(item.get("open", True)) is not bool:
            raise ValueError("卡点格式无效")
        blockers.append({"id": text(item.get("id"), 120), "title": text(item.get("title"), 300), "reason": text(item.get("reason", "")), "impact": text(item.get("impact", "")), "owner": text(item.get("owner", "Codex"), 100), "resolve_when": text(item.get("resolve_when", "")), "open": item.get("open", True)})
    if len({x["id"] for x in blockers}) != len(blockers) or any(not x["id"] or not x["title"] for x in blockers):
        raise ValueError("卡点标识须非空且唯一")
    for item in raw_evidence:
        if isinstance(item, str):
            evidence.append({"label": text(item, 300), "reference": ""})
        elif isinstance(item, dict):
            evidence.append({"label": text(item.get("label"), 300), "reference": text(item.get("reference", ""), 2000)})
        else:
            raise ValueError("证据格式无效")
    opened = [b for b in blockers if b["open"]]
    if status == "blocked" and not opened:
        raise ValueError("受阻状态须说明开放卡点")
    if status == "completed" and (not plan or any(x["status"] != "completed" for x in plan) or not any(x["label"] for x in evidence) or opened):
        raise ValueError("完成须清单全完成、有证据且无未解除卡点")
    return {"revision": revision, "title": text(value.get("title", ""), 200), "status": status, "current_step": text(value.get("current_step", ""), 300), "plan": plan, "blockers": blockers, "evidence": evidence}


def enqueue(root, event):
    root = Path(root)
    event = {"version": 1, "event_id": uuid.uuid4().hex, "recorded_at": time.time(), **event}
    atomic_json(root / "inbox" / (event["event_id"] + ".json"), event)
    return event["event_id"]


class Store:
    def __init__(self, root, readonly=False):
        self.root = Path(root).resolve()
        self.timings = TimingIndex()
        if not readonly:
            self.root.mkdir(parents=True, exist_ok=True)
        database = self.root / "events.sqlite3"
        if readonly:
            self.db = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=2)
        else:
            self.db = sqlite3.connect(database, timeout=8)
            self.db.execute("pragma journal_mode=WAL")
            self.db.executescript("""
                create table if not exists events (id text primary key, task_id text not null, kind text not null, at real not null, payload text not null);
                create table if not exists tasks (id text primary key, payload text not null);
                create index if not exists event_task_at on events(task_id, at);
            """)

    def close(self):
        self.db.close()

    def ingest(self, event):
        if not isinstance(event, dict) or not isinstance(event.get("kind"), str) or event["kind"] not in RUNTIME_EVENTS | {"progress", "plan"}:
            raise ValueError("事件类型无效")
        session = event.get("session_id")
        host = event.get("host_id")
        if not isinstance(session, str) or not session or not isinstance(host, str) or not host:
            raise ValueError("事件身份无效")
        timestamp = event.get("recorded_at")
        if type(timestamp) not in [int, float] or not 0 < timestamp < time.time() + 300:
            raise ValueError("事件时间无效")
        event_id = text(event.get("event_id"), 160)
        if not event_id:
            raise ValueError("事件标识缺失")
        task_id = host + ":" + session
        agent_id = event.get("agent_id")
        kind = event["kind"]
        progress = validate_report(event.get("report")) if kind == "progress" else None
        safe_event = {"event_id": event_id, "kind": kind, "recorded_at": timestamp, "session_id": session, "host_id": host}
        for field in ["cwd", "turn_id", "agent_id", "tool_name", "tool_use_id"]:
            safe_event[field] = text(event.get(field) or "", 2000 if field == "cwd" else 200)
        if progress:
            safe_event["report"] = progress
        if kind == "plan":
            safe_event["plan"] = validate_report({"revision": 0, "status": "running", "plan": event.get("plan", [])})["plan"]
        event = safe_event
        with self.db:
            if self.db.execute("select 1 from events where id=?", (event_id,)).fetchone():
                return False
            row = self.db.execute("select payload from tasks where id=?", (task_id,)).fetchone()
            task = json.loads(row[0]) if row else {"id": task_id, "session_id": session, "host_id": host, "cwd": text(event.get("cwd", ""), 2000), "title": "", "status": "unreported", "runtime": "unknown", "runtime_at": 0, "updated_at": 0, "revision": -1, "plan": [], "blockers": [], "evidence": [], "current_step": "", "agents": {}, "active_tools": {}, "history": []}
            if agent_id:
                agent_id = text(agent_id, 160)
                old = task["agents"].get(agent_id, {})
                if timestamp >= old.get("updated_at", 0):
                    task["agents"][agent_id] = {"runtime": "idle" if kind in {"SubagentStop", "Stop", "SessionEnd"} else "active", "updated_at": timestamp, "tool_name": text(event.get("tool_name", ""), 200)}
            elif progress and progress["revision"] > task["revision"]:
                task.update(progress)
            elif kind == "plan" and timestamp >= task.get("plan_at", 0):
                if task["revision"] < 0:
                    plan = event.get("plan", [])
                    checked = validate_report({"revision": 0, "status": "running", "plan": plan})
                    task["plan"] = checked["plan"]
                    task["plan_at"] = timestamp
            elif timestamp >= task["runtime_at"] and kind in RUNTIME_EVENTS:
                task["runtime_at"] = timestamp
                if kind in {"Stop", "SessionEnd"}:
                    task["runtime"] = "idle"
                    task["active_tools"] = {}
                elif kind == "Interrupt":
                    task["runtime"] = "interrupted"
                    task["active_tools"] = {}
                elif kind == "PermissionRequest":
                    task["runtime"] = "waiting_approval"
                elif kind in {"SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse"}:
                    task["runtime"] = "active"
                    call_id = event.get("tool_use_id")
                    if kind == "PreToolUse" and call_id:
                        task["active_tools"][text(call_id, 160)] = text(event.get("tool_name", ""), 200)
                    elif kind == "PostToolUse" and call_id:
                        task["active_tools"].pop(call_id, None)
            task["updated_at"] = max(timestamp, task["updated_at"])
            task["history"] = sorted([{"kind": kind, "at": timestamp, "tool": text(event.get("tool_name", ""), 200), "status": progress["status"] if progress else "", "agent_id": agent_id or ""}] + task["history"], key=lambda x: x["at"], reverse=True)[:100]
            self.db.execute("insert into events values (?,?,?,?,?)", (event_id, task_id, kind, timestamp, json.dumps(event, ensure_ascii=False)))
            self.db.execute("insert or replace into tasks values (?,?)", (task_id, json.dumps(task, ensure_ascii=False)))
        return True

    def tasks(self):
        self.timings.update(self.db)
        return sorted((self.timings.attach(json.loads(row[0])) for row in self.db.execute("select payload from tasks")), key=lambda x: x["updated_at"], reverse=True)

    def revision(self, session_id, host_id):
        row = self.db.execute("select payload from tasks where id=?", (host_id + ":" + session_id,)).fetchone()
        return json.loads(row[0])["revision"] if row else -1
