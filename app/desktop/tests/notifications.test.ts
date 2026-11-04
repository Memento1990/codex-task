import assert from 'node:assert/strict';
import test from 'node:test';
import {
  filterNotificationRecords, notificationChannelState, notificationEventName, notificationPreview,
  notificationRecordState, notificationTime, safeNotificationError, validNotificationWebhook,
  type NotificationChannel, type NotificationRecord,
} from '../src/notification-model.ts';

test('only platform HTTPS robot URLs without credentials or extra components are accepted', () => {
  assert.equal(validNotificationWebhook('wecom', 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=example'), true);
  assert.equal(validNotificationWebhook('feishu', 'https://open.feishu.cn/open-apis/bot/v2/hook/abc-123_xyz'), true);
  for (const url of [
    'http://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x',
    'https://qyapi.weixin.qq.com.evil.test/cgi-bin/webhook/send?key=x',
    'https://qyapi.weixin.qq.com@evil.test/cgi-bin/webhook/send?key=x',
    'https://user:secret@qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x',
    'https://qyapi.weixin.qq.com:8443/cgi-bin/webhook/send?key=x',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send/extra?key=x',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x&key=y',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x&extra=y',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x#fragment',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x.y',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=%61',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/./send?key=x',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/foo/../send?key=x',
    'https://qyapi.weixin.qq.com\\cgi-bin/webhook/send?key=x',
    ' https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x',
    'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=x\n',
  ]) assert.equal(validNotificationWebhook('wecom', url), false, url);
  for (const url of [
    'https://open.feishu.cn/open-apis/bot/v2/hook/',
    'https://open.feishu.cn/open-apis/bot/v2/hook/token/more',
    'https://open.feishu.cn/open-apis/bot/v2/hook/token?key=x',
    'https://open.feishu.cn/open-apis/bot/v2/hook/token#fragment',
    'https://evil.test/open-apis/bot/v2/hook/token',
    'https://open.feishu.cn/open-apis/bot/v2/hook/%2Ftoken',
  ]) assert.equal(validNotificationWebhook('feishu', url), false, url);
  assert.equal(validNotificationWebhook('wechat', 'https://open.feishu.cn/open-apis/bot/v2/hook/token'), false);
  assert.equal(validNotificationWebhook('wecom', 'not a url'), false);
});

test('accepted means platform acceptance, never read; unknown status remains unknown', () => {
  assert.deepEqual(notificationRecordState('accepted'), { label: '平台已受理 · 非已读', tone: 'success' });
  assert.equal(notificationRecordState('failed').tone, 'warning');
  assert.equal(notificationRecordState('pending').label, '等待投递');
  assert.equal(notificationRecordState('expired').label, '已过期 · 未投递');
  assert.equal(notificationRecordState('new-status').label, '状态待确认');
});

test('configuration cannot imply delivery and WeChat requires explicit binding and enablement', () => {
  const channel: NotificationChannel = { id: 'wecom', name: '企微', enabled: true, configured: true, status: 'ready', keyword: '', has_signing_secret: false };
  assert.equal(notificationChannelState(channel), '已配置 · 不代表测试通过');
  assert.equal(notificationChannelState({ ...channel, id: 'wechat' }), '已启用 · 请留意会话状态');
  assert.equal(notificationChannelState({ ...channel, id: 'wechat',configured:false }), '扫码绑定后启用');
  assert.equal(notificationChannelState({ ...channel, id: 'wechat',enabled:false }), '已绑定 · 未启用');
  assert.deepEqual(notificationRecordState('unknown'),{label:'结果未知 · 不自动重发',tone:'warning'});
  assert.equal(notificationChannelState({ ...channel, configured: false }), '未配置');
  assert.equal(notificationChannelState({ ...channel, enabled: false }), '已配置 · 已停用');
});

test('failed filter excludes pending and accepted records without mutating input', () => {
  const records: NotificationRecord[] = ['pending', 'accepted', 'failed'].map((status, i) => ({ id: String(i), created_at: 0, channel: 'wecom', event: 'test', title: '示例', status, attempts: 1, error: null }));
  assert.deepEqual(filterNotificationRecords(records, true).map(record => record.id), ['2']);
  assert.equal(filterNotificationRecords(records, false), records);
  assert.equal(records.length, 3);
});

test('preview is explicitly generic and uses no local paths, code or logs', () => {
  for (const event of ['blocker_opened', 'task_completed', 'blocker_resolved'] as const) {
    assert.match(notificationPreview(event).title, /示例任务/);
    assert.equal(notificationPreview(event).body, '请在本机看板中查看任务状态并处理。');
  }
  assert.equal(notificationEventName('test'), '测试通知');
  assert.equal(notificationEventName('unknown'), '通知事件');
  assert.equal(notificationTime('invalid'), '时间未知');
});

test('raw backend errors never expose webhook credentials in UI', () => {
  const result = safeNotificationError('request failed https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=secret');
  assert.equal(result.includes('secret'), false);
  assert.equal(result.includes('https:'), false);
  assert.equal(safeNotificationError(null), '');
});
