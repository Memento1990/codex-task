from floatboard.notifications import ChangeDetector


def task(**overrides):
    value = dict(id="task-a", title="任务 A", status="running", runtime="active", plan=[dict(id="one", title="步骤一", status="inProgress")], blockers=[])
    value.update(overrides)
    return value


def test_initial_snapshot_is_silent_and_not_replayed():
    detector = ChangeDetector()
    assert detector.update([task()]) == []
    assert detector.update([task(runtime="idle", updated_at=99999, timing=dict(seconds=999))]) == []
    assert ChangeDetector().update([task(status="completed")]) == []


def test_new_reported_task_coalesces_initial_plan():
    detector = ChangeDetector()
    detector.update([])
    notice, = detector.update([task()])
    assert notice["task_id"] == "task-a" and notice["lines"] == ["新增任务 · 进行中"]
    assert detector.update([task()]) == []


def test_runtime_only_task_waits_for_first_real_report():
    detector = ChangeDetector()
    detector.update([])
    assert detector.update([task(status="unreported", plan=[])]) == []
    assert "新增任务" in detector.update([task()])[0]["lines"][0]


def test_status_and_plan_changes_are_merged_and_capped():
    detector = ChangeDetector()
    original = task()
    detector.update([original])
    changed = task(status="blocked", plan=[dict(id="one", title="步骤一", status="completed"), *[dict(id=str(i), title="新增步骤"+str(i), status="pending") for i in range(6)]])
    notice, = detector.update([changed])
    assert len(notice["lines"]) == 4 and notice["lines"][-1] == "另有 5 项变化"
    assert "有卡点" in notice["lines"][0] and "已完成" in notice["lines"][1]
    assert original["plan"][0]["status"] == "inProgress"


def test_blocker_create_resolve_remove_and_reopen():
    detector = ChangeDetector()
    detector.update([task()])
    blocker = dict(id="dependency", title="缺少依赖", open=True)
    blocked = task(blockers=[blocker])
    notice, = detector.update([blocked])
    assert notice["tab"] == 1 and notice["lines"] == ["新增卡点 · 缺少依赖"]
    closed = task(blockers=[{**blocker, "open": False}])
    assert detector.update([closed])[0]["lines"] == ["卡点已解除 · 缺少依赖"]
    assert detector.update([blocked])[0]["lines"] == ["卡点待处理 · 缺少依赖"]
    assert detector.update([task()])[0]["lines"] == ["卡点已解除 · 缺少依赖"]
    assert detector.update([task()]) == []


def test_detector_keeps_its_own_snapshot_and_multi_task_isolation():
    detector = ChangeDetector()
    original = task()
    other = task(id="task-b")
    detector.update([original, other])
    original["plan"][0]["status"] = "completed"
    notice, = detector.update([original, other])
    assert notice["task_id"] == "task-a" and notice["lines"] == ["步骤一 · 已完成"]


def test_new_task_reports_initial_blocker_and_same_session_new_title():
    detector = ChangeDetector()
    detector.update([])
    notice, = detector.update([task(status="blocked", blockers=[dict(id="dep", title="等待依赖", open=True)])])
    assert notice["tab"] == 1 and notice["lines"][-1] == "新增卡点 · 等待依赖"
    detector.update([task(status="completed")])
    notice, = detector.update([task(title="后续新任务")])
    assert notice["title"] == "后续新任务" and notice["lines"] == ["新增任务 · 进行中"]
