import type { Timer } from './model';

export function compactTime(timer: Timer | undefined, now: number): string {
  if (!timer) return '—';
  if (timer.started_at === null && !timer.partial) return '未开始';
  const seconds = Math.max(0, Math.floor(timer.seconds + (timer.since === null ? 0 : now - timer.since)));
  if (!Number.isFinite(seconds) || timer.started_at === null && seconds === 0) return '—';
  const days = Math.floor(seconds / 86400), hours = Math.floor(seconds / 3600), minutes = Math.floor(seconds / 60);
  const value = days ? `${days}d` : hours ? `${hours}h${String(minutes % 60).padStart(2, '0')}m` : minutes ? `${minutes}m` : `${seconds}s`;
  return (timer.partial ? '≥ ' : '') + value;
}

export function firstRecorded(at: number | null | undefined, now: number): string {
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return '时间未知';
  const seconds = Math.max(0, Math.floor(now - at));
  return seconds < 60 ? '刚刚' : seconds < 3600 ? `${Math.floor(seconds / 60)} 分钟前` : seconds < 86400 ? `${Math.floor(seconds / 3600)} 小时前` : `${Math.floor(seconds / 86400)} 天前`;
}

export function recordedDate(at: number | null | undefined): string {
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return '时间未记录';
  const date = new Date(at * 1000), pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function modalFocus(node: HTMLElement) {
  const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const focusable = () => [...node.querySelectorAll<HTMLElement>('button,input,select,textarea,a[href],[tabindex="0"]')].filter(el => !el.matches(':disabled') && !el.closest('[inert]') && el.getClientRects().length);
  queueMicrotask(() => (node.querySelector<HTMLElement>('[data-initial-focus]') || focusable()[0] || node).focus());
  const trap = (event: KeyboardEvent) => {
    if (event.key !== 'Tab') return;
    const elements = focusable(), first = elements[0], last = elements.at(-1);
    if (!elements.length) { event.preventDefault(); node.focus(); }
    else if (!node.contains(document.activeElement) || !event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
  };
  node.addEventListener('keydown', trap);
  return { destroy() { node.removeEventListener('keydown', trap); if (previous?.isConnected) previous.focus(); } };
}
