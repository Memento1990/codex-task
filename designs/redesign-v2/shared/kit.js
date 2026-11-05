/* 共享辅助函数 —— 只放与视觉无关的逻辑，三套原型各自定义自己的视觉语言。 */
(function () {
  const pad = (n) => String(n).padStart(2, '0');

  /* 还原 model.ts 的 elapsed()：自然经过时间，含等待与受阻。 */
  function fmtElapsed(seconds) {
    if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '—';
    seconds = Math.max(0, Math.floor(seconds));
    const d = Math.floor(seconds / 86400);
    const h = Math.floor(seconds / 3600) % 24;
    const m = Math.floor(seconds / 60) % 60;
    const s = seconds % 60;
    if (d) return h ? `${d}天${h}小时` : `${d}天`;
    if (h) return m ? `${h}小时${m}分` : `${h}小时`;
    if (m) return s ? `${m}分${s}秒` : `${m}分`;
    return `${s}秒`;
  }

  /* 紧凑型：42m / 1h08m / 3d —— 用于密集视图。 */
  function fmtCompact(seconds) {
    if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '—';
    seconds = Math.max(0, Math.floor(seconds));
    if (seconds >= 86400) return Math.floor(seconds / 86400) + 'd';
    if (seconds >= 3600) return Math.floor(seconds / 3600) + 'h' + pad(Math.floor(seconds / 60) % 60) + 'm';
    if (seconds >= 60) return Math.floor(seconds / 60) + 'm';
    return seconds + 's';
  }

  /* 计时器跑动中：从 startedAt 累计到现在；已结束：用固定区间。 */
  function elapsed(t) {
    if (t.timing) return Math.max(0, t.timing.seconds + (t.timing.since == null ? 0 : Date.now() / 1000 - t.timing.since));
    if (!t.startedAt) return null;
    return (t.endedAt || Date.now() / 1000) - t.startedAt;
  }

  function fmtClock(ts) {
    if (!ts) return '—';
    const d = new Date(ts * 1000);
    return pad(d.getHours()) + ':' + pad(d.getMinutes()) + ':' + pad(d.getSeconds());
  }

  function fmtDateTime(ts) {
    if (!ts) return '—';
    const d = new Date(ts * 1000);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  function ago(ts, now) {
    if (!ts) return '—';
    const s = Math.max(0, Math.floor((now || Date.now() / 1000) - ts));
    if (s < 60) return '刚刚';
    if (s < 3600) return Math.floor(s / 60) + ' 分钟前';
    if (s < 86400) return Math.floor(s / 3600) + ' 小时前';
    return Math.floor(s / 86400) + ' 天前';
  }

  const openBlockers = (t) => t.blockers.filter((b) => b.open);
  const doneSteps = (t) => t.plan.filter((s) => s.status === 'completed').length;
  const pct = (t) => (t.plan.length ? doneSteps(t) / t.plan.length : 0);

  /* 任务状态 → 语义色调，三套原型共用语义、各自映射颜色。 */
  function tone(t) {
    if (t.status === 'completed') return 'done';
    if (t.status === 'cancelled') return 'cancelled';
    if (t.status === 'unreported') return 'unknown';
    if (openBlockers(t).length) return 'blocked';
    if (t.status === 'waiting') return 'waiting';
    return 'running';
  }

  const STATUS = {
    running: '执行中', blocked: '有卡点', waiting: '待确认',
    completed: '已完成', cancelled: '已取消', unreported: '未汇报',
    pending: '待执行', inProgress: '进行中',
  };
  const RUNTIME = {
    active: '运行中', idle: '本轮结束', interrupted: '已中断',
    waiting_approval: '等待审批', unknown: '未知',
  };
  const STEPMARK = { completed: '✓', inProgress: '▸', blocked: '!', pending: '·' };

  function esc(v) {
    return String(v == null ? '' : v).replace(/[&<>"']/g, (c) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  }

  /* 归一化为“动作项”：卡点与待确认都算需要介入，排序按开放时长。 */
  function actionItems(tasks) {
    const items = [];
    for (const t of tasks) {
      if (['completed','cancelled','unreported'].includes(t.status)) continue;
      for (const b of t.blockers) if (b.open) items.push({ kind: 'blocker', task: t, blocker: b, at: b.openedAt });
      if (t.status === 'waiting' && !openBlockers(t).length) {
        items.push({ kind: 'waiting', task: t, at: t.waitingSince, blocker: null });
      }
    }
    return items.sort((a, b) => (a.at ?? Infinity) - (b.at ?? Infinity));
  }

  window.KIT = {
    fmtElapsed, fmtCompact, elapsed, fmtClock, fmtDateTime, ago, pad,
    openBlockers, doneSteps, pct, tone, STATUS, RUNTIME, STEPMARK, esc, actionItems,
  };
})();
