export type Timer = { started_at: number | null; ended_at: number | null; seconds: number; since: number | null; partial: boolean } | null;
export type Step = { id: string; title: string; status: string; note: string; timing?: Timer };
export type Blocker = { id: string; title: string; open: boolean; reason: string; impact: string; owner: string; resolve_when: string; timing?: Timer };
export type Evidence = { id?:string; label: string; reference: string; at?:number|null; time_source?:'first_reported'|'unknown' };
export type Task = { archived?:boolean; source_task_id?:string; id: string; session_id: string; host_id: string; cwd: string; title: string; status: string; runtime: string; updated_at: number; current_step: string; plan: Step[]; blockers: Blocker[]; evidence: Evidence[]; timing?: Timer; waiting_since?:number|null };
export type DockSummaryMode = 'always'|'hover'|'never';
export const dockSummaryMode = (options: {dock_summary_mode?:unknown;dock_hover_summary?:boolean}): DockSummaryMode => ['always','hover','never'].includes(String(options.dock_summary_mode)) ? options.dock_summary_mode as DockSummaryMode : options.dock_hover_summary === false ? 'never' : 'hover';
export const dockSummaryVisible = (mode: DockSummaryMode, hovered: boolean): boolean => mode === 'always' || mode === 'hover' && hovered;
export type Options = { dock_summary_mode?: DockSummaryMode; dock_hover_summary?: boolean; auto_start: boolean; show_taskbar: boolean; theme: string; notifications: boolean; notification_seconds: number };
export type Notice = { task_id: string; title: string; lines: string[]; tab: number; sticky?: boolean };
export const normalizeTab = (value: unknown): number => value === 1 || value === 2 ? value : 0;
export const isReportedTask = (task: Task): boolean => task.status !== 'unreported' || (task.plan?.length || 0) > 0;
export const taskTitle = (task: Task): string => task.title || (task.plan.length ? '已同步计划' : '等待结构化汇报');
export const isBlockerNotice = (notice: Notice): boolean => notice.tab === 1;
export const dragThresholdReached = (x:number,y:number,nextX:number,nextY:number,buttons:number): boolean => (buttons & 1) === 1 && Math.hypot(nextX-x,nextY-y) >= 4;
export const noticeAutoDismiss = (notice: Notice): boolean => notice.sticky !== true;
export type Connection = { codex_home?:string;codex_mode?:'local'|'isolated';isolated?:boolean;state: string; config_state?: string; error?: string; diagnostics?: unknown; recovery_required?: boolean; hook_seen?: boolean; hook_error?: string; upgrade_available?: boolean; runtime_update_available?: boolean; mcp_connected?: boolean; mcp_connected_at?:number|null; session_received_at?:number|null; sync_verified_at?:number|null };
export function connectionTone(connection:Connection):'confirmed'|'neutral'|'problem' {
  if(['conflict','partial','error','recovery_required'].includes(connection.state)||connection.config_state==='partial'||connection.config_state==='error')return 'problem';
  if(['session_received','sync_verified'].includes(connection.state)||connection.mcp_connected===true)return 'confirmed';
  return 'neutral';
}
export function evidenceTime(value:number|null|undefined):string {
  return typeof value==='number'&&Number.isFinite(value)&&value>0?new Date(value*1000).toLocaleString('zh-CN',{hour12:false}):'未提供证据时间';
}
export const runningTaskSyncPrompt = '请将本会话正在进行的任务同步到 Codex 任务侧窗。仅使用本会话 Hook 提供的绑定信息和对应连接的 report_progress，基于当前可确认上下文汇报任务标题、完整步骤及状态、当前步骤、开放卡点和已有验收证据。这是接入后的当前快照，不补造历史、计时或证据。保持原任务继续执行，不因汇报创建子 Agent。若工具或有效绑定不可用，请说明尚未载入，不要猜会话 ID/token、手动调用 hook_context、修改配置或中断任务。';
export type IntegrationStep = { title:string; description:string; action:'connect'|'recover'|'check'|'details'|'done'; button:string; stage:number; verified:boolean };
export function integrationStep(connection: Connection): IntegrationStep {
  const step=(title:string,description:string,action:IntegrationStep['action'],button:string,stage:number,verified=false):IntegrationStep=>({title,description,action,button,stage,verified});
  if(connection.config_state==='partial')return step('配置需要检查','接入配置部分缺失，请查看差异与诊断。不会把连接待确认当作配置完整，也不会自动覆盖配置。','details','查看配置问题',0);
  if(connection.config_state==='error')return step('配置需要检查','配置读取或校验失败，请先查看诊断。已有回执不能代替配置校验。','details','查看配置问题',0);
  switch(connection.state){
    case 'missing': return step('尚未配置','看板会在当前用户目录安装小型连接程序，并配置 Codex 接入。不需要管理员权限，也不会修改项目。','connect','连接 Codex',0);
    case 'recovery_required': return step('发现旧版连接','先查看将恢复的配置，再确认恢复连接。未确认前不会接管或覆盖旧配置。','recover','查看并恢复连接',0);
    case 'conflict': return step('连接配置需要确认','发现不能安全接管的配置。看板未覆盖或追加重复配置，请先查看诊断中的冲突项。','details','查看冲突详情',0);
    case 'awaiting_reload':
    case 'awaiting_session':
    case 'ready':
    case 'configured': return connection.mcp_connected
      ?step('连接已确认 · 等待会话事件','Codex 曾加载此连接组件，尚未观察到本次接入的会话事件。空闲不算异常；请在 Codex 新建或继续真实会话，再刷新状态。','check','刷新状态',2)
      :step('配置完成 · 连接待确认','本工具配置已写入，尚未收到 Codex 回执；这不代表未授权。若已在 Codex 授权，请新建或继续一次真实会话，再刷新状态。可以稍后完成，无需反复授权或强制重启。','check','刷新状态',1);
    case 'partial': return step('配置需要检查','接入配置部分缺失，请查看差异与诊断。现有证据不能代替完整配置检查。','details','查看配置问题',0);
    case 'session_received': return step('会话事件已收到','会话事件链路已确认；任务同步尚未验证，不代表未接入。正常使用 Codex，真实任务汇报到达后将继续验证。','check','刷新状态',2);
    case 'sync_verified': return step('接入与任务同步已验证','已收到真实任务汇报，最近证据时间见下方。该回执不代表 Codex 此刻实时在线。','done','完成',3,true);
    case 'error': return step('暂时无法验证连接','查看诊断以确认原因。检查只读取当前状态，不会重写配置，也不会停止 Codex。','check','重新检查',0);
    default: return step('连接尚待验证','当前状态尚未得到完整验证。可检查连接或查看高级诊断；历史记录不代表当前连接已同步。','check','检查连接',1);
  }
}
export function connectionStatus(connection: Connection): string {
  if(['partial','error'].includes(connection.config_state || ''))return '配置需要检查';
  if(['ready','configured','awaiting_reload','awaiting_session'].includes(connection.state))return connection.mcp_connected?'连接已确认 · 待会话':'配置完成 · 连接待确认';
  return ({missing:'尚未配置',recovery_required:'旧连接待恢复',conflict:'配置需要检查',partial:'配置需要检查',session_received:'会话事件已收到',sync_verified:'接入与同步已验证',error:'接入异常'} as Record<string,string>)[connection.state] || '连接待验证';
}
export type StorageDiagnostic = {stage:string;sqlite_code:number|null;sqlite_extended_code:number|null};
export type Snapshot = { collector?:{diagnostic?:StorageDiagnostic;status:string;heartbeat_age:number|null;pid:number|null;version:string|null;start_error:string|null;data_root:string;guidance:string}; storage?: {status:'ok'|'error';message:string;diagnostic?:StorageDiagnostic}; tasks: Task[]; notices: Notice[]; healthy: boolean; now: number; options: Options; connection: Connection; diagnostics?: unknown; data_location?:DataLocation; restart_required?:boolean };
export type DataLocation = { current: string; restart_required: boolean };
export type UpdateSnapshot = {current_version:string;state:string;latest_version?:string|null;release_notes?:string|null;release_url?:string|null;downloaded_bytes:number;total_bytes?:number|null;error?:string|null;last_checked?:number|null;preferences:{auto_check:boolean;auto_download:boolean};supported:boolean;installation_result?:{status?:string;version?:string;message?:string}|null};
export function updatePresentation(update:UpdateSnapshot):{title:string;description:string;action:'check'|'download'|'install'|null;button:string} {
  if(!update.supported || update.state==='unsupported')return {title:'当前安装方式暂不支持自动更新',description:'当前构建未配置可用的更新签名或安装方式不受支持，无法安全自动更新。可继续使用当前版本，或查看官方发布页面。',action:null,button:''};
  switch(update.state){
    case 'checking':return {title:'正在检查更新',description:'正在获取版本信息，不会影响 Codex 任务。',action:null,button:'检查中…'};
    case 'up_to_date':return {title:'当前已是最新版本',description:'最近一次检查未发现更高版本。',action:'check',button:'重新检查'};
    case 'available':return {title:'发现可用更新',description:'下载完成后仍需你确认安装，不会自动重启看板。',action:'download',button:'下载更新'};
    case 'downloading':return {title:'正在下载更新',description:'可以关闭此面板，下载会在后台继续。',action:null,button:'下载中…'};
    case 'ready':return {title:'更新已准备就绪',description:'确认后安装并重启看板，不会结束 Codex 或正在进行的任务。',action:'install',button:'安装并重启…'};
    case 'installing':return {title:'正在安装更新',description:'看板可能暂时关闭，请勿重复启动安装。Codex 不会被结束。',action:null,button:'安装中…'};
    case 'error':return {title:'暂时无法完成更新',description:'当前版本仍可继续使用。可以稍后重试。',action:'check',button:'重新检查'};
    case 'idle':return {title:'检查可用更新',description:'后台检查和下载由看板管理，安装前始终征求确认。',action:'check',button:'检查更新'};
    default:return {title:'更新状态待确认',description:'暂不执行下载或安装，请重新检查版本信息。',action:'check',button:'检查更新'};
  }
}
export function updateProgress(downloaded:number,total?:number|null):number|null {
  if(!Number.isFinite(total) || !total || total<=0)return null;
  return Math.min(100,Math.max(0,Math.floor((Number.isFinite(downloaded)?downloaded:0)/total*100)));
}
export function updateBytes(bytes:number):string {
  if(!Number.isFinite(bytes)||bytes<0)return '0 B';
  return bytes>=1048576?`${(bytes/1048576).toFixed(1)} MB`:bytes>=1024?`${(bytes/1024).toFixed(1)} KB`:`${Math.floor(bytes)} B`;
}
export function safeUpdateUrl(value:string|undefined|null):boolean {
  try{const url=new URL(value || '');return url.protocol==='https:'&&url.hostname==='github.com'&&!url.port&&!url.username&&!url.password&&/^\/Memento1990\/codex-task\/releases(?:\/|$)/i.test(url.pathname);}catch{return false;}
}
export function createTestNoticeFactory() {
  let sequence = 0;
  return (options: Options): Notice & {seconds:number;theme:string} => ({task_id:`settings-preview-${++sequence}`,title:'消息通知预览',lines:['任务或清单变化时，在这里提醒。'],tab:0,seconds:options.notification_seconds,theme:options.theme});
}
export function noticeStack<T extends Notice>(items: T[], item: T): T[] {
  const previous = items.find(notice => notice.task_id === item.task_id);
  const merged = previous?.sticky && !item.sticky ? {...item, sticky:true} : item;
  const next = [...items.filter(notice => notice.task_id !== item.task_id), merged];
  const ordinary = next.filter(noticeAutoDismiss).slice(-4);
  return next.filter(notice => !noticeAutoDismiss(notice) || ordinary.includes(notice));
}
export function diagnosticText(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value == null) return '';
  if (Array.isArray(value)) return value.map(diagnosticText).filter(Boolean).join('\n');
  return JSON.stringify(value, null, 2);
}
export const defaults: Options = { dock_summary_mode: 'hover', dock_hover_summary: true, auto_start: false, show_taskbar: false, theme: 'dark', notifications: true, notification_seconds: 3 };
export const status: Record<string,string> = { running: '执行中', blocked: '有卡点', waiting: '待确认', completed: '已完成', cancelled: '已取消', unreported: '未汇报', pending: '待执行', inProgress: '进行中' };
export const runtime: Record<string,string> = { active: '运行中', idle: '本轮结束', interrupted: '已中断', waiting_approval: '等待审批', unknown: '未知' };
export const openBlockers = (task: Task) => task.blockers.filter(item => item.open);
export const done = (task: Task) => task.plan.filter(item => item.status === 'completed').length;
export type TaskTone = 'running'|'blocked'|'waiting'|'done'|'unknown'|'cancelled';
export function summaryTone(task: Task): TaskTone {
  if (task.status === 'completed') return 'done';
  if (task.status === 'cancelled') return 'cancelled';
  if (task.status === 'unreported') return 'unknown';
  if (openBlockers(task).length) return 'blocked';
  return task.status === 'waiting' ? 'waiting' : 'running';
}
export function taskCounts(tasks: Task[]) {
  const counts = {running:0,blocked:0,waiting:0,done:0,unreported:0,cancelled:0,total:0,blockers:0};
  for (const task of tasks.filter(isReportedTask)) {
    const tone = summaryTone(task);
    counts[tone === 'unknown' ? 'unreported' : tone]++;
    counts.total++;
    counts.blockers += openBlockers(task).length;
  }
  return counts;
}
export function matchesTask(task: Task, query: string): boolean {
  const terms = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  if (!terms.length) return true;
  const text = [task.id,task.source_task_id,task.title,task.cwd,task.session_id,task.host_id,task.current_step,
    ...(task.plan || []).map(item=>item.title),
    ...(task.blockers || []).flatMap(item=>[item.title,item.reason,item.owner]),
    ...(task.evidence || []).flatMap(item=>[item.label,item.reference])].join('\n').toLocaleLowerCase();
  return terms.every(term=>text.includes(term));
}
export function filterTasks(tasks: Task[], filter = 'active', query = '', showDone = false): Task[] {
  return tasks.filter(task => {
    if (!isReportedTask(task) || !matchesTask(task,query)) return false;
    const tone = summaryTone(task);
    if (filter === 'all') return true;
    if (filter === 'active') return !['done','cancelled'].includes(tone) || showDone && tone === 'done';
    return tone === ({completed:'done',unreported:'unknown'} as Record<string,string>)[filter] || tone === filter;
  });
}
export function elapsed(timer: Timer | undefined, now: number): string {
  if (!timer) return '起点未记录';
  if (timer.started_at === null && timer.seconds <= 0 && timer.since === null) return timer.partial ? '起点未记录' : '未开始';
  const seconds = Math.max(0, Math.floor(timer.seconds + (timer.since === null ? 0 : now - timer.since)));
  const d = Math.floor(seconds / 86400), h = Math.floor(seconds / 3600) % 24, m = Math.floor(seconds / 60) % 60, s = seconds % 60;
  const value = d ? `${d} 天${h ? ` ${h} 小时` : ''}` : h ? `${h} 小时${m ? ` ${m} 分` : ''}` : m ? `${m} 分${s ? ` ${s} 秒` : ''}` : `${s} 秒`;
  return (timer.partial || timer.started_at === null ? '已记录 ' : '') + value;
}
export function dockSummary(tasks: Task[], selected: string) {
  const reported = tasks.filter(task => !task.archived && isReportedTask(task));
  const active = reported.filter(task => !['completed','cancelled'].includes(task.status));
  const task = active.find(task => task.id === selected) || active.find(task => openBlockers(task).length > 0) || active[0];
  const blockers = reported.reduce((sum, task) => sum + openBlockers(task).length, 0);
  const allCompleted = reported.length > 0 && reported.every(task => task.status === 'completed');
  const total = task?.plan.length || 0, completed = task ? done(task) : 0;
  return {task, blockers, active:active.length, allCompleted, total, completed, progress:total ? completed / total : null,
    title:task ? taskTitle(task) : allCompleted ? '所有任务已完成' : '暂无进行中的任务',
    badge:blockers ? `! ${blockers}` : allCompleted ? '✓' : total ? `${completed}/${total}` : '—',
    step:task?.current_step || task?.plan.find(step => step.status === 'inProgress')?.title || (total ? '等待下一步进展' : '暂无可计算进度')};
}
export function visibleTasks(tasks: Task[], filter: string): Task[] {
  return filterTasks(tasks,filter === 'all' ? 'active' : filter);
}
export function safeReference(value: string): boolean {
  if (/^(?:[A-Z]:[\\/])[^\r\n]+\.(?:md|txt|json|log|csv|png|jpe?g|webp|pdf|html)$/i.test(value)) return true;
  try { const url = new URL(value); return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password; } catch { return false; }
}
