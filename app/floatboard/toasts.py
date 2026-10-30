"""右下角有限叠放的消息浮窗，不激活当前工作窗口。"""
from PySide6.QtCore import QObject, Qt, QTimer
from PySide6.QtWidgets import QApplication, QFrame, QHBoxLayout, QLabel, QPushButton, QVBoxLayout

from .themes import palette, stylesheet

MAX_TOASTS = 4
TOAST_WIDTH = 344
GAP = 8
MARGIN = 16


class Toast(QFrame):
    def __init__(self, stack, notice):
        super().__init__(None, Qt.WindowType.Tool | Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint | Qt.WindowType.WindowDoesNotAcceptFocus)
        self.stack = stack
        self.setObjectName("toast")
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
        self.setFixedWidth(TOAST_WIDTH)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(12, 10, 10, 10)
        layout.setSpacing(4)
        header = QHBoxLayout()
        header.setSpacing(6)
        self.title = QLabel()
        self.title.setTextFormat(Qt.TextFormat.PlainText)
        self.title.setProperty("heading", True)
        self.title.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        header.addWidget(self.title, 1)
        self.dismiss = QPushButton("×")
        self.dismiss.setProperty("quiet", True)
        self.dismiss.setFixedSize(24, 24)
        self.dismiss.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.dismiss.setAccessibleName("关闭通知")
        self.dismiss.clicked.connect(lambda: stack.remove(self))
        header.addWidget(self.dismiss)
        layout.addLayout(header)
        self.body = QLabel()
        self.body.setTextFormat(Qt.TextFormat.PlainText)
        self.body.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        layout.addWidget(self.body)
        self.timer = QTimer(self)
        self.timer.setSingleShot(True)
        self.timer.timeout.connect(lambda: stack.remove(self))
        self.hovered = False
        self.remaining = 0
        self.update_notice(notice)

    def apply_theme(self):
        theme = self.stack.board.theme
        self.setPalette(palette(theme))
        self.setStyleSheet(stylesheet(theme) + "QFrame#toast {background:" + theme["surface"] + ";border:1px solid " + theme["stroke"] + ";border-radius:7px;}")

    def update_notice(self, notice):
        self.notice = notice
        self.apply_theme()
        self.title.setText(self.title.fontMetrics().elidedText(notice["title"], Qt.TextElideMode.ElideRight, TOAST_WIDTH - 70))
        metrics = self.body.fontMetrics()
        self.body.setText("\n".join(metrics.elidedText(line, Qt.TextElideMode.ElideRight, TOAST_WIDTH - 28) for line in notice["lines"]))
        self.setToolTip("点击查看任务")
        self.setFixedHeight(self.sizeHint().height())
        self.remaining = self.stack.board.options["notification_seconds"] * 1000
        self.timer.stop()
        if not self.hovered:
            self.timer.start(self.remaining)

    def enterEvent(self, event):
        self.hovered = True
        if self.timer.isActive():
            self.remaining = max(1, self.timer.remainingTime())
            self.timer.stop()
        super().enterEvent(event)

    def leaveEvent(self, event):
        self.hovered = False
        self.timer.start(self.remaining)
        super().leaveEvent(event)

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.stack.board.open_notice(self.notice)
            self.stack.remove(self)
        super().mouseReleaseEvent(event)


class ToastStack(QObject):
    def __init__(self, board):
        super().__init__(board)
        self.board = board
        self.items = []
        board.destroyed.connect(self.clear)

    def show_notice(self, notice):
        existing = next((item for item in self.items if item.notice["task_id"] == notice["task_id"]), None)
        if existing:
            existing.update_notice(notice)
            self.items.remove(existing)
            self.items.append(existing)
        else:
            self.items.append(Toast(self, notice))
        self.reposition()
        for item in self.items:
            item.show()

    def reposition(self):
        screen = QApplication.screenAt(self.board.frameGeometry().center()) or QApplication.primaryScreen()
        bounds = screen.availableGeometry()
        while len(self.items) > 1 and (len(self.items) > MAX_TOASTS or sum(item.height() + GAP for item in self.items) > bounds.height() - MARGIN * 2):
            self.remove(self.items[0], reposition=False)
        bottom = bounds.bottom() - MARGIN
        for item in reversed(self.items):
            item.move(max(bounds.left(), bounds.right() - item.width() - MARGIN), bottom - item.height() + 1)
            bottom -= item.height() + GAP

    def remove(self, item, reposition=True):
        if item in self.items:
            self.items.remove(item)
            item.timer.stop()
            item.hide()
            item.deleteLater()
            if reposition:
                self.reposition()

    def clear(self):
        for item in list(self.items):
            self.remove(item, reposition=False)
