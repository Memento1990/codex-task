import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('running task onboarding only guides the original task conversation',()=>{
  const source=readFileSync(new URL('../src/App.svelte',import.meta.url),'utf8');
  for(const text of ['复制同步指令','回到原任务聊天','等当前步骤结束后粘贴发送','无需新建或停止任务','任务真实汇报到达后，本页将继续验证'])assert.ok(source.includes(text),text);
  assert.doesNotMatch(source,/侧边会话|侧边提问/);
});

test('GUI version stays bound to package metadata rather than a literal or Reporter',()=>{
  const read=(name:string)=>readFileSync(new URL(name,import.meta.url),'utf8');
  const version=JSON.parse(read('../package.json')).version;
  assert.equal(JSON.parse(read('../src-tauri/tauri.conf.json')).version,version);
  assert.ok(read('../src/version.ts').includes('metadata.version'));
  for(const name of ['App.svelte','Updates.svelte']){
    const source=read('../src/'+name);
    assert.ok(source.includes('appVersion'));
    assert.doesNotMatch(source,/['">]\d+\.\d+\.\d+["'<]/);
  }
});
