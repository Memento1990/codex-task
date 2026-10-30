"""从既有事件只读还原计时，兼容仍在运行的旧采集器。"""
import copy
import json
import time


def clock():
    return dict(started_at=None, ended_at=None, seconds=0, since=None, partial=False)


def advance(timer, active, at, finished=False):
    if active and timer['since'] is None:
        timer['started_at'] = timer['started_at'] or at
        timer['since'] = at
        timer['ended_at'] = None
    elif not active and timer['since'] is not None:
        timer['seconds'] += max(0, at - timer['since'])
        timer['since'] = None
        timer['ended_at'] = at
    if finished and timer['started_at'] is None and timer['ended_at'] is None:
        timer['partial'] = True
        timer['ended_at'] = at


def elapsed(timer, now=None):
    if not timer or timer['partial'] and timer['started_at'] is None:
        return None
    now = time.time() if now is None else now
    return timer['seconds'] + (max(0, now - timer['since']) if timer['since'] is not None else 0)


def human_duration(seconds):
    if seconds is None:
        return '起点未记录'
    seconds = max(0, int(seconds))
    for unit, name, next_unit, next_name in [(86400, '天', 3600, '小时'), (3600, '小时', 60, '分'), (60, '分', 1, '秒')]:
        if seconds >= unit:
            major, remainder = divmod(seconds, unit)
            minor = remainder // next_unit
            return f'{major} {name}' + (f' {minor} {next_name}' if minor else '')
    return f'{seconds} 秒'


def duration_text(timer, prefix='耗时', now=None):
    seconds = elapsed(timer, now)
    if seconds is None:
        return '起点未记录'
    if timer['started_at'] is None:
        return '未开始'
    return ('已记录 ' if timer['partial'] else prefix + ' ') + human_duration(seconds)


def timing_tooltip(timer):
    if not timer:
        return '没有可还原的开始记录'
    fmt = lambda at: time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(at)) if at else '未记录'
    value = elapsed(timer)
    return ('开始：' + fmt(timer['started_at']) + '\n' + ('进行中' if timer['since'] is not None else '结束：' + fmt(timer['ended_at']))
            + ('\n累计：' + str(int(value)) + ' 秒' if value is not None else '\n起点未记录')
            + ('\n记录不完整，耗时仅包含已记录的部分。' if timer['partial'] else '')
            + '\n按收到的报告记录自然经过时间，包含等待；恢复后累计。')


class TimingIndex:
    def __init__(self):
        self.cursor = 0
        self.states = {}

    def update(self, db):
        high = db.execute('select coalesce(max(rowid),0) from events').fetchone()[0]
        for task_id, kind, at, raw in db.execute("select task_id,kind,at,payload from events where rowid>? and rowid<=? and kind in ('progress','plan') order by rowid", (self.cursor, high)):
            event = json.loads(raw)
            if event.get('agent_id'):
                continue
            state = self.states.setdefault(task_id, dict(revision=-1, title='', status='unreported', last_at=0, plan_at=0, timing=clock(), steps={}, blockers={}))
            if kind == 'progress':
                report = event['report']
                if report['revision'] <= state['revision']:
                    continue
                if state['status'] in {'completed', 'cancelled'} and report['status'] not in {'completed', 'cancelled'} and report['title'] != state['title']:
                    state.update(timing=clock(), steps={}, blockers={})
                state.update(revision=report['revision'], title=report['title'], status=report['status'])
            else:
                if state['revision'] >= 0 or at < state['plan_at']:
                    continue
                state['plan_at'] = at
                report = dict(status='running', plan=event['plan'], blockers=[])
            at = max(at, state['last_at'])
            state['last_at'] = at
            terminal = report['status'] in {'completed', 'cancelled'}
            advance(state['timing'], not terminal, at, terminal)
            for key, values in [('steps', report['plan']), ('blockers', report['blockers'])]:
                present = set()
                for item in values:
                    present.add(item['id'])
                    timer = state[key].setdefault(item['id'], clock())
                    active = item['open'] if key == 'blockers' else item['status'] in {'inProgress', 'blocked'}
                    finished = not item['open'] if key == 'blockers' else item['status'] == 'completed'
                    advance(timer, active and not terminal, at, finished)
                for identifier, timer in state[key].items():
                    if identifier not in present:
                        advance(timer, False, at)
        self.cursor = high

    def attach(self, task):
        state = self.states.get(task['id'])
        task['timing'] = copy.copy(state['timing']) if state else None
        for key, values in [('steps', task['plan']), ('blockers', task['blockers'])]:
            for item in values:
                timer = state[key].get(item['id']) if state else None
                item['timing'] = copy.copy(timer) if timer else None
        return task
