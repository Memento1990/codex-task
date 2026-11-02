export type Timer = { started_at: number | null; ended_at: number | null; seconds: number; since: number | null; partial: boolean } | null;
export type Step = { id: string; title: string; status: string; note: string; timing?: Timer };
export type Blocker = { id: string; title: string; open: boolean; reason: string; impact: string; owner: string; resolve_when: string; timing?: Timer };
export type Task = { id: string; session_id: string; host_id: string; cwd: string; title: string; status: string; runtime: string; updated_at: number; current_step: string; plan: Step[]; blockers: Blocker[]; evidence: { label: string; reference: string }[]; history: { kind: string; at: number; tool: string; status: string; agent_id: string }[]; timing?: Timer };
export type Options = { auto_start: boolean; show_taskbar: boolean; theme: string; notifications: boolean; notification_seconds: number };
export type Notice = { task_id: string; title: string; lines: string[]; tab: number };
export type Connection = { state: string; config_state?: string; error?: string; diagnostics?: unknown; recovery_required?: boolean; hook_seen?: boolean; hook_error?: string; upgrade_available?: boolean; mcp_connected?: boolean };
export type IntegrationStep = { title:string; description:string; action:'connect'|'recover'|'check'|'details'|'done'; button:string; stage:number; verified:boolean };
export function integrationStep(connection: Connection): IntegrationStep {
  const step=(title:string,description:string,action:IntegrationStep['action'],button:string,stage:number,verified=false):IntegrationStep=>({title,description,action,button,stage,verified});
  switch(connection.state){
    case 'missing': return step('连接你的 Codex','看板会在当前用户目录安装小型连接程序，并配置 Codex 接入。不需要管理员权限，也不会修改项目。','connect','连接 Codex',0);
    case 'recovery_required': return step('发现旧版连接','先查看将恢复的配置，再确认恢复连接。未确认前不会接管或覆盖旧配置。','recover','查看并恢复连接',0);
    case 'conflict': return step('连接配置需要确认','发现不能安全接管的配置。看板未覆盖或追加重复配置，请先查看诊断中的冲突项。','details','查看冲突详情',0);
    case 'awaiting_reload': return step('配置已准备，等待 Codex 载入','如 Codex 原生界面提示，请确认启用与信任。方便时使用其重载功能，或完整退出并重启 Codex Desktop；仅新建标签页可能复用旧进程。可以稍后完成，不会终止正在进行的任务。','check','检查连接',1);
    case 'awaiting_session': return step('等待当前 Codex 会话','连接配置已就绪。请在已载入连接的 Codex 中开始或继续会话，看板会自动验证；没有业务任务并不代表失败。','check','检查连接',2);
    case 'session_received': return step('已收到当前会话','当前会话已到达，尚未验证任务进度同步。正常使用 Codex，收到真实结构化任务汇报后将自动完成验证。','check','检查同步',2);
    case 'sync_verified': return step('任务同步已验证','已收到当前连接的真实任务汇报。之后任务变化会自动显示在看板。','done','完成',3,true);
    case 'error': return step('暂时无法验证连接','查看诊断以确认原因。检查只读取当前状态，不会重写配置，也不会停止 Codex。','check','重新检查',0);
    default: return step('连接尚待验证','当前状态尚未得到完整验证。可检查连接或查看高级诊断；历史记录不代表当前连接已同步。','check','检查连接',1);
  }
}
export function connectionStatus(connection: Connection): string {
  return ({missing:'尚未接入',recovery_required:'旧连接待恢复',conflict:'接入冲突',awaiting_reload:'等待载入连接',awaiting_session:'等待当前会话',session_received:'已收到当前会话',sync_verified:'同步已验证',error:'接入异常'} as Record<string,string>)[connection.state] || '连接待验证';
}
export type Snapshot = { tasks: Task[]; notices: Notice[]; healthy: boolean; now: number; options: Options; connection: Connection; diagnostics?: unknown };
export type DataLocation = { current: string; restart_required: boolean };
export function createTestNoticeFactory() {
  let sequence = 0;
  return (options: Options): Notice & {seconds:number;theme:string} => ({task_id:`settings-preview-${++sequence}`,title:'消息通知预览',lines:['任务或清单变化时，在这里提醒。'],tab:0,seconds:options.notification_seconds,theme:options.theme});
}
export function noticeStack<T extends Notice>(items: T[], item: T): T[] {
  return [...items.filter(previous => previous.task_id !== item.task_id), item].slice(-4);
}
export function diagnosticText(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value == null) return '';
  if (Array.isArray(value)) return value.map(diagnosticText).filter(Boolean).join('\n');
  return JSON.stringify(value, null, 2);
}
export const defaults: Options = { auto_start: false, show_taskbar: false, theme: 'light', notifications: true, notification_seconds: 3 };
export const status: Record<string,string> = { running: '执行中', blocked: '有卡点', waiting: '待确认', completed: '已完成', cancelled: '已取消', unreported: '未汇报', pending: '待执行', inProgress: '进行中' };
export const runtime: Record<string,string> = { active: '运行中', idle: '本轮结束', interrupted: '已中断', waiting_approval: '等待审批', unknown: '未知' };
export const kinds: Record<string,string> = { SessionStart: '会话开始', UserPromptSubmit: '收到新请求', PreToolUse: '调用工具', PostToolUse: '工具返回', Stop: '本轮结束', SessionEnd: '会话结束', PermissionRequest: '请求审批', Interrupt: '执行中断', progress: '更新进度', plan: '更新计划', SubagentStart: '子任务开始', SubagentStop: '子任务结束' };
export const openBlockers = (task: Task) => task.blockers.filter(item => item.open);
export const done = (task: Task) => task.plan.filter(item => item.status === 'completed').length;
export function elapsed(timer: Timer | undefined, now: number): string {
  if (!timer) return '起点未记录';
  if (timer.started_at === null) return timer.partial ? '起点未记录' : '未开始';
  const seconds = Math.max(0, Math.floor(timer.seconds + (timer.since === null ? 0 : now - timer.since)));
  const d = Math.floor(seconds / 86400), h = Math.floor(seconds / 3600) % 24, m = Math.floor(seconds / 60) % 60, s = seconds % 60;
  const value = d ? `${d} 天${h ? ` ${h} 小时` : ''}` : h ? `${h} 小时${m ? ` ${m} 分` : ''}` : m ? `${m} 分${s ? ` ${s} 秒` : ''}` : `${s} 秒`;
  return (timer.partial ? '已记录 ' : '') + value;
}
export function visibleTasks(tasks: Task[], filter: string): Task[] {
  return tasks.filter(task => task.status !== 'completed' && (filter === 'all' || filter === 'blocked' && openBlockers(task).length > 0 || filter === 'running' && task.status === 'running'));
}
export function safeReference(value: string): boolean {
  if (/^(?:[A-Z]:[\\/])[^\r\n]+\.(?:md|txt|json|log|csv|png|jpe?g|webp|pdf|html)$/i.test(value)) return true;
  try { const url = new URL(value); return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password; } catch { return false; }
}
