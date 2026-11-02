<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { action, call, native, onHost, saveOptions } from './host';
  import { connectionStatus, integrationStep, createTestNoticeFactory, defaults, diagnosticText, done, elapsed, noticeStack, openBlockers, runtime, status, visibleTasks, type Connection, type DataLocation, type Notice, type Options, type Snapshot, type Task } from './model';
  import Icon from './Icon.svelte';
  import TaskDetail from './TaskDetail.svelte';
  type Scope = {task_id?: string; scope?: string; tab?: number};
  const initial = (window as unknown as {__FLOATBOARD_VIEW__?:{view:string;value?:Scope;notice?:unknown}}).__FLOATBOARD_VIEW__ || {view:'main'};
  const demo = new URLSearchParams(location.search).has('demo');
  let view = $state(initial.view);
  let tasks = $state<Task[]>([]), options = $state<Options>({...defaults}), healthy = $state(false), connection = $state<Connection>({state:'missing'});
  let selected = $state(''), tab = $state(view === 'main' ? 1 : 0), filter = $state('all'), scope = $state<Scope>(initial.value || {});
  let now = $state(Date.now()/1000), error = $state(''), pinned = $state(true), menu = $state(false), loading = $state(true);
  let dialog = $state<'settings'|'integration'|null>(null), draft = $state<Options>({...defaults}), busy = $state(false), note = $state(''), advanced = $state(false), disconnectConfirm = $state(false);
  let changes = $state<{title:string;key:string;before:string;after:string;diff:string}[]>([]);
  let demoHistory = $state(false), scopeTask = $state('');
  let maximized = $state(false), diagnostics = $state(''), connectionError = $state(''), recoveryConfirm = $state(false), recoveryPreviewReady = $state(false);
  let wizard = $derived(integrationStep(connection));
  const migrationWarning = '迁移前先用新版重新连接，然后完整退出所有 Codex / app-server 会话及其他看板实例，暂停全部上报；迁移不是在线操作。完成后重启看板，再启动 Codex 并按正常流程信任新版 Hook。旧版 Hook 不支持迁移后的版本恢复。必须保留旧目录用于路径重定向，不可删除。';
  let dataLocation = $state<DataLocation>({current:'',restart_required:false}), dataPath = $state(''), dataNote = $state(''), dataLoading = $state(false), dataBusy = $state(false), migrateConfirm = $state(false), producersStopped = $state(false);
  const previewNotice = createTestNoticeFactory();
  let connectionLabel = $derived(loading?'正在读取':!healthy?'采集离线':connectionStatus(connection));
  type Toast = Notice & {key:number;remaining:number;started:number;timer?:ReturnType<typeof setTimeout>};
  let toasts = $state<Toast[]>([]), toastKey = 0;
  let live = $derived(view === 'completed' || demoHistory ? tasks.filter(t => scope.scope ? t.id === scope.scope : t.status === 'completed') : visibleTasks(tasks,filter));
  let current = $derived(live.find(t => t.id === selected) || live[0]);
  let completed = $derived(tasks.filter(t => t.status === 'completed'));
  let active = $derived(tasks.filter(t => t.status !== 'completed'));
  let blocked = $derived(active.filter(t => openBlockers(t).length > 0));
  function trapDialog(node: HTMLDivElement) {
    const previous=document.activeElement as HTMLElement | null;
    const focusable=()=>Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),summary')).filter(el=>el.offsetParent!==null);
    queueMicrotask(()=>focusable()[0]?.focus());
    const keydown=(event:KeyboardEvent)=>{if(event.key!=='Tab')return;const items=focusable(),first=items[0],last=items.at(-1);if(!first){event.preventDefault();node.focus();}else if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}};
    node.addEventListener('keydown',keydown);
    return {destroy(){node.removeEventListener('keydown',keydown);previous?.focus();}};
  }

  function choose(task: Task) { selected=task.id; tab=view === 'main' && !demoHistory && openBlockers(task).length ? 1 : 0; saveWindow(); }
  function setFilter(value:string){filter=value;const candidates=visibleTasks(tasks,value);const task=candidates.find(t=>t.id===selected)||candidates[0];if(task)choose(task);else saveWindow();}
  function saveWindow() { if(native && view === 'main') void call('window_save',{selected,tab,filter,pinned}); }
  async function openCompleted(value:Scope = {}) {
    if(native) await action('completed',value); else { demoHistory=true;scope=value;selected=value.task_id || value.scope || ''; tab=value.tab || 0; }
  }
  function settings() {menu=false;draft={...options};note='';dataNote='';migrateConfirm=false;producersStopped=false;dialog='settings';void loadDataLocation();}
  async function loadDataLocation() {
    dataLoading=true;
    try {dataLocation=native?await call<DataLocation>('data_location'):{current:'演示模式 · 无本地数据目录',restart_required:false};dataPath=dataLocation.current;}
    catch(e){dataNote='读取数据目录失败：'+String(e);}finally{dataLoading=false;}
  }
  async function migrateData() {
    dataBusy=true;dataNote='';
    try {dataLocation=await call<DataLocation>('migrate_data',{path:dataPath.trim(),producers_stopped:producersStopped});dataPath=dataLocation.current;migrateConfirm=false;dataNote=dataLocation.restart_required?'迁移完成。请重启看板，保留旧目录用于路径重定向，不可删除。':'数据目录已更新。';}
    catch(e){dataNote='迁移失败：'+String(e);}finally{dataBusy=false;}
  }
  async function windowAction(name:string) {
    try {if(native){const result=await action(name) as {maximized?:boolean}|null;if(typeof result?.maximized==='boolean')maximized=result.maximized;}else if(name==='toggle_maximize')maximized=!maximized;}
    catch(e){if(name==='restart' || name==='data_folder')dataNote='操作失败：'+String(e);else error='窗口操作失败：'+String(e);}
  }
  function dragToolbar(event:MouseEvent) {
    if(event.button!==0)return;
    if(event.detail===2){event.preventDefault();void windowAction('toggle_maximize');}
    else void windowAction('drag');
  }
  async function integration() {menu=false;note='';advanced=false;dialog='integration';disconnectConfirm=false;recoveryConfirm=false;recoveryPreviewReady=false;changes=[];}
  async function runConnect(remove=false,confirmRecovery=false) {
    if(busy)return;
    busy=true;note='';
    try { const result=native?await call<{message?:string;success?:boolean}>(remove?'disconnect':'connect',remove?{}:{confirm_recovery:confirmRecovery}):{success:true,message:'隔离演示：未修改本机配置。'}; note=result?.message || (remove?'已断开连接':'接入操作已返回，正在检查实际状态。');disconnectConfirm=false;if(result?.success!==false)recoveryConfirm=false; await refresh(); if(advanced)await loadDiff(); }
    catch(e){note='操作失败：'+String(e);} finally{busy=false;}
  }
  async function previewRecovery() {
    busy=true;note='';recoveryConfirm=true;recoveryPreviewReady=false;
    try{changes=native?(await call<{changes:typeof changes}>('preview',{remove:false})).changes:[];recoveryPreviewReady=true;}
    catch(e){note='无法预览恢复内容，未更改配置：'+String(e);}finally{busy=false;}
  }
  async function integrationNext(){
    if(wizard.action==='connect')await runConnect();
    else if(wizard.action==='recover')await previewRecovery();
    else if(wizard.action==='details'){advanced=true;}
    else if(wizard.action==='done')dialog=null;
    else {busy=true;note='';try{await refresh();note=error || '检查完成；后续将继续自动验证。';}finally{busy=false;}}
  }
  async function loadDiff(remove=false) {advanced=true;try{changes=native?(await call<{changes:typeof changes}>('preview',{remove})).changes:[];}catch(e){note=String(e);}}
  async function confirmDisconnect() {disconnectConfirm=true;await loadDiff(true);}
  async function saveSettings() {
    if(dataLocation.restart_required){note='数据已迁移，请先重启看板后再保存设置。';return;}
    busy=true;note='';
    try {options=native?await saveOptions(draft):{...draft};dialog=null;}catch(e){note='保存失败：'+String(e);}finally{busy=false;}
  }
  async function copy(text:string){if(native)await action('copy',text);else await navigator.clipboard.writeText(text).catch(()=>{});}
  async function closeView(){if(native)await action('hide');else {demoHistory=false;scope={};selected=scopeTask;}}
  function resetScope(value:Scope){scope=value;selected=value.task_id || value.scope || '';tab=value.tab || 0;}
  async function refresh() {
    if(view === 'toast' || !native) return;
    try { const snapshot=await call<Snapshot>('snapshot',{notify:view==='main'}); tasks=snapshot.tasks;options=snapshot.options;healthy=snapshot.healthy;connection=snapshot.connection;connectionError=snapshot.connection.error || snapshot.connection.hook_error || '';diagnostics=diagnosticText(snapshot.connection.diagnostics || snapshot.diagnostics);now=snapshot.now;error='';loading=false;
      if(!live.some(t=>t.id===selected)){const first=view==='main'?(live.find(t=>openBlockers(t).length)||live[0]):live[0];selected=first?.id||'';if(first)tab=view==='main'&&openBlockers(first).length?1:0;}
      if(view==='main')for(const notice of snapshot.notices)await action('toast',{...notice,seconds:options.notification_seconds,theme:options.theme,completed:tasks.find(t=>t.id===notice.task_id)?.status==='completed'});
    }catch(e){error='数据暂不可读 · 保留最近快照';loading=false;healthy=false;}
  }
  function removeToast(key:number){const item=toasts.find(t=>t.key===key);if(item?.timer)clearTimeout(item.timer);toasts=toasts.filter(t=>t.key!==key);void resizeToast();}
  function pauseToast(item:Toast){if(item.timer){clearTimeout(item.timer);item.timer=undefined;item.remaining=Math.max(0,item.remaining-(Date.now()-item.started));}}
  function resumeToast(item:Toast){if(!item.timer){item.started=Date.now();item.timer=setTimeout(()=>removeToast(item.key),item.remaining);}}
  async function resizeToast(){await tick();if(native){const stack=document.querySelector('.toast-stack');const limit=window.screen.availHeight-24;while(toasts.length>1&&stack&&stack.scrollHeight>limit){const old=toasts[0];if(old.timer)clearTimeout(old.timer);toasts=toasts.slice(1);await tick();}if(toasts.length)await action('toast_size',stack?.scrollHeight || 1);else await action('hide');}}
  function addToast(value:Notice & {seconds?:number;theme?:string;completed?:boolean}){
    if(value.theme)options={...options,theme:value.theme};
    const old=toasts.find(t=>t.task_id===value.task_id);if(old?.timer)clearTimeout(old.timer);
    toasts=toasts.filter(t=>t.task_id!==value.task_id);
    const item:Toast={...value,key:++toastKey,remaining:(value.seconds || 3)*1000,started:Date.now()};
    const next=noticeStack(toasts,item);
    for(const previous of toasts)if(!next.includes(previous)&&previous.timer)clearTimeout(previous.timer);
    toasts=next;resumeToast(item);void resizeToast();
  }
  async function testNotice(){if(!draft.notifications){note='请先开启消息浮窗通知';return;}try{const value=previewNotice(draft);if(native)await action('toast',value);else note='通知预览：'+draft.notification_seconds+' 秒';}catch(e){note='通知发送失败：'+String(e);}}
  onMount(()=>{
    let stopped=false;let unlisten=()=>{};let timer:ReturnType<typeof setTimeout>;
    async function loop(){await refresh();if(!stopped)timer=setTimeout(loop,700);}
    if(native){
      if(view==='toast')document.documentElement.style.background='transparent';
      void onHost(value=>{
        if(value.type==='settings')settings();
        if(value.type==='window_state'){const state=(value.value || value) as {maximized?:boolean};if(typeof state.maximized==='boolean')maximized=state.maximized;}
        if(value.type==='scope')resetScope(value.value as Scope);
        if(value.type==='select'){const v=value.value as Scope;selected=v.task_id || selected;filter='all';tab=v.tab || 0;}
        if(value.type==='options')options=value.options as Options;
        if(value.type==='notice' && view==='toast')addToast(value.value as Notice);
        if(value.type==='clear')for(const toast of [...toasts])removeToast(toast.key);
      }).then(async fn=>{if(stopped){fn();return;}unlisten=fn;if(view==='toast')await action('toast_ready');}).catch(e=>{error='窗口事件监听失败：'+String(e);});
      if(view!=='toast')void windowAction('window_state');
      if(view==='main')void call<Record<string,unknown>>('window_load').then(state=>{selected=String(state.selected || '');pinned=state.pinned!==false;filter=String(state.filter || 'all');if(typeof state.tab==='number')tab=state.tab;});
      if(view==='toast' && initial.notice)addToast(initial.notice as Notice);
      void loop();
    }else if(demo){void fetch('/fixture.json').then(r=>r.json()).then((snapshot:Snapshot)=>{tasks=snapshot.tasks;options=snapshot.options;healthy=true;connection=snapshot.connection || {state:'missing'};now=snapshot.now;selected=tasks.find(t=>openBlockers(t).length)?.id || tasks[0]?.id || '';loading=false;});}
    else {loading=false;error='请从桌面程序启动';}
    const clock=setInterval(()=>{now=Date.now()/1000;},1000);
    return ()=>{stopped=true;clearTimeout(timer);clearInterval(clock);unlisten();for(const toast of toasts)if(toast.timer)clearTimeout(toast.timer);};
  });
</script>

<svelte:window onkeydown={e=>{if(e.key==='Escape'){if(dialog&&!busy&&!dataBusy)dialog=null;else if(view==='completed'||demoHistory)void closeView();else menu=false;}}}/>
<div class="app" class:dark={options.theme==='dark'} class:toast-app={view==='toast'}>
{#if view==='toast'}
  <div class="toast-stack">{#each toasts as toast (toast.key)}<article class="toast" onpointerenter={()=>pauseToast(toast)} onpointerleave={()=>resumeToast(toast)}><button class="toast-open" onclick={()=>{void action('open_notice',toast);removeToast(toast.key);}}><strong>{toast.title}</strong>{#each toast.lines as line}<span>{line}</span>{/each}</button><button class="toast-close" aria-label="关闭通知" onclick={()=>removeToast(toast.key)}>×</button></article>{/each}</div>
{:else}
  <header class="toolbar">
    <!-- 专用拖动区不包含任何按钮，避免窗口拖动抢占控件点击。 -->
    <div class="toolbar-drag" role="presentation" onmousedown={dragToolbar}><span class="app-mark">&gt;_</span><strong>{view==='completed'||demoHistory?'已完成记录':'卡点调度台'}</strong><span class="connection" class:offline={!healthy || !wizard.verified} title={connectionError || connectionLabel}><i></i>{connectionLabel}</span></div>
    <div class="toolbar-actions">{#if view==='main'&&!demoHistory}<button class="icon-button" class:pressed={pinned} title={pinned?'取消置顶':'置顶'} aria-label={pinned?'取消置顶':'置顶'} onclick={()=>{pinned=!pinned;if(native)void action('pin',pinned);saveWindow();}} aria-pressed={pinned}><Icon name="pin"/></button><div class="menu-anchor"><button class="icon-button" title="更多操作" aria-label="更多操作" aria-expanded={menu} onclick={()=>menu=!menu}><Icon name="more"/></button>{#if menu}<div class="dropdown"><button onclick={settings}>设置</button><button onclick={integration}>Codex 接入</button><hr><button onclick={()=>{if(native)void action('quit');}}>退出看板</button></div>{/if}</div>{/if}</div>
    <div class="window-controls"><button title="最小化" aria-label="最小化" onclick={()=>windowAction('minimize')}><Icon name="minimize" size={14}/></button><button title={maximized?'还原':'最大化'} aria-label={maximized?'还原':'最大化'} onclick={()=>windowAction('toggle_maximize')}><Icon name={maximized?'restore':'maximize'} size={14}/></button><button class="window-close" title="关闭到托盘" aria-label={view==='completed'||demoHistory?'关闭已完成记录':'关闭到托盘'} onclick={closeView}><Icon name="close" size={14}/></button></div>
  </header>
  {#if error || connectionError}<div class="offline-banner" role="status">{error || connectionError}</div>{/if}
  {#if view==='main'&&!demoHistory}<nav class="filters" aria-label="任务筛选">{#each [{key:'all',label:'全部',count:active.length},{key:'blocked',label:'有卡点',count:blocked.length},{key:'running',label:'执行中',count:active.filter(t=>t.status==='running').length}] as item}<button class:active={filter===item.key} onclick={()=>setFilter(item.key)}>{item.label}<small>{item.count}</small></button>{/each}<span class="filter-meta">{tasks.reduce((sum,t)=>sum+openBlockers(t).length,0)} 个未解除卡点</span></nav>{/if}
  <main class="workspace" class:history={view==='completed'||demoHistory} class:steps-only={Boolean(scope.scope)&&(view==='completed'||demoHistory)}>
    {#if !scope.scope || view==='main'&&!demoHistory}<aside class="task-sidebar"><div class="list-caption">{view==='completed'||demoHistory?'已完成任务':'任务'}<span>{live.length}</span></div><div class="task-list" data-testid="task-list">{#each live as task (task.id)}<button class="task-row" class:selected={current?.id===task.id} onclick={()=>choose(task)}><div class="task-row-top"><span class="state-dot {task.status}"></span><strong>{task.title || '未汇报任务'}</strong></div><div class="task-row-meta"><span class="{task.status}">{status[task.status]}</span><span>{done(task)}/{task.plan.length}</span><span>{elapsed(task.timing,now)}</span></div>{#if openBlockers(task).length}<div class="task-blocked">! {openBlockers(task).length} 个卡点 · {openBlockers(task)[0].title}</div>{:else}<div class="task-current">{task.current_step || `Codex ${runtime[task.runtime]}`}</div>{/if}<div class="progress"><i style:width={`${task.plan.length?done(task)/task.plan.length*100:0}%`}></i></div></button>{/each}{#if !live.length}<div class="empty small">{loading?'正在读取任务':view==='completed'||demoHistory?'暂无已完成任务':'当前筛选下没有任务'}</div>{/if}</div>
      {#if view==='main'&&!demoHistory&&completed.length}<button class="completed-entry" onclick={()=>{scopeTask=selected;void openCompleted();}}><span>已完成任务 <b>{completed.length}</b></span><span>查看 ↗</span></button>{/if}
    </aside>{/if}
    {#if current}<TaskDetail task={current} bind:tab {now} completedOnly={view==='completed'||demoHistory} onCompleted={()=>{scopeTask=selected;void openCompleted({scope:current.id});}}/>{:else}<section class="empty detail-empty">{#if loading}正在读取任务{:else if tasks.length&&view==='main'&&!demoHistory}{active.length?'当前筛选下没有任务':'全部任务已完成'}{:else if view==='completed'||demoHistory}暂无已完成记录{:else}<strong>{wizard.title}</strong><button class="primary" onclick={integration}>{connection.state==='missing'?'连接 Codex':'查看连接进度'}</button>{/if}</section>{/if}
  </main>
  <footer><span>{demo?'隔离演示数据':`共 ${tasks.length} 个任务 · 完成 ${completed.length}`}</span><span class="footer-actions">{#if current}<button onclick={()=>copy(current.session_id)}>复制会话 ID</button>{/if}<small>0.8.1</small></span></footer>
  {#if dialog}<div class="modal-backdrop"><div use:trapDialog tabindex="-1" class="modal" class:advanced={advanced&&dialog==='integration'} role="dialog" aria-modal="true" aria-label={dialog==='settings'?'看板设置':'Codex 接入'}><div class="modal-title"><strong>{dialog==='settings'?'设置':'Codex 接入'}</strong><button aria-label="关闭设置" disabled={busy || dataBusy} onclick={()=>dialog=null}>×</button></div>
    {#if dialog==='settings'}<form onsubmit={e=>{e.preventDefault();void saveSettings();}}><label class="check"><input type="checkbox" bind:checked={draft.auto_start}>开机自启</label><label class="check"><input type="checkbox" bind:checked={draft.show_taskbar}>在任务栏中显示</label><label class="field">主题<select bind:value={draft.theme}><option value="light">白色</option><option value="dark">黑色</option></select></label><label class="check"><input type="checkbox" bind:checked={draft.notifications}>消息浮窗通知</label><label class="field">通知停留<span><input class="seconds" type="number" min="1" max="30" step="1" bind:value={draft.notification_seconds} disabled={!draft.notifications}> 秒</span></label>
      <section class="data-settings" aria-labelledby="data-heading"><div class="data-heading"><strong id="data-heading">数据目录</strong><button type="button" disabled={dataLoading || dataBusy || !native} onclick={()=>windowAction('data_folder')} title="打开当前数据目录"><Icon name="folder" size={14}/> 打开</button></div><p class="data-current">{dataLoading?'正在读取目录…':dataLocation.current || '目录暂不可用'}</p><label class="data-path">迁移到<input type="text" bind:value={dataPath} disabled={dataLoading || dataBusy || !native || dataLocation.restart_required} placeholder="输入新的完整目录路径" oninput={()=>migrateConfirm=false}></label><p class="data-hint">迁移会复制现有数据，重启后会话将重新载入。{migrationWarning}</p>
      {#if migrateConfirm}<div class="confirm-box" role="alert"><strong>确认迁移数据？</strong><p class="data-current">目标：{dataPath.trim()}</p><p>{migrationWarning}</p><label class="check"><input type="checkbox" bind:checked={producersStopped} disabled={dataBusy}>我已退出所有 Codex 会话及其他看板，暂停全部上报</label><div><button type="button" disabled={dataBusy} onclick={()=>migrateConfirm=false}>取消</button><button type="button" class="primary" disabled={dataBusy || !producersStopped} onclick={migrateData}>{dataBusy?'迁移中…':'确认迁移'}</button></div></div>{:else}<div class="data-actions"><button type="button" disabled={dataLoading || dataBusy || !native || !dataPath.trim() || dataPath.trim()===dataLocation.current || dataLocation.restart_required} onclick={()=>migrateConfirm=true}>迁移数据…</button>{#if dataLocation.restart_required}<button type="button" class="primary" disabled={dataBusy} onclick={()=>windowAction('restart')}>重启并使用新目录</button>{/if}</div>{/if}<div class="form-note" role="status">{dataNote}</div></section>
      <div class="form-note" role="status">{note}</div><div class="modal-actions"><button type="button" onclick={testNotice}>测试通知</button><span></span><button type="button" disabled={busy || dataBusy} onclick={()=>dialog=null}>取消</button><button class="primary" disabled={busy || dataBusy || dataLocation.restart_required} title={dataLocation.restart_required?'请先重启看板后再保存设置':undefined} type="submit">{busy?'保存中…':'保存'}</button></div></form>
    {:else}
      <ol class="connection-steps" aria-label="接入进度">{#each ['准备连接','Codex 载入','验证同步'] as label,index}<li class:finished={wizard.stage>index} class:current={wizard.stage===index} aria-current={wizard.stage===index?'step':undefined}><span>{wizard.stage>index?'✓':index+1}</span>{label}</li>{/each}</ol>
      <div class="integration-status" role="status"><span class="integration-icon" class:ready={wizard.verified}>{wizard.verified?'✓':'○'}</span><div><strong>{busy?'正在处理…':wizard.title}</strong><p>{wizard.description}</p></div></div>
      {#if connectionError}<div class="form-note" role="alert">{connectionError}</div>{/if}
      {#if recoveryConfirm}<section class="confirm-box recovery-confirm" aria-label="确认恢复旧版连接"><strong>恢复前请确认配置预览</strong><p>只恢复识别为本工具的旧版连接，并安装当前用户级连接程序。陌生配置不会覆盖；取消不会更改任何配置。</p>{#if recoveryPreviewReady}<div class="diff-list">{#each changes as change}<details open><summary>{change.title}<small>{change.key}</small></summary><div class="diff-summary"><span>当前：{change.before}</span><span>恢复后：{change.after}</span></div><pre>{change.diff}</pre></details>{/each}{#if !changes.length}<p>未返回可确认的配置差异，请稍后检查或查看高级诊断。</p>{/if}</div>{/if}<div><button disabled={busy} onclick={()=>recoveryConfirm=false}>取消恢复</button><button class="primary" disabled={busy || !recoveryPreviewReady || !changes.length} onclick={()=>runConnect(false,true)}>{busy?'正在检查…':'确认恢复并连接'}</button></div></section>
      {:else}<div class="integration-actions"><button class="primary" disabled={busy} onclick={integrationNext}>{busy?'正在处理…':wizard.button}</button></div>{/if}
      <div class="form-note" role="status">{note}</div>
      <button class="text-button" disabled={busy} onclick={()=>advanced?advanced=false:loadDiff()} aria-expanded={advanced}>高级选项 {advanced?'⌄':'›'}</button>
      {#if advanced}<section class="integration-advanced" aria-label="高级连接选项"><dl class="connection-facts"><div><dt>配置状态</dt><dd>{connection.config_state || '未确认'}</dd></div><div><dt>真实 Codex 连接</dt><dd>{connection.mcp_connected?'已收到初始化':'尚未验证'}</dd></div><div><dt>任务同步</dt><dd>{wizard.verified?'当前连接已验证':'尚未验证'}</dd></div></dl>
      <details class="connection-diagnostics" open={wizard.action==='details'}><summary>连接诊断</summary>{#if connectionError}<p>{connectionError}</p>{/if}<pre>{diagnostics || '暂无额外诊断。历史记录和本地自检不代表当前 Codex 已连通。'}</pre></details>
      <div class="diff-list">{#each changes as change}<details><summary>{change.title}<small>{change.key}</small></summary><div class="diff-summary"><span>当前：{change.before}</span><span>变更后：{change.after}</span></div><pre>{#each change.diff.split('\n') as line}<span class:added={line.startsWith('+')&&!line.startsWith('+++')} class:removed={line.startsWith('-')&&!line.startsWith('---')}>{line}{'\n'}</span>{/each}</pre></details>{/each}{#if !changes.length}<div class="empty small">当前没有配置差异</div>{/if}</div>
      <p class="data-hint">重新配置会更新用户级连接程序。仅在需要修复接入时使用；不会自动重启 Codex。</p><button disabled={busy || recoveryConfirm || connection.state==='conflict'} onclick={()=>connection.state==='recovery_required'?previewRecovery():runConnect()}>重新配置连接…</button>
      {#if disconnectConfirm}<div class="confirm-box">仅移除本工具仍匹配的接入配置，保留任务数据。<div><button disabled={busy} onclick={()=>disconnectConfirm=false}>取消</button><button disabled={busy} onclick={()=>runConnect(true)}>确认断开</button></div></div>{:else}<button class="danger text-button" disabled={busy || recoveryConfirm} onclick={confirmDisconnect}>断开连接…</button>{/if}</section>{/if}
      <div class="modal-actions"><span></span><button disabled={busy} onclick={()=>dialog=null}>{wizard.verified?'关闭':'稍后完成'}</button></div>
    {/if}
  </div></div>{/if}
{/if}
</div>
