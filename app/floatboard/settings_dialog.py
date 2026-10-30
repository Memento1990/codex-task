"""普通用户只需切换开关并保存。"""
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QCheckBox, QComboBox, QDialog, QFormLayout, QHBoxLayout, QLabel, QPushButton, QSpinBox, QVBoxLayout

from .themes import palette


class SettingsDialog(QDialog):
    def __init__(self, board):
        super().__init__(board)
        self.board = board
        self.setWindowTitle("看板设置")
        self.setStyleSheet(board.styleSheet())
        self.setPalette(palette(board.theme))
        self.setFixedWidth(360)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(18, 16, 18, 16)
        layout.setSpacing(12)
        form = QFormLayout()
        form.setVerticalSpacing(12)
        self.auto_start = QCheckBox("开机自启")
        self.taskbar = QCheckBox("在任务栏中显示")
        self.notifications = QCheckBox("消息浮窗通知")
        self.auto_start.setChecked(board.options["auto_start"])
        self.taskbar.setChecked(board.options["show_taskbar"])
        self.notifications.setChecked(board.options["notifications"])
        form.addRow(self.auto_start)
        form.addRow(self.taskbar)
        self.theme = QComboBox()
        self.theme.addItem("白色", "light")
        self.theme.addItem("黑色", "dark")
        self.theme.setCurrentIndex(self.theme.findData(board.options["theme"]))
        form.addRow("主题", self.theme)
        form.addRow(self.notifications)
        self.seconds = QSpinBox()
        self.seconds.setRange(1, 30)
        self.seconds.setSuffix(" 秒")
        self.seconds.setValue(board.options["notification_seconds"])
        self.seconds.setEnabled(self.notifications.isChecked())
        self.notifications.toggled.connect(self.seconds.setEnabled)
        form.addRow("通知停留", self.seconds)
        layout.addLayout(form)
        self.note = QLabel()
        self.note.setTextFormat(Qt.TextFormat.PlainText)
        self.note.setWordWrap(True)
        self.note.setProperty("muted", True)
        layout.addWidget(self.note)
        actions = QHBoxLayout()
        self.test = QPushButton("测试通知")
        self.test.clicked.connect(self.test_notice)
        actions.addWidget(self.test)
        actions.addStretch()
        cancel = QPushButton("取消")
        cancel.clicked.connect(self.reject)
        actions.addWidget(cancel)
        self.save_button = QPushButton("保存")
        self.save_button.setProperty("primary", True)
        self.save_button.clicked.connect(self.save)
        actions.addWidget(self.save_button)
        layout.addLayout(actions)

    def test_notice(self):
        if not self.notifications.isChecked():
            self.note.setText("请先开启消息浮窗通知")
            return
        self.board.toast_stack.show_notice(dict(task_id="settings-preview", title="消息通知预览", lines=["任务或清单变化时，在这里提醒。"], tab=0))
        toast = self.board.toast_stack.items[-1]
        toast.remaining = self.seconds.value() * 1000
        toast.timer.start(toast.remaining)

    def save(self):
        try:
            self.board.apply_options(dict(auto_start=self.auto_start.isChecked(), show_taskbar=self.taskbar.isChecked(), theme=self.theme.currentData(), notifications=self.notifications.isChecked(), notification_seconds=self.seconds.value()))
        except (OSError, ValueError) as error:
            self.note.setText("保存失败：" + str(error))
            return
        self.accept()
