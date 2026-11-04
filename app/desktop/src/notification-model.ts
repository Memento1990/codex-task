import type { WeixinLabSnapshot } from './weixin-lab-model';
export type NotificationChannelId = 'wechat' | 'wecom' | 'feishu';
export type NotificationEvent = 'blocker_opened' | 'task_completed' | 'blocker_resolved';
export type NotificationSubscriptions = Record<NotificationEvent, boolean>;
export interface NotificationChannel {
  id: NotificationChannelId;
  name: string;
  enabled: boolean;
  configured: boolean;
  status: string;
  keyword: string;
  has_signing_secret: boolean;
}
export interface NotificationRecord {
  id: string;
  created_at: number | string;
  channel: NotificationChannelId;
  event: string;
  title: string;
  status: string;
  attempts: number;
  error?: string | null;
}
export interface NotificationSnapshot {
  enabled: boolean;
  subscriptions: NotificationSubscriptions;
  channels: NotificationChannel[];
  records: NotificationRecord[];
  service_error?: string | null;
  weixin?: WeixinLabSnapshot;
}
export const notificationEvents: { id: NotificationEvent; name: string; detail: string }[] = [
  { id: 'blocker_opened', name: '卡点出现', detail: '需要你介入时通知' },
  { id: 'task_completed', name: '任务完成', detail: '任务结束时通知' },
  { id: 'blocker_resolved', name: '卡点解除', detail: '问题解决后同步状态' },
];
export const notificationChannelNames: Record<NotificationChannelId, string> = { wechat: '微信机器人', wecom: '企微机器人', feishu: '飞书机器人' };
export function validNotificationWebhook(id: NotificationChannelId, value: string): boolean {
  if (id === 'wechat' || /[@\\%\u0000-\u0020\u007f]/.test(value) || value.includes('/../') || value.includes('/./')) return false;
  try {
    const url = new URL(value.trim());
    if (url.protocol !== 'https:' || url.username || url.password || url.port || url.hash) return false;
    if (id === 'wecom') return url.hostname === 'qyapi.weixin.qq.com' && url.pathname === '/cgi-bin/webhook/send'
      && url.searchParams.getAll('key').length === 1 && /^[A-Za-z0-9_-]+$/.test(url.searchParams.get('key') ?? '')
      && [...url.searchParams.keys()].every(key => key === 'key');
    return url.hostname === 'open.feishu.cn' && /^\/open-apis\/bot\/v2\/hook\/[A-Za-z0-9_-]+$/.test(url.pathname) && !url.search;
  } catch { return false; }
}
export function notificationRecordState(status: string): { label: string; tone: 'neutral' | 'success' | 'warning' } {
  switch (status) {
    case 'sent': case 'accepted': case 'delivered': case 'success': return { label: '平台已受理 · 非已读', tone: 'success' };
    case 'unknown': return { label: '结果未知 · 不自动重发', tone: 'warning' };
    case 'failed': return { label: '投递失败', tone: 'warning' };
    case 'pending': case 'queued': return { label: '等待投递', tone: 'neutral' };
    case 'sending': return { label: '正在投递', tone: 'neutral' };
    case 'retrying': case 'retry_wait': return { label: '等待自动重试', tone: 'neutral' };
    case 'skipped': case 'cancelled': return { label: '未投递', tone: 'neutral' };
    case 'expired': return { label: '已过期 · 未投递', tone: 'neutral' };
    default: return { label: '状态待确认', tone: 'neutral' };
  }
}
export function notificationChannelState(channel: NotificationChannel): string {
  if (channel.id === 'wechat') return !channel.configured ? '扫码绑定后启用' : channel.enabled ? '已启用 · 请留意会话状态' : '已绑定 · 未启用';
  if (!channel.configured) return '未配置';
  if (!channel.enabled) return '已配置 · 已停用';
  if (['failed', 'error'].includes(channel.status)) return '已配置 · 检查投递记录';
  return '已配置 · 不代表测试通过';
}
export function filterNotificationRecords(records: NotificationRecord[], failedOnly: boolean): NotificationRecord[] {
  return failedOnly ? records.filter(record => ['failed','unknown'].includes(record.status)) : records;
}
export function notificationEventName(event: string): string {
  return notificationEvents.find(item => item.id === event)?.name || (event === 'test' ? '测试通知' : '通知事件');
}
export function notificationPreview(event: NotificationEvent): { title: string; body: string } {
  const title = { blocker_opened: '需要关注 · 示例任务', task_completed: '任务完成 · 示例任务', blocker_resolved: '卡点解除 · 示例任务' }[event];
  return { title, body: '请在本机看板中查看任务状态并处理。' };
}
export function notificationTime(value: string | number): string {
  const date = new Date(typeof value === 'number' ? (value < 1e12 ? value * 1000 : value) : value);
  return Number.isNaN(date.getTime()) ? '时间未知' : date.toLocaleString('zh-CN', { hour12: false });
}
// Defense in depth: backend errors must never turn webhook URLs or secrets into UI output.
export function safeNotificationError(value: string | null | undefined): string {
  return value ? '投递失败，请检查网络、机器人配置和平台安全设置。' : '';
}
