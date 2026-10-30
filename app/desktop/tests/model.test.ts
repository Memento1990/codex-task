import test from 'node:test';
import assert from 'node:assert/strict';
import { elapsed, safeReference, visibleTasks, type Task } from '../src/model.ts';
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
test('evidence references reject executable schemes, credentials and UNC',()=>{
  assert.equal(safeReference('https://example.test/a'),true);
  assert.equal(safeReference('https://user:pass@example.test'),false);
  assert.equal(safeReference('javascript:alert(1)'),false);
  assert.equal(safeReference('C:\\reports\\result.pdf'),true);
  assert.equal(safeReference('C:\\tools\\run.exe'),false);
  assert.equal(safeReference('\\\\server\\share\\a.html'),false);
});
