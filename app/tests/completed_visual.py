"""二级完成窗口的 Windows Qt 渲染，只使用合成数据。"""
import os
os.environ.setdefault("QT_QPA_PLATFORM", "windows")
from datetime import datetime
import json
from pathlib import Path
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).parents[1]))
from PySide6.QtCore import QCoreApplication, QEvent, Qt
from PySide6.QtWidgets import QApplication
from floatboard.core import Store, atomic_json
from floatboard.gui import Board

base = Path(__file__).parents[1]
output = base / "docs/evidence/FEAT-007"
root = output / ("completed-data-" + uuid.uuid4().hex[:8])
root.mkdir(parents=True)
os.environ["CODEX_HOME"] = str(root / "codex-home")
store = Store(root)
now = time.time()
plan = [dict(id="scope", title="核对二级窗口与查看入口", status="completed"),
        dict(id="style", title="优化完成记录层级与交互", status="completed"),
        dict(id="test", title="验证 Windows 布局与状态联动", status="inProgress"),
        dict(id="package", title="构建并更新本机看板", status="pending")]


def send(session, title, status, steps, revision=0):
    store.ingest(dict(event_id=uuid.uuid4().hex, recorded_at=now - (180 if revision == 0 else 60), kind="progress", session_id=session, host_id="completed-fixture", cwd=str(base), report=dict(revision=revision, title=title, status=status, current_step="验证清单已完成，证据已保存" if status == "completed" else "验证二级查看不打断当前任务", plan=steps, blockers=[], evidence=[dict(label="Windows 隔离验收资料", reference="https://example.test/evidence")])) )


send("active", "已完成记录二级窗口优化", "running", plan)
send("waiting", "报表导出链路验证 · 演示", "waiting", [dict(id="a", title="核对导出权限", status="pending")])
for i in range(12):
    steps = [dict(id="check", title="执行无副作用检查", status="inProgress")]
    title = ["接口回归与结果归档", "配置核验与发布记录", "任务接入与通知验证"][i % 3] + f" · {i + 1:02}"
    send("done-" + str(i), title, "running", steps)
    send("done-" + str(i), title, "completed", [dict(id="check", title="执行无副作用检查", status="completed", note="确认测试结果与验收证据一致")], 1)
atomic_json(root / "health.json", dict(at=time.time(), status="ok"))
atomic_json(root / "window.json", dict(selected="completed-fixture:active", tab=0))
app = QApplication([])
board = Board(root)
board.timer.stop()
board.show()
checks, screenshots, metrics = {}, [], {}


def settle():
    QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
    app.processEvents()


def capture(widget, name):
    settle()
    assert widget.grab().save(str(output / name))
    screenshots.append(name)


try:
    for theme in ("light", "dark"):
        board.apply_options({**board.options, "theme": theme, "notifications": False})
        if board.completed_window:
            board.completed_window.close()
        capture(board, theme + "-main.png")
        checks[theme + "_main_only_active_tasks"] = board.list.count() == 2 and board.completed_tasks_button.count == 12
        checks[theme + "_entry_compact_and_accessible"] = board.completed_tasks_button.height() == 30 and board.completed_tasks_button.accessibleName().endswith("12 项")
        checks[theme + "_entry_children_fit"] = all(board.completed_tasks_button.rect().contains(child.geometry()) for child in (board.completed_tasks_button.caption, board.completed_tasks_button.badge, board.completed_tasks_button.action))
        metrics[theme + "_entry_height"] = board.completed_tasks_button.height()
        board.completed_steps_button.click()
        dialog = board.completed_window
        capture(dialog, theme + "-completed-steps.png")
        checks[theme + "_completed_steps_only"] = "核对二级窗口与查看入口" in dialog.browsers[0].toPlainText() and "验证 Windows 布局与状态联动" not in dialog.browsers[0].toPlainText()
        checks[theme + "_non_modal"] = not dialog.isModal()
        dialog.close()
        board.completed_tasks_button.click()
        capture(dialog, theme + "-completed-tasks.png")
        checks[theme + "_tasks_selectable"] = dialog.list.count() == 12 and "执行无副作用检查" in dialog.browsers[0].toPlainText()
        checks[theme + "_tabs_fit"] = dialog.tabs.tabBar().rect().contains(dialog.tabs.tabBar().tabRect(3))
        checks[theme + "_dialog_inside_screen"] = dialog.screen().availableGeometry().contains(dialog.geometry())
        checks[theme + "_main_selection_unchanged"] = board.selected == "completed-fixture:active" and board.tabs.currentIndex() == 0
        dialog.list.setCurrentRow(6)
        selected = dialog.selected
        dialog.list.verticalScrollBar().setValue(dialog.list.verticalScrollBar().maximum())
        position = dialog.list.verticalScrollBar().value()
        board.refresh()
        checks[theme + "_selection_and_scroll_survive_refresh"] = dialog.selected == selected and dialog.list.verticalScrollBar().value() == position
        dialog.close()

    long_steps = [dict(id=str(i), title=f"检查记录 {i + 1:02} · 保留完整标题和验收信息", status="completed", note="长备注应在二级窗口内自然换行，不占用主窗进行中的空间。" * 3) for i in range(36)]
    send("active", "超长已完成清单与备注的内部滚动验证", "running", [*long_steps, dict(id="last", title="剩余检查", status="pending")], 1)
    board.refresh()
    board.completed_steps_button.click()
    capture(board.completed_window, "dark-long-checklist.png")
    checks["long_checklist_scrolls_inside"] = board.completed_window.browsers[0].verticalScrollBar().maximum() > 0 and not board.completed_window.browsers[0].horizontalScrollBar().isVisible()
    board.completed_window.close()
    send("active", "超长已完成清单与备注的内部滚动验证", "completed", long_steps, 2)
    send("waiting", "报表导出链路验证 · 演示", "completed", [dict(id="a", title="核对导出权限", status="completed")], 1)
    board.refresh()
    for theme in ("light", "dark"):
        board.apply_options({**board.options, "theme": theme})
        capture(board, theme + "-all-completed.png")
        checks[theme + "_all_completed_compact"] = board.list.isHidden() and board.tabs.isHidden() and board.height() <= 240
        metrics[theme + "_all_completed_height"] = board.height()
        board.completed_tasks_button.click()
        checks[theme + "_all_completed_still_accessible"] = board.completed_window.list.count() == 14
        board.completed_window.close()
    record = dict(at=datetime.now().astimezone().isoformat(timespec="seconds"), command="python tests/completed_visual.py", scope="Synthetic data, Windows Qt render and interaction; not OS screenshot or native desktop click", qt_platform=app.platformName(), data_dir=str(root), checks=checks, metrics=metrics, screenshots=screenshots)
    (output / "completed-visual.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(record, ensure_ascii=False))
    assert all(checks.values())
finally:
    if board.completed_window:
        board.completed_window.close()
    board.hide()
    board.tray.hide()
    if board.store:
        board.store.close()
    store.close()
