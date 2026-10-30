"""接入状态的 Windows Qt 视觉验收；仅使用隔离配置与数据。"""
import os
os.environ.setdefault('QT_QPA_PLATFORM', 'windows')
import argparse
import json
from pathlib import Path
import sys
import time
import uuid
sys.path.insert(0,str(Path(__file__).parents[1]))
from PySide6.QtCore import QCoreApplication, QEvent
from PySide6.QtWidgets import QApplication
from floatboard.core import Store, atomic_json
from floatboard.gui import Board, IntegrationDialog

if __name__ == '__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--interactive',action='store_true')
    args=parser.parse_args()
    output=Path(__file__).parents[1]/'docs/evidence/FEAT-003'
    root=output/('connect-visual-'+uuid.uuid4().hex[:8]); root.mkdir(parents=True)
    home=root/'codex';home.mkdir()
    (home/'config.toml').write_text('# 隔离验收配置\nmodel="fixture"\n\n[other]\napi_key="fixture-private-value"\n',encoding='utf-8')
    os.environ['CODEX_HOME']=str(home)
    store=Store(root)
    atomic_json(root/'health.json',dict(at=time.time(),status='ok'))
    app=QApplication([])
    board=Board(root);board.setWindowTitle('任务侧窗 · FEAT-003 隔离验收');board.timer.stop();board.show()
    dialog=IntegrationDialog(board); dialog.timer.stop()
    checks={}
    def capture(widget,name):
        QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete)
        app.processEvents()
        assert widget.grab().save(str(output/name))
    def wait_job():
        deadline=time.monotonic()+15
        while dialog.job and time.monotonic()<deadline:
            app.processEvents();time.sleep(.005)
        assert dialog.job is None
    try:
        capture(board,'connect-empty.png')
        dialog.show();capture(dialog,'connect-simple.png')
        checks['advanced_collapsed']=dialog.advanced.isHidden()
        dialog.advanced_toggle.setChecked(True);dialog.toggle_advanced();capture(dialog,'connect-advanced.png')
        checks['ten_items']=dialog.changes.topLevelItemCount()==10
        dialog.diff_tabs.setCurrentIndex(1);capture(dialog,'connect-code.png')
        checks['scoped_diff_no_unrelated_secret']='fixture-private-value' not in dialog.code.toPlainText()
        dialog.advanced_toggle.setChecked(False);dialog.toggle_advanced()
        dialog.execute();wait_job();capture(dialog,'connect-ready.png')
        checks['ready_not_false_connected']=dialog.title.text()=='接入已就绪，等待首次同步'
        capture(board,'connect-empty-ready.png')
        checks['empty_list_hidden']=board.list.isHidden() and board.progress.isHidden()
        (output/'connection-visual.json').write_text(json.dumps(dict(qt_platform=app.platformName(),data_dir=str(root),checks=checks),ensure_ascii=False,indent=2),encoding='utf-8')
        print(json.dumps(dict(data_dir=str(root),checks=checks),ensure_ascii=False),flush=True)
        if args.interactive:
            dialog.close();board.timer.start(700);app.exec()
        if not all(checks.values()):raise SystemExit(1)
    finally:
        dialog.close();board.hide();board.tray.hide()
        if board.store:board.store.close()
        store.close()
