import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const ui=readFileSync(new URL('../src/DatabaseRepair.svelte',import.meta.url),'utf8');
const dialog=readFileSync(new URL('../src/UtilityDialog.svelte',import.meta.url),'utf8');
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
  assert.match(markup,/\{#if confirm\}\s*<UtilityDialog title="修复并重启看板？" locked=\{busy\|\|blocked\} onClose=\{\(\)=>confirm=false\}/);
  assert.match(markup,/onclick=\{repairRestart\}>确认修复并重启/);
  assert.match(ui,/if\(!confirm\|\|busy\|\|blocked\|\|!native\|\|!loaded\|\|!known\|\|error\)return/);
  assert.match(ui,/await call<RepairSnapshot>\('database_repair',\{action:'repair_restart',confirmed:true\}\)/);
  assert.equal((ui.match(/action:'repair_restart'/g)||[]).length,1);
  assert.doesNotMatch(ui,/recover_copy|action:'inspect'|host_action|\baction\(|location\.reload|window\.close|relaunch|process\.exit/);
  assert.match(dialog,/use:modalFocus/);
  assert.match(dialog,/if\(event.key==='Escape'\)\{event\.preventDefault\(\);event\.stopPropagation\(\);close\(\);\}/);
  assert.match(dialog,/function close\(\)\{if\(!locked\)onClose\(\);\}/);
});

test('all repair lifecycle states have plain language presentation and uncertain recovery is not success',()=>{
  for(const status of ['not_started','preparing','ready','blocked','failed','committed','awaiting_health','health_failed','completed']){
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
  assert.match(markup,/snapshot\.recovery_mode==='full'/);
  assert.match(markup,/事件、耗时和历史归档均已保留/);
  assert.match(markup,/部分抢救只保留副本并停止，不自动启用/);
});

test('blocked and failure views preserve records and render host-safe stage messages as escaped text',()=>{
  assert.match(ui,/blocked\?:boolean;dataDirectory\?:string;onbusychange\?:\(v:boolean\)=>void/);
  assert.match(markup,/disabled=\{blocked\|\|busy\|\|!native\|\|!loaded\|\|!known\|\|!!error\}/);
  assert.match(markup,/原始记录仍保留，正在进行的会话未被结束/);
  assert.match(markup,/原始记录仍保留，正在进行的会话不会被结束/);
  assert.match(ui,/catch\{/);
  assert.match(markup,/snapshot\?\.message\|\|/);
  assert.doesNotMatch(ui,/String\(e\)|error\.message|JSON\.stringify|\{@html/);
  assert.doesNotMatch(markup,/数据库|WAL|完整性|recover_copy|inspect|control\//);
  assert.match(markup,/role="alert"/);
  assert.match(markup,/aria-live="polite"/);
  assert.match(markup,/\{actionError\}/);
  assert.match(markup,/\{snapshot\.last_attempt\.message\|\|snapshot\.last_attempt\.reason\}/);
  const polling=ui.slice(ui.indexOf('async function loadSnapshot'),ui.indexOf('function reason'));
  assert.doesNotMatch(polling,/actionError\s*=/);
});

test('repair state and original directory remain visible outside the confirmation dialog',()=>{
  const base=markup.slice(0,markup.indexOf('{#if confirm}'));
  assert.match(base,/<section class="settings-section"><h3 id="repair-heading">修复状态/);
  assert.match(base,/原始记录目录/);
  assert.match(base,/snapshot\?\.source\|\|dataDirectory\|\|'目录暂不可用'/);
  assert.doesNotMatch(ui,/call[^\n]+data_location/);
});
