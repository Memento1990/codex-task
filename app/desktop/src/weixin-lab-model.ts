// Experimental connection lab only. Never task subscriptions or channel configuration.
export type WeixinLabState = 'disconnected' | 'restored' | 'starting_login' | 'waiting_scan' | 'scanned' | 'need_verifycode' | 'expired' | 'logged_in' | 'binding' | 'bound' | 'sending' | 'paused' | 'error';
export interface WeixinLabSnapshot {
  status: WeixinLabState;
  busy: boolean;
  active: boolean;
  logged_in: boolean;
  bound: boolean;
  restored: boolean;
  qrcode_img_content: string;
  qr_expires_at: number;
  binding_code: string;
  binding_expires_at: number;
  recipient_masked: string;
  last_received_at: number;
  last_sent_at: number;
  error: string;
  last_test_status: string;
  pause_until: number;
}
export function weixinLabStep(snapshot: WeixinLabSnapshot | null): number {
  if (!snapshot || snapshot.status === 'restored') return 0;
  if (snapshot.bound) return 2;
  return snapshot.logged_in ? 1 : 0;
}
export function weixinLabStatus(snapshot: WeixinLabSnapshot | null): { title: string; detail: string } {
  if (!snapshot) return { title: '正在读取实验室状态', detail: '尚未连接微信，也不会发送消息。' };
  switch (snapshot.status) {
    case 'disconnected': return { title: '尚未连接', detail: '点击连接后才会请求真实登录二维码。' };
    case 'restored': return { title: '检测到本机保存的连接', detail: '需要你确认恢复，才会重新接收消息；不会自动发送测试。' };
    case 'starting_login': return { title: '正在获取登录二维码', detail: '网络请求在后台进行，你可以关闭此页面。' };
    case 'waiting_scan': return { title: '等待微信扫码', detail: '请使用你希望接收测试通知的微信扫码并确认。' };
    case 'scanned': return { title: '已扫码，等待手机确认', detail: '请在微信中继续完成登录确认。' };
    case 'need_verifycode': return { title: '需要验证码', detail: '请输入手机显示的验证码，仅用于这次登录。' };
    case 'expired': return { title: '二维码已过期', detail: '刷新二维码后重新扫码；旧二维码不可继续使用。' };
    case 'logged_in': case 'binding': return { title: '已登录，等待绑定收件人', detail: '在扫码微信与机器人的私聊中发送下方的一次性绑定码。' };
    case 'bound': return { title: '收件人已绑定', detail: '可发送一条文本测试；平台受理不代表对方已读。' };
    case 'sending': return { title: '正在发送文本测试', detail: '请在微信中核对；平台受理不代表对方已读。' };
    case 'paused': return { title: '连接已暂停', detail: '检查会话状态后手动恢复，不会在后台不断重试。' };
    default: return { title: '连接需要处理', detail: '请检查网络或重新连接，诊断中不显示身份凭据。' };
  }
}
export function validWeixinVerifyCode(code: string): boolean { return /^[0-9]{1,16}$/.test(code.trim()); }
// Opaque payload for local QR encoding; never a remote image src or HTML.
export function weixinQrPayload(value: string | undefined): string | null {
  return value && value.length <= 8192 && !/[\u0000-\u001f\u007f]/.test(value) ? value : null;
}
export function weixinLabRemaining(expiresAt: number | undefined, now: number): string {
  if (!expiresAt) return '';
  const seconds = Math.ceil((expiresAt * 1000 - now) / 1000);
  return seconds > 0 ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}` : '已过期';
}
export function weixinLabTime(value: number | undefined): string {
  return value ? new Date(value * 1000).toLocaleString('zh-CN', { hour12: false }) : '尚无记录';
}
export function weixinTestStatus(value: string | undefined): string {
  return value === 'accepted' ? '平台已受理 · 非已读' : value === 'pending' ? '正在发送测试' : value === 'failed' || value === 'unknown' ? '测试结果未确认，请在微信核对后再重试' : '尚未发送测试';
}
