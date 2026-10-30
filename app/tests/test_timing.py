import json

import pytest

from floatboard.core import Store
from floatboard.timing import elapsed, human_duration, duration_text
from test_core import event, report


def submit(store, at, revision, **kwargs):
    store.ingest(event("progress", recorded_at=at, report=report(revision=revision, **kwargs)))
    return store.tasks()[0]


def test_task_step_blocker_freeze_resume_and_restart(tmp_path):
    store = Store(tmp_path)
    try:
        task = submit(store, 1000, 0)
        assert elapsed(task['timing'], 1010) == 10
        assert elapsed(task['plan'][0]['timing'], 1010) == 10
        task = submit(store, 1020, 1, status='blocked', plan=[dict(id='check', title='检查', status='blocked')], blockers=[dict(id='dep', title='依赖', open=True)])
        assert elapsed(task['blockers'][0]['timing'], 1040) == 20
        task = submit(store, 1050, 2, status='completed', plan=[dict(id='check', title='检查', status='completed')], blockers=[dict(id='dep', title='依赖', open=False)], evidence=[dict(label='通过')])
        assert elapsed(task['timing'], 2000) == 50
        assert elapsed(task['plan'][0]['timing'], 2000) == 50
        assert elapsed(task['blockers'][0]['timing'], 2000) == 30
        task = submit(store, 1100, 3, blockers=[dict(id='dep', title='依赖', open=True)])
        assert elapsed(task['timing'], 1110) == 60
        assert elapsed(task['blockers'][0]['timing'], 1110) == 40
        reader = Store(tmp_path, readonly=True)
        try:
            assert reader.tasks()[0]['timing'] == task['timing']
            assert reader.tasks()[0]['plan'][0]['timing'] == task['plan'][0]['timing']
            assert reader.tasks()[0]['blockers'][0]['timing'] == task['blockers'][0]['timing']
        finally:
            reader.close()
        # 计时来自只读回放，旧采集器写出的数据库投影仍保持原协议。
        raw = json.loads(store.db.execute('select payload from tasks').fetchone()[0])
        assert 'timing' not in raw and 'timing' not in raw['plan'][0]
    finally:
        store.close()


def test_pending_unknown_start_and_stale_revision(tmp_path):
    store = Store(tmp_path)
    try:
        task = submit(store, 1000, 0, plan=[dict(id='check', title='检查', status='pending')])
        assert elapsed(task['plan'][0]['timing'], 1010) == 0
        task = submit(store, 1020, 1, status='completed', plan=[dict(id='check', title='检查', status='completed')], evidence=[dict(label='通过')])
        assert elapsed(task['plan'][0]['timing'], 1030) is None
        task = submit(store, 1040, 0)
        assert elapsed(task['timing'], 2000) == 20
        assert task['plan'][0]['timing']['partial']
    finally:
        store.close()


def test_removed_blocker_stops_then_reopens_and_new_task_resets(tmp_path):
    store = Store(tmp_path)
    try:
        submit(store, 1000, 0, blockers=[dict(id='dep', title='依赖', open=True)])
        submit(store, 1020, 1)
        task = submit(store, 1100, 2, blockers=[dict(id='dep', title='依赖', open=True)])
        assert elapsed(task['blockers'][0]['timing'], 1110) == 30
        submit(store, 1120, 3, status='completed', plan=[dict(id='check', title='检查', status='completed')], evidence=[dict(label='通过')])
        task = submit(store, 1200, 4, title='新需求')
        assert elapsed(task['timing'], 1210) == 10
        assert elapsed(task['plan'][0]['timing'], 1210) == 10
    finally:
        store.close()


def test_late_time_and_child_do_not_change_parent_timing(tmp_path):
    store = Store(tmp_path)
    try:
        submit(store, 1000, 0)
        store.ingest(event('progress', recorded_at=1100, agent_id='child', report=report(revision=20)))
        task = submit(store, 990, 1, status='completed', plan=[dict(id='check', title='检查', status='completed')], evidence=[dict(label='通过')])
        assert elapsed(task['timing'], 2000) == 0
        assert elapsed(task['timing'], 900) >= 0
    finally:
        store.close()


def test_initial_terminal_unknown_and_resume_records_lower_bound(tmp_path):
    store = Store(tmp_path)
    try:
        task = submit(store, 1000, 0, status='completed', plan=[dict(id='check', title='检查', status='completed')], evidence=[dict(label='通过')])
        assert elapsed(task['timing'], 1100) is None
        assert duration_text(task['timing'], now=1100) == '起点未记录'
        task = submit(store, 1100, 1)
        assert duration_text(task['timing'], now=1110) == '已记录 10 秒'
        assert duration_text(task['plan'][0]['timing'], now=1110) == '已记录 10 秒'
    finally:
        store.close()


def test_hook_plan_timing_and_cancelled_freeze(tmp_path):
    store = Store(tmp_path)
    try:
        store.ingest(event('SessionStart', recorded_at=900))
        assert elapsed(store.tasks()[0]['timing'], 1000) is None
        store.ingest(event('plan', recorded_at=1000, plan=[dict(id='check', title='检查', status='inProgress')]))
        assert elapsed(store.tasks()[0]['plan'][0]['timing'], 1010) == 10
        task = submit(store, 1020, 0, status='cancelled')
        assert elapsed(task['timing'], 2000) == 20
        assert elapsed(task['plan'][0]['timing'], 2000) == 20
    finally:
        store.close()


@pytest.mark.parametrize('seconds,expected', [(0,'0 秒'),(59,'59 秒'),(60,'1 分'),(61,'1 分 1 秒'),(3599,'59 分 59 秒'),(3600,'1 小时'),(3960,'1 小时 6 分'),(90000,'1 天 1 小时'),(None,'起点未记录')])
def test_readable_duration(seconds, expected):
    assert human_duration(seconds) == expected
