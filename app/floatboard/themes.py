"""两个固定主题，共用同一套控件样式。"""
from string import Template
from PySide6.QtGui import QColor, QPalette

LIGHT = {"surface": "#ffffff", "canvas": "#f5f5f5", "layer": "#fafafa", "text": "#242424", "muted": "#616161", "stroke": "#d1d1d1", "divider": "#ebebeb", "accent": "#0f6cbd", "selected": "#ebf3fc", "hover": "#f0f0f0", "success": "#107c10", "success_bg": "#f1faf1", "warning": "#875100", "warning_bg": "#fff4ce", "track": "#e0e0e0", "scroll": "#c5c5c5"}
DARK = {"surface":"#202020", "canvas":"#191919", "layer":"#252525", "text":"#f2f2f2", "muted":"#b8b8b8", "stroke":"#525252", "divider":"#383838", "accent":"#75b6ed", "selected":"#193a54", "hover":"#303030", "success":"#7bdc8b", "success_bg":"#173a25", "warning":"#f2c96d", "warning_bg":"#423719", "track":"#454545", "scroll":"#707070"}
LIGHT["danger_bg"] = "#fde7e9"
DARK["danger_bg"] = "#4b2429"
PALETTES = dict(light=LIGHT,dark=DARK)
STYLE = Template("""
QWidget {font-family:'Segoe UI Variable','Segoe UI','Microsoft YaHei UI'; font-size:12px; color:$text;}
QDialog, QMessageBox {background:$surface;}
QWidget#board {background:$surface; border:1px solid $stroke; border-radius:9px;}
QFrame#header {background:$canvas; border-bottom:1px solid $track; border-top-left-radius:9px; border-top-right-radius:9px;}
QFrame#taskTools {background:$layer; border-bottom:1px solid $track;}
QFrame#footer {background:$canvas; border-top:1px solid $stroke;}
QLabel[muted=true] {color:$muted; font-size:12px;}
QLabel[heading=true] {font-weight:600; font-size:13px;}
QPushButton {background:$surface; border:1px solid $stroke; border-radius:4px; padding:4px 8px; min-height:18px;}
QPushButton:hover {background:$hover;} QPushButton:checked {background:$selected; color:$accent;}
QPushButton:focus {border-color:$accent;} QPushButton:disabled {color:$muted;}
QPushButton[quiet=true] {border:0; background:transparent;} QPushButton[quiet=true]:hover {background:$hover;}
QPushButton[primary=true] {background:$accent; color:$surface; border:0;}
QPushButton[primary=true]:disabled {background:$canvas; color:$muted; border:1px solid $stroke;}
QCheckBox {spacing:8px;}
QCheckBox::indicator:unchecked {background:$surface; border:1px solid $stroke; border-radius:3px; width:14px; height:14px;}
QSpinBox {border:1px solid $stroke; border-radius:4px; padding:3px 6px; background:$surface;}
QComboBox {border:1px solid $stroke; border-radius:4px; padding:3px 6px; background:$surface;}
QComboBox QAbstractItemView {background:$surface; color:$text; selection-background-color:$selected; selection-color:$accent;}
QListWidget {border:0; border-bottom:1px solid $track; background:$canvas; outline:none; padding:0;}
QTabWidget::pane {border:0; border-top:1px solid $divider; background:$surface;}
QTabBar::tab {padding:8px 10px; min-width:48px; margin:0 3px; color:$muted; border:0;}
QTabBar::tab:selected {color:$accent; font-weight:600;} QTabBar::tab:hover:!selected {background:$layer;}
QTabBar {qproperty-drawBase:0;}
QScrollArea {border:0; background:$surface;} QScrollArea>QWidget>QWidget {background:$surface;}
QScrollBar:vertical {width:5px; background:transparent; margin:2px;} QScrollBar::handle:vertical {background:$scroll; border-radius:2px; min-height:24px;}
QScrollBar::add-line:vertical,QScrollBar::sub-line:vertical {height:0px;} QScrollBar::add-page:vertical,QScrollBar::sub-page:vertical {background:transparent;}
QProgressBar {border:0; height:3px; max-height:3px; background:$track; border-radius:2px;} QProgressBar::chunk {background:$accent; border-radius:2px;}
QFrame[card=true] {border-bottom:1px solid $divider;}
QTreeWidget, QTextBrowser {border:1px solid $divider; border-radius:4px; background:$surface;}
QTreeWidget::item {height:30px; border-bottom:1px solid $divider; padding:2px 5px;}
QHeaderView::section {background:$canvas; border:0; border-bottom:1px solid $stroke; padding:7px 6px; font-weight:600;}
QLabel#connectionTitle {font-size:17px; font-weight:600;}
QPushButton[quiet=true]:checked {background:$selected; color:$accent;}
QPushButton[completedEntry=true] {background:$layer; border:1px solid transparent; border-top-color:$divider; border-radius:4px; padding:0; min-height:28px; max-height:28px;}
QPushButton[completedEntry=true]:hover {background:$hover;}
QPushButton[completedEntry=true]:focus {border:1px solid $accent;}
QLabel[completedCount=true] {background:$canvas; border-radius:6px; padding:0 5px;}
QLabel[completedAction=true] {color:$accent;}
QListWidget#completedHistory {background:$layer; border:1px solid $divider; border-radius:4px;}
QListWidget#completedHistory::item {padding:0 8px; border-bottom:1px solid $divider;}
QListWidget#completedHistory::item:selected {background:$selected; color:$accent;}
QListWidget#completedHistory::item:hover:!selected {background:$hover;}
QTextBrowser#completedContent {border:0; padding:4px 8px;}
QMenu {background:$surface; border:1px solid $stroke; padding:4px;} QMenu::item {padding:7px 12px;} QMenu::item:selected {background:$selected; color:$accent;}
""")

def colors(theme):
    return {key:theme[tone] for key,tone in dict(running="accent",blocked="warning",waiting="warning",completed="success",cancelled="muted",unreported="muted").items()}

def stylesheet(theme):
    return STYLE.substitute(theme)

def palette(theme):
    result=QPalette()
    for role,key in [(QPalette.ColorRole.Window,"surface"),(QPalette.ColorRole.WindowText,"text"),(QPalette.ColorRole.Base,"surface"),(QPalette.ColorRole.AlternateBase,"canvas"),(QPalette.ColorRole.Text,"text"),(QPalette.ColorRole.Button,"canvas"),(QPalette.ColorRole.ButtonText,"text"),(QPalette.ColorRole.Highlight,"selected"),(QPalette.ColorRole.HighlightedText,"text"),(QPalette.ColorRole.Accent,"accent"),(QPalette.ColorRole.ToolTipBase,"surface"),(QPalette.ColorRole.ToolTipText,"text")]:
        result.setColor(role,QColor(theme[key]))
    return result
