import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const ui=readFileSync(new URL('../src/DatabaseRepair.svelte',import.meta.url),'utf8');
const markup=ui.slice(ui.indexOf('<section'),ui.indexOf('<style>'));

test('repair loads its snapshot on mount and polls without replacing newer action results',()=>{
  assert.match(ui,/import \{ onMount \} from 'svelte'/);
  assert.match(ui,/onMount\(\(\)=>\{/);
  assert.match(ui,/await call<RepairSnapshot>\('database_repair',\{action:'snapshot'\}\)/);
  assert.match(ui,/if\(native\)void loop\(\)/);
  assert.match(ui,/observed!==revision/);
  assert.match(ui,/clearTimeout\(timer\);onbusychange\(false\)/);
});

test('single repair entry requires one confirmation and leaves restart ownership to host',()=>{
  assert.match(markup,/onclick=\{\(\)=>confirm=true\}/);
  assert.match(markup,/aria-label="修复并重启确认"/);
  assert.match(markup,/onclick=\{repairRestart\}>确认修复并重启/);
  assert.match(ui,/if\(!confirm\|\|busy\|\|blocked\|\|!native\|\|!loaded\|\|!known\|\|error\)return/);
  assert.match(ui,/await call<RepairSnapshot>\('database_repair',\{action:'repair_restart',confirmed:true\}\)/);
  assert.equal((ui.match(/action:'repair_restart'/g)||[]).length,1);
  assert.doesNotMatch(ui,/recover_copy|action:'inspect'|host_action|\baction\(|location\.reload|window\.close|relaunch|process\.exit/);
  assert.match(ui,/event\.preventDefault\(\);event\.stopPropagation\(\);confirm=false/);
});

test('all repair lifecycle states have plain language presentation and uncertain recovery is not success',()=>{
  for(const status of ['not_started','preparing','ready','blocked','failed','committed','awaiting_health','completed']){
    assert.ok(ui.includes(`${status}:`),status);
  }
  assert.match(ui,/activeStatuses:RepairStatus\[\]=\['preparing','ready','committed','awaiting_health'\]/);
  assert.match(ui,/onbusychange\(busy\)/);
  assert.match(ui,/snapshot\?\.status==='completed'/);
  assert.match(ui,/已恢复 \$\{recoveredCount\} 个任务，重启后的检查已通过/);
  assert.match(ui,/snapshot\?\.status==='committed'\|\|snapshot\?\.status==='awaiting_health'/);
  assert.match(ui,/\$\{recoveredCount\} 个任务等待验证，目前还不能确认修复成功/);
  assert.match(ui,/Number\.isSafeInteger\(snapshot\.recovered_tasks\)/);
  assert.match(ui,/Object\.hasOwn\(labels,snapshot\.status\)/);
});

test('blocked and failure views preserve records and sessions without exposing backend errors',()=>{
  assert.match(ui,/blocked\?:boolean;onbusychange\?:\(v:boolean\)=>void/);
  assert.match(markup,/disabled=\{blocked\|\|busy\|\|!native\|\|!loaded\|\|!known\|\|!!error\}/);
  assert.match(markup,/原始记录仍保留，正在进行的会话未被结束/);
  assert.match(markup,/原始记录仍保留，正在进行的会话不会被结束/);
  assert.match(ui,/catch\{/);
  assert.doesNotMatch(ui,/String\(e\)|error\.message|JSON\.stringify|\{snapshot\.(message|details)\}/);
  assert.doesNotMatch(markup,/数据库|WAL|完整性|recover_copy|inspect|control\//);
  assert.match(markup,/role="alert"/);
  assert.match(markup,/aria-live="polite"/);
});
