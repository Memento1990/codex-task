<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { action, call, native, onHost, saveOptions } from './host';
  import { defaults, done, elapsed, openBlockers, runtime, status, visibleTasks, type Notice, type Options, type Snapshot, type Task } from './model';
  import TaskDetail from './TaskDetail.svelte';
  type Scope = {task_id?: string; scope?: string; tab?: number};
  const initial = (window as unknown as {__FLOATBOARD_VIEW__?:{view:string;value?:Scope;notice?:unknown}}).__FLOATBOARD_VIEW__ || {view:'main'};
  const demo = new URLSearchParams(location.search).has('demo');
  let view = $state(initial.view);
  let tasks = $state<Task[]>([]), options = $state<Options>({...defaults}), healthy = $state(false), connection = $state('missing');
  let selected = $state(''), tab = $state(view === 'main' ? 1 : 0), filter = $state('all'), scope = $state<Scope>(initial.value || {});
  let now = $state(Date.now()/1000), error = $state(''), pinned = $state(true), menu = $state(false), loading = $state(true);
  let dialog = $state<'settings'|'integration'|null>(null), draft = $state<Options>({...defaults}), busy = $state(false), note = $state(''), advanced = $state(false), disconnectConfirm = $state(false);
  let changes = $state<{title:string;key:string;before:string;after:string;diff:string}[]>([]);
  let demoHistory = $state(false), scopeTask = $state('');
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
  function settings() {menu=false;draft={...options};note='';dialog='settings';}
  async function integration() {menu=false;note='';advanced=false;dialog='integration';disconnectConfirm=false;changes=[];}
  async function runConnect(remove=false) {
    busy=true;note='';
    try { if(native) await call(remove?'disconnect':'connect'); else await new Promise(r=>setTimeout(r,200)); note=remove?'已断开连接':'连接成功';disconnectConfirm=false; await refresh(); if(advanced)await loadDiff(); }
    catch(e){note='操作失败：'+String(e);} finally{busy=false;}
  }
  async function loadDiff(remove=false) {advanced=true;try{changes=native?(await call<{changes:typeof changes}>('preview',{remove})).changes:[];}catch(e){note=String(e);}}
  async function confirmDisconnect() {disconnectConfirm=true;await loadDiff(true);}
  async function saveSettings() {
    busy=true;note='';
    try {options=native?await saveOptions(draft):{...draft};dialog=null;}catch(e){note='保存失败：'+String(e);}finally{busy=false;}
  }
  async function copy(text:string){if(native)await action('copy',text);else await navigator.clipboard.writeText(text).catch(()=>{});}
  async function helpConnect(){await copy(native?await call<string>('connect_request'):'隔离演示：请帮我接入 Codex 任务看板');note='已复制请求，可粘贴给 Codex';}
  async function closeView(){if(native)await action('hide');else {demoHistory=false;scope={};selected=scopeTask;}}
  function resetScope(value:Scope){scope=value;selected=value.task_id || value.scope || '';tab=value.tab || 0;}
  async function refresh() {
    if(view === 'toast' || !native) return;
    try { const snapshot=await call<Snapshot>('snapshot',{notify:view==='main'}); tasks=snapshot.tasks;options=snapshot.options;healthy=snapshot.healthy;connection=snapshot.connection.state;now=snapshot.now;error='';loading=false;
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
    if(toasts.length>=4)removeToast(toasts[0].key);
    toasts=[...toasts,item];resumeToast(item);void resizeToast();
  }
  async function testNotice(){if(!draft.notifications){note='请先开启消息浮窗通知';return;}const value={task_id:'settings-preview',title:'消息通知预览',lines:['任务或清单变化时，在这里提醒。'],tab:0,seconds:draft.notification_seconds,theme:draft.theme};if(native)await action('toast',value);else {note='通知预览：'+draft.notification_seconds+' 秒';}}
  onMount(()=>{
    let stopped=false;let unlisten=()=>{};let timer:ReturnType<typeof setTimeout>;
    async function loop(){await refresh();if(!stopped)timer=setTimeout(loop,700);}
    if(native){
      if(view==='toast')document.documentElement.style.background='transparent';
      void onHost(value=>{
        if(value.type==='settings')settings();
        if(value.type==='scope')resetScope(value.value as Scope);
        if(value.type==='select'){const v=value.value as Scope;selected=v.task_id || selected;filter='all';tab=v.tab || 0;}
        if(value.type==='options')options=value.options as Options;
        if(value.type==='notice' && view==='toast')addToast(value.value as Notice);
        if(value.type==='clear')for(const toast of [...toasts])removeToast(toast.key);
      }).then(fn=>{unlisten=fn;});
      if(view==='main')void call<Record<string,unknown>>('window_load').then(state=>{selected=String(state.selected || '');pinned=state.pinned!==false;filter=String(state.filter || 'all');if(typeof state.tab==='number')tab=state.tab;});
      if(view==='toast' && initial.notice)addToast(initial.notice as Notice);
      void loop();
    }else if(demo){void fetch('/fixture.json').then(r=>r.json()).then((snapshot:Snapshot)=>{tasks=snapshot.tasks;options=snapshot.options;healthy=true;connection='ready';now=snapshot.now;selected=tasks.find(t=>openBlockers(t).length)?.id || tasks[0]?.id || '';loading=false;});}
    else {loading=false;error='请从桌面程序启动';}
    const clock=setInterval(()=>{now=Date.now()/1000;},1000);
    return ()=>{stopped=true;clearTimeout(timer);clearInterval(clock);unlisten();for(const toast of toasts)if(toast.timer)clearTimeout(toast.timer);};
  });
</script>

<svelte:window onkeydown={e=>{if(e.key==='Escape'){if(dialog&&!busy)dialog=null;else if(view==='completed'||demoHistory)void closeView();else menu=false;}}}/>
<div class="app" class:dark={options.theme==='dark'} class:toast-app={view==='toast'}>
{#if view==='toast'}
  <div class="toast-stack">{#each toasts as toast (toast.key)}<article class="toast" onpointerenter={()=>pauseToast(toast)} onpointerleave={()=>resumeToast(toast)}><button class="toast-open" onclick={()=>{void action('open_notice',toast);removeToast(toast.key);}}><strong>{toast.title}</strong>{#each toast.lines as line}<span>{line}</span>{/each}</button><button class="toast-close" aria-label="关闭通知" onclick={()=>removeToast(toast.key)}>×</button></article>{/each}</div>
{:else}
  <header class="toolbar"><span class="app-mark">&gt;_</span><strong>{view==='completed'||demoHistory?'已完成记录':'卡点调度台'}</strong><span class="connection" class:offline={!healthy}><i></i>{healthy?'本地服务正常':'采集离线'}</span><div class="toolbar-actions">{#if view==='main'&&!demoHistory}<button class:pressed={pinned} onclick={()=>{pinned=!pinned;if(native)void action('pin',pinned);saveWindow();}} aria-pressed={pinned}>置顶</button><div class="menu-anchor"><button aria-label="更多操作" aria-expanded={menu} onclick={()=>menu=!menu}>···</button>{#if menu}<div class="dropdown"><button onclick={settings}>设置</button><button onclick={integration}>Codex 接入</button><button onclick={()=>{menu=false;if(native)void action('data_folder');}}>数据目录</button><hr><button onclick={()=>{if(native)void action('quit');}}>退出看板</button></div>{/if}</div>{:else}<button onclick={closeView}>关闭</button>{/if}</div></header>
  {#if error}<div class="offline-banner" role="status">{error}</div>{/if}
  {#if view==='main'&&!demoHistory}<nav class="filters" aria-label="任务筛选">{#each [{key:'all',label:'全部',count:active.length},{key:'blocked',label:'有卡点',count:blocked.length},{key:'running',label:'执行中',count:active.filter(t=>t.status==='running').length}] as item}<button class:active={filter===item.key} onclick={()=>setFilter(item.key)}>{item.label}<small>{item.count}</small></button>{/each}<span class="filter-meta">{tasks.reduce((sum,t)=>sum+openBlockers(t).length,0)} 个未解除卡点</span></nav>{/if}
  <main class="workspace" class:history={view==='completed'||demoHistory} class:steps-only={Boolean(scope.scope)&&(view==='completed'||demoHistory)}>
    {#if !scope.scope || view==='main'&&!demoHistory}<aside class="task-sidebar"><div class="list-caption">{view==='completed'||demoHistory?'已完成任务':'任务'}<span>{live.length}</span></div><div class="task-list" data-testid="task-list">{#each live as task (task.id)}<button class="task-row" class:selected={current?.id===task.id} onclick={()=>choose(task)}><div class="task-row-top"><span class="state-dot {task.status}"></span><strong>{task.title || '未汇报任务'}</strong></div><div class="task-row-meta"><span class="{task.status}">{status[task.status]}</span><span>{done(task)}/{task.plan.length}</span><span>{elapsed(task.timing,now)}</span></div>{#if openBlockers(task).length}<div class="task-blocked">! {openBlockers(task).length} 个卡点 · {openBlockers(task)[0].title}</div>{:else}<div class="task-current">{task.current_step || `Codex ${runtime[task.runtime]}`}</div>{/if}<div class="progress"><i style:width={`${task.plan.length?done(task)/task.plan.length*100:0}%`}></i></div></button>{/each}{#if !live.length}<div class="empty small">{loading?'正在读取任务':view==='completed'||demoHistory?'暂无已完成任务':'当前筛选下没有任务'}</div>{/if}</div>
      {#if view==='main'&&!demoHistory&&completed.length}<button class="completed-entry" onclick={()=>{scopeTask=selected;void openCompleted();}}><span>已完成任务 <b>{completed.length}</b></span><span>查看 ↗</span></button>{/if}
    </aside>{/if}
    {#if current}<TaskDetail task={current} bind:tab {now} completedOnly={view==='completed'||demoHistory} onCompleted={()=>{scopeTask=selected;void openCompleted({scope:current.id});}}/>{:else}<section class="empty detail-empty">{#if loading}正在读取任务{:else if tasks.length&&view==='main'&&!demoHistory}{active.length?'当前筛选下没有任务':'全部任务已完成'}{:else if view==='completed'||demoHistory}暂无已完成记录{:else}<strong>{connection==='ready'?'等待首次任务同步':'连接你的 Codex'}</strong><button class="primary" onclick={integration}>{connection==='ready'?'查看连接状态':'连接 Codex'}</button>{/if}</section>{/if}
  </main>
  <footer><span>{demo?'隔离演示数据':`共 ${tasks.length} 个任务 · 完成 ${completed.length}`}</span><span class="footer-actions">{#if current}<button onclick={()=>copy(current.session_id)}>复制会话 ID</button>{/if}<small>0.7.0</small></span></footer>
  {#if dialog}<div class="modal-backdrop"><div use:trapDialog tabindex="-1" class="modal" class:advanced={advanced&&dialog==='integration'} role="dialog" aria-modal="true" aria-label={dialog==='settings'?'看板设置':'Codex 接入'}><div class="modal-title"><strong>{dialog==='settings'?'设置':'Codex 接入'}</strong><button aria-label="关闭设置" disabled={busy} onclick={()=>dialog=null}>×</button></div>
    {#if dialog==='settings'}<form onsubmit={e=>{e.preventDefault();void saveSettings();}}><label class="check"><input type="checkbox" bind:checked={draft.auto_start}>开机自启</label><label class="check"><input type="checkbox" bind:checked={draft.show_taskbar}>在任务栏中显示</label><label class="field">主题<select bind:value={draft.theme}><option value="light">白色</option><option value="dark">黑色</option></select></label><label class="check"><input type="checkbox" bind:checked={draft.notifications}>消息浮窗通知</label><label class="field">通知停留<span><input class="seconds" type="number" min="1" max="30" step="1" bind:value={draft.notification_seconds} disabled={!draft.notifications}> 秒</span></label><div class="form-note" role="status">{note}</div><div class="modal-actions"><button type="button" onclick={testNotice}>测试通知</button><span></span><button type="button" disabled={busy} onclick={()=>dialog=null}>取消</button><button class="primary" disabled={busy} type="submit">{busy?'保存中…':'保存'}</button></div></form>
    {:else}<div class="integration-status"><span class="integration-icon" class:ready={connection==='ready'}>{connection==='ready'?'✓':'○'}</span><div><strong>{connection==='ready'?'Codex 已连接':'连接你的 Codex'}</strong><p>{connection==='ready'?'新会话会自动同步任务进度':'自动检查并保留其他配置'}</p></div></div><div class="integration-actions"><button class="primary" disabled={busy} onclick={()=>runConnect()}>{busy?'连接中…':connection==='ready'?'重新检查连接':'连接 Codex'}</button><button disabled={busy} onclick={helpConnect}>让 Codex 帮我连接</button></div><div class="form-note" role="status">{note}</div><button class="text-button" onclick={()=>advanced?advanced=false:loadDiff()} aria-expanded={advanced}>高级选项 {advanced?'⌄':'›'}</button>{#if advanced}<div class="diff-list">{#each changes as change}<details><summary>{change.title}<small>{change.key}</small></summary><div class="diff-summary"><span>当前：{change.before}</span><span>变更后：{change.after}</span></div><pre>{#each change.diff.split('\n') as line}<span class:added={line.startsWith('+')&&!line.startsWith('+++')} class:removed={line.startsWith('-')&&!line.startsWith('---')}>{line}{'\n'}</span>{/each}</pre></details>{/each}{#if !changes.length}<div class="empty small">当前配置无需变更</div>{/if}</div>{#if disconnectConfirm}<div class="confirm-box">仅移除本工具仍匹配的接入配置，保留任务数据。<div><button disabled={busy} onclick={()=>disconnectConfirm=false}>取消</button><button disabled={busy} onclick={()=>runConnect(true)}>确认断开</button></div></div>{:else}<button class="danger text-button" disabled={busy} onclick={confirmDisconnect}>断开连接…</button>{/if}{/if}<div class="modal-actions"><span></span><button disabled={busy} onclick={()=>dialog=null}>关闭</button></div>{/if}
  </div></div>{/if}
{/if}
</div>
