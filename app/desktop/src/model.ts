export type Timer = { started_at: number | null; ended_at: number | null; seconds: number; since: number | null; partial: boolean } | null;
export type Step = { id: string; title: string; status: string; note: string; timing?: Timer };
export type Blocker = { id: string; title: string; open: boolean; reason: string; impact: string; owner: string; resolve_when: string; timing?: Timer };
export type Task = { id: string; session_id: string; host_id: string; cwd: string; title: string; status: string; runtime: string; updated_at: number; current_step: string; plan: Step[]; blockers: Blocker[]; evidence: { label: string; reference: string }[]; history: { kind: string; at: number; tool: string; status: string; agent_id: string }[]; timing?: Timer };
export type Options = { auto_start: boolean; show_taskbar: boolean; theme: string; notifications: boolean; notification_seconds: number };
export type Notice = { task_id: string; title: string; lines: string[]; tab: number };
export type Snapshot = { tasks: Task[]; notices: Notice[]; healthy: boolean; now: number; options: Options; connection: { state: string; error?: string } };
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
