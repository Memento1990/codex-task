/* 共享合成数据 — 原型视图模型，实际字段适配见 ../DATA-CONTRACT.md。
   所有时间为相对加载时刻的“多少秒前”，避免原型里出现过期日期。
   三套原型共用这一份数据，保证对比时变量单一。 */
(function () {
  const LOAD = Date.now() / 1000;

  const task = (o) => ({
    id: o.id,
    title: o.title,
    status: o.status,              // running | blocked | waiting | completed | unreported
    runtime: o.runtime || 'active',// active | idle | interrupted | waiting_approval
    cwd: o.cwd,
    sessionId: o.sessionId,
    hostId: o.hostId || 'win-dev-01',
    startedAgo: o.startedAgo,
    endedAgo: o.endedAgo ?? null,
    currentStep: o.currentStep || '',
    tag: o.tag,
    plan: (o.plan || []).map((s, i) => ({
      id: o.id + '-s' + i,
      title: s[0],
      status: s[1],                // completed | inProgress | blocked | pending
      note: s[2] || '',
      startedAgo: s[3] ?? null,
      endedAgo: s[4] ?? null,
    })),
    blockers: (o.blockers || []).map((b, i) => ({
      id: o.id + '-b' + i,
      title: b.title,
      open: b.open,
      reason: b.reason,
      impact: b.impact || '',
      owner: b.owner,
      resolveWhen: b.resolveWhen,
      openedAgo: b.openedAgo,
      closedAgo: b.closedAgo ?? null,
    })),
    evidence: (o.evidence || []).map((e, i) => ({
      id: o.id + '-e' + i,
      label: e[0],
      reference: e[1] || '',
      atAgo: e[2] ?? null,
    })),
  });

  const tasks = [
    task({
      id: 'pay',
      title: '支付重构 · webhook 签名迁移',
      status: 'running',
      cwd: 'D:\\projects\\pay-core',
      sessionId: '01J9X2K7QF3M8N4R6T0V8WY2ZB',
      startedAgo: 2538,
      currentStep: '迁移 webhook 签名校验',
      tag: '支付重构',
      plan: [
        ['梳理旧签名调用点', 'completed', '17 处调用，集中在 4 个 handler', 2538, 2310],
        ['新增 v2 签名工具与单测', 'completed', '覆盖时间戳偏移与重放', 2310, 1980],
        ['迁移 webhook 签名校验', 'inProgress', '正在替换 handler 入口', 1980, null],
        ['补齐失败重试与告警', 'blocked', '依赖测试环境可用', 1980, null],
        ['灰度 10% 流量比对', 'pending', ''],
        ['全量切换并保留旧路径', 'pending', ''],
        ['回收旧签名代码', 'pending', ''],
      ],
      blockers: [
        {
          title: '测试环境凭据失效',
          open: true,
          reason: '沙箱签名密钥于今日轮换，本地 .env 仍为旧值，校验一律返回 401。',
          impact: '签名迁移无法在沙箱验证；涉及 2 个步骤与全部灰度方案。',
          owner: '你',
          resolveWhen: '换取新密钥并重跑 test:webhook 全绿',
          openedAgo: 1320,
        },
        {
          title: '灰度名单未确认',
          open: true,
          reason: '运营侧尚未给出首批 10% 商户名单的冻结时间。',
          impact: '不阻塞编码，但会推迟灰度开始时间约半天。',
          owner: '产品 · 林',
          resolveWhen: '名单冻结并写入灰度配置',
          openedAgo: 2100,
        },
      ],
      evidence: [
        ['单测报告', 'D:\\projects\\pay-core\\reports\\signature-v2.html', 1980],
        ['需求澄清记录', 'https://example.invalid/docs/pay-webhook-v2', 2410],
      ],
    }),
    task({
      id: 'log',
      title: '日志清洗管道 · 字段归一',
      status: 'running',
      cwd: 'D:\\projects\\logpipe',
      sessionId: '01J9X4AB2C7D9E1F3G5H7J9K1L',
      startedAgo: 1560,
      currentStep: '补齐 trace_id 回填',
      tag: '日志清洗',
      plan: [
        ['盘点源字段与目标 schema', 'completed', '', 1560, 1400],
        ['实现字段映射表', 'completed', '覆盖率 94%', 1400, 1140],
        ['补齐 trace_id 回填', 'inProgress', '剩余 3 个上游服务', 1140, null],
        ['幂等重放验证', 'pending', ''],
        ['压测 5k/s', 'pending', ''],
        ['接入调度', 'pending', ''],
        ['补文档', 'pending', ''],
        ['上线灰度', 'pending', ''],
        ['监控看板', 'pending', ''],
      ],
      blockers: [],
      evidence: [['字段映射覆盖率', 'D:\\projects\\logpipe\\coverage.json', 1400]],
    }),
    task({
      id: 'mig',
      title: '数据迁移 · SQLite 到新目录',
      status: 'running',
      cwd: 'D:\\projects\\codex-floatboard',
      sessionId: '01J9X6CD4E8F0G2H4J6K8L0M2N',
      startedAgo: 900,
      currentStep: '比对源库与目标表结构',
      tag: '数据迁移',
      plan: [
        ['演练迁移脚本', 'completed', '', 900, 620],
        ['比对源库与目标表结构', 'inProgress', '', 620, null],
        ['选择低峰窗口', 'pending', ''],
        ['正式迁移并回滚验证', 'pending', ''],
      ],
      blockers: [
        {
          title: '迁移窗口等待你确认',
          open: true,
          reason: '迁移要求全部 Codex 会话暂停上报，需要你决定一个业务低峰时段。',
          impact: '脚本已就绪，无法继续执行后续两步。',
          owner: '你',
          resolveWhen: '给出时间窗口并暂停上报',
          openedAgo: 620,
        },
      ],
      evidence: [['表结构差异', 'D:\\projects\\codex-floatboard\\artifacts\\schema-diff.md', 620]],
    }),
    task({
      id: 'rpt',
      title: '报表导出链路验证',
      status: 'waiting',
      runtime: 'waiting_approval',
      cwd: 'D:\\projects\\reporting',
      sessionId: '01J9X8EF6G0H2J4K6L8M0N2P4Q',
      startedAgo: 3300,
      currentStep: '等待验收账号授权',
      tag: '报表导出',
      plan: [
        ['跑通导出 CSV', 'completed', '', 3300, 3100],
        ['校验大文件分片', 'completed', '', 3100, 2820],
        ['验收账号权限核对', 'blocked', '', 2820, null],
        ['导出审计日志接入', 'pending', ''],
        ['回归全部导出格式', 'pending', ''],
      ],
      blockers: [
        {
          title: '验收账号缺少导出权限',
          open: true,
          reason: 'UAT 账号仅有只读角色，导出接口返回 403。',
          impact: '最后两步无法验证，任务整体停在验收阶段。',
          owner: '测试负责人 · 周',
          resolveWhen: '为该账号追加 report:export 权限',
          openedAgo: 2820,
        },
      ],
      evidence: [['分片校验记录', 'https://example.invalid/qa/report-chunking', 2820]],
    }),
    task({
      id: 'doc',
      title: '接入文档更新',
      status: 'completed',
      runtime: 'idle',
      cwd: 'D:\\projects\\codex-task',
      sessionId: '01J9XA1B3C5D7E9F1G3H5J7K9M',
      startedAgo: 9000,
      endedAgo: 5400,
      tag: '文档',
      plan: [
        ['核对现有章节', 'completed', '', 9000, 8400],
        ['补 hook 生命周期说明', 'completed', '', 8400, 7200],
        ['补接入排错清单', 'completed', '', 7200, 6600],
        ['截图重制', 'completed', '', 6600, 6000],
        ['交叉校对', 'completed', '', 6000, 5700],
        ['提交并合并', 'completed', '', 5700, 5400],
      ],
      blockers: [],
      evidence: [
        ['文档预览', 'https://example.invalid/docs/integration', 5600],
        ['校对记录', 'D:\\projects\\codex-task\\docs\\proofread.md', 5700],
        ['改动清单', 'D:\\projects\\codex-task\\docs\\changes.md', 5450],
      ],
    }),
    task({
      id: 'i18n',
      title: '多语言文案梳理',
      status: 'completed',
      runtime: 'idle',
      cwd: 'D:\\projects\\pay-core',
      sessionId: '01J9XB2C4D6E8F0G2H4J6K8L0N',
      startedAgo: 14400,
      endedAgo: 10800,
      tag: '文案',
      plan: [
        ['导出待翻译条目', 'completed', '', 14400, 13200],
        ['去重与合并', 'completed', '', 13200, 12000],
        ['交由翻译', 'completed', '', 12000, 11400],
        ['回填并校验占位符', 'completed', '', 11400, 10800],
      ],
      blockers: [],
      evidence: [
        ['条目清单', 'D:\\projects\\pay-core\\i18n\\entries.csv', 13200],
        ['占位符校验', 'D:\\projects\\pay-core\\i18n\\checked.log', 10900],
      ],
    }),
    task({
      id: 'unrep',
      title: '等待结构化汇报',
      status: 'unreported',
      runtime: 'unknown',
      cwd: 'D:\\projects\\unknown',
      sessionId: '01J9XC3D5E7F9G1H3J5K7L9M1P',
      startedAgo: 180,
      tag: '未汇报',
      plan: [],
      blockers: [],
      evidence: [],
    }),
  ];

  const abs = (ago) => (ago === null || ago === undefined ? null : LOAD - ago);

  const resolve = (t) => ({
    ...t,
    startedAt: abs(t.startedAgo),
    endedAt: abs(t.endedAgo),
    plan: t.plan.map((s) => ({ ...s, startedAt: abs(s.startedAgo), endedAt: abs(s.endedAgo) })),
    waitingSince: t.status === 'waiting' ? abs(2820) : null,
    blockers: t.blockers.map((b) => ({ ...b, openedAt: abs(b.openedAgo), closedAt: abs(b.closedAgo) })),
    evidence: t.evidence.map((e) => ({ ...e, at: abs(e.atAgo), timeSource: e.atAgo == null ? 'unknown' : 'first_reported' })),
  });

  window.FB = {
    now: () => Date.now() / 1000,
    tasks: tasks.map(resolve),
    connection: {
      state: 'sync_verified',
      configState: 'ready',
      codexMode: 'local',
      codexHome: 'C:\\MockUser\\.codex',
      mcpConnected: true,
      mcpConnectedAgo: 3120,
      sessionReceivedAgo: 3000,
      syncVerifiedAgo: 2880,
      mcpConnectedAt: LOAD - 3120,
      sessionReceivedAt: LOAD - 3000,
      syncVerifiedAt: LOAD - 2880,
      runtimeUpdateAvailable: false,
    },
    options: {
      autoStart: false,
      showTaskbar: false,
      theme: 'dark',
      notifications: true,
      notificationSeconds: 3,
      dockSummaryMode: 'hover',
    },
    dataLocation: {
      current: 'C:\\MockUser\\AppData\\Local\\MockFloatboard\\data',
      restartRequired: false,
    },
    notifications: {
      enabled: true,
      subscriptions: { blocker_opened: true, task_completed: true, blocker_resolved: false },
      channels: [
        { id: 'wechat', name: '微信机器人', enabled: true, configured: true, status: 'ok', keyword: '' },
        { id: 'wecom', name: '企微机器人', enabled: true, configured: true, status: 'ok', keyword: 'codex' },
        { id: 'feishu', name: '飞书机器人', enabled: false, configured: false, status: '', keyword: '' },
      ],
      records: [
        { id: 'r1', atAgo: 1320, channel: 'wechat', event: '卡点出现', title: '支付重构 · 测试环境凭据失效', status: 'sent', attempts: 1 },
        { id: 'r2', atAgo: 4860, channel: 'wecom', event: '卡点出现', title: '支付重构 · 灰度名单未确认', status: 'sent', attempts: 1 },
        { id: 'r3', atAgo: 5400, channel: 'wechat', event: '任务完成', title: '接入文档更新', status: 'sent', attempts: 1 },
        { id: 'r4', atAgo: 4200, channel: 'wecom', event: '卡点出现', title: '报表导出 · 验收账号缺少导出权限', status: 'failed', attempts: 3 },
        { id: 'r5', atAgo: 10800, channel: 'feishu', event: '任务完成', title: '多语言文案梳理', status: 'skipped', attempts: 0 },
      ],
    },
    updates: { currentVersion: '0.14.1', latestVersion: '0.15.0', state: 'available', autoCheck: true, autoDownload: true },
    repair: { status: 'not_started' },
    weixin: { status: 'bound', recipientMasked: 'c****@wx', lastSentAgo: 1320, lastTestStatus: 'accepted' },
  };

  // 完整样例提供区间；“部分计时”场景会显式移除未知历史。
  for (const t of window.FB.tasks) {
    for (const s of t.plan) {
      s.segments = s.startedAt ? [{ startedAt: s.startedAt, endedAt: s.endedAt }] : [];
    }
  }

  /* 由模拟计时字段还原的条目，不是新增的“动态流水”数据源。 */
  window.FB.timeline = function (tasks) {
    const out = [];
    for (const t of tasks) {
      for (const s of t.plan) {
        if (s.status === 'completed' && s.endedAt) out.push({ at: s.endedAt, kind: 'step', task: t, title: `完成 ${s.title}`, note: s.note });
        if (s.status === 'blocked' && s.startedAt) out.push({ at: s.startedAt, kind: 'step-blocked', task: t, title: `受阻于 ${s.title}`, note: s.note });
        if (s.status === 'inProgress' && s.startedAt) out.push({ at: s.startedAt, kind: 'step-start', task: t, title: `开始 ${s.title}`, note: s.note });
      }
      for (const b of t.blockers) {
        out.push({ at: b.openedAt, kind: b.open ? 'blocker-open' : 'blocker-closed', task: t, title: b.title, note: b.reason, blocker: b });
        if (b.closedAt) out.push({ at: b.closedAt, kind: 'blocker-resolved', task: t, title: `解除 ${b.title}`, note: b.resolveWhen, blocker: b });
      }
      for (const e of t.evidence) if (e.at) out.push({ at: e.at, kind: 'evidence', task: t, title: `证据 ${e.label}`, note: e.reference });
      if (t.endedAt) out.push({ at: t.endedAt, kind: 'task-done', task: t, title: `任务完成 ${t.title}`, note: `${t.plan.length} 个步骤全部完成` });
    }
    return out.sort((a, b) => b.at - a.at);
  };
})();
