"""耗时的 Windows 原生 Qt 渲染，全部数据隔离。"""
import os
import argparse
os.environ.setdefault('QT_QPA_PLATFORM','windows')
from pathlib import Path
import json
import sys
import time
import uuid
sys.path.insert(0,str(Path(__file__).parents[1]))
from PySide6.QtCore import Qt,QCoreApplication,QEvent
from PySide6.QtWidgets import QApplication
from floatboard.core import Store,atomic_json
from floatboard.gui import Board

base=Path(__file__).parents[1]
parser=argparse.ArgumentParser()
parser.add_argument('--evidence-dir',default=str(base/'docs/evidence/FEAT-004'))
output=Path(parser.parse_args().evidence_dir).resolve()
root=output/('visual-data-'+uuid.uuid4().hex[:8]);root.mkdir(parents=True)
store=Store(root);now=time.time()
def send(session,at,rev,status,steps,blockers=()):
 store.ingest(dict(event_id=uuid.uuid4().hex,recorded_at=at,kind='progress',session_id=session,host_id='timing-fixture',cwd=str(base),report=dict(revision=rev,title={'run':'任务、清单与卡点耗时记录','done':'完成后冻结与重新打开验证','blocked':'报表导出联调 · 隔离演示'}[session],status=status,current_step='验证易读计时与原生窗口',plan=steps,blockers=list(blockers),evidence=[dict(label='隔离演示资料',reference='')])) )
steps=[dict(id='a',title='核对计时来源与等待时间口径',status='inProgress'),dict(id='b',title='实现任务、清单与卡点计时',status='pending'),dict(id='c',title='验证边界和 Windows 窗口布局',status='pending')]
send('run',now-200,0,'running',steps)
steps=[dict(id='a',title='核对计时来源与等待时间口径',status='completed'),dict(id='b',title='实现任务、清单与卡点计时',status='inProgress'),dict(id='c',title='验证边界和 Windows 窗口布局',status='pending')]
send('run',now-125,1,'running',steps)
send('done',now-3900,0,'running',[dict(id='x',title='执行无副作用验证',status='inProgress')])
send('done',now-61,1,'completed',[dict(id='x',title='执行无副作用验证',status='completed')])
b=[dict(id='auth',title='等待演示依赖准备（隔离资料）',reason='用于验证卡点持续计时。',impact='仅影响此演示任务。',owner='验收工具',resolve_when='演示依赖恢复',open=True)]
send('blocked',now-700,0,'blocked',[dict(id='dep',title='等待依赖后继续执行',status='blocked')],b)
atomic_json(root/'health.json',dict(at=now,status='ok'))
atomic_json(root/'window.json',dict(selected='timing-fixture:run',tab=0,pinned=True))
app=QApplication([]);board=Board(root);board.timer.stop();board.show()
checks={}
def capture(name,widget=None):
 QCoreApplication.sendPostedEvents(None,QEvent.Type.DeferredDelete);app.processEvents()
 (widget or board).grab().save(str(output/name))
def select(session):
 if session=='done':board.show_completed(selected='timing-fixture:done');return
 for i in range(board.list.count()):
  if (board.list.item(i).data(Qt.ItemDataRole.UserRole) or '').endswith(':'+session):board.list.setCurrentRow(i);break
try:
 capture('timing-running.png')
 checks['active_rows_and_completed_group_fit']=all(board.list.viewport().rect().contains(board.list.visualItemRect(board.list.item(i))) for i in range(board.list.count()))
 checks['task_elapsed_visible']='耗时 3 分' in board.task_elapsed.text()
 checks['step_pending_distinct']=any(w.text()=='未开始' for w,_,_ in board.duration_labels)
 checks['metadata_fits']=board.task_meta.fontMetrics().horizontalAdvance(board.task_meta.text())<=board.task_meta.width()
 select('blocked');board.tabs.setCurrentIndex(1);capture('timing-blocker.png')
 checks['blocker_live_duration']=any('持续 11 分' in w.text() for w,_,_ in board.duration_labels)
 select('done');board.completed_window.tabs.setCurrentIndex(0);capture('timing-completed.png',board.completed_window)
 checks['completed_duration_frozen']='耗时 1 小时 3 分'==board.completed_window.task_elapsed.text()
 record=dict(qt_platform=app.platformName(),data_dir=str(root),checks=checks,screenshots=['timing-running.png','timing-blocker.png','timing-completed.png'])
 (output/'timing-visual.json').write_text(json.dumps(record,ensure_ascii=False,indent=2),encoding='utf-8')
 print(json.dumps(record,ensure_ascii=False))
 if not all(checks.values()):raise SystemExit(1)
finally:
 board.hide();board.tray.hide()
 if board.completed_window:board.completed_window.close()
 if board.store:board.store.close()
 store.close()
