import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const source=(p:string)=>readFileSync(new URL(p,import.meta.url),'utf8');
test('formal Weixin is reachable outside DEV lab and requires explicit confirmation',()=>{
 const ui=source('../src/Notifications.svelte');const channel=source('../src/WeixinChannel.svelte');
 assert.match(ui,/WeixinChannel snapshot=/);assert.doesNotMatch(ui,/暂不可用 · 待接入协议/);
 for(const term of ['notification_weixin_action','notification_channel_save','notification_test','确认启用','状态未知']){
  if(term==='确认启用')assert.match(channel,/确认.*启用/);else assert.ok(channel.includes(term));
 }
 assert.doesNotMatch(channel,/error\s*=\s*String\(/);
});
