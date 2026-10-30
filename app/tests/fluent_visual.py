"""隔离演示投影的 Qt 渲染证据；不读取日常任务。"""
import os
import argparse
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from datetime import datetime
import json
from pathlib import Path
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).parents[1]))
from PySide6.QtCore import QCoreApplication, QEvent, Qt
from PySide6.QtWidgets import QApplication, QLabel

from floatboard.core import Store, atomic_json
from floatboard.gui import Board, IntegrationDialog


ROOT = Path(__file__).parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--evidence-dir", default=str(ROOT / "docs/evidence/FEAT-002"))
OUTPUT = Path(parser.parse_args().evidence_dir).resolve()
DATA = OUTPUT / ("visual-data-" + time.strftime("%H%M%S") + "-" + uuid.uuid4().hex[:6])


def project(store, session, title, status, plan, blockers=(), revision=0):
    store.ingest(dict(event_id=uuid.uuid4().hex, recorded_at=time.time(), kind="progress", session_id=session, host_id="visual-fixture", cwd=str(ROOT), report=dict(revision=revision, title=title, status=status, current_step=next((step["title"] for step in plan if step["status"] != "completed"), "演示清单已完成"), plan=plan, blockers=list(blockers), evidence=[dict(label="原生窗口检查 · 演示资料", reference="")])))
    store.ingest(dict(event_id=uuid.uuid4().hex, recorded_at=time.time(), kind="PreToolUse" if status == "running" else "Stop", session_id=session, host_id="visual-fixture", cwd=str(ROOT), tool_name="fixture-tool", tool_use_id=uuid.uuid4().hex))


if __name__ == "__main__":
    DATA.mkdir(parents=True, exist_ok=True)
    store = Store(DATA)
    plan = [dict(id="scope", title="核对现有窗口与交互", status="completed"), dict(id="style", title="应用 Fluent 2 视觉规范", status="completed"), dict(id="native", title="验证原生窗口与状态展示", status="inProgress", note="三条任务可见，详情在内部滚动"), dict(id="package", title="构建并验证 Windows 包", status="pending")]
    blocker = dict(id="auth", title="验收账号尚未授权（演示）", reason="专用验收场景：账号缺少导出权限。", impact="真实导出验收等待授权；本地检查已保留。", owner="测试环境负责人", resolve_when="确认测试账号权限后继续验收。", open=True)
    project(store, "visual-ui", "任务侧窗 Fluent 2 视觉升级", "running", plan)
    project(store, "visual-export", "报表导出链路联调 · 演示任务", "blocked", [dict(id="contract", title="核对接口与字段", status="completed"), dict(id="account", title="等待验收账号授权", status="blocked")], [blocker])
    project(store, "visual-docs", "发布说明与证据归档 · 演示任务", "completed", [dict(id="archive", title="整理交付清单", status="completed")])
    atomic_json(DATA / "health.json", {"at": time.time(), "status": "ok", "rejected": 0})
    atomic_json(DATA / "window.json", {"x": 1150, "y": 100, "selected": "visual-fixture:visual-ui", "tab": 0, "pinned": True})
    app = QApplication([])
    app.setQuitOnLastWindowClosed(False)
    board = Board(DATA)
    board.timer.stop()
    board.show()
    checks = {}
    screenshots = []

    def capture(name, widget=None):
        QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
        app.processEvents()
        path = OUTPUT / name
        assert (widget or board).grab().save(str(path))
        screenshots.append(name)

    def select(session):
        target = next(task for task in board.tasks if task["session_id"] == session)
        if target["status"] == "completed":
            board.show_completed(selected=target["id"])
            return
        if board.completed_window:
            board.completed_window.close()
        for index in range(board.list.count()):
            item = board.list.item(index)
            if (item.data(Qt.ItemDataRole.UserRole) or "").endswith(":" + session):
                board.list.setCurrentItem(item)
                break
        app.processEvents()

    try:
        select("visual-ui")
        viewport = board.list.viewport().rect()
        checks["active_rows_and_completed_group_fit"] = all(viewport.contains(board.list.visualItemRect(board.list.item(index))) for index in range(board.list.count()))
        checks["completed_tasks_folded"] = board.completed_tasks_button.count == 1 and not any(board.list.item(index).data(Qt.ItemDataRole.UserRole) == "visual-fixture:visual-docs" for index in range(board.list.count()))
        checks["completed_steps_folded"] = board.completed_steps_button.count == 2 and not any(widget.text() == "核对现有窗口与交互" for widget in board.tabs.widget(0).findChildren(QLabel))
        checks["fixed_window"] = board.width() == 392 and board.height() == 570
        checks["four_tabs_fit"] = board.tabs.tabBar().rect().contains(board.tabs.tabBar().tabRect(3))
        checks["detail_scrolls_inside_window"] = not board.tabs.widget(0).horizontalScrollBar().isVisible()
        capture("fluent-running.png")
        board.completed_steps_button.click()
        capture("fluent-completed-steps.png", board.completed_window)
        checks["completed_steps_secondary_window"] = "核对现有窗口与交互" in board.completed_window.browsers[0].toPlainText()
        board.completed_window.close()
        select("visual-export")
        board.tabs.setCurrentIndex(1)
        capture("fluent-blockers.png")
        checks["blocker_reason_visible"] = any("账号缺少" in label.text() for label in board.tabs.widget(1).findChildren(QLabel))
        board.tabs.setCurrentIndex(2)
        capture("fluent-evidence.png")
        board.tabs.setCurrentIndex(3)
        capture("fluent-activity.png")
        select("visual-docs")
        board.completed_window.tabs.setCurrentIndex(0)
        capture("fluent-completed.png", board.completed_window)
        checks["completed_task_selectable_in_secondary"] = board.completed_window.selected == "visual-fixture:visual-docs" and board.selected == "visual-fixture:visual-export"
        select("visual-ui")
        atomic_json(DATA / "health.json", {"at": 1, "status": "storage_error", "rejected": 0})
        board.refresh()
        capture("fluent-offline.png")
        checks["offline_preserves_tasks"] = len(board.tasks) == 3 and "离线" in board.health.text()
        dialog = IntegrationDialog(board)
        dialog.show()
        app.processEvents()
        assert dialog.grab().save(str(OUTPUT / "fluent-settings.png"))
        screenshots.append("fluent-settings.png")
        dialog.close()
        project(store, "visual-ui", "任务侧窗视觉升级与超长中文任务标题以及窄窗布局边界回归检查" * 3, "running", plan, revision=1)
        board.refresh()
        app.processEvents()
        capture("fluent-long-title.png")
        checks["long_title_elided"] = board.task_title.text().endswith("…") and board.task_title.fontMetrics().horizontalAdvance(board.task_title.text()) <= board.task_title.width()
        project(store, "visual-ui", "任务侧窗 Fluent 2 视觉升级", "running", plan, revision=2)
        atomic_json(DATA / "health.json", {"at": time.time(), "status": "ok", "rejected": 0})
        board.refresh()
        board.persist()
        capture("fluent-final.png")
        record = {"at": datetime.now().astimezone().isoformat(timespec="seconds"), "command": "python tests/fluent_visual.py", "scope": "Qt Widgets render of synthetic fixture; not OS screenshot or real Codex session", "qt_platform": app.platformName(), "data_dir": str(DATA), "checks": checks, "screenshots": screenshots}
        (OUTPUT / "qt-visual.json").write_text(json.dumps(record, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(record, ensure_ascii=False))
        if not all(checks.values()):
            raise SystemExit(1)
    finally:
        board.hide()
        if board.completed_window:
            board.completed_window.close()
        board.tray.hide()
        if board.store:
            board.store.close()
        store.close()
