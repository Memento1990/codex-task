import test from 'node:test';
import assert from 'node:assert/strict';
import { runningTaskSyncPrompt, isBlockerNotice, dragThresholdReached, dockSummaryMode, dockSummaryVisible, dockSummary, noticeAutoDismiss, normalizeTab, isReportedTask, taskTitle, type Notice, connectionTone, evidenceTime, safeUpdateUrl, updateBytes, updatePresentation, updateProgress, type UpdateSnapshot, connectionStatus, createTestNoticeFactory, defaults, diagnosticText, elapsed, noticeStack, safeReference, visibleTasks, summaryTone, taskCounts, matchesTask, filterTasks, type Task } from '../src/model.ts';
test('running task sync asks for a bound current snapshot without fabricated history',()=>{
  for(const text of ['本会话 Hook','report_progress','当前快照','不补造历史','不要猜会话 ID/token','中断任务'])assert.ok(runningTaskSyncPrompt.includes(text));
});
test('blocker reminder styling is independent from sticky persistence',()=>{
  const notice:Notice={task_id:'a',title:'Notice',lines:[],tab:0,sticky:true};
  assert.equal(isBlockerNotice(notice),false);assert.equal(noticeAutoDismiss(notice),false);
  assert.equal(isBlockerNotice({...notice,tab:1,sticky:false}),true);
});
test('drag starts only after four pixels while primary button is held',()=>{
  assert.equal(dragThresholdReached(10,10,13,10,1),false);
  assert.equal(dragThresholdReached(10,10,14,10,1),true);
  assert.equal(dragThresholdReached(10,10,13,13,1),true);
  assert.equal(dragThresholdReached(10,10,20,20,0),false);
  assert.equal(dragThresholdReached(10,10,20,20,2),false);
});
test('summary modes prefer new setting with legacy boolean fallback',()=>{
  assert.equal(dockSummaryMode({}),'hover');assert.equal(dockSummaryMode({dock_hover_summary:false}),'never');assert.equal(dockSummaryMode({dock_hover_summary:true}),'hover');
  for(const mode of ['always','hover','never'] as const)assert.equal(dockSummaryMode({dock_summary_mode:mode,dock_hover_summary:false}),mode);
  assert.equal(dockSummaryMode({dock_summary_mode:'invalid',dock_hover_summary:false}),'never');
  assert.equal(dockSummaryVisible('always',false),true);assert.equal(dockSummaryVisible('always',true),true);
  assert.equal(dockSummaryVisible('hover',false),false);assert.equal(dockSummaryVisible('hover',true),true);
  assert.equal(dockSummaryVisible('never',true),false);assert.equal(dockSummaryVisible('never',false),false);
});
test('dock summary uses focused live task and reports global blockers without averaging',()=>{
  const tasks=[{id:'a',title:'Focused',status:'running',plan:[{status:'completed'},{status:'inProgress'},{status:'pending'}],blockers:[],current_step:'Testing'},{id:'b',title:'Other',status:'waiting',plan:[{status:'completed'}],blockers:[{open:true},{open:false}]},{id:'old',archived:true,status:'completed',plan:[],blockers:[{open:true}]}] as unknown as Task[];
  const summary=dockSummary(tasks,'a');
  assert.equal(summary.title,'Focused');assert.equal(summary.progress,1/3);assert.equal(summary.blockers,1);assert.equal(summary.badge,'! 1');assert.equal(summary.active,2);assert.equal(summary.step,'Testing');
  assert.equal(dockSummary(tasks,'old').task?.id,'b');
});
test('dock never invents progress without a plan and excludes archived completion',()=>{
  const task={id:'live',title:'No plan',status:'running',plan:[],blockers:[]} as unknown as Task;
  assert.equal(dockSummary([task],'live').progress,null);assert.equal(dockSummary([task],'live').badge,'—');
  assert.equal(dockSummary([],'').allCompleted,false);
  assert.equal(dockSummary([{...task,status:'completed'}],'').badge,'✓');
  assert.equal(dockSummary([{...task,status:'completed',archived:true}],'').allCompleted,false);
  assert.equal(dockSummary([{...task,status:'unreported'}],'').active,0);
});
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
test('blocker notices remain manual until dismissed and ordinary previews remain bounded',()=>{
  const sticky:Notice={task_id:'blocker',title:'Blocked',lines:[],tab:1,sticky:true};
  const preview=createTestNoticeFactory();
  const ordinary=Array.from({length:6},()=>preview(defaults));
  const stack=ordinary.reduce((items,item)=>noticeStack(items,item),[sticky] as Notice[]);
  assert.equal(stack.length,5);
  assert.equal(stack[0],sticky);
  assert.equal(noticeAutoDismiss(sticky),false);
  assert.equal(noticeAutoDismiss(ordinary[0]),true);
  const updated=noticeStack(stack,{...sticky,title:'Resolved',sticky:false});
  assert.equal(updated.at(-1)?.sticky,true);
  assert.equal(noticeAutoDismiss(updated.at(-1)!),false);
  const afterManualClose=updated.filter(item=>item.task_id!=='blocker');
  assert.equal(noticeStack(afterManualClose,{...sticky,sticky:false}).at(-1)?.sticky,false);
  const manySticky=Array.from({length:8},(_,index)=>({...sticky,task_id:`blocker-${index}`})).reduce((items,item)=>noticeStack(items,item),[] as Notice[]);
  assert.equal(manySticky.length,8);
});
test('legacy activity tabs normalize to checklist without hiding completed data',()=>{
  for(const tab of [3,-1,4,null,undefined,'1',NaN])assert.equal(normalizeTab(tab),0);
  assert.equal(normalizeTab(1),1);
  assert.equal(normalizeTab(2),2);
});
test('hook-only placeholders stay hidden but plans and real progress remain visible',()=>{
  const tasks=[{id:'hook',status:'unreported',plan:[],title:'',blockers:[]},{id:'plan',status:'unreported',plan:[{id:'s'}],title:'',blockers:[]},{id:'progress',status:'running',plan:[],title:'Reported',blockers:[]},{id:'done',status:'completed',plan:[],title:'',blockers:[]}] as unknown as Task[];
  assert.deepEqual(tasks.filter(isReportedTask).map(task=>task.id),['plan','progress','done']);
  assert.deepEqual(visibleTasks(tasks,'all').map(task=>task.id),['plan','progress']);
  assert.equal(taskTitle(tasks[1]),'已同步计划');
  assert.equal(taskTitle(tasks[2]),'Reported');
});
test('matrix counts and filters use the same mutually exclusive business categories',()=>{
  const base={title:'',plan:[],blockers:[],evidence:[]} as unknown as Task;
  const tasks=[
    {...base,id:'hook',status:'unreported'},
    {...base,id:'plan',status:'unreported',plan:[{id:'s'}]},
    {...base,id:'running',status:'running'},
    {...base,id:'blocked',status:'running',blockers:[{open:true},{open:true}]},
    {...base,id:'waiting',status:'waiting'},
    {...base,id:'waiting-blocked',status:'waiting',blockers:[{open:true}]},
    {...base,id:'done',status:'completed',archived:true},
    {...base,id:'cancelled',status:'cancelled',blockers:[{open:true}]},
  ] as Task[];
  assert.equal(summaryTone(tasks[3]),'blocked');
  assert.equal(summaryTone(tasks[5]),'blocked');
  assert.equal(summaryTone(tasks[7]),'cancelled');
  assert.deepEqual(taskCounts(tasks),{running:1,blocked:2,waiting:1,done:1,unreported:1,cancelled:1,total:7,blockers:4});
  assert.deepEqual(filterTasks(tasks).map(task=>task.id),['plan','running','blocked','waiting','waiting-blocked']);
  assert.deepEqual(filterTasks(tasks,'active','',true).map(task=>task.id),['plan','running','blocked','waiting','waiting-blocked','done']);
  assert.deepEqual(filterTasks(tasks,'running').map(task=>task.id),['running']);
  assert.deepEqual(filterTasks(tasks,'blocked').map(task=>task.id),['blocked','waiting-blocked']);
  assert.deepEqual(filterTasks(tasks,'waiting','',true).map(task=>task.id),['waiting']);
  assert.deepEqual(filterTasks(tasks,'unreported').map(task=>task.id),['plan']);
  assert.deepEqual(filterTasks(tasks,'completed').map(task=>task.id),['done']);
  assert.equal(filterTasks(tasks,'all').length,7);
});
test('matrix search matches context and evidence with all query terms without changing totals',()=>{
  const task={id:'live',source_task_id:'source',title:'Release API',cwd:'D:/Project/Board',session_id:'session-alpha',host_id:'workstation',current_step:'Validating',status:'waiting',plan:[{title:'Compile'}],blockers:[{title:'Network approval',reason:'Firewall',owner:'Operations',open:true}],evidence:[{label:'Result',reference:'https://example.test/report'}]} as Task;
  assert.equal(matchesTask(task,' API   board '),true);
  for(const term of ['SOURCE','session-alpha','workstation','validating','compile','network','firewall','operations','example.test/report'])assert.equal(matchesTask(task,term),true);
  assert.equal(matchesTask(task,'API absent'),false);
  assert.equal(filterTasks([task],'blocked','approval api').length,1);
  assert.equal(filterTasks([task],'waiting','approval api').length,0);
  assert.equal(taskCounts([task]).blocked,1);
});
test('partial timer elapsed remains cumulative across paused intervals',()=>{
  assert.equal(defaults.theme,'dark');
  assert.equal(elapsed({started_at:10,ended_at:200,seconds:25,since:null,partial:true},1000),'已记录 25 秒');
  assert.equal(elapsed({started_at:null,ended_at:200,seconds:25,since:null,partial:true},1000),'已记录 25 秒');
  assert.equal(elapsed({started_at:null,ended_at:200,seconds:0,since:null,partial:true},1000),'起点未记录');
});
test('historical hooks and local config do not imply a verified current connection',()=>{
  assert.equal(connectionStatus({state:'ready',hook_seen:true}),'配置完成 · 连接待确认');
  assert.equal(connectionStatus({state:'session_received',hook_seen:true}),'会话事件已收到');
  assert.equal(connectionStatus({state:'sync_verified'}),'接入与同步已验证');
});
test('layered connection evidence does not confuse pending tasks with offline',()=>{
  assert.equal(connectionTone({state:'awaiting_reload'}),'neutral');
  assert.equal(connectionTone({state:'awaiting_session',mcp_connected:true}),'confirmed');
  assert.equal(connectionTone({state:'session_received'}),'confirmed');
  assert.equal(connectionStatus({state:'sync_verified',config_state:'partial'}),'配置需要检查');
  assert.equal(connectionTone({state:'sync_verified',config_state:'partial'}),'problem');
  assert.equal(evidenceTime(null),'未提供证据时间');
});
test('diagnostics preserve strings, arrays and structured details',()=>{
  assert.equal(diagnosticText(undefined),'');
  assert.equal(diagnosticText(['waiting','collector unavailable']),'waiting\ncollector unavailable');
  assert.match(diagnosticText({state:'pending'}),/pending/);
});
const update:UpdateSnapshot={current_version:'0.9.0',state:'idle',supported:true,downloaded_bytes:0,preferences:{auto_check:true,auto_download:true}};
test('update states have one safe primary action and never install automatically',()=>{
  assert.equal(updatePresentation({...update,state:'available'}).action,'download');
  assert.equal(updatePresentation({...update,state:'ready'}).action,'install');
  for(const state of ['checking','downloading','installing'])assert.equal(updatePresentation({...update,state}).action,null);
  assert.equal(updatePresentation({...update,state:'future'}).action,'check');
  assert.equal(updatePresentation({...update,state:'ready',supported:false}).action,null);
  assert.match(updatePresentation({...update,state:'unsupported'}).description,/签名/);
  assert.match(updatePresentation({...update,state:'ready'}).description,/不会结束 Codex/);
});
test('download progress handles unknown totals and clamps malformed values',()=>{
  assert.equal(updateProgress(10,null),null);
  assert.equal(updateProgress(10,0),null);
  assert.equal(updateProgress(10,NaN),null);
  assert.equal(updateProgress(25,100),25);
  assert.equal(updateProgress(200,100),100);
  assert.equal(updateProgress(-10,100),0);
  assert.equal(updateBytes(1048576),'1.0 MB');
  assert.equal(updateBytes(NaN),'0 B');
});
test('update links are restricted to official GitHub releases',()=>{
  assert.equal(safeUpdateUrl('https://github.com/Memento1990/codex-task/releases/latest'),true);
  for(const url of ['http://github.com/Memento1990/codex-task/releases/latest','https://github.com/other/repo/releases','https://github.com/Memento1990/codex-task/releases-evil','https://github.com.evil.test/Memento1990/codex-task/releases','https://user@github.com/Memento1990/codex-task/releases','javascript:alert(1)'])assert.equal(safeUpdateUrl(url),false);
});
test('evidence references reject executable schemes, credentials and UNC',()=>{
  assert.equal(safeReference('https://example.test/a'),true);
  assert.equal(safeReference('https://user:pass@example.test'),false);
  assert.equal(safeReference('javascript:alert(1)'),false);
  assert.equal(safeReference('C:\\reports\\result.pdf'),true);
  assert.equal(safeReference('C:\\tools\\run.exe'),false);
  assert.equal(safeReference('\\\\server\\share\\a.html'),false);
});
