import os
import time
import threading
os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from pathlib import Path

import pytest
from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import QApplication

from floatboard.core import Store
from floatboard.gui import Board, IntegrationDialog, safe_reference
from PySide6.QtWidgets import QMessageBox, QSystemTrayIcon
from test_core import event, report


@pytest.fixture(scope="module")
def qt_app():
    return QApplication.instance() or QApplication([])


def test_window_reads_real_projection_and_preserves_view(qt_app, tmp_path):
    store = Store(tmp_path)
    store.ingest(event("progress", report=report(title="真实投影标题", status="blocked", blockers=[dict(id="dep", title="缺少验收结果", open=True, resolve_when="验证完成")], evidence=[])))
    board = Board(tmp_path)
    try:
        board.timer.stop()
        assert board.width() == 392 and board.height() == 570
        assert board.windowFlags() & Qt.WindowType.WindowStaysOnTopHint
        assert board.list.count() == 1
        assert board.task_title.text() == "真实投影标题"
        assert board.tabs.tabText(1) == "卡点 1"
        board.tabs.setCurrentIndex(1)
        board.toggle_collapse()
        assert board.height() == 44 and board.body.isHidden()
        board.toggle_collapse()
        assert board.height() == 570 and not board.body.isHidden()
        board.move(25, 35)
        board.persist()
    finally:
        board.tray.hide()
        if board.store:
            board.store.close()
        board.deleteLater()
    reopened = Board(tmp_path)
    try:
        reopened.timer.stop()
        assert reopened.tabs.currentIndex() == 1
        assert reopened.selected.endswith(":test")
        assert len(reopened.tasks) == 1
    finally:
        reopened.tray.hide()
        if reopened.store:
            reopened.store.close()
        reopened.deleteLater()
        store.close()


def test_empty_has_no_demo_tasks(qt_app, tmp_path, monkeypatch):
    monkeypatch.setenv("CODEX_HOME", str(tmp_path / "empty-codex"))
    board = Board(tmp_path)
    try:
        board.timer.stop()
        assert not board.tasks and board.list.count() == 0
        assert board.task_title.text() == "连接你的 Codex"
        assert board.list.isHidden() and board.progress.isHidden()
        assert board.tabs.tabBar().isHidden()
    finally:
        board.tray.hide()
        board.deleteLater()


def test_three_tasks_fit_and_long_title_stays_inside_window(qt_app, tmp_path):
    store = Store(tmp_path)
    for index in range(3):
        store.ingest(event("progress", session=f"layout-{index}", report=report(title="长标题边界检查与任务侧窗原生布局验证" * 4)))
    board = Board(tmp_path)
    try:
        board.timer.stop()
        board.show()
        qt_app.processEvents()
        viewport = board.list.viewport().rect()
        assert board.list.count() == 3
        assert all(viewport.contains(board.list.visualItemRect(board.list.item(index))) for index in range(3))
        assert board.list.horizontalScrollBar().maximum() == 0
        assert board.task_title.fontMetrics().horizontalAdvance(board.task_title.text()) <= board.task_title.contentsRect().width()
        assert board.tabs.tabBar().rect().contains(board.tabs.tabBar().tabRect(3))
    finally:
        board.hide()
        if getattr(board, "completed_window", None):
            board.completed_window.close()
        board.tray.hide()
        if board.store:
            board.store.close()
        board.deleteLater()
        store.close()


@pytest.mark.parametrize("value", ["javascript:alert(1)", "file:///C:/Windows/System32/cmd.exe", "powershell:run", r"\\host\share\file.md", "https://user:pass@example.com/", "relative.md"])
def test_untrusted_reference_not_executable(value):
    assert safe_reference(value) is None


def test_document_and_web_references(tmp_path):
    doc = tmp_path / "evidence.txt"
    doc.write_text("test", encoding="utf-8")
    assert safe_reference(str(doc)).isLocalFile()
    assert safe_reference("https://developers.openai.com/codex/hooks/").scheme() == "https"


def test_window_pin_hide_restore_and_offline_retains_snapshot(qt_app, tmp_path, monkeypatch):
    store = Store(tmp_path)
    store.ingest(event("progress", report=report(evidence=[dict(label="本地证据", reference="https://example.com/")])) )
    board = Board(tmp_path)
    try:
        board.timer.stop()
        board.pin.setChecked(False)
        board.toggle_pin()
        assert not board.windowFlags() & Qt.WindowType.WindowStaysOnTopHint
        monkeypatch.setattr(QSystemTrayIcon, "isSystemTrayAvailable", lambda: True)
        board.hide_to_tray()
        assert not board.isVisible()
        board.restore()
        assert board.isVisible()
        (tmp_path / "health.json").write_text('{"at":1,"status":"storage_error","rejected":2}', encoding="utf-8")
        board.refresh()
        assert "离线" in board.health.text() and "异常 2" in board.health.text()
        assert board.task_title.text() == "专用验收任务"
        board.copy_session()
        assert qt_app.clipboard().text() == "test"
    finally:
        board.hide()
        board.tray.hide()
        board.store.close()
        board.deleteLater()
        store.close()


def wait_for_job(qt_app, dialog):
    deadline = time.monotonic() + 15
    while dialog.job is not None and time.monotonic() < deadline:
        qt_app.processEvents()
        time.sleep(0.005)
    assert dialog.job is None


def test_config_dialog_one_click_and_advanced_remove(qt_app, tmp_path, monkeypatch):
    home = tmp_path / "isolated-codex"
    home.mkdir()
    original = '# 夹具注释\nmodel = "fixture"\n'
    path = home / "config.toml"
    path.write_text(original, encoding="utf-8")
    monkeypatch.setenv("CODEX_HOME", str(home))
    board = Board(tmp_path / "board")
    dialog = IntegrationDialog(board)
    try:
        board.timer.stop()
        dialog.timer.stop()
        assert dialog.advanced.isHidden() and dialog.width() == 480
        assert dialog.apply.isEnabled() and not hasattr(dialog, "confirm")
        dialog.advanced_toggle.setChecked(True)
        dialog.toggle_advanced()
        assert dialog.width() == 760 and dialog.changes.topLevelItemCount() == 10
        assert "绿色" in dialog.code.toPlainText() and "+" in dialog.code.toPlainText()
        assert path.read_text(encoding="utf-8") == original
        # 普通连接不能再弹核对或确认。
        def unexpected_confirmation(*_):
            pytest.fail("连接不应弹确认")
        monkeypatch.setattr(QMessageBox, "question", unexpected_confirmation)
        dialog.execute()
        assert not dialog.apply.isEnabled() and dialog.job is not None
        wait_for_job(qt_app, dialog)
        assert "codex_floatboard" in path.read_text(encoding="utf-8")
        assert dialog.title.text() == "接入已就绪，等待首次同步"
        assert "等待首次任务同步" == board.task_title.text()
        assert dialog.changes.topLevelItemCount() == 0
        dialog.copy_request()
        assert "--connect" in qt_app.clipboard().text()
        assert str(board.root) in qt_app.clipboard().text()
        monkeypatch.setattr(QMessageBox, "question", lambda *_: QMessageBox.StandardButton.No)
        dialog.disconnect()
        assert dialog.job is None and "codex_floatboard" in path.read_text(encoding="utf-8")
        monkeypatch.setattr(QMessageBox, "question", lambda *_: QMessageBox.StandardButton.Yes)
        dialog.disconnect()
        wait_for_job(qt_app, dialog)
        assert "codex_floatboard" not in path.read_text(encoding="utf-8")
        assert "夹具注释" in path.read_text(encoding="utf-8")
    finally:
        dialog.close()
        dialog.deleteLater()
        board.tray.hide()
        board.deleteLater()


def test_connect_failure_keeps_ui_responsive_and_can_retry(qt_app, tmp_path, monkeypatch):
    import floatboard.gui as gui
    monkeypatch.setenv("CODEX_HOME", str(tmp_path / "codex"))
    board = Board(tmp_path / "board")
    dialog = IntegrationDialog(board)
    released = threading.Event()
    original_connect = gui.connect
    def fail(_):
        assert released.wait(5)
        raise ValueError("预检失败：演示程序缺失")
    monkeypatch.setattr(gui, "connect", fail)
    try:
        board.timer.stop()
        dialog.timer.stop()
        dialog.execute()
        dialog.reject()
        assert dialog.job and not dialog.close_button.isEnabled()
        QTimer.singleShot(0, released.set)
        wait_for_job(qt_app, dialog)
        assert released.is_set()
        assert dialog.title.text() == "连接失败"
        assert "演示程序缺失" in dialog.description.text()
        assert dialog.apply.isEnabled() and dialog.apply.text() == "重试连接"
        assert not (tmp_path / "codex/config.toml").exists()
        monkeypatch.setattr(gui, "connect", original_connect)
        dialog.execute()
        wait_for_job(qt_app, dialog)
        assert "失败" not in dialog.title.text()
        assert "codex_floatboard" in (tmp_path / "codex/config.toml").read_text(encoding="utf-8")
    finally:
        released.set()
        if dialog.job:
            wait_for_job(qt_app, dialog)
        dialog.close()
        dialog.deleteLater()
        board.tray.hide()
        board.deleteLater()


def test_elapsed_ticks_without_rebuilding_detail_and_freezes(qt_app, tmp_path, monkeypatch):
    from PySide6.QtWidgets import QLabel
    from floatboard import timing
    store = Store(tmp_path)
    store.ingest(event('progress', recorded_at=1000, report=report(status='blocked', blockers=[dict(id='dep', title='等待依赖', open=True)])))
    monkeypatch.setattr(timing.time, 'time', lambda: 1061)
    board = Board(tmp_path)
    try:
        board.timer.stop()
        assert board.task_elapsed.text() == '耗时 1 分 1 秒'
        assert any(w.text() == '耗时 1 分 1 秒' for w in board.tabs.widget(0).findChildren(QLabel))
        assert any(w.text() == '持续 1 分 1 秒' for w in board.tabs.widget(1).findChildren(QLabel))
        original_row = board.panes[0].itemAt(0).widget()
        monkeypatch.setattr(timing.time, 'time', lambda: 1122)
        board.refresh()
        assert board.task_elapsed.text() == '耗时 2 分 2 秒'
        assert board.panes[0].itemAt(0).widget() is original_row
        store.ingest(event('progress', recorded_at=1122, report=report(revision=1, status='completed', plan=[dict(id='check', title='检查', status='completed')], blockers=[dict(id='dep', title='等待依赖', open=False)], evidence=[dict(label='通过')])))
        board.refresh()
        board.completed_tasks_button.click()
        dialog = board.completed_window
        monkeypatch.setattr(timing.time, 'time', lambda: 2000)
        board.refresh()
        assert dialog.task_elapsed.text() == '耗时 2 分 2 秒'
        assert '累计 2 分 2 秒' in dialog.browsers[1].toPlainText()
        assert '累计：122 秒' in dialog.task_elapsed.toolTip()
    finally:
        board.hide()
        if getattr(board, "completed_window", None):
            board.completed_window.close()
        board.tray.hide()
        if board.store:
            board.store.close()
        board.deleteLater()
        store.close()


@pytest.fixture
def completed_groups(qt_app, tmp_path, monkeypatch):
    from floatboard.core import atomic_json
    monkeypatch.setenv("CODEX_HOME", str(tmp_path / "codex"))
    plan = [dict(id="a", title="已核对范围", status="completed"),
            dict(id="b", title="已确定方案", status="completed"),
            dict(id="c", title="执行检查", status="inProgress"),
            dict(id="d", title="整理证据", status="pending")]
    store = Store(tmp_path)
    store.ingest(event("progress", session="running", recorded_at=1000, report=report(title="进行中的任务", plan=plan)))
    store.ingest(event("progress", session="done", recorded_at=1100, report=report(title="已完成的任务", status="completed", plan=[dict(id="old", title="已归档检查", status="completed")], evidence=[dict(label="已通过")])) )
    atomic_json(tmp_path / "window.json", dict(selected="test-host:done"))
    board = Board(tmp_path)
    board.timer.stop()
    try:
        yield board, store, plan
    finally:
        board.hide()
        if getattr(board, "completed_window", None):
            board.completed_window.close()
        board.tray.hide()
        if board.store:
            board.store.close()
        board.deleteLater()
        store.close()


def task_ids(board):
    return [board.list.item(i).data(Qt.ItemDataRole.UserRole) for i in range(board.list.count())
            if board.list.item(i).data(Qt.ItemDataRole.UserRole)]


def step_texts(board):
    from PySide6.QtWidgets import QLabel
    return [label.text() for i in range(board.panes[0].count())
            if (widget := board.panes[0].itemAt(i).widget()) and widget.property("card")
            for label in widget.findChildren(QLabel)]


def test_completed_entries_open_secondary_windows(completed_groups):
    board, _, _ = completed_groups
    assert task_ids(board) == ["test-host:running"]
    assert board.selected == "test-host:running"
    assert board.completed_tasks_button.count == 1
    assert board.completed_steps_button.count == 2
    assert "已核对范围" not in step_texts(board)
    board.completed_steps_button.click()
    dialog = board.completed_window
    assert dialog.isVisible() and dialog.scope_task_id == "test-host:running"
    assert "已核对范围" in dialog.browsers[0].toPlainText()
    assert "执行检查" not in dialog.browsers[0].toPlainText()
    assert "耗时" in dialog.browsers[0].toPlainText() or "起点未记录" in dialog.browsers[0].toPlainText()
    assert board.selected == "test-host:running"


def test_completed_window_reuses_instance_and_keeps_main_selection(completed_groups):
    board, _, _ = completed_groups
    board.tabs.setCurrentIndex(2)
    board.completed_steps_button.click()
    dialog = board.completed_window
    dialog.close()
    assert board.tabs.currentIndex() == 2
    board.completed_tasks_button.click()
    assert board.completed_window is dialog and dialog.scope_task_id is None
    assert dialog.selected == "test-host:done"
    assert "已归档检查" in dialog.browsers[0].toPlainText()
    assert "已通过" in dialog.browsers[2].toPlainText()
    assert board.selected == "test-host:running" and board.tabs.currentIndex() == 2
    dialog.close()
    board.completed_steps_button.click()
    assert board.completed_window is dialog
    assert dialog.scope_task_id == "test-host:running"
    assert task_ids(board) == ["test-host:running"]


def test_completion_reopen_updates_history_without_disrupting_main(completed_groups):
    board, store, plan = completed_groups
    board.completed_tasks_button.click()
    dialog = board.completed_window
    finished = [{**step, "status": "completed"} for step in plan]
    store.ingest(event("progress", session="running", recorded_at=1200, report=report(revision=1, status="completed", plan=finished, evidence=[dict(label="检查通过")])))
    board.refresh()
    assert not task_ids(board) and board.selected == ""
    assert board.task_title.text() == "全部任务已完成"
    assert board.list.isHidden() and board.completed_tasks_button.count == 2
    assert board.tabs.isHidden() and board.height() <= 240
    assert not board.copy.isEnabled() and board.tabs.tabBar().isHidden()
    assert dialog.selected == "test-host:done" and dialog.list.count() == 2
    store.ingest(event("progress", session="done", recorded_at=1300, report=report(revision=1, title="重新执行原任务", plan=[dict(id="old", title="重新检查", status="inProgress")])))
    board.refresh()
    assert task_ids(board) == ["test-host:done"]
    assert board.height() == 570
    assert dialog.selected == "test-host:running" and dialog.list.count() == 1
    assert "已核对范围" in dialog.browsers[0].toPlainText()


def test_completed_steps_refresh_and_resumed_step_leaves_history(completed_groups):
    board, store, plan = completed_groups
    board.completed_steps_button.click()
    dialog = board.completed_window
    store.ingest(event("PreToolUse", session="running"))
    board.refresh()
    assert dialog.scope_task_id == "test-host:running"
    resumed = [{**step, "status": "inProgress"} if step["id"] == "a" else step for step in plan]
    store.ingest(event("progress", session="running", recorded_at=1400, report=report(revision=1, plan=resumed)))
    board.refresh()
    assert "已核对范围" in step_texts(board)
    assert board.completed_steps_button.count == 1
    assert "已核对范围" not in dialog.browsers[0].toPlainText()
    assert "已确定方案" in dialog.browsers[0].toPlainText()


def test_completed_entry_stays_outside_long_task_list(qt_app, completed_groups):
    board, store, _ = completed_groups
    for index in range(10):
        store.ingest(event("progress", session="extra-" + str(index), report=report(title="并行任务")))
    board.refresh()
    board.show()
    qt_app.processEvents()
    assert board.list.verticalScrollBar().maximum() > 0
    board.list.verticalScrollBar().setValue(board.list.verticalScrollBar().maximum())
    assert board.completed_tasks_button.parent() is not board.list
    assert board.completed_tasks_button.isVisible()
    assert board.completed_tasks_button.height() <= 32
    assert board.completed_tasks_button.geometry().top() >= board.list.geometry().bottom()


def test_completed_window_long_content_preserves_scroll_and_escapes_html(qt_app, completed_groups):
    board, store, _ = completed_groups
    plan = [dict(id=str(i), title="<b>完成记录</b> · " + str(i), status="completed", note="说明和备注" * 15) for i in range(30)]
    store.ingest(event("progress", session="running", report=report(revision=1, plan=[*plan, dict(id="pending", title="待执行", status="pending")])))
    board.refresh()
    board.completed_steps_button.click()
    dialog = board.completed_window
    qt_app.processEvents()
    browser = dialog.browsers[0]
    bar = browser.verticalScrollBar()
    assert bar.maximum() > 0
    bar.setValue(bar.maximum() // 2)
    position = bar.value()
    store.ingest(event("PreToolUse", session="running"))
    board.refresh()
    qt_app.processEvents()
    assert bar.value() == position
    assert "<b>完成记录</b>" in browser.toPlainText()
    assert not browser.horizontalScrollBar().isVisible()
    assert dialog.width() <= dialog.screen().availableGeometry().width()


def test_completed_window_updates_theme_and_handles_empty_history(completed_groups):
    board, store, _ = completed_groups
    board.completed_tasks_button.click()
    dialog = board.completed_window
    board.apply_options({**board.options, "theme": "dark"})
    assert dialog.theme is board.theme
    assert board.theme["text"] in dialog.browsers[0].document().defaultStyleSheet()
    store.ingest(event("progress", session="done", report=report(revision=1, plan=[dict(id="old", title="重新执行", status="inProgress")])))
    board.refresh()
    assert dialog.list.count() == 0 and not dialog.selected
    assert "暂无已完成任务" in dialog.task_title.text()
    assert "已归档检查" not in dialog.browsers[0].toPlainText()
    assert board.completed_tasks_button.isHidden()


def test_completed_keyboard_open_escape_and_hide(qt_app, completed_groups, monkeypatch):
    from PySide6.QtTest import QTest
    board, _, _ = completed_groups
    board.show()
    board.completed_tasks_button.setFocus()
    QTest.keyClick(board.completed_tasks_button, Qt.Key.Key_Space)
    dialog = board.completed_window
    assert dialog.isVisible()
    QTest.keyClick(dialog, Qt.Key.Key_Escape)
    assert not dialog.isVisible() and board.selected == "test-host:running"
    board.completed_tasks_button.click()
    monkeypatch.setattr(QSystemTrayIcon, "isSystemTrayAvailable", lambda: True)
    board.hide_to_tray()
    assert not board.isVisible() and not dialog.isVisible()


def test_completed_selection_resets_detail_scroll_but_not_main(qt_app, completed_groups):
    board, store, _ = completed_groups
    plan = [dict(id=str(i), title="完成项 " + str(i), status="completed", note="详情" * 15) for i in range(25)]
    store.ingest(event("progress", session="done", report=report(revision=1, title="历史长清单", status="completed", plan=plan, evidence=[dict(label="通过")])) )
    store.ingest(event("progress", session="done-short", report=report(title="历史短清单", status="completed", plan=[dict(id="one", title="另一项检查", status="completed")], evidence=[dict(label="通过")])) )
    board.refresh()
    board.show_completed(selected="test-host:done")
    dialog = board.completed_window
    qt_app.processEvents()
    bar = dialog.browsers[0].verticalScrollBar()
    bar.setValue(bar.maximum())
    assert bar.value() > 0
    for index in range(dialog.list.count()):
        if dialog.list.item(index).data(Qt.ItemDataRole.UserRole) == "test-host:done-short":
            dialog.list.setCurrentRow(index)
            break
    assert "另一项检查" in dialog.browsers[0].toPlainText()
    assert bar.value() == 0 and board.selected == "test-host:running"


def test_settings_save_cancel_restore_theme_and_window_flags(qt_app, completed_groups):
    from floatboard.gui import SettingsDialog
    from floatboard.themes import DARK
    board, _, _ = completed_groups
    assert board.windowType() == Qt.WindowType.Tool
    dialog = SettingsDialog(board)
    dialog.theme.setCurrentIndex(dialog.theme.findData("dark"))
    dialog.taskbar.setChecked(True)
    dialog.seconds.setValue(8)
    dialog.reject()
    assert board.options["theme"] == "light" and not board.options["show_taskbar"]
    dialog.save()
    assert dialog.result() == dialog.DialogCode.Accepted
    assert board.theme == DARK and DARK["surface"] in board.styleSheet()
    assert board.windowType() == Qt.WindowType.Window
    assert board.options["notification_seconds"] == 8
    board.move(25, 35)
    board.persist()
    reopened = Board(board.root)
    try:
        reopened.timer.stop()
        assert reopened.options["theme"] == "dark" and reopened.options["show_taskbar"]
        assert reopened.options["notification_seconds"] == 8
        integration = IntegrationDialog(reopened)
        assert DARK["surface"] in integration.styleSheet()
        integration.timer.stop()
        integration.deleteLater()
    finally:
        dialog.deleteLater()
        reopened.tray.hide()
        if reopened.store:
            reopened.store.close()
        reopened.deleteLater()


def test_settings_failure_stays_open_and_keeps_previous_options(completed_groups, monkeypatch):
    from floatboard.gui import SettingsDialog
    from floatboard import preferences
    board, _, _ = completed_groups
    dialog = SettingsDialog(board)
    dialog.theme.setCurrentIndex(1)
    monkeypatch.setattr(preferences, "save", lambda *_: (_ for _ in ()).throw(OSError("无法保存")))
    dialog.save()
    assert "保存失败" in dialog.note.text()
    assert board.options["theme"] == "light"
    dialog.deleteLater()


def test_notifications_deduplicate_disable_and_do_not_replay(completed_groups):
    board, store, plan = completed_groups
    assert not board.toast_stack.items
    store.ingest(event("progress", session="running", report=report(revision=1, plan=[*plan, dict(id="e", title="新增验收", status="pending")])))
    board.refresh()
    assert len(board.toast_stack.items) == 1
    first = board.toast_stack.items[0]
    board.refresh()
    assert board.toast_stack.items == [first]
    board.apply_options({**board.options, "notifications": False})
    assert not board.toast_stack.items
    store.ingest(event("progress", session="running", report=report(revision=2, plan=plan)))
    board.refresh()
    board.apply_options({**board.options, "notifications": True})
    assert not board.toast_stack.items
    store.ingest(event("progress", session="running", report=report(revision=3, status="waiting", plan=plan)))
    board.refresh()
    assert len(board.toast_stack.items) == 1
    board.toast_stack.clear()


def test_toast_stack_merge_pause_close_and_click_completed_task(qt_app, completed_groups):
    from PySide6.QtCore import QEvent, QPointF
    from PySide6.QtGui import QEnterEvent, QMouseEvent
    board, _, _ = completed_groups
    stack = board.toast_stack
    notice = dict(task_id="test-host:done", title="任务完成", lines=["完成清单"], tab=0)
    stack.show_notice(notice)
    toast = stack.items[0]
    assert toast.timer.interval() == 3000 and toast.timer.isActive()
    assert toast.testAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
    assert toast.windowFlags() & Qt.WindowType.WindowDoesNotAcceptFocus
    toast.enterEvent(QEnterEvent(QPointF(1, 1), QPointF(1, 1), QPointF(1, 1)))
    assert not toast.timer.isActive() and toast.remaining > 0
    stack.show_notice({**notice, "lines": ["清单已归档", "保留证据"]})
    assert stack.items == [toast] and not toast.timer.isActive()
    toast.leaveEvent(QEvent(QEvent.Type.Leave))
    assert toast.timer.isActive()
    for i in range(5):
        stack.show_notice({**notice, "task_id": str(i)})
    assert len(stack.items) <= 4
    assert [item.y() for item in stack.items] == sorted(item.y() for item in stack.items)
    for first, second in zip(stack.items, stack.items[1:]):
        assert first.geometry().bottom() < second.y()
    removed = stack.items[-1]
    removed.dismiss.click()
    assert removed not in stack.items
    stack.show_notice(notice)
    toast = stack.items[-1]
    board.hide()
    event = QMouseEvent(QEvent.Type.MouseButtonRelease, QPointF(toast.rect().center()), QPointF(toast.mapToGlobal(toast.rect().center())), Qt.MouseButton.LeftButton, Qt.MouseButton.NoButton, Qt.KeyboardModifier.NoModifier)
    toast.mouseReleaseEvent(event)
    assert board.isVisible() and board.selected == "test-host:running"
    assert board.completed_window.isVisible()
    assert board.completed_window.selected == "test-host:done"
    assert "已归档检查" in board.completed_window.browsers[0].toPlainText()
    assert toast not in stack.items
    stack.clear()


def test_toast_timer_really_expires(qt_app, completed_groups):
    from PySide6.QtCore import QEventLoop
    board, _, _ = completed_groups
    board.toast_stack.show_notice(dict(task_id="timer", title="计时检查", lines=["自动消失"], tab=0))
    toast = board.toast_stack.items[0]
    loop = QEventLoop()
    toast.timer.timeout.connect(loop.quit)
    toast.timer.start(1)
    QTimer.singleShot(1000, loop.quit)
    loop.exec()
    assert toast not in board.toast_stack.items
