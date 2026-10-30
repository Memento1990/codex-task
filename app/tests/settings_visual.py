"""Windows Qt 两主题与消息叠放验收，只用隔离数据。"""
import os
os.environ.setdefault("QT_QPA_PLATFORM", "windows")
import ctypes
from datetime import datetime
import json
from pathlib import Path
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).parents[1]))
from PySide6.QtCore import QCoreApplication, QEvent, Qt
from PySide6.QtGui import QPalette
from PySide6.QtWidgets import QApplication
from floatboard.core import Store, atomic_json
from floatboard.gui import Board, IntegrationDialog
from floatboard.settings_dialog import SettingsDialog

base = Path(__file__).parents[1]
output = base / "docs/evidence/FEAT-006"
root = output / ("settings-data-" + uuid.uuid4().hex[:8])
root.mkdir(parents=True)
os.environ["CODEX_HOME"] = str(root / "codex-home")
app = QApplication([])
store = Store(root)
store.ingest(dict(event_id=uuid.uuid4().hex, recorded_at=time.time(), kind="progress", session_id="settings-fixture", host_id="settings-fixture", cwd=str(base), report=dict(revision=0, title="看板设置与桌面消息通知 · 隔离演示", status="running", current_step="核对两种主题与右下角叠放", plan=[dict(id="scope", title="核对默认设置", status="completed"), dict(id="theme", title="验证黑白主题与消息浮窗", status="inProgress"), dict(id="package", title="验证构建包与本机更新", status="pending")], blockers=[], evidence=[dict(label="隔离验收资料")])) )
atomic_json(root / "health.json", dict(at=time.time(), status="ok"))
board = Board(root)
board.timer.stop()
board.show()
dialogs = []
checks = {}
screenshots = []


def settle():
    QCoreApplication.sendPostedEvents(None, QEvent.Type.DeferredDelete)
    app.processEvents()


def capture(widget, name):
    settle()
    assert widget.grab().save(str(output / name))
    screenshots.append(name)


try:
    settle()
    get_style = ctypes.windll.user32.GetWindowLongW
    get_style.argtypes = [ctypes.c_void_p, ctypes.c_int]
    get_style.restype = ctypes.c_long
    checks["native_default_toolwindow_no_appwindow"] = bool(get_style(int(board.winId()), -20) & 0x80) and not bool(get_style(int(board.winId()), -20) & 0x40000)
    checks["first_load_silent"] = not board.toast_stack.items
    for theme in ("light", "dark"):
        board.apply_options({**board.options, "theme": theme})
        capture(board, theme + "-board.png")
        dialog = SettingsDialog(board)
        dialogs.append(dialog)
        dialog.show()
        capture(dialog, theme + "-preferences.png")
        checks[theme + "_defaults_and_controls"] = not dialog.auto_start.isChecked() and not dialog.taskbar.isChecked() and dialog.notifications.isChecked() and dialog.seconds.value() == 3
        checks[theme + "_dialog_text_not_clipped"] = all(control.height() >= control.fontMetrics().height() for control in [dialog.auto_start, dialog.taskbar, dialog.notifications, dialog.seconds, dialog.theme])
        dialog.hide()
        integration = IntegrationDialog(board)
        integration.timer.stop()
        dialogs.append(integration)
        integration.show()
        capture(integration, theme + "-integration.png")
        integration.advanced_toggle.click()
        capture(integration, theme + "-integration-advanced.png")
        checks[theme + "_advanced_diff_available"] = integration.changes.topLevelItemCount() == 10
        checks[theme + "_integration_palette"] = integration.palette().color(QPalette.ColorRole.Window).name() == board.theme["surface"]
        integration.hide()
        for i in range(3):
            board.toast_stack.show_notice(dict(task_id="preview-" + str(i), title=["新增任务 · 配置检查", "任务完成 · 验收记录", "新增卡点 · 等待依赖"][i], lines=[["开始核对默认设置"], ["检查完成 · 证据已保存"], ["验收账号尚未准备", "点击查看对应任务"]][i], tab=0))
            board.toast_stack.items[-1].timer.stop()
        settle()
        items = board.toast_stack.items
        bounds = board.screen().availableGeometry()
        checks[theme + "_stack_inside_workarea"] = all(bounds.contains(item.geometry()) for item in items)
        checks[theme + "_stack_no_overlap"] = all(a.geometry().bottom() < b.y() for a, b in zip(items, items[1:]))
        checks[theme + "_toast_nonactivating"] = all(item.testAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating) and bool(item.windowFlags() & Qt.WindowType.WindowDoesNotAcceptFocus) for item in items)
        for i, item in enumerate(items):
            capture(item, theme + "-toast-" + str(i) + ".png")
            checks[theme + "_toast_" + str(i) + "_text_fits"] = item.body.height() >= item.body.fontMetrics().height() * len(item.notice["lines"])
        board.toast_stack.clear()
    board.apply_options({**board.options, "show_taskbar": True})
    settle()
    checks["native_taskbar_toggle_removes_tool_style"] = not bool(get_style(int(board.winId()), -20) & 0x80)
    board.apply_options({**board.options, "show_taskbar": False})
    settle()
    checks["native_taskbar_toggle_restores_tool_style"] = bool(get_style(int(board.winId()), -20) & 0x80)
    record = dict(at=datetime.now().astimezone().isoformat(timespec="seconds"), command="python tests/settings_visual.py", qt_platform=app.platformName(), scope="Qt Windows isolated render and read-only Win32 style check; no OS screenshot or native input", data_dir=str(root), checks=checks, screenshots=screenshots)
    (output / "settings-visual.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(record, ensure_ascii=False))
    assert all(checks.values())
finally:
    board.toast_stack.clear()
    for dialog in dialogs:
        dialog.close()
        dialog.deleteLater()
    board.hide()
    board.tray.hide()
    if board.store:
        board.store.close()
    store.close()
