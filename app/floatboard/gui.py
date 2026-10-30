"""方案 A：紧凑原生任务侧窗；界面只读任务投影。"""
import json
import html
import os
from pathlib import Path
import sqlite3
import time
import subprocess
from urllib.parse import urlsplit

from PySide6.QtCore import QPoint, QRect, QRectF, Qt, QThread, QTimer, QUrl, QSize
from PySide6.QtGui import QColor, QDesktopServices, QFont, QFontMetrics, QIcon, QPainter, QPen, QPixmap
from PySide6.QtWidgets import (QApplication, QDialog, QFrame, QHeaderView, QHBoxLayout, QLabel, QListWidget, QListWidgetItem, QMenu, QMessageBox, QProgressBar, QPushButton, QScrollArea, QSizePolicy, QStyledItemDelegate, QStyle, QSystemTrayIcon, QTabBar, QTabWidget, QTextBrowser, QTreeWidget, QTreeWidgetItem, QVBoxLayout, QWidget)

from .core import Store, atomic_json
from .integration import apply_preview, codex_home, connect, connection_state, preview
from .runtime import command
from .timing import duration_text, timing_tooltip
from . import preferences
from .themes import LIGHT, PALETTES, colors, palette, stylesheet
from .notifications import ChangeDetector
from .toasts import ToastStack
from .settings_dialog import SettingsDialog

STATUS = {"running": "进行中", "blocked": "有卡点", "waiting": "待确认", "completed": "已完成", "cancelled": "已取消", "unreported": "未汇报"}
RUNTIME = {"active": "运行中", "idle": "本轮结束", "interrupted": "已中断", "waiting_approval": "等待审批", "unknown": "未知"}
STEP = {"pending": "待执行", "inProgress": "进行中", "completed": "已完成", "blocked": "受阻"}
KIND = {"SessionStart": "会话开始", "UserPromptSubmit": "收到新请求", "PreToolUse": "调用工具", "PostToolUse": "工具返回", "Stop": "本轮结束", "SessionEnd": "会话结束", "PermissionRequest": "请求审批", "Interrupt": "执行中断", "progress": "更新进度", "plan": "更新计划", "SubagentStart": "子任务开始", "SubagentStop": "子任务结束"}



class TaskDelegate(QStyledItemDelegate):
    def paint(self, painter, option, index):
        task = index.data(Qt.ItemDataRole.UserRole + 1)
        if not task:
            return super().paint(painter, option, index)
        theme = self.parent().window().theme
        painter.save()
        painter.setClipRect(option.rect)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        selected = bool(option.state & QStyle.StateFlag.State_Selected)
        hovered = bool(option.state & QStyle.StateFlag.State_MouseOver)
        rect = option.rect.adjusted(5, 2, -5, -2)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(theme["surface" if selected else "hover" if hovered else "canvas"]))
        painter.drawRoundedRect(rect, 5, 5)
        if selected:
            painter.setBrush(QColor(theme["accent"]))
            painter.drawRoundedRect(QRectF(rect.left(), rect.top() + 15, 3, 18), 1.5, 1.5)
        tone = colors(theme)[task["status"]]
        marker = QRect(rect.left() + 9, rect.top() + 12, 24, 24)
        painter.setBrush(QColor(theme["selected"] if selected else theme["surface"]))
        painter.drawRoundedRect(marker, 5, 5)
        painter.drawPixmap(marker.left() + 4, marker.top() + 4, status_marker(task["status"], tone))
        font = QFont(option.font)
        font.setPixelSize(12)
        painter.setFont(font)
        painter.setPen(QColor(tone))
        metrics = QFontMetrics(font)
        tag = STATUS[task["status"]]
        tag_width = metrics.horizontalAdvance(tag)
        tag_left = rect.right() - 8 - tag_width
        painter.drawText(QRect(tag_left, rect.top() + 6, tag_width, 18), Qt.AlignmentFlag.AlignVCenter, tag)
        title_rect = QRect(marker.right() + 9, rect.top() + 5, tag_left - marker.right() - 18, 19)
        font.setPixelSize(13)
        font.setWeight(QFont.Weight.DemiBold)
        painter.setFont(font)
        painter.setPen(QColor(theme["text"]))
        title = task["title"] or "未汇报 · " + task["session_id"][-8:]
        painter.drawText(title_rect, Qt.AlignmentFlag.AlignVCenter, QFontMetrics(font).elidedText(title, Qt.TextElideMode.ElideRight, title_rect.width()))
        font.setPixelSize(12)
        font.setWeight(QFont.Weight.Normal)
        painter.setFont(font)
        painter.setPen(QColor(theme["muted"]))
        completed = sum(step["status"] == "completed" for step in task["plan"])
        ratio = (f"{completed}/{len(task['plan'])}" if task["plan"] else "未汇报") + " · " + duration_text(task.get('timing'), prefix='用时')
        ratio_width = QFontMetrics(font).horizontalAdvance(ratio)
        painter.drawText(QRect(rect.right() - ratio_width - 8, rect.top() + 27, ratio_width, 17), Qt.AlignmentFlag.AlignVCenter, ratio)
        project = Path(task["cwd"]).name or task["host_id"]
        meta = project + " · " + RUNTIME[task["runtime"]]
        meta_rect = QRect(title_rect.left(), rect.top() + 27, rect.right() - ratio_width - 20 - title_rect.left(), 17)
        painter.drawText(meta_rect, Qt.AlignmentFlag.AlignVCenter, QFontMetrics(font).elidedText(meta, Qt.TextElideMode.ElideRight, meta_rect.width()))
        painter.restore()


class FluentTabBar(QTabBar):
    def paintEvent(self, event):
        super().paintEvent(event)
        if self.currentIndex() < 0:
            return
        rect = self.tabRect(self.currentIndex())
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(self.window().theme["accent"]))
        painter.drawRoundedRect(QRectF(rect.center().x() - 8, rect.bottom() - 3, 16, 3), 1.5, 1.5)


def status_marker(status, tone):
    """使用同一套线条绘制状态，避免不同字体的符号缺字或基线偏移。"""
    pixmap = QPixmap(16, 16)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    pen = QPen(QColor(tone), 1.5, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap, Qt.PenJoinStyle.RoundJoin)
    painter.setPen(pen)
    if status == "completed":
        painter.drawLine(3, 8, 6, 11)
        painter.drawLine(6, 11, 13, 4)
    elif status in {"blocked", "waiting"}:
        painter.drawLine(8, 3, 8, 9)
        painter.drawPoint(8, 12)
    elif status == "cancelled":
        painter.drawLine(4, 8, 12, 8)
    else:
        painter.drawEllipse(QRectF(3.5, 3.5, 9, 9))
        if status in {"running", "inProgress"}:
            painter.setBrush(QColor(tone))
            painter.setPen(Qt.PenStyle.NoPen)
            painter.drawEllipse(QRectF(6, 6, 4, 4))
    painter.end()
    return pixmap


def label(value="", muted=False, heading=False, wrap=False):
    widget = QLabel(value)
    widget.setTextFormat(Qt.TextFormat.PlainText)
    widget.setWordWrap(wrap)
    widget.setProperty("muted", muted)
    widget.setProperty("heading", heading)
    return widget


def button(value, slot, quiet=False, width=None):
    widget = QPushButton(value)
    widget.setProperty("quiet", quiet)
    widget.clicked.connect(slot)
    if width:
        widget.setFixedWidth(width)
    return widget


class CompletedEntry(QPushButton):
    def __init__(self, title, count, slot):
        super().__init__()
        self.title = title
        self.setProperty("completedEntry", True)
        self.setFixedHeight(30)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Fixed)
        row = QHBoxLayout(self)
        row.setContentsMargins(10, 0, 10, 0)
        row.setSpacing(6)
        self.caption = label(title, muted=True)
        self.badge = label(muted=True)
        self.badge.setProperty("completedCount", True)
        self.action = label("查看  ›")
        self.action.setProperty("completedAction", True)
        for widget in (self.caption, self.badge):
            row.addWidget(widget)
        row.addStretch()
        row.addWidget(self.action)
        for widget in (self.caption, self.badge, self.action):
            widget.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.set_count(count)
        self.clicked.connect(slot)

    def set_count(self, count):
        self.count = count
        self.badge.setText(str(count))
        self.setAccessibleName(f"查看{self.title}，共 {count} 项")


def icon():
    pixmap = QPixmap(32, 32)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    painter.setBrush(QColor(LIGHT["accent"]))
    painter.setPen(Qt.PenStyle.NoPen)
    painter.drawRoundedRect(1, 1, 30, 30, 7, 7)
    painter.setPen(QColor("white"))
    painter.setFont(QFont("Consolas", 15, QFont.Weight.Bold))
    painter.drawText(pixmap.rect(), Qt.AlignmentFlag.AlignCenter, ">_")
    painter.end()
    return QIcon(pixmap)


def age(at):
    seconds = max(0, int(time.time() - at))
    if seconds < 60:
        return "刚刚更新"
    if seconds < 3600:
        return str(seconds // 60) + " 分钟未更新"
    return str(seconds // 3600) + " 小时未更新"


def safe_reference(reference):
    """只打开文档、图片和常规网页，不交给 shell 执行任意协议或程序。"""
    if not reference or reference.startswith(("\\\\", "//")):
        return None
    if len(reference) > 2000:
        return None
    parsed = urlsplit(reference)
    if parsed.scheme in {"http", "https"} and parsed.hostname and not parsed.username and not parsed.password:
        return QUrl(reference)
    path = Path(reference)
    if path.is_absolute() and path.is_file() and path.suffix.lower() in {".md", ".txt", ".json", ".log", ".csv", ".png", ".jpg", ".jpeg", ".webp", ".pdf", ".html"}:
        return QUrl.fromLocalFile(str(path))
    return None


class CompletedDialog(QDialog):
    """完成记录独立浏览，选择和滚动不影响主窗。"""
    def __init__(self, board):
        super().__init__(board, Qt.WindowType.Tool)
        self.board = board
        self.scope_task_id = None
        self.selected = ""
        self.list_key = None
        self.rendered_task = None
        self.html_cache = [None] * 4
        self.theme = board.theme
        self.setModal(False)
        self.setWindowIcon(icon())
        bounds = board.screen().availableGeometry()
        self.setMinimumSize(min(640, bounds.width() - 24), min(400, bounds.height() - 24))
        self.resize(min(760, bounds.width() - 24), min(520, bounds.height() - 24))
        self.move(max(bounds.left(), min(board.x() - self.width() - 12, bounds.right() - self.width())), max(bounds.top(), min(board.y(), bounds.bottom() - self.height())))
        outer = QVBoxLayout(self)
        outer.setContentsMargins(12, 10, 12, 10)
        outer.setSpacing(8)
        toolbar = QHBoxLayout()
        self.heading = label(heading=True)
        toolbar.addWidget(self.heading, 1)
        self.copy = button("复制会话 ID", self.copy_session, quiet=True)
        toolbar.addWidget(self.copy)
        outer.addLayout(toolbar)
        content = QHBoxLayout()
        content.setSpacing(12)
        self.list = QListWidget()
        self.list.setObjectName("completedHistory")
        self.list.setAccessibleName("已完成任务列表")
        self.list.setFixedWidth(216)
        self.list.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.list.setVerticalScrollMode(QListWidget.ScrollMode.ScrollPerPixel)
        self.list.currentItemChanged.connect(self.select_task)
        content.addWidget(self.list)
        details = QVBoxLayout()
        details.setSpacing(6)
        self.task_title = label(heading=True, wrap=True)
        self.task_title.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        self.task_title.setMaximumHeight(64)
        details.addWidget(self.task_title)
        summary = QHBoxLayout()
        self.task_meta = label(muted=True)
        self.task_elapsed = label(muted=True)
        summary.addWidget(self.task_meta, 1)
        summary.addWidget(self.task_elapsed)
        details.addLayout(summary)
        self.task_current = label(wrap=True)
        self.task_current.setMaximumHeight(48)
        details.addWidget(self.task_current)
        self.tabs = QTabWidget()
        self.tabs.setTabBar(FluentTabBar())
        self.browsers = []
        for name in ("清单", "卡点", "证据", "动态"):
            browser = QTextBrowser()
            browser.setObjectName("completedContent")
            browser.setOpenLinks(False)
            browser.setOpenExternalLinks(False)
            browser.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
            browser.anchorClicked.connect(self.open_reference)
            self.tabs.addTab(browser, name)
            self.browsers.append(browser)
        details.addWidget(self.tabs, 1)
        content.addLayout(details, 1)
        outer.addLayout(content, 1)
        close = button("关闭", self.close)
        outer.addWidget(close, 0, Qt.AlignmentFlag.AlignRight)
        self.apply_theme()

    def apply_theme(self):
        self.theme = self.board.theme
        self.setStyleSheet(stylesheet(self.theme))
        self.setPalette(palette(self.theme))
        for browser in self.browsers:
            browser.document().setDefaultStyleSheet(f"body {{ color:{self.theme['text']}; font-size:12px; }} p {{ margin:8px 0; }} .meta {{ color:{self.theme['muted']}; }} a {{ color:{self.theme['accent']}; }}")
        self.html_cache = [None] * 4

    def set_scope(self, task_id=None, selected=None, tab=0):
        self.scope_task_id = task_id
        self.selected = task_id or selected or self.selected
        self.list_key = None
        self.tabs.setCurrentIndex(tab)
        self.setWindowTitle("已完成清单" if task_id else "已完成任务")
        self.list.setVisible(task_id is None)
        self.refresh()

    def select_task(self, current, _):
        if current:
            self.selected = current.data(Qt.ItemDataRole.UserRole)
            self.render_task()

    def refresh(self):
        tasks = [task for task in self.board.tasks if task["id"] == self.scope_task_id] if self.scope_task_id else [task for task in self.board.tasks if task["status"] == "completed"]
        if not any(task["id"] == self.selected for task in tasks):
            self.selected = tasks[0]["id"] if tasks else ""
        if not self.scope_task_id:
            self.heading.setText(f"已完成任务 · {len(tasks)}")
            key = [(task["id"], task["title"], duration_text(task.get("timing"))) for task in tasks]
            if key != self.list_key:
                self.list_key = key
                position = self.list.verticalScrollBar().value()
                self.list.blockSignals(True)
                self.list.clear()
                for task in tasks:
                    item = QListWidgetItem((task["title"] or "未命名任务") + "\n" + duration_text(task.get("timing")))
                    item.setData(Qt.ItemDataRole.UserRole, task["id"])
                    item.setToolTip(task["title"] + "\n" + task["cwd"])
                    item.setSizeHint(QSize(190, 54))
                    self.list.addItem(item)
                    if task["id"] == self.selected:
                        self.list.setCurrentItem(item)
                self.list.verticalScrollBar().setValue(position)
                self.list.blockSignals(False)
        self.render_task()

    def render_task(self):
        task = next((task for task in self.board.tasks if task["id"] == self.selected), None)
        if self.rendered_task != self.selected:
            self.rendered_task = self.selected
            self.html_cache = [None] * 4
            for browser in self.browsers:
                browser.verticalScrollBar().setValue(0)
        self.copy.setEnabled(task is not None)
        self.task_title.setText(task["title"] if task else "暂无已完成清单" if self.scope_task_id else "暂无已完成任务")
        self.task_title.setToolTip(self.task_title.text())
        self.task_meta.setText(STATUS[task["status"]] + " · Codex " + RUNTIME[task["runtime"]] if task else "")
        self.task_meta.setToolTip(task["cwd"] if task else "")
        self.task_current.setText(task["current_step"] if task else "")
        self.task_current.setToolTip(self.task_current.text())
        self.task_current.setVisible(bool(self.task_current.text()))
        self.task_elapsed.setText(duration_text(task.get("timing")) if task else "")
        self.task_elapsed.setToolTip(timing_tooltip(task.get("timing")) if task else "")
        groups = [[], [], [], []]

        def escaped(value):
            return html.escape(str(value)).replace("\n", "<br>")

        def row(title, lines=(), link=None):
            heading = escaped(title)
            if link:
                heading = '<a href="' + html.escape(link, quote=True) + '">' + heading + '</a>'
            return '<p><b>' + heading + '</b>' + ''.join('<br><span class="meta">' + escaped(line) + '</span>' for line in lines if line) + '</p>'

        if task:
            for step in task["plan"]:
                if step["status"] == "completed":
                    groups[0].append(row(step["title"], [duration_text(step.get("timing")), step["note"]]))
            for blocker in sorted(task["blockers"], key=lambda item: not item["open"]):
                groups[1].append(row(blocker["title"], ["受阻" if blocker["open"] else "已解除", duration_text(blocker.get("timing"), "持续" if blocker["open"] else "累计"), "原因：" + blocker["reason"], "影响：" + blocker["impact"], "处理方：" + blocker["owner"], "解除条件：" + blocker["resolve_when"]]))
            for evidence in task["evidence"]:
                reference = evidence["reference"]
                groups[2].append(row(evidence["label"], [reference], reference if safe_reference(reference) else None))
            for entry in task["history"]:
                groups[3].append(row(time.strftime("%H:%M:%S", time.localtime(entry["at"])) + "  " + KIND.get(entry["kind"], entry["kind"]), [entry["tool"], STATUS.get(entry["status"], ""), "子任务 " + entry["agent_id"][:12] if entry["agent_id"] else ""]))
        if self.scope_task_id:
            self.heading.setText(f"已完成清单 · {len(groups[0])}")
        for index, (name, empty) in enumerate((("清单", "暂无已完成清单"), ("卡点", "暂无已汇报卡点"), ("证据", "暂无验收证据"), ("动态", "暂无动态"))):
            self.tabs.setTabText(index, name + (f" {len(groups[index])}" if groups[index] else ""))
            markup = ''.join(groups[index]) or row(empty)
            if markup != self.html_cache[index]:
                browser = self.browsers[index]
                position = browser.verticalScrollBar().value()
                browser.setHtml('<body>' + markup + '</body>')
                browser.verticalScrollBar().setValue(position)
                self.html_cache[index] = markup

    def copy_session(self):
        task = next((task for task in self.board.tasks if task["id"] == self.selected), None)
        if task:
            QApplication.clipboard().setText(task["session_id"])

    def open_reference(self, url):
        target = safe_reference(url.toString())
        if target:
            QDesktopServices.openUrl(target)


class DragHeader(QFrame):
    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.drag_offset = event.globalPosition().toPoint() - self.window().pos()
            event.accept()

    def mouseMoveEvent(self, event):
        if event.buttons() & Qt.MouseButton.LeftButton and getattr(self, "drag_offset", None) is not None:
            self.window().move(event.globalPosition().toPoint() - self.drag_offset)
            event.accept()

    def mouseReleaseEvent(self, event):
        self.drag_offset = None


class ConnectionJob(QThread):
    def __init__(self, root, remove=False):
        super().__init__()
        self.root, self.remove = root, remove
        self.result, self.error = None, ""

    def run(self):
        try:
            self.result = apply_preview(self.root, preview(self.root, remove=True)) if self.remove else connect(self.root)
        except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
            self.error = str(error)


def connection_request(root):
    args = command("--connect", root)
    quote = lambda value: "'" + str(value).replace("'", "''") + "'"
    invocation = "& " + " ".join(quote(value) for value in args)
    return ("请帮我连接本机的 Codex 任务侧窗。请执行以下 PowerShell 命令，它会自动预检、备份并仅添加任务侧窗的本地配置：\n\n"
            + invocation + "\n\n请根据命令结果检查接入是否就绪；如失败请说明原因。不要改动其他配置、扩大权限、修改 Hook 信任列表或重启现有任务。"
            "如果还需 Codex 正常允许连接，请用普通语言告诉我下一步；真实收到任务前不要宣称自动同步成功。")


class IntegrationDialog(QDialog):
    def __init__(self, board):
        super().__init__(board)
        self.root = board.root
        self.board = board
        self.theme = board.theme
        self.plan = None
        self.job = None
        self.last_state = None
        self.setWindowTitle("Codex 接入设置")
        self.setStyleSheet(board.styleSheet())
        self.setPalette(palette(board.theme))
        self.setFixedSize(480, 292)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 18, 20, 16)
        layout.setSpacing(12)
        self.title = label()
        self.title.setObjectName("connectionTitle")
        layout.addWidget(self.title)
        self.description = label(wrap=True, muted=True)
        self.description.setMinimumHeight(38)
        layout.addWidget(self.description)
        row = QHBoxLayout()
        self.apply = button("连接 Codex", self.execute)
        self.apply.setProperty("primary", True)
        self.assist = button("让 Codex 帮我连接", self.copy_request)
        row.addWidget(self.apply)
        row.addWidget(self.assist)
        row.addStretch()
        layout.addLayout(row)
        self.note = label("自动检查 · 自动备份 · 保留其他设置", muted=True, wrap=True)
        self.note.setMinimumHeight(30)
        layout.addWidget(self.note)
        self.advanced_toggle = button("高级选项  ›", self.toggle_advanced, quiet=True)
        self.advanced_toggle.setCheckable(True)
        layout.addWidget(self.advanced_toggle, 0, Qt.AlignmentFlag.AlignLeft)
        self.advanced = QWidget()
        advanced = QVBoxLayout(self.advanced)
        advanced.setContentsMargins(0, 0, 0, 0)
        advanced.setSpacing(8)
        self.path = label(muted=True)
        self.path.setToolTip(str(codex_home() / "config.toml"))
        advanced.addWidget(self.path)
        tools = QHBoxLayout()
        self.summary = label("配置差异", heading=True)
        tools.addWidget(self.summary, 1)
        self.refresh_preview = button("刷新预览", self.inspect, quiet=True)
        tools.addWidget(self.refresh_preview)
        advanced.addLayout(tools)
        self.diff_tabs = QTabWidget()
        self.diff_tabs.setTabBar(FluentTabBar())
        self.changes = QTreeWidget()
        self.changes.setHeaderLabels(["配置项", "当前", "连接后"])
        self.changes.setRootIsDecorated(False)
        self.changes.setSelectionMode(QTreeWidget.SelectionMode.NoSelection)
        self.changes.header().setSectionResizeMode(QHeaderView.ResizeMode.Stretch)
        self.code = QTextBrowser()
        self.code.setOpenLinks(False)
        self.diff_tabs.addTab(self.changes, "前后对照")
        self.diff_tabs.addTab(self.code, "配置代码")
        advanced.addWidget(self.diff_tabs, 1)
        self.remove = button("断开连接…", self.disconnect)
        advanced.addWidget(self.remove, 0, Qt.AlignmentFlag.AlignLeft)
        layout.addWidget(self.advanced, 1)
        self.advanced.hide()
        footer = QHBoxLayout()
        footer.addStretch()
        self.close_button = button("完成", self.close)
        footer.addWidget(self.close_button)
        layout.addLayout(footer)
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.refresh_state)
        self.timer.start(1000)
        self.refresh_state()

    def refresh_state(self):
        if self.job:
            return
        state = connection_state(self.root)
        key = (state["state"], state["error"], bool(self.board.tasks))
        if key == self.last_state:
            return
        self.last_state = key
        ready = state["state"] == "ready"
        title, description = {
            "missing": ("连接 Codex", "连接后，在这里查看任务进度和卡点。"),
            "partial": ("连接需要修复", "部分接入设置缺失，点击修复即可。"),
            "error": ("暂时无法连接", state["error"]),
            "ready": ("已收到 Codex 任务" if self.board.tasks else "接入已就绪，等待首次同步", "已接收到任务数据，后续更新将在看板显示。" if self.board.tasks else "在 Codex 新会话中允许连接后，任务会自动出现在看板。现有任务可继续运行。")
        }[state["state"]]
        self.title.setText(title)
        self.title.setStyleSheet("color:" + self.board.theme["success" if ready and self.board.tasks else "text"])
        self.description.setText(description)
        self.apply.setText("检查连接" if ready else "修复连接" if state["state"] == "partial" else "连接 Codex")
        self.remove.setEnabled(state["state"] in {"ready", "partial"})

    def toggle_advanced(self):
        expanded = self.advanced_toggle.isChecked()
        self.advanced.setVisible(expanded)
        self.advanced_toggle.setText("高级选项  ⌄" if expanded else "高级选项  ›")
        self.setFixedSize(760 if expanded else 480, 620 if expanded else 292)
        bounds = self.screen().availableGeometry()
        self.move(max(bounds.left(), min(self.x(), bounds.right() - self.width())), max(bounds.top(), min(self.y(), bounds.bottom() - self.height())))
        if expanded:
            self.inspect()

    def inspect(self):
        if self.job:
            return
        self.changes.clear()
        try:
            self.plan = preview(self.root)
            self.path.setText(self.path.fontMetrics().elidedText(self.plan["path"], Qt.TextElideMode.ElideMiddle, 710))
            changes = self.plan["changes"]
            self.summary.setText(f"待添加 {len(changes)} 项" if changes else "无需改动 · 接入设置完整")
            blocks = []
            for change in changes:
                item = QTreeWidgetItem([change["title"], change["before"], change["after"]])
                item.setToolTip(0, change["key"])
                item.setForeground(2, QColor(self.board.theme["success"]))
                self.changes.addTopLevelItem(item)
                lines = []
                for line in change["diff"].splitlines():
                    if line.startswith(("---", "+++", "@@")):
                        continue
                    tone = self.board.theme["success_bg"] if line.startswith("+") and not line.startswith("+++") else self.board.theme["danger_bg"] if line.startswith("-") and not line.startswith("---") else self.board.theme["layer"]
                    lines.append('<div style="background:' + tone + ';white-space:pre-wrap;font-family:Consolas;">' + html.escape(line) + '</div>')
                blocks.append("<h4>" + html.escape(change["key"]) + "</h4>" + "".join(lines))
            self.code.setHtml('<p style="color:' + self.board.theme["muted"] + '">绿色 + 新增 · 红色 − 移除 · 仅显示任务侧窗配置</p>' + ("".join(blocks) or "<p>无需修改配置。</p>"))
        except (OSError, ValueError, KeyError, TypeError) as error:
            self.plan = None
            self.summary.setText("无法生成预览")
            self.code.setPlainText(str(error))

    def execute(self):
        self.start_job()

    def start_job(self, remove=False):
        if self.job:
            return
        for control in (self.apply, self.assist, self.remove, self.refresh_preview, self.advanced_toggle, self.close_button):
            control.setEnabled(False)
        self.title.setText("正在断开连接…" if remove else "正在连接 Codex…")
        self.description.setText("正在自动检查并保存设置，请稍候。")
        self.note.setText("连接过程中无需核对配置代码。")
        self.job = ConnectionJob(self.root, remove)
        self.job.finished.connect(self.finish_job)
        self.job.start()

    def finish_job(self):
        job, self.job = self.job, None
        for control in (self.apply, self.assist, self.remove, self.refresh_preview, self.advanced_toggle, self.close_button):
            control.setEnabled(True)
        self.last_state = None
        self.refresh_state()
        if job.error:
            self.title.setText("连接失败" if not job.remove else "断开失败")
            self.description.setText(job.error)
            self.apply.setText("重试连接")
            self.note.setText("请重试，或让 Codex 帮忙检查。")
        else:
            self.note.setText("已断开；任务数据已保留。" if job.remove else "检查通过 · 原配置已自动保护")
        if not self.advanced.isHidden():
            self.inspect()
        self.board.connection_checked_at = 0
        self.board.refresh()
        job.deleteLater()

    def disconnect(self):
        if QMessageBox.question(self, "断开 Codex", "断开后停止新会话的自动上报。已有任务数据和其他设置会保留。", QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No, QMessageBox.StandardButton.No) != QMessageBox.StandardButton.Yes:
            return
        self.start_job(remove=True)

    def copy_request(self):
        QApplication.clipboard().setText(connection_request(self.root))
        self.note.setText("请求已复制，粘贴到 Codex 即可让它帮你连接。")

    def reject(self):
        if not self.job:
            super().reject()

    def closeEvent(self, event):
        if self.job:
            event.ignore()
        else:
            super().closeEvent(event)


class Board(QWidget):
    def __init__(self, root):
        super().__init__()
        self.root = Path(root)
        self.options = preferences.load(self.root)
        self.theme = PALETTES[self.options["theme"]]
        self.colors = colors(self.theme)
        self.notices = ChangeDetector()
        self.toast_stack = ToastStack(self)
        self.settings_path = self.root / "window.json"
        try:
            self.settings = json.loads(self.settings_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            self.settings = {}
        self.pinned = self.settings.get("pinned", True)
        self.collapsed = False
        self.expanded_height = 570
        self.selected = self.settings.get("selected", "")
        self.tasks = []
        self.render_key = None
        self.connection_checked_at = 0
        self.connection = {"state": "missing", "error": ""}
        self.store = None
        self.duration_labels = []
        self.completed_window = None
        self.completed_steps_button = None
        self.setObjectName("board")
        self.setWindowTitle("Codex · 任务侧窗")
        self.setWindowIcon(icon())
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | (Qt.WindowType.Window if self.options["show_taskbar"] else Qt.WindowType.Tool) | (Qt.WindowType.WindowStaysOnTopHint if self.pinned else Qt.WindowType.Widget))
        self.setFixedSize(392, 570)
        self.setStyleSheet(stylesheet(self.theme))
        self.setPalette(palette(self.theme))
        outer = QVBoxLayout(self)
        outer.setContentsMargins(1, 1, 1, 1)
        outer.setSpacing(0)
        header = DragHeader()
        header.setObjectName("header")
        header.setFixedHeight(42)
        row = QHBoxLayout(header)
        row.setContentsMargins(12, 0, 7, 0)
        row.setSpacing(5)
        app_icon = label()
        app_icon.setPixmap(icon().pixmap(22, 22))
        app_icon.setFixedWidth(24)
        app_icon.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        row.addWidget(app_icon)
        title = label("Codex · 任务侧窗", heading=True)
        title.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        row.addWidget(title, 1)
        self.pin = button("置顶", self.toggle_pin, quiet=True)
        self.pin.setCheckable(True)
        self.pin.setChecked(self.pinned)
        self.pin.setToolTip("系统级置顶")
        self.fold = button("收起", self.toggle_collapse, quiet=True)
        menu = button("···", self.menu, quiet=True, width=26)
        menu.setAccessibleName("更多操作")
        hide = button("×", self.hide_to_tray, quiet=True, width=26)
        hide.setAccessibleName("隐藏到托盘")
        hide.setToolTip("隐藏到托盘；采集继续")
        for widget in [self.pin, self.fold, menu, hide]:
            row.addWidget(widget)
        outer.addWidget(header)
        self.body = QWidget()
        body = QVBoxLayout(self.body)
        body.setContentsMargins(0, 0, 0, 0)
        body.setSpacing(0)
        tools = QFrame()
        tools.setObjectName("taskTools")
        tools.setFixedHeight(35)
        status = QHBoxLayout(tools)
        status.setContentsMargins(12, 0, 12, 0)
        self.health = label("采集器连接中", muted=True)
        self.count = label("0 个任务", muted=True)
        status.addWidget(self.health, 1)
        status.addWidget(self.count)
        body.addWidget(tools)
        self.list = QListWidget()
        self.list.setFixedHeight(160)
        self.list.setItemDelegate(TaskDelegate(self.list))
        self.list.setAccessibleName("任务列表")
        self.list.currentItemChanged.connect(self.select_task)
        self.list.setVerticalScrollMode(QListWidget.ScrollMode.ScrollPerPixel)
        body.addWidget(self.list)
        self.completed_tasks_button = CompletedEntry("已完成任务", 0, self.open_completed_tasks)
        self.completed_tasks_button.hide()
        body.addWidget(self.completed_tasks_button)
        detail = QVBoxLayout()
        detail.setContentsMargins(13, 10, 13, 8)
        detail.setSpacing(5)
        self.task_title = label("等待 Codex 任务接入", heading=True)
        self.task_title.setFixedHeight(20)
        self.task_title.ensurePolished()
        self.task_title.setToolTip("任务标题")
        self.task_meta = label("未接入", muted=True)
        self.task_elapsed = label(muted=True)
        summary = QHBoxLayout()
        summary.setContentsMargins(0, 0, 0, 0)
        summary.setSpacing(8)
        summary.addWidget(self.task_meta, 1)
        summary.addWidget(self.task_elapsed)
        self.task_current = label("", wrap=True)
        self.task_current.setMaximumHeight(34)
        self.progress = QProgressBar()
        self.progress.setRange(0, 100)
        self.progress.setTextVisible(False)
        detail.addWidget(self.task_title)
        detail.addLayout(summary)
        detail.addWidget(self.task_current)
        detail.addWidget(self.progress)
        body.addLayout(detail)
        self.tabs = QTabWidget()
        self.tabs.setTabBar(FluentTabBar())
        self.tabs.setAccessibleName("任务详情")
        self.panes = []
        for name in ["清单", "卡点", "证据", "动态"]:
            scroll = QScrollArea()
            scroll.setWidgetResizable(True)
            scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
            content = QWidget()
            pane = QVBoxLayout(content)
            pane.setContentsMargins(13, 8, 13, 8)
            pane.setSpacing(4)
            if name == "清单":
                pane.setSizeConstraint(QVBoxLayout.SizeConstraint.SetMinAndMaxSize)
            pane.addStretch()
            scroll.setWidget(content)
            self.tabs.addTab(scroll, name)
            self.panes.append(pane)
        self.tabs.setCurrentIndex(min(3, max(0, int(self.settings.get("tab", 0)))))
        self.tabs.currentChanged.connect(self.persist)
        body.addWidget(self.tabs, 1)
        footer_frame = QFrame()
        footer_frame.setObjectName("footer")
        footer_frame.setFixedHeight(32)
        footer = QHBoxLayout(footer_frame)
        footer.setContentsMargins(12, 0, 9, 0)
        self.updated = label("", muted=True)
        footer.addWidget(self.updated, 1)
        self.copy = button("复制会话 ID", self.copy_session, quiet=True)
        self.copy.setEnabled(False)
        footer.addWidget(self.copy)
        body.addWidget(footer_frame)
        outer.addWidget(self.body, 1)
        self.tray = QSystemTrayIcon(icon(), self)
        self.tray.setToolTip("Codex 任务侧窗")
        tray_menu = QMenu(self)
        tray_menu.setStyleSheet(self.styleSheet())
        self.tray_menu = tray_menu
        tray_menu.addAction("显示任务侧窗", self.restore)
        tray_menu.addAction("设置", self.open_settings)
        tray_menu.addAction("Codex 接入设置", self.integration)
        tray_menu.addSeparator()
        tray_menu.addAction("退出看板（采集继续）", self.quit_board)
        self.tray.setContextMenu(tray_menu)
        self.tray.activated.connect(lambda reason: self.restore() if reason in {QSystemTrayIcon.ActivationReason.Trigger, QSystemTrayIcon.ActivationReason.DoubleClick} else None)
        self.tray.show()
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.refresh)
        self.timer.start(700)
        self.place()
        self.refresh()

    def place(self):
        screen = QApplication.primaryScreen().availableGeometry()
        position = QPoint(int(self.settings.get("x", screen.right() - 412)), int(self.settings.get("y", screen.top() + 60)))
        target = QApplication.screenAt(position) or QApplication.primaryScreen()
        bounds = target.availableGeometry()
        self.move(max(bounds.left(), min(position.x(), bounds.right() - self.width())), max(bounds.top(), min(position.y(), bounds.bottom() - self.height())))

    def persist(self, *_):
        if not hasattr(self, "tabs"):
            return
        try:
            atomic_json(self.settings_path, {"x": self.x(), "y": self.y(), "pinned": self.pinned, "selected": self.selected, "tab": self.tabs.currentIndex()})
        except OSError:
            self.health.setText("窗口设置未保存")

    def moveEvent(self, event):
        super().moveEvent(event)
        self.persist()

    def toggle_pin(self):
        self.pinned = self.pin.isChecked()
        self.setWindowFlag(Qt.WindowType.WindowStaysOnTopHint, self.pinned)
        self.show()
        self.persist()

    def toggle_collapse(self):
        self.collapsed = not self.collapsed
        self.body.setVisible(not self.collapsed)
        self.setFixedHeight(44 if self.collapsed else self.expanded_height)
        self.fold.setText("展开" if self.collapsed else "收起")

    def hide_to_tray(self):
        self.persist()
        if self.completed_window:
            self.completed_window.close()
        if QSystemTrayIcon.isSystemTrayAvailable():
            self.hide()
        else:
            self.showMinimized()

    def restore(self):
        self.showNormal()
        self.raise_()
        self.activateWindow()

    def quit_board(self):
        self.persist()
        self.tray.hide()
        self.toast_stack.clear()
        if self.completed_window:
            self.completed_window.close()
        if self.store:
            self.store.close()
        QApplication.quit()

    def closeEvent(self, event):
        event.ignore()
        self.hide_to_tray()

    def menu(self):
        menu = QMenu(self)
        menu.addAction("设置", self.open_settings)
        menu.addAction("Codex 接入设置", self.integration)
        menu.addAction("打开本地数据目录", lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.root))))
        menu.addSeparator()
        menu.addAction("退出看板（采集继续）", self.quit_board)
        menu.exec(self.mapToGlobal(QPoint(self.width() - 220, 42)))

    def integration(self):
        IntegrationDialog(self).exec()

    def open_settings(self):
        SettingsDialog(self).exec()

    def apply_options(self, value):
        self.options = preferences.save(self.root, value)
        self.theme = PALETTES[self.options["theme"]]
        self.colors = colors(self.theme)
        self.setStyleSheet(stylesheet(self.theme))
        self.setPalette(palette(self.theme))
        self.tray_menu.setStyleSheet(self.styleSheet())
        visible = self.isVisible()
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | (Qt.WindowType.Window if self.options["show_taskbar"] else Qt.WindowType.Tool) | (Qt.WindowType.WindowStaysOnTopHint if self.pinned else Qt.WindowType.Widget))
        if visible:
            self.show()
        if not self.options["notifications"]:
            self.toast_stack.clear()
        for toast in self.toast_stack.items:
            toast.apply_theme()
        if self.completed_window:
            self.completed_window.apply_theme()
        self.render_key = None
        self.refresh()

    def open_notice(self, notice):
        task = next((task for task in self.tasks if task["id"] == notice["task_id"]), None)
        if task:
            if task["status"] == "completed":
                self.restore()
                self.show_completed(selected=task["id"], tab=notice["tab"])
                return
            self.selected = task["id"]
            self.render_tasks()
            self.list.scrollToItem(self.list.currentItem())
            self.tabs.setCurrentIndex(notice["tab"])
            self.persist()
        self.restore()

    def copy_session(self):
        task = next((t for t in self.tasks if t["id"] == self.selected), None)
        if task:
            QApplication.clipboard().setText(task["session_id"])

    def select_task(self, current, _):
        if current and current.data(Qt.ItemDataRole.UserRole):
            self.selected = current.data(Qt.ItemDataRole.UserRole)
            self.render_detail()
            self.persist()

    def open_completed_tasks(self):
        self.show_completed()

    def open_completed_steps(self):
        self.show_completed(task_id=self.selected)

    def show_completed(self, task_id=None, selected=None, tab=0):
        if not self.completed_window:
            self.completed_window = CompletedDialog(self)
        self.completed_window.set_scope(task_id, selected, tab)
        self.completed_window.show()
        self.completed_window.raise_()
        self.completed_window.activateWindow()

    def render_tasks(self):
        ongoing = [task for task in self.tasks if task["status"] != "completed"]
        completed = [task for task in self.tasks if task["status"] == "completed"]
        if not any(task["id"] == self.selected for task in ongoing):
            self.selected = ongoing[0]["id"] if ongoing else ""
        self.list.blockSignals(True)
        scroll_position = self.list.verticalScrollBar().value()
        self.list.clear()
        for task in ongoing:
            count = sum(step["status"] == "completed" for step in task["plan"])
            project = Path(task["cwd"]).name or task["host_id"]
            title = task["title"] or "未汇报 · " + task["session_id"][-8:]
            item = QListWidgetItem(title + "\n" + STATUS[task["status"]] + " · " + project[:25] + f" · {count}/{len(task['plan'])}")
            item.setToolTip(title + "\n" + task["cwd"])
            item.setData(Qt.ItemDataRole.UserRole, task["id"])
            item.setData(Qt.ItemDataRole.UserRole + 1, task)
            item.setSizeHint(QSize(350, 52))
            self.list.addItem(item)
            if task["id"] == self.selected:
                self.list.setCurrentItem(item)
        self.completed_tasks_button.set_count(len(completed))
        self.completed_tasks_button.setVisible(bool(completed))
        self.list.setFixedHeight(min(128 if completed else 160, len(ongoing) * 52 + 2))
        self.list.verticalScrollBar().setValue(scroll_position)
        self.list.blockSignals(False)
        self.render_detail()

    def refresh(self):
        request = self.root / "show.request"
        if request.exists():
            request.unlink(missing_ok=True)
            self.restore()
        try:
            health = json.loads((self.root / "health.json").read_text(encoding="utf-8"))
            healthy = 0 <= time.time() - health["at"] < 5 and health.get("status") == "ok"
            self.health.setText("● 本地服务正常" if healthy else "● 采集离线 · 最近快照")
            self.health.setToolTip("本机采集服务状态；Codex 是否已同步以任务数据为准。")
            if health.get("rejected"):
                self.health.setText(self.health.text() + " · 异常 " + str(health["rejected"]))
            self.health.setStyleSheet("color:" + self.theme["success" if healthy else "warning"])
        except (OSError, ValueError, TypeError, KeyError):
            self.health.setText("● 本地服务连接中")
            self.health.setStyleSheet("color:" + self.theme["muted"])
        try:
            if not self.store:
                self.store = Store(self.root, readonly=True)
            self.tasks = self.store.tasks()
            changes = self.notices.update(self.tasks)
            if self.options["notifications"]:
                for change in changes:
                    self.toast_stack.show_notice(change)
        except (OSError, ValueError, sqlite3.Error):
            self.health.setText("● 数据暂不可读 · 保留最近快照")
        opened = sum(sum(b["open"] for b in t["blockers"]) for t in self.tasks)
        self.count.setText(f"{len(self.tasks)} 任务 · {opened} 卡点")
        if time.monotonic() - self.connection_checked_at > 2:
            self.connection = connection_state(self.root)
            self.connection_checked_at = time.monotonic()
        key = json.dumps([self.tasks, self.connection], ensure_ascii=False)
        if key != self.render_key:
            self.render_key = key
            self.render_tasks()
        if self.completed_window and self.completed_window.isVisible():
            self.completed_window.refresh()
        task = next((t for t in self.tasks if t["id"] == self.selected), None)
        self.updated.setText(age(task["updated_at"]) if task else "已完成任务可查看" if self.tasks else "尚无任务数据")
        self.refresh_durations()

    def duration_label(self, timer, prefix='耗时'):
        widget = label(duration_text(timer, prefix), muted=True)
        widget.setToolTip(timing_tooltip(timer))
        self.duration_labels.append((widget, timer, prefix))
        return widget

    def refresh_durations(self):
        for widget, timer, prefix in self.duration_labels:
            value = duration_text(timer, prefix)
            if widget.text() != value:
                widget.setText(value)
            widget.setToolTip(timing_tooltip(timer))
        self.list.viewport().update()

    def clear_pane(self, pane):
        while pane.count():
            item = pane.takeAt(0)
            if item.widget():
                item.widget().deleteLater()

    def card(self, pane, title, lines=(), tone=None):
        card = QFrame()
        card.setProperty("card", True)
        layout = QVBoxLayout(card)
        layout.setContentsMargins(0, 0, 0, 8)
        layout.setSpacing(4)
        heading = label(title, wrap=True)
        if tone:
            heading.setStyleSheet("color:" + tone)
        heading.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        layout.addWidget(heading)
        for value in lines:
            if value:
                child = label(value, muted=True, wrap=True)
                child.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
                layout.addWidget(child)
        pane.addWidget(card)
        return layout

    def step_row(self, pane, step):
        card = QFrame()
        card.setProperty("card", True)
        row = QHBoxLayout(card)
        row.setContentsMargins(0, 7, 0, 7)
        row.setSpacing(8)
        tone = {"pending": self.theme["muted"], "inProgress": self.theme["accent"], "completed": self.theme["success"], "blocked": self.theme["warning"]}[step["status"]]
        marker = label()
        marker.setPixmap(status_marker(step["status"], tone))
        marker.setAccessibleName(STEP[step["status"]])
        marker.setFixedWidth(16)
        marker.setStyleSheet("color:" + tone)
        row.addWidget(marker, 0, Qt.AlignmentFlag.AlignTop)
        content = QVBoxLayout()
        content.setSpacing(3)
        title = label(step["title"], wrap=True)
        title.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        content.addWidget(title)
        if step["note"]:
            content.addWidget(label(step["note"], muted=True, wrap=True))
        row.addLayout(content, 1)
        row.setAlignment(content, Qt.AlignmentFlag.AlignTop)
        side = QVBoxLayout()
        side.setSpacing(3)
        status = label(STEP[step["status"]], muted=True)
        status.setStyleSheet("color:" + tone)
        side.addWidget(status, 0, Qt.AlignmentFlag.AlignRight)
        timer = self.duration_label(step.get('timing'))
        side.addWidget(timer, 0, Qt.AlignmentFlag.AlignRight)
        row.addLayout(side)
        row.setAlignment(side, Qt.AlignmentFlag.AlignTop)
        pane.addWidget(card)

    def render_detail(self):
        self.duration_labels = []
        self.completed_steps_button = None
        for pane in self.panes:
            self.clear_pane(pane)
        task = next((t for t in self.tasks if t["id"] == self.selected), None)
        self.copy.setEnabled(task is not None)
        self.list.setVisible(self.list.count() > 0)
        self.tabs.setVisible(task is not None or not self.tasks)
        self.expanded_height = 240 if self.tasks and not task else 570
        if not self.collapsed:
            self.setFixedHeight(self.expanded_height)
        self.tabs.tabBar().setVisible(task is not None)
        self.progress.setVisible(task is not None and bool(task["plan"]))
        self.task_current.setVisible(task is not None)
        self.task_elapsed.setVisible(task is not None)
        if not task and self.tasks:
            self.task_title.setText("全部任务已完成")
            self.task_meta.setText("已完成 " + str(len(self.tasks)))
            self.task_meta.setStyleSheet("color:" + self.theme["success"])
            self.tabs.setCurrentIndex(0)
        elif not task:
            state = self.connection["state"]
            ready = state == "ready"
            self.task_title.setText("等待首次任务同步" if ready else "连接你的 Codex")
            self.task_meta.setText("接入已就绪" if ready else "连接需要修复" if state in {"partial", "error"} else "尚未连接")
            self.task_meta.setStyleSheet("color:" + self.theme["muted"])
            self.task_current.setText("")
            self.progress.setValue(0)
            self.tabs.setCurrentIndex(0)
            self.panes[0].addWidget(label("在 Codex 新会话中允许连接后，进度和卡点会显示在这里。" if ready else "连接后自动汇集任务进度、卡点和验收证据。", muted=True, wrap=True))
            action = button("查看连接状态" if ready else "连接 Codex", self.integration)
            action.setProperty("primary", True)
            self.panes[0].addWidget(action, 0, Qt.AlignmentFlag.AlignLeft)
        else:
            self.duration_labels.append((self.task_elapsed, task.get('timing'), '耗时'))
            self.refresh_durations()
            self.task_title.setText(self.task_title.fontMetrics().elidedText(task["title"] or "等待结构化汇报", Qt.TextElideMode.ElideRight, 362))
            self.task_title.setToolTip(task["title"])
            completed = sum(x["status"] == "completed" for x in task["plan"])
            total = len(task["plan"])
            self.task_meta.setText(STATUS[task["status"]] + "  ·  Codex " + RUNTIME[task["runtime"]] + (f"  ·  {completed}/{total}" if total else "  ·  未汇报清单"))
            self.task_meta.setStyleSheet("color:" + self.colors[task["status"]])
            self.task_current.setText(task["current_step"] or ("尚未收到结构化进度" if not task["plan"] else ""))
            self.task_current.setToolTip(self.task_current.text())
            self.progress.setValue(round(completed * 100 / total) if total else 0)
            opened = sum(b["open"] for b in task["blockers"])
            self.tabs.setTabText(1, "卡点" + (f" {opened}" if opened else ""))
            for step in (step for step in task["plan"] if step["status"] != "completed"):
                self.step_row(self.panes[0], step)
            finished = [step for step in task["plan"] if step["status"] == "completed"]
            if finished:
                self.completed_steps_button = CompletedEntry("已完成清单", len(finished), self.open_completed_steps)
                self.panes[0].addWidget(self.completed_steps_button)
            if not task["plan"]:
                self.panes[0].addWidget(label("尚未汇报清单", muted=True))
            for blocker in sorted(task["blockers"], key=lambda b: not b["open"]):
                layout = self.card(self.panes[1], ("!  " if blocker["open"] else "✓  ") + blocker["title"], [("原因：" + blocker["reason"]) if blocker["reason"] else "", ("影响：" + blocker["impact"]) if blocker["impact"] else "", "处理方：" + blocker["owner"], ("解除条件：" + blocker["resolve_when"]) if blocker["resolve_when"] else "", "已解除" if not blocker["open"] else ""], self.theme["warning" if blocker["open"] else "success"])
                layout.insertWidget(1, self.duration_label(blocker.get('timing'), '持续' if blocker['open'] else '累计'))
            if not task["blockers"]:
                self.panes[1].addWidget(label("暂无已汇报卡点", muted=True))
            for item in task["evidence"]:
                layout = self.card(self.panes[2], item["label"], [item["reference"]])
                url = safe_reference(item["reference"])
                if url:
                    layout.addWidget(button("打开证据", lambda checked=False, target=url: QDesktopServices.openUrl(target)))
            if not task["evidence"]:
                self.panes[2].addWidget(label("暂无验收证据", muted=True))
            for entry in task["history"]:
                self.card(self.panes[3], time.strftime("%H:%M:%S", time.localtime(entry["at"])) + "  " + KIND.get(entry["kind"], entry["kind"]), [entry["tool"], STATUS.get(entry["status"], ""), "子任务 " + entry["agent_id"][:12] if entry["agent_id"] else ""])
        for pane in self.panes:
            pane.addStretch()
