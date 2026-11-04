import test from 'node:test';
import assert from 'node:assert/strict';
import { validWeixinVerifyCode, weixinLabRemaining, weixinLabStatus, weixinLabStep, weixinQrPayload, weixinTestStatus, type WeixinLabSnapshot } from '../src/weixin-lab-model.ts';
const state = (status: WeixinLabSnapshot['status'], patch: Partial<WeixinLabSnapshot> = {}): WeixinLabSnapshot => ({ status, busy:false, active:false, logged_in:false, bound:false, restored:false, qrcode_img_content:'', qr_expires_at:0, binding_code:'', binding_expires_at:0, recipient_masked:'', last_received_at:0, last_sent_at:0, error:'', last_test_status:'', pause_until:0, ...patch });
test('weixin lab steps distinguish login binding and real test readiness', () => {
  assert.equal(weixinLabStep(null), 0);
  assert.equal(weixinLabStep(state('restored', { logged_in:true, bound:true })), 0);
  assert.equal(weixinLabStep(state('logged_in', {logged_in:true})), 1);
  assert.equal(weixinLabStep(state('binding', {logged_in:true})), 1);
  assert.equal(weixinLabStep(state('bound', {logged_in:true,bound:true})), 2);
});
test('restored state requires user opt in and accepted is not read', () => {
  assert.match(weixinLabStatus(state('restored')).detail, /需要你确认恢复/);
  assert.match(weixinLabStatus(state('bound')).detail, /不代表对方已读/);
  assert.match(weixinTestStatus('accepted'), /非已读/);
  assert.match(weixinTestStatus('unknown'), /未确认/);
});
test('QR payload is bounded opaque text for local encoding not a remote image', () => {
  assert.equal(weixinQrPayload('https://example.org/weixin?q=opaque'), 'https://example.org/weixin?q=opaque');
  assert.equal(weixinQrPayload('opaque-login-value'), 'opaque-login-value');
  assert.equal(weixinQrPayload(undefined), null);
  assert.equal(weixinQrPayload('a'.repeat(8193)), null);
  assert.equal(weixinQrPayload('unsafe\nline'), null);
});
test('verification form matches backend digit limit and expiry uses server seconds', () => {
  assert.ok(validWeixinVerifyCode('123456'));
  assert.ok(!validWeixinVerifyCode('12a456'));
  assert.ok(!validWeixinVerifyCode(''));
  assert.ok(!validWeixinVerifyCode('1'.repeat(17)));
  assert.equal(weixinLabRemaining(130, 100000), '0:30');
  assert.equal(weixinLabRemaining(100, 100000), '已过期');
  assert.equal(weixinLabRemaining(undefined, 100000), '');
});
test('errors never echo backend diagnostic payload or secrets', () => {
  assert.ok(!JSON.stringify(weixinLabStatus(state('error', {error:'bot_token=private-value'}))).includes('private-value'));
});
