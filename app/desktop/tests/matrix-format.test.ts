import test from 'node:test';
import assert from 'node:assert/strict';
import { compactTime, firstRecorded, recordedDate } from '../src/matrix-format.ts';

test('matrix elapsed keeps accumulated timer segments and freezes stopped timers', () => {
  const running = {started_at:10,ended_at:null,seconds:3540,since:100,partial:false};
  assert.equal(compactTime(running,220),'1h01m');
  assert.equal(compactTime({...running,ended_at:220,since:null,seconds:3660},100000),'1h01m');
  assert.equal(compactTime({...running,seconds:259200,since:null},100000),'3d');
});

test('matrix elapsed distinguishes missing, unstarted, and partially recorded histories', () => {
  assert.equal(compactTime(undefined,5000),'—');
  assert.equal(compactTime(null,5000),'—');
  const unknown = {started_at:null,ended_at:null,seconds:0,since:null,partial:true};
  assert.equal(compactTime(unknown,5000),'—');
  assert.equal(compactTime({...unknown,partial:false},5000),'未开始');
  assert.equal(compactTime({...unknown,seconds:125},5000),'≥ 2m');
  assert.equal(compactTime({...unknown,since:4970},5000),'≥ 30s');
});

test('evidence display uses first recorded time and leaves absent time unknown', () => {
  assert.equal(firstRecorded(undefined,5000),'时间未知');
  assert.equal(firstRecorded(null,5000),'时间未知');
  assert.equal(firstRecorded(0,5000),'时间未知');
  assert.equal(firstRecorded(4970,5000),'刚刚');
  assert.equal(firstRecorded(4880,5000),'2 分钟前');
  assert.equal(firstRecorded(1400,5000),'1 小时前');
  assert.equal(firstRecorded(86400,259200),'2 天前');
  assert.equal(recordedDate(undefined),'时间未记录');
  assert.equal(recordedDate(null),'时间未记录');
  const at = new Date(2026,10,5,9,7).getTime()/1000;
  assert.equal(recordedDate(at),'2026-11-05 09:07');
});
