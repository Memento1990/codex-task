import test from 'node:test';
import assert from 'node:assert/strict';
import { integrationStep, connectionStatus, createTestNoticeFactory, defaults, diagnosticText, elapsed, noticeStack, safeReference, visibleTasks, type Task } from '../src/model.ts';
test('human elapsed freezes completed timers and distinguishes unknown',()=>{
  assert.equal(elapsed(null,100),'起点未记录');
  assert.equal(elapsed({started_at:null,ended_at:null,seconds:0,since:null,partial:false},100),'未开始');
  assert.equal(elapsed({started_at:10,ended_at:null,seconds:10,since:50,partial:false},111),'1 分 11 秒');
  assert.equal(elapsed({started_at:10,ended_at:70,seconds:60,since:null,partial:false},5000),'1 分');
});
test('filters completed separately and uses open blocker semantics',()=>{
  const tasks=[{id:'a',status:'running',blockers:[]},{id:'b',status:'waiting',blockers:[{open:true}]},{id:'c',status:'completed',blockers:[]}] as unknown as Task[];
  assert.deepEqual(visibleTasks(tasks,'all').map(t=>t.id),['a','b']);
  assert.deepEqual(visibleTasks(tasks,'blocked').map(t=>t.id),['b']);
  assert.deepEqual(visibleTasks(tasks,'running').map(t=>t.id),['a']);
});
test('notification previews are independent and retain only the latest four',()=>{
  const preview=createTestNoticeFactory();
  const notices=Array.from({length:6},()=>preview({...defaults,notification_seconds:9,theme:'dark'}));
  assert.equal(new Set(notices.map(item=>item.task_id)).size,6);
  assert.equal(notices[0].seconds,9);
  assert.equal(notices[0].theme,'dark');
  const stack=notices.reduce((items,item)=>noticeStack(items,item),[] as typeof notices);
  assert.deepEqual(stack.map(item=>item.task_id),notices.slice(-4).map(item=>item.task_id));
  const replaced=noticeStack(stack,{...stack[0],title:'updated'});
  assert.equal(replaced.length,4);
  assert.equal(replaced.at(-1)?.title,'updated');
});
test('historical hooks and local config do not imply a verified current connection',()=>{
  for(const state of ['ready','partial','awaiting_reload','awaiting_session','session_received','error']){
    assert.equal(integrationStep({state,hook_seen:true,mcp_connected:true}).verified,false);
  }
  assert.equal(connectionStatus({state:'ready',hook_seen:true}),'连接待验证');
  assert.equal(connectionStatus({state:'session_received',hook_seen:true}),'已收到当前会话');
  assert.equal(connectionStatus({state:'sync_verified'}),'同步已验证');
  assert.equal(integrationStep({state:'sync_verified'}).verified,true);
});
test('wizard provides one safe next action for each connection phase',()=>{
  assert.equal(integrationStep({state:'missing'}).action,'connect');
  assert.equal(integrationStep({state:'recovery_required',recovery_required:true}).action,'recover');
  assert.equal(integrationStep({state:'conflict'}).action,'details');
  for(const state of ['awaiting_reload','awaiting_session','session_received','error','future_state'])assert.equal(integrationStep({state}).action,'check');
  assert.equal(integrationStep({state:'sync_verified'}).action,'done');
  assert.match(integrationStep({state:'missing'}).description,/当前用户目录安装/);
  assert.match(integrationStep({state:'awaiting_reload'}).description,/稍后完成/);
});
test('diagnostics preserve strings, arrays and structured details',()=>{
  assert.equal(diagnosticText(undefined),'');
  assert.equal(diagnosticText(['waiting','collector unavailable']),'waiting\ncollector unavailable');
  assert.match(diagnosticText({state:'pending'}),/pending/);
});
test('evidence references reject executable schemes, credentials and UNC',()=>{
  assert.equal(safeReference('https://example.test/a'),true);
  assert.equal(safeReference('https://user:pass@example.test'),false);
  assert.equal(safeReference('javascript:alert(1)'),false);
  assert.equal(safeReference('C:\\reports\\result.pdf'),true);
  assert.equal(safeReference('C:\\tools\\run.exe'),false);
  assert.equal(safeReference('\\\\server\\share\\a.html'),false);
});
