import concurrent.futures
import io
import json
import time
import uuid
import sqlite3

import pytest

from floatboard.core import Store, authenticate, enqueue, register_session, validate_report
from floatboard.ingest import hook, progress, serve_mcp
from floatboard.runtime import ProcessLock, drain


def report(**overrides):
    value = dict(revision=0, title="专用验收任务", status="running", current_step="检查", plan=[dict(id="check", title="检查", status="inProgress")], blockers=[], evidence=[])
    value.update(overrides)
    return value


def event(kind="SessionStart", session="test", **overrides):
    value = dict(event_id=uuid.uuid4().hex, recorded_at=time.time(), kind=kind, session_id=session, host_id="test-host", cwd="C:/isolated")
    value.update(overrides)
    return value


def test_concurrent_registration_has_one_binding(tmp_path):
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
        records = list(pool.map(lambda _: register_session(tmp_path, "same", "C:/test"), range(40)))
    assert len({x["session_token"] for x in records}) == 1


def test_authentication_rejects_cross_session(tmp_path):
    a = register_session(tmp_path, "a", "C:/a")
    register_session(tmp_path, "b", "C:/b")
    with pytest.raises(ValueError):
        authenticate(tmp_path, "b", a["session_token"])
    with pytest.raises(ValueError):
        authenticate(tmp_path, "missing", "x")


@pytest.mark.parametrize("overrides", [dict(revision=True), dict(status=[]), dict(plan=[None]), dict(plan=[dict(id="x", title="", status="pending")]), dict(blockers=[dict(id="x", title="x", open="true")]), dict(evidence=[None]), dict(status="blocked"), dict(status="completed"), dict(status="completed", plan=[dict(id="x", title="x", status="completed")])])
def test_invalid_reports(overrides):
    with pytest.raises(ValueError):
        validate_report(report(**overrides))


def test_completion_requires_evidence_and_no_blockers():
    valid = report(status="completed", plan=[dict(id="x", title="x", status="completed")], evidence=[dict(label="核验通过")])
    assert validate_report(valid)["status"] == "completed"
    with pytest.raises(ValueError):
        validate_report({**valid, "blockers": [dict(id="x", title="待确认", open=True)]})


def test_lifecycle_does_not_complete_or_clear_blocker(tmp_path):
    store = Store(tmp_path)
    store.ingest(event("progress", report=report(status="blocked", blockers=[dict(id="dep", title="等待依赖", open=True)])))
    store.ingest(event("Stop"))
    task = store.tasks()[0]
    assert task["runtime"] == "idle" and task["status"] == "blocked"
    assert task["blockers"][0]["open"]
    store.close()


def test_out_of_order_and_dedup(tmp_path):
    store = Store(tmp_path)
    now = time.time()
    newest = event("progress", report=report(revision=5))
    assert store.ingest(newest)
    assert not store.ingest(newest)
    store.ingest(event("progress", report=report(revision=1, title="旧快照")))
    store.ingest(event("Stop", recorded_at=now))
    store.ingest(event("PreToolUse", recorded_at=now - 10, tool_use_id="old", tool_name="old"))
    task = store.tasks()[0]
    assert task["revision"] == 5 and task["title"] != "旧快照"
    assert task["runtime"] == "idle" and not task["active_tools"]
    store.close()


def test_child_events_do_not_change_parent(tmp_path):
    store = Store(tmp_path)
    store.ingest(event("Stop", session="a"))
    store.ingest(event("PreToolUse", session="b"))
    store.ingest(event("SubagentStart", session="a", agent_id="child"))
    tasks = {t["session_id"]: t for t in store.tasks()}
    assert tasks["a"]["runtime"] == "idle" and tasks["b"]["runtime"] == "active"
    assert tasks["a"]["agents"]["child"]["runtime"] == "active"
    store.close()


def test_arbitrary_event_fields_not_persisted(tmp_path):
    store = Store(tmp_path)
    store.ingest(event(raw_prompt="PRIVATE_CONTENT", session_token="SECRET_TOKEN", tool_input="PRIVATE_COMMAND"))
    saved = store.db.execute("select payload from events").fetchone()[0]
    assert "PRIVATE_CONTENT" not in saved and "SECRET_TOKEN" not in saved and "PRIVATE_COMMAND" not in saved
    store.close()


def test_queue_restart_and_invalid_input(tmp_path):
    for _ in range(5):
        enqueue(tmp_path, event())
    (tmp_path / "inbox" / "broken.json").write_text("[1]", encoding="utf-8")
    store = Store(tmp_path)
    assert drain(tmp_path, store) == (5, 1)
    store.close()
    store = Store(tmp_path)
    assert len(store.tasks()) == 1 and drain(tmp_path, store) == (0, 0)
    assert len(list((tmp_path / "rejected").glob("*.json"))) == 1
    store.close()


def test_hook_and_mcp_bind_and_do_not_leak(tmp_path):
    context = hook(tmp_path, dict(hook_event_name="SessionStart", session_id="real", cwd="C:/test"))
    assert "additionalContext" in context["hookSpecificOutput"]
    binding = register_session(tmp_path, "real", "C:/test")
    result = progress(tmp_path, {**report(), "session_id": "real", "session_token": binding["session_token"]})
    assert result["accepted"]
    assert binding["session_token"] not in json.dumps(result)
    store = Store(tmp_path)
    drain(tmp_path, store)
    assert binding["session_token"] not in json.dumps(store.tasks())
    assert store.tasks()[0]["title"] == "专用验收任务"
    store.close()


@pytest.mark.parametrize("payload", [[], None, "text", {"hook_event_name": "PostToolUse", "session_id": "s", "tool_name": "update_plan", "tool_input": None}])
def test_hook_malformed_is_harmless(tmp_path, payload):
    assert isinstance(hook(tmp_path, payload), dict)


def test_mcp_malformed_requests_do_not_terminate(tmp_path):
    incoming = io.StringIO('\n'.join(["[]", "null", '{"id":1,"method":"initialize","params":null}', '{"id":2,"method":"tools/list"}', '{"id":3,"method":"ping"}']) + '\n')
    outgoing = io.StringIO()
    serve_mcp(tmp_path, incoming, outgoing)
    responses = [json.loads(x) for x in outgoing.getvalue().splitlines()]
    assert any(x.get("id") == 3 and "result" in x for x in responses)


def test_process_singleton(tmp_path):
    first, second = ProcessLock(tmp_path / "lock"), ProcessLock(tmp_path / "lock")
    assert first.acquire()
    assert not second.acquire()
    first.close()
    assert second.acquire()
    second.close()


def test_native_plan_status_mapping_and_resume_revision(tmp_path):
    hook(tmp_path, dict(hook_event_name="PostToolUse", session_id="plan", cwd="C:/test", tool_name="update_plan", tool_input={"plan": [{"step": "检查依赖", "status": "in_progress"}]}))
    store = Store(tmp_path)
    drain(tmp_path, store)
    assert store.tasks()[0]["plan"][0]["status"] == "inProgress"
    binding = register_session(tmp_path, "plan", "C:/test")
    progress(tmp_path, {**report(revision=8), "session_id": "plan", "session_token": binding["session_token"]})
    context = hook(tmp_path, dict(hook_event_name="UserPromptSubmit", session_id="plan", cwd="C:/test"))
    assert "last_revision=8" in context["hookSpecificOutput"]["additionalContext"]
    store.close()


def test_storage_failure_keeps_queued_event(tmp_path):
    identifier = enqueue(tmp_path, event())
    class Unavailable:
        def ingest(self, value):
            raise sqlite3.OperationalError("test only")
    with pytest.raises(sqlite3.OperationalError):
        drain(tmp_path, Unavailable())
    assert (tmp_path / "inbox" / (identifier + ".json")).exists()


@pytest.mark.parametrize("override", [dict(kind=[]), dict(session_id=""), dict(host_id=None), dict(recorded_at=float("nan")), dict(recorded_at=-1), dict(event_id="")])
def test_invalid_event_envelope(tmp_path, override):
    store = Store(tmp_path)
    try:
        with pytest.raises(ValueError):
            store.ingest(event(**override))
        assert not store.tasks()
    finally:
        store.close()


def test_parent_runtime_approval_interrupt_and_tool_pairing(tmp_path):
    store = Store(tmp_path)
    for kind in ["PreToolUse", "PostToolUse", "PermissionRequest", "Interrupt"]:
        store.ingest(event(kind, tool_use_id="test-call", tool_name="read-only-check"))
        task = store.tasks()[0]
        if kind == "PreToolUse":
            assert "test-call" in task["active_tools"]
        elif kind == "PostToolUse":
            assert not task["active_tools"]
        elif kind == "PermissionRequest":
            assert task["runtime"] == "waiting_approval"
        else:
            assert task["runtime"] == "interrupted"
        assert task["status"] == "unreported"
    store.close()


def test_mcp_invalid_tool_and_storage_failure_are_recoverable(tmp_path, monkeypatch):
    import floatboard.ingest as ingest
    def unavailable(*_):
        raise OSError("secret-detail-must-not-leak")
    monkeypatch.setattr(ingest, "progress", unavailable)
    requests = [dict(id=1, method="tools/call", params=dict(name="unknown")), dict(id=2, method="tools/call", params=dict(name="report_progress", arguments={})), dict(id=3, method="unknown"), dict(id=4, method="initialize", params="bad"), dict(id=5, method="ping")]
    output = io.StringIO()
    serve_mcp(tmp_path, io.StringIO("".join(json.dumps(x) + "\n" for x in requests)), output)
    replies = [json.loads(x) for x in output.getvalue().splitlines()]
    assert [x.get("error", {}).get("code") for x in replies[:4]] == [-32602, -32603, -32601, -32602]
    assert "secret-detail" not in output.getvalue() and replies[-1]["result"] == {}
