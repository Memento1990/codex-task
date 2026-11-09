<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { action, call, native, onHost, saveOptions } from './host';
  import { runningTaskSyncPrompt, connectionTone, isBlockerNotice, dragThresholdReached, dockSummaryMode, dockSummaryVisible, dockSummary, connectionStatus, createTestNoticeFactory, defaults, diagnosticText, noticeStack, noticeAutoDismiss, normalizeTab, isReportedTask, openBlockers, filterTasks, type Connection, type DataLocation, type Notice, type Options, type Snapshot, type Task } from './model';
  import Icon from './Icon.svelte';
  import MatrixBoard from './MatrixBoard.svelte';
  import TaskDrawer from './TaskDrawer.svelte';
  import CompletedRecords from './CompletedRecords.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import Updates from './Updates.svelte';
  import Notifications from './Notifications.svelte';
  import WeixinLab from './WeixinLab.svelte';
  import AgentMaintenance from './AgentMaintenance.svelte';
  import Integration from './Integration.svelte';
  import { appVersion } from './version';
  import { utilityKind, type UtilityKind } from './utility-window';
  type Scope = {task_id?: string; scope?: string; tab?: number};
  const initial = (window as unknown as {__FLOATBOARD_VIEW__?:{view:string;kind?:string;value?:Scope;notice?:unknown}}).__FLOATBOARD_VIEW__ || {view:'main'};
  const development = (window as unknown as {__FLOATBOARD_DEVELOPMENT__?:boolean}).__FLOATBOARD_DEVELOPMENT__===true;
  let view = $state(initial.view);
  const utility = initial.view==='utility' ? utilityKind(initial.kind) : null;
  let tasks = $state<Task[]>([]), options = $state<Options>({...defaults}), healthy = $state(false), connection = $state<Connection>({state:'missing'});
  let selected = $state(''), tab = $state(initial.value ? normalizeTab(initial.value.tab) : 0), filter = $state('active'), scope = $state<Scope>(initial.value || {});
  let query=$state(''), drawerId=$state(''), palette=$state(false), historyQuery=$state(''), historyPage=$state(1), historySize=$state(20);
  let menuOpen=$state(false), menuButton=$state<HTMLButtonElement>(), menuPanel=$state<HTMLDivElement>();
  async function openMenu(){menuOpen=!menuOpen;palette=false;if(menuOpen){await tick();menuPanel?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();}}
  function closeMenu(restoreFocus=false){menuOpen=false;if(restoreFocus)menuButton?.focus();}
  function menuAction(run:()=>unknown){closeMenu();void run();}
  function menuKeys(event:KeyboardEvent){const items=Array.from(menuPanel?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)')||[]);const index=items.indexOf(document.activeElement as HTMLButtonElement);if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeMenu(true);}else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){event.preventDefault();items[event.key==='Home'?0:event.key==='End'?items.length-1:(index+(event.key==='ArrowDown'?1:-1)+items.length)%items.length]?.focus();}else if(event.key==='Tab')closeMenu();}
  function outsideMenu(event:PointerEvent){if(menuOpen&&event.target instanceof Node&&!menuPanel?.contains(event.target)&&!menuButton?.contains(event.target))closeMenu();}
  function openSearch(){closeMenu();palette=true;}
  let drawerTask=$derived(tasks.find(task=>task.id===drawerId)||null);
  let now = $state(Date.now()/1000), error = $state(''), storageError = $state(''), notificationError = $state(''), collectorNote = $state(''), snapshotFailed = $state(false), pinned = $state(true), loading = $state(true);
  let dockCollapsed = $state(false), dockSide = $state<'left'|'right'>('right'), dockHovered = $state(false);
  let dock = $derived(dockSummary(tasks, selected));
  let summaryMode = $derived(dockSummaryMode(options)), dockDragging = $state(false);
  let dockPointer: {id:number;x:number;y:number;target:HTMLButtonElement}|null = null;
  let suppressDockClick = false;
  function receiveDock(value:unknown){if(view!=='main'||utility)return;const state=value as {collapsed?:boolean;side?:string;hovered?:boolean;dragging?:boolean};if(typeof state.collapsed==='boolean')dockCollapsed=state.collapsed;if(state.side==='left'||state.side==='right')dockSide=state.side;if(typeof state.hovered==='boolean')dockHovered=state.hovered;if(typeof state.dragging==='boolean')dockDragging=state.dragging;}
  function startDockPointer(event:PointerEvent){if(event.button!==0||!event.isPrimary)return;suppressDockClick=false;dockPointer={id:event.pointerId,x:event.clientX,y:event.clientY,target:event.currentTarget as HTMLButtonElement};dockPointer.target.setPointerCapture(event.pointerId);}
  function moveDockPointer(event:PointerEvent){if(!dockPointer||event.pointerId!==dockPointer.id||Math.hypot(event.clientX-dockPointer.x,event.clientY-dockPointer.y)<4)return;const pointer=dockPointer;dockPointer=null;suppressDockClick=true;dockDragging=true;if(pointer.target.hasPointerCapture(pointer.id))pointer.target.releasePointerCapture(pointer.id);if(native)void action('dock_drag').catch(e=>{dockDragging=false;error='拖动头像失败：'+String(e);});else dockDragging=false;}
  function endDockPointer(event:PointerEvent){if(dockPointer?.id===event.pointerId){if(dockPointer.target.hasPointerCapture(event.pointerId))dockPointer.target.releasePointerCapture(event.pointerId);dockPointer=null;}}
  function clickDock(event:MouseEvent){if(dockDragging){event.preventDefault();return;}if(event.detail!==0&&suppressDockClick){event.preventDefault();suppressDockClick=false;return;}void restoreDock();}
  $effect(()=>{if(dockCollapsed&&!dockDragging){const mode=summaryMode;if(mode!=='hover')void hoverDock(mode==='always');}});
  async function collapseDock(){await tick();try{if(native){await action('dock_set_enabled',true);await action('dock_collapse','right');}else{dockCollapsed=true;dockSide='right';}}catch(e){error='收起面板失败：'+String(e);}}
  async function restoreDock(){try{if(native)await action('dock_restore');else dockCollapsed=false;dockHovered=false;}catch(e){error='恢复面板失败：'+String(e);}}
  async function hoverDock(value:boolean){if(dockDragging)return;const hover=dockSummaryVisible(summaryMode,value);if(hover===dockHovered)return;dockHovered=hover;if(native)try{await action('dock_hover',hover);}catch{dockHovered=false;}}
  $effect(()=>{document.documentElement.classList.toggle('dock-transparent',dockCollapsed);return()=>document.documentElement.classList.remove('dock-transparent');});
  $effect(()=>{if(native&&view==='main'){const enabled=!dialog&&!demoHistory&&!drawerId&&!palette;void action('dock_set_enabled',enabled).catch(()=>{});}});
  let updateBusy = $state(false), maintenanceBusy = $state(false), notificationBusy = $state(false);
  let dialog = $state<UtilityKind|null>(null), busy = $state(false), note = $state('');
  let settingsSaving=$state(false), preferenceRevision=0;
  let demoHistory = $state(false), scopeTask = $state('');
  let maximized = $state(false), diagnostics = $state(''), connectionError = $state('');
  let tone=$derived(connectionTone(connection));
  let modalTitle=$derived(dialog==='settings'?'看板设置':dialog==='updates'?'版本更新':dialog==='notifications'?'消息通知':dialog==='maintenance'?'备份与诊断':dialog==='weixin-lab'?'微信连接验证 · DEV':'Codex 接入');
  function closeDialog(){if(!modalLocked){if(utility)void action('utility_busy',false).then(()=>action('utility_close')).catch(e=>note=String(e));else dialog=null;}}
  async function openUtility(kind:UtilityKind){if(native){try{await action('utility_open',kind);}catch(e){error='打开功能窗口失败：'+String(e);}}else if(kind==='settings')settings();else if(kind==='integration')void integration();else dialog=kind;}
  $effect(()=>{if(native&&utility)void action('utility_busy',modalLocked).catch(()=>{});});
  function escapeDialog(event:KeyboardEvent){if(event.defaultPrevented)return;if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'&&!utility){event.preventDefault();closeMenu();palette=!palette;return;}if(event.key!=='Escape')return;if(menuOpen){event.preventDefault();closeMenu(true);return;}if(palette){event.preventDefault();palette=false;return;}if(drawerId){event.preventDefault();drawerId='';return;}if(dockCollapsed){event.preventDefault();void hoverDock(false);return;}if(dialog){event.preventDefault();if(modalLocked)return;closeDialog();}else if(view==='completed'||demoHistory)void closeView();}
  let dataLocation = $state<DataLocation>({current:''}), dataNote = $state(''), dataLoading = $state(false), dataBusy = $state(false);
  let modalLocked=$derived(busy||settingsSaving||dataBusy||updateBusy||maintenanceBusy||notificationBusy||(Boolean(utility)&&loading));
  const previewNotice = createTestNoticeFactory();
  let connectionLabel = $derived(loading?'正在读取':snapshotFailed?'数据服务异常':storageError?'任务库异常':!healthy?'采集心跳异常':connectionStatus(connection));
  type Toast = Notice & {key:number;remaining:number;started:number;timer?:ReturnType<typeof setTimeout>};
  let toasts = $state<Toast[]>([]), toastKey = 0;
  let live = $derived(view === 'completed' || demoHistory ? tasks.filter(t => scope.scope ? t.id === scope.scope : t.status === 'completed') : filterTasks(tasks,filter,query));
  let current = $derived(tasks.find(t => t.id === selected) || live[0]);
  let completed = $derived(tasks.filter(t => t.status === 'completed'));
    function trapDialog(node: HTMLDivElement) {
    const previous=document.activeElement as HTMLElement | null;
    const focusable=()=>Array.from(node.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),select:not(:disabled),a[href],summary,[tabindex="0"]')).filter(el=>el.getClientRects().length>0&&!el.closest('[inert]'));
    queueMicrotask(()=>node.querySelector<HTMLElement>('.modal-title button')?.focus());
    const keydown=(event:KeyboardEvent)=>{if(event.defaultPrevented||event.key!=='Tab')return;const items=focusable(),first=items[0],last=items.at(-1);if(!first){event.preventDefault();node.focus();}else if(event.shiftKey&&(document.activeElement===first||!node.contains(document.activeElement))){event.preventDefault();last?.focus();}else if(!event.shiftKey&&(document.activeElement===last||!node.contains(document.activeElement))){event.preventDefault();first.focus();}};
    node.addEventListener('keydown',keydown);
    return {destroy(){node.removeEventListener('keydown',keydown);if(previous?.isConnected)previous.focus();}};
  }

  function showDetails(task:Task,nextTab=0){selected=task.id;drawerId=task.id;tab=normalizeTab(nextTab);palette=false;saveWindow();}
  function saveWindow() { if(native && view === 'main') void call('window_save',{selected,tab,filter,pinned,details_collapsed:false}).catch(()=>{}); }
  $effect(()=>{if(native&&view==='main'&&!loading)saveWindow();});
  async function openCompleted(value:Scope = {}) {
    if(native) await action('completed',value); else { demoHistory=true;resetScope(value); }
  }
  function settings() {if(native&&utility!=='settings'){void openUtility('settings');return;}note='';dataNote='';dialog='settings';void loadDataLocation();}
  async function loadDataLocation() {
    dataLoading=true;
    try {dataLocation=native?await call<DataLocation>('data_location'):{current:'演示模式 · 无本地数据目录'};}
    catch(e){dataNote='读取数据目录失败：'+String(e);}finally{dataLoading=false;}
  }
  async function windowAction(name:string) {
    try {if(native){const result=await action(name) as {maximized?:boolean}|null;if(typeof result?.maximized==='boolean')maximized=result.maximized;}else if(name==='toggle_maximize')maximized=!maximized;}
    catch(e){if(name==='restart' || name==='data_folder')dataNote='操作失败：'+String(e);else error='窗口操作失败：'+String(e);}
  }
  let toolbarPointer: {id:number;x:number;y:number;target:HTMLElement}|null=null;
  function clearToolbarPointer(){const pointer=toolbarPointer;toolbarPointer=null;if(pointer?.target.hasPointerCapture(pointer.id))pointer.target.releasePointerCapture(pointer.id);}
  function startToolbarPointer(event:PointerEvent){if(event.button!==0||!event.isPrimary)return;clearToolbarPointer();toolbarPointer={id:event.pointerId,x:event.clientX,y:event.clientY,target:event.currentTarget as HTMLElement};toolbarPointer.target.setPointerCapture(event.pointerId);}
  function moveToolbarPointer(event:PointerEvent){if(!toolbarPointer||toolbarPointer.id!==event.pointerId)return;if(!(event.buttons&1)){clearToolbarPointer();return;}if(!dragThresholdReached(toolbarPointer.x,toolbarPointer.y,event.clientX,event.clientY,event.buttons))return;clearToolbarPointer();void windowAction('drag');}
  function endToolbarPointer(event:PointerEvent){if(toolbarPointer?.id===event.pointerId)clearToolbarPointer();}
  function blurWindow(){clearToolbarPointer();closeMenu();}
  async function integration() {if(native&&utility!=='integration'){await openUtility('integration');return;}note='';dialog='integration';}
  async function changeSetting(patch:Partial<Options>) {
    if(settingsSaving||dataBusy||maintenanceBusy||updateBusy)return;
    const previous={...options};++preferenceRevision;settingsSaving=true;note='';options={...options,...patch};
    try {const saved=native?await saveOptions(patch):{...options};options=saved;if(saved.native_error){note='设置已保存，窗口效果应用失败：'+saved.native_error;if(!dialog)error=note;}}
    catch(e){if(!String(e).includes('尚未确认'))options=previous;note='保存失败：'+String(e);if(native)try{const latest=await call<Snapshot>('snapshot',{notify:false});options=latest.options;}catch{note+='；暂时无法确认当前设置，请稍后重新打开设置。';}if(!dialog)error=note;}
    finally{++preferenceRevision;settingsSaving=false;}
  }
  async function copy(text:string){try{if(native)await action('copy',text);else await navigator.clipboard.writeText(text);}catch(e){error='复制失败：'+String(e);}}
  async function openEvidence(reference:string){try{if(native)await action('open_reference',reference);else error='仅桌面应用可打开本地证据，可复制引用。';}catch(e){error='无法打开证据：'+String(e);}}
  async function returnMain(){if(modalLocked)return;if(native&&utility)try{await action('show');}catch(e){note='无法返回主窗：'+String(e);return;}closeDialog();}
  async function togglePin(){const next=!pinned;try{if(native)await action('pin',next);pinned=next;saveWindow();}catch(e){error='置顶操作失败：'+String(e);}}
  function measureDock(node:HTMLElement){let last=0;const report=()=>{const height=Math.ceil(node.getBoundingClientRect().height);if(height===last)return;last=height;if(native)void action('dock_measure',{height}).catch(e=>error='摘要尺寸调整失败：'+String(e));};const observer=new ResizeObserver(report);observer.observe(node);report();return{destroy(){observer.disconnect();}};}
  async function closeView(){if(native)await action('hide');else {demoHistory=false;scope={};selected=scopeTask;}}
  function resolveScope(){if((view!=='completed'&&!demoHistory)||!scope.task_id)return;const target=tasks.find(t=>t.status==='completed'&&t.id===scope.task_id)||tasks.find(t=>t.status==='completed'&&t.source_task_id===scope.task_id);if(target){selected=target.id;drawerId=target.id;tab=normalizeTab(scope.tab);scope={...scope,task_id:undefined};}}
  function resetScope(value:Scope){scope=value;selected=value.task_id || value.scope || '';tab=normalizeTab(value.tab);drawerId='';historyPage=1;historyQuery='';resolveScope();}
  let observedCommit:{database_instance_id?:string;commit_sequence?:number}={};
  async function refresh() {
    const firstSnapshot=loading;
    if(view === 'toast' || !native) return;
    const observedPreferences=preferenceRevision;
    try { const snapshot=await call<Snapshot>('snapshot',{notify:view==='main'}); observedCommit={database_instance_id:snapshot.database_instance_id,commit_sequence:snapshot.commit_sequence};storageError=snapshot.storage?.status==='error'?snapshot.storage.message:'';if(!storageError)tasks=snapshot.tasks.filter(isReportedTask);if(!settingsSaving&&observedPreferences===preferenceRevision)options=snapshot.options;healthy=snapshot.healthy;connection=snapshot.connection;connectionError=snapshot.connection.error || snapshot.connection.hook_error || '';collectorNote=[snapshot.collector?.start_error,!healthy?snapshot.collector?.guidance:'',snapshot.collector?.version?`采集版本：${snapshot.collector.version}`:''].filter(Boolean).join('\n');diagnostics=[diagnosticText(snapshot.connection.diagnostics || snapshot.diagnostics),storageError,snapshot.storage?.diagnostic?'任务读取诊断：'+diagnosticText(snapshot.storage.diagnostic):'',snapshot.collector?.diagnostic?'采集诊断：'+diagnosticText(snapshot.collector.diagnostic):'',collectorNote].filter(Boolean).join('\n');now=snapshot.now;if(snapshotFailed)error='';snapshotFailed=false;loading=false;
      if(snapshot.data_location)dataLocation=snapshot.data_location;
      if(firstSnapshot&&view==='main'&&!selected)selected=tasks.find(t=>openBlockers(t).length)?.id||tasks.find(t=>t.status!=='completed')?.id||'';
      if(selected&&!tasks.some(t=>t.id===selected)){selected='';drawerId='';}
      resolveScope();
      if(view==='main')for(const notice of snapshot.notices){try{await action('toast',{...notice,seconds:options.notification_seconds,theme:options.theme,completed:tasks.find(t=>t.id===notice.task_id)?.status==='completed'});notificationError='';}catch{notificationError='浮窗通知暂时不可用；任务读取与采集状态不受此提示影响。';}}
    }catch{error='数据服务暂不可读 · 保留最近快照；请打开接入诊断检查，不要删除数据。';snapshotFailed=true;loading=false;}
  }
  function removeToast(key:number){const item=toasts.find(t=>t.key===key);if(item?.timer)clearTimeout(item.timer);toasts=toasts.filter(t=>t.key!==key);void resizeToast();}
  function pauseToast(item:Toast){if(item.timer){clearTimeout(item.timer);item.timer=undefined;item.remaining=Math.max(0,item.remaining-(Date.now()-item.started));}}
  function resumeToast(item:Toast){if(noticeAutoDismiss(item)&&!item.timer){item.started=Date.now();item.timer=setTimeout(()=>removeToast(item.key),item.remaining);}}
  async function resizeToast(){await tick();if(native){const stack=document.querySelector<HTMLElement>('.toast-stack');const limit=Math.max(1,window.screen.availHeight-24);if(stack)stack.style.maxHeight=`${limit}px`;if(toasts.length)await action('toast_size',Math.min(limit,stack?.scrollHeight || 1));else await action('hide');}}
  function addToast(value:Notice & {seconds?:number;theme?:string;completed?:boolean}){
    if(value.theme)options={...options,theme:value.theme};
    const old=toasts.find(t=>t.task_id===value.task_id);if(old?.timer)clearTimeout(old.timer);
    toasts=toasts.filter(t=>t.task_id!==value.task_id);
    const item:Toast={...value,sticky:old?.sticky || value.sticky,key:++toastKey,remaining:(value.seconds || 3)*1000,started:Date.now()};
    const next=noticeStack(toasts,item);
    for(const previous of toasts)if(!next.includes(previous)&&previous.timer)clearTimeout(previous.timer);
    toasts=next;resumeToast(item);void resizeToast();
  }
  async function testNotice(sticky=false){if(!options.notifications){note='请先开启消息浮窗通知';return;}try{const value={...previewNotice(options),...(sticky?{title:'卡点通知预览',lines:['预览卡点 · 需要介入'],tab:1,sticky:true}:{})};if(native)await action('toast',value);else note='通知预览：'+(sticky?'卡点通知保留到手动关闭':options.notification_seconds+' 秒');}catch(e){note='通知发送失败：'+String(e);}}
  onMount(()=>{
    let stopped=false;let unlisten=()=>{};let timer:ReturnType<typeof setTimeout>;
    let utilityInitialized=false;
    let lastSnapshot=0;
    async function loop(){
      let needsSnapshot=true;
      if(!loading&&!snapshotFailed)try{const next=await call<typeof observedCommit>('watch',observedCommit);needsSnapshot=next.database_instance_id!==observedCommit.database_instance_id||next.commit_sequence!==observedCommit.commit_sequence||Date.now()-lastSnapshot>5000;}catch{needsSnapshot=true;}
      if(stopped)return;
      if(needsSnapshot){await refresh();lastSnapshot=Date.now();}
      if(utility&&!utilityInitialized&&!stopped){utilityInitialized=true;if(utility==='settings')settings();else if(utility==='integration')void integration();else dialog=utility;}
      if(!stopped)timer=setTimeout(loop,snapshotFailed?700:0);
    }
    if(native){
      if(view==='toast')document.documentElement.style.background='transparent';
      void onHost(value=>{
        if(value.type==='dock_state')receiveDock(value);
        if(value.type==='window_blur')blurWindow();
        if(value.type==='settings')settings();
        if(value.type==='window_state'){const state=(value.value || value) as {maximized?:boolean};if(typeof state.maximized==='boolean')maximized=state.maximized;}
        if(value.type==='scope')resetScope(value.value as Scope);
        if(value.type==='select'){const v=value.value as Scope;selected=v.task_id || selected;drawerId=selected;filter='all';tab=normalizeTab(v.tab);}
        if(value.type==='options'){++preferenceRevision;options=value.options as Options;if(!options.notifications)for(const toast of [...toasts])removeToast(toast.key);}
        if(value.type==='notice' && view==='toast')addToast(value.value as Notice);
        if(value.type==='clear')for(const toast of [...toasts])removeToast(toast.key);
      }).then(async fn=>{if(stopped){fn();return;}unlisten=fn;if(view==='toast')await action('toast_ready');}).catch(e=>{error='窗口事件监听失败：'+String(e);});
      if(view!=='toast')void windowAction('window_state');
      const loadWindow=view==='main'?call<Record<string,unknown>>('window_load').then(state=>{selected=String(state.selected || '');pinned=state.pinned!==false;filter=String(state.filter || 'active');tab=normalizeTab(state.tab);}):Promise.resolve();
      if(view==='main')void action('dock_state').then(receiveDock).catch(()=>{});
      if(view==='toast' && initial.notice)addToast(initial.notice as Notice);
      if(view!=='toast')void loadWindow.catch(()=>{}).then(()=>loop());
    }else {loading=false;error='请从桌面程序启动';}
    const clock=setInterval(()=>{now=Date.now()/1000;},1000);
    return ()=>{stopped=true;clearTimeout(timer);clearInterval(clock);unlisten();for(const toast of toasts)if(toast.timer)clearTimeout(toast.timer);};
  });
</script>

<svelte:window onkeydown={escapeDialog} onblur={blurWindow} onpointerdown={outsideMenu}/>
<div class="app matrix-a" class:dark={options.theme==='dark'} class:toast-app={view==='toast'}>
{#if view==='toast'}
  <div class="toast-stack">{#each toasts as toast (toast.key)}<article class="toast" class:blocker-toast={isBlockerNotice(toast)} onpointerenter={()=>pauseToast(toast)} onpointerleave={()=>resumeToast(toast)}><button class="toast-open" onclick={()=>{void action('open_notice',toast);removeToast(toast.key);}}>{#if isBlockerNotice(toast)}<span class="toast-blocker-label"><svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="M7.1 2.5a1 1 0 0 1 1.8 0l5.5 10A1 1 0 0 1 13.5 14h-11a1 1 0 0 1-.9-1.5l5.5-10Z" stroke="currentColor"/><path d="M8 6v3m0 2v.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>卡点提醒 · 需要关注</span>{/if}<strong>{toast.title}</strong>{#each toast.lines as line}<span>{line}</span>{/each}</button><button class="toast-close" aria-label="关闭通知" onclick={()=>removeToast(toast.key)}>×</button></article>{/each}</div>
{:else}
  {#if !utility&&dockCollapsed}<div class="dock-shell" class:left={dockSide==='left'} class:expanded={dockHovered} onpointerenter={()=>hoverDock(true)} onpointerleave={()=>hoverDock(false)} role="presentation">
    <button class="dock-ball" class:attention={dock.blockers>0} class:finished={dock.allCompleted} aria-label={`${dock.title}，${dock.badge}，点击恢复面板`} aria-describedby={dockHovered?'dock-summary':undefined} onfocus={()=>hoverDock(true)} onblur={()=>hoverDock(false)} onpointerdown={startDockPointer} onpointermove={moveDockPointer} onpointerup={endDockPointer} onpointercancel={endDockPointer} onclick={clickDock}>
      <svg viewBox="0 0 58 58" aria-hidden="true"><circle class="dock-ring-track" cx="29" cy="29" r="25"/><circle class="dock-ring" class:unknown={dock.progress===null&&!dock.allCompleted} cx="29" cy="29" r="25" pathLength="100" stroke-dasharray={dock.allCompleted?'100 100':dock.progress===null?'5 7':`${dock.progress*100} 100`}/><circle class="dock-core" cx="29" cy="29" r="19"/></svg>
      <b class="dock-progress">{dock.total?`${dock.completed}/${dock.total}`:'—'}</b>
      {#if dock.blockers}<span class="dock-badge">{dock.blockers}</span>{/if}
    </button>
    {#if dockHovered&&summaryMode!=='never'}<div use:measureDock class="dock-summary" id="dock-summary" role="tooltip"><small class="dock-summary-label">当前任务</small><strong>{dock.title}</strong><span>{dock.step}</span><small>清单 {dock.completed}/{dock.total} · <b>{dock.blockers} 卡点</b> · {dock.active} 活跃</small></div>{/if}
  </div>{/if}
  {#if !utility}<div class="panel-shell" hidden={dockCollapsed} inert={dockCollapsed}>
  <header class="toolbar">
    <div class="toolbar-drag" role="presentation" onpointerdown={startToolbarPointer} onpointermove={moveToolbarPointer} onpointerup={endToolbarPointer} onpointercancel={endToolbarPointer} onlostpointercapture={endToolbarPointer} ondblclick={()=>{clearToolbarPointer();void windowAction('toggle_maximize');}}><span class="app-mark">&gt;_</span><strong>{view==='completed'||demoHistory?'完成记录':'巡检台'}</strong>{#if development}<small class="dev-tag" title="隔离开发版">DEV</small>{/if}<span class="connection" class:offline={!healthy || tone==='problem'} class:unconfirmed={healthy && tone==='neutral'} title={connectionError || connectionLabel}><i></i>{connectionLabel}</span></div>
    <div class="toolbar-actions">{#if view==='main'&&!demoHistory}<button class="icon-button" title="搜索任务或命令 · Ctrl+K" aria-label="搜索任务或命令" onclick={openSearch}><Icon name="search" size={14}/></button><button class="icon-button" title="收起为头像球" aria-label="收起为头像球" onclick={collapseDock}><Icon name="dock" size={14}/></button><button class="icon-button" class:pressed={pinned} title={pinned?'取消置顶':'置顶窗口'} aria-label={pinned?'取消置顶':'置顶窗口'} aria-pressed={pinned} onclick={togglePin}><Icon name="pin" size={14}/></button><button class="icon-button" title={options.theme==='dark'?'切换为浅色':'切换为暗色'} aria-label={options.theme==='dark'?'切换为浅色':'切换为暗色'} disabled={settingsSaving} onclick={()=>changeSetting({theme:options.theme==='dark'?'light':'dark'})}><Icon name={options.theme==='dark'?'sun':'moon'} size={14}/></button><div class="toolbar-menu-anchor"><button bind:this={menuButton} class="icon-button" class:pressed={menuOpen} title="更多" aria-label="更多" aria-haspopup="menu" aria-expanded={menuOpen} aria-controls="toolbar-menu" onclick={openMenu}><Icon name="more" size={14}/></button>{#if menuOpen}<div bind:this={menuPanel} id="toolbar-menu" class="toolbar-menu" role="menu" aria-label="更多操作" tabindex="-1" onkeydown={menuKeys}><button role="menuitem" onclick={()=>menuAction(settings)}>看板设置</button><button role="menuitem" onclick={()=>menuAction(integration)}>Codex 接入</button><button role="menuitem" onclick={()=>menuAction(()=>openUtility('notifications'))}>消息通知</button><button role="menuitem" onclick={()=>menuAction(()=>openUtility('updates'))}>版本更新</button><button role="menuitem" onclick={()=>menuAction(()=>openUtility('maintenance'))}>备份与诊断</button><div class="menu-divider" role="separator"></div><button role="menuitem" onclick={()=>menuAction(()=>openCompleted())}>完成记录</button><button role="menuitem" onclick={()=>menuAction(()=>windowAction('quit'))}>退出看板</button></div>{/if}</div>{/if}</div>
    <div class="window-controls"><button title="最小化" aria-label="最小化" onclick={()=>windowAction('minimize')}><Icon name="minimize" size={11}/></button><button title={maximized?'还原':'最大化'} aria-label={maximized?'还原':'最大化'} onclick={()=>windowAction('toggle_maximize')}><Icon name={maximized?'restore':'maximize'} size={11}/></button><button class="window-close" title="关闭到托盘" aria-label={view==='completed'||demoHistory?'关闭已完成记录':'关闭到托盘'} onclick={closeView}><Icon name="close" size={11}/></button></div>
  </header>
  {#if error || storageError || connectionError || notificationError}<div class="offline-banner" role="status"><span>{[error,storageError,connectionError,notificationError].filter(Boolean).join(' · ')}</span>{#if error}<button aria-label="关闭提示" onclick={()=>error=''}>×</button>{/if}<button onclick={integration}>诊断</button></div>{/if}
  <main class="matrix-workspace" class:history={view==='completed'||demoHistory}>
    {#if view==='completed'||demoHistory}<CompletedRecords {tasks} {now} bind:query={historyQuery} bind:page={historyPage} bind:pageSize={historySize} scopeTask={scope.scope} onDetails={showDetails}/>
    {:else}<MatrixBoard {tasks} {now} bind:selected bind:filter bind:query {loading} readError={storageError||snapshotFailed?'任务读取异常，保留最近快照':''} onDetails={showDetails} onOpenEvidence={openEvidence} onCopy={copy} onCompleted={()=>openCompleted()} onCommand={openSearch}/>{/if}
  </main>
  <footer><span>共 {tasks.length} 个任务 · 完成 {completed.length} · {tasks.reduce((sum,t)=>sum+openBlockers(t).length,0)} 个未解除卡点</span><span class="footer-actions">{#if current}<button onclick={()=>copy(current.session_id)}>复制会话 ID</button>{/if}<small title="当前看板版本">{appVersion}</small></span></footer>
  <TaskDrawer task={drawerTask} {now} bind:tab onClose={()=>drawerId=''} onOpenEvidence={openEvidence} onCopy={copy}/>
  {#if palette}<CommandPalette tasks={view==='completed'||demoHistory?completed:tasks} onClose={()=>palette=false} onSelect={task=>showDetails(task)} commands={[
    {id:'main',label:'返回主窗',detail:'导航',run:()=>{if(native&&view==='completed')void windowAction('show');else{demoHistory=false;scope={};}}},
    {id:'completed',label:'完成记录',detail:'导航',run:()=>openCompleted()},
    {id:'settings',label:'看板设置',detail:'功能',run:settings},
    {id:'integration',label:'Codex 接入',detail:'功能',run:integration},
    {id:'notifications',label:'消息通知',detail:'功能',run:()=>openUtility('notifications')},
    {id:'updates',label:'版本更新',detail:'功能',run:()=>openUtility('updates')},
    {id:'maintenance',label:'备份与诊断',detail:'功能',run:()=>openUtility('maintenance')},
    {id:'pin',label:pinned?'取消置顶':'置顶窗口',detail:'窗口',run:togglePin},
    {id:'theme',label:options.theme==='dark'?'切换为浅色':'切换为暗色',detail:'窗口',run:()=>changeSetting({theme:options.theme==='dark'?'light':'dark'})},
    ...(current?[{id:'copy-session',label:'复制会话 ID',detail:'当前任务',run:()=>copy(current.session_id)}]:[]),
    {id:'dock',label:'收起为头像球',detail:'窗口',run:collapseDock},
    {id:'sync',label:'复制任务同步指令',detail:'复制后发送到原任务聊天',run:()=>copy(runningTaskSyncPrompt)},
    {id:'close',label:view==='completed'||demoHistory?'关闭完成记录':'关闭到托盘',detail:'窗口',run:closeView},
    ...(development&&connection.codex_mode==='isolated'?[{id:'weixin-lab',label:'微信连接验证 · DEV',detail:'开发工具',run:()=>openUtility('weixin-lab')}]:[]),
    {id:'quit',label:'退出看板',detail:'应用',run:()=>{if(native)void action('quit');}}
  ].filter(command=>view==='main'&&!demoHistory||['main','close'].includes(command.id))}/>{/if}
  </div>{/if}
  {#if dialog}<div class="modal-backdrop" class:utility-backdrop={Boolean(utility)}><div use:trapDialog tabindex="-1" class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-heading" aria-busy={modalLocked}><div class="modal-title"><div class="utility-drag" role="presentation" onpointerdown={utility?startToolbarPointer:undefined} onpointermove={utility?moveToolbarPointer:undefined} onpointerup={utility?endToolbarPointer:undefined} onpointercancel={utility?endToolbarPointer:undefined}><span class="app-mark">&gt;_</span><strong id="modal-heading">{modalTitle}</strong>{#if utility}<small>{development?'DEV · 独立工具窗':'独立工具窗'}</small>{/if}</div><button title={`关闭${modalTitle}`} aria-label={`关闭${modalTitle}`} disabled={modalLocked} onclick={closeDialog}><Icon name="close"/></button></div>
    {#if dialog==='settings'}<div class="modal-form"><div class="modal-body">
      <section class="settings-section"><h3>外观与启动</h3>
        <label class="settings-row"><span class="row-label">登录时打开看板<small>后台始终随用户登录启动</small></span><input aria-label="登录时打开看板" type="checkbox" checked={options.auto_start} disabled={modalLocked||development} onchange={e=>changeSetting({auto_start:e.currentTarget.checked})}></label>
        <label class="settings-row"><span class="row-label">任务栏显示<small>关闭时仅保留托盘图标</small></span><input aria-label="任务栏显示" type="checkbox" checked={options.show_taskbar} disabled={modalLocked} onchange={e=>changeSetting({show_taskbar:e.currentTarget.checked})}></label>
        <label class="settings-row"><span class="row-label">桌面浮窗通知<small>任务与清单变化时在屏幕角落提醒</small></span><input aria-label="桌面浮窗通知" type="checkbox" checked={options.notifications} disabled={modalLocked} onchange={e=>changeSetting({notifications:e.currentTarget.checked})}></label>
        <label class="settings-row"><span class="row-label">主题</span><select aria-label="主题" value={options.theme} disabled={modalLocked} onchange={e=>changeSetting({theme:e.currentTarget.value})}><option value="dark">暗色</option><option value="light">浅色</option></select></label>
        <label class="settings-row"><span class="row-label">普通通知停留秒数<small>卡点通知保留到手动关闭</small></span><input aria-label="普通通知停留秒数" type="number" min="1" max="60" step="1" value={options.notification_seconds} disabled={modalLocked} onchange={e=>changeSetting({notification_seconds:Number(e.currentTarget.value)})}></label>
        <label class="settings-row"><span class="row-label">头像球摘要</span><select aria-label="头像球摘要" value={dockSummaryMode(options)} disabled={modalLocked} onchange={e=>changeSetting({dock_summary_mode:e.currentTarget.value as Options['dock_summary_mode']})}><option value="hover">悬停显示</option><option value="always">固定显示</option><option value="never">不显示</option></select></label>
        <div class="utility-actions"><button disabled={modalLocked} onclick={()=>testNotice()}>预览普通通知</button><button disabled={modalLocked} onclick={()=>testNotice(true)}>预览卡点通知</button></div>
      </section>
      <section class="settings-section"><h3>数据目录</h3><div class="settings-row"><code class="data-current">{dataLoading?'正在读取目录…':dataLocation.current||'目录暂不可用'}</code><button disabled={dataLoading||!native} onclick={()=>windowAction('data_folder')}>打开目录</button></div>{#if dataNote}<div class="form-note" role="status">{dataNote}</div>{/if}</section>
      <section class="settings-section"><h3>外部消息渠道 <span>微信 / 企微 / 飞书</span></h3><div class="settings-row"><span class="row-label">独立于浮窗通知<small>外部渠道有自己的开关与事件订阅</small></span><button onclick={()=>openUtility('notifications')}>配置消息通知 ↗</button></div></section>
      <section class="settings-section"><h3>备份与诊断</h3><button onclick={()=>openUtility('maintenance')}>备份与诊断 ↗</button></section>
      <section class="settings-section"><h3>版本 <span>{appVersion}</span></h3><div class="settings-row"><span class="row-label">当前版本 {appVersion}<small>自动检查与下载，安装前始终需要确认</small></span><button onclick={()=>openUtility('updates')}>检查更新 ↗</button></div></section>
    </div><div class="modal-actions"><small class="settings-feedback" class:failed={Boolean(note)} role="status" title={note}>{note||(settingsSaving?'正在保存…':'更改即时保存')}</small><button disabled={modalLocked} onclick={returnMain}>返回主窗</button></div></div>
    {:else if dialog==='notifications'}
      <div class="modal-body"><Notifications visible blocked={busy||dataBusy} onbusychange={value=>notificationBusy=value}/></div>
      <div class="modal-actions"><span></span><button type="button" disabled={modalLocked} onclick={returnMain}>返回主窗</button></div>
    {:else if dialog==='weixin-lab'&&development}
      <div class="modal-body"><WeixinLab visible blocked={busy||dataBusy} onbusychange={value=>notificationBusy=value}/></div>
      <div class="modal-actions"><span></span><button type="button" disabled={modalLocked} onclick={returnMain}>返回主窗</button></div>
    {:else if dialog==='maintenance'}
      <div class="modal-body"><AgentMaintenance blocked={busy||dataBusy||updateBusy} onbusychange={value=>maintenanceBusy=value}/></div>
      <div class="modal-actions"><span></span><button disabled={modalLocked} onclick={returnMain}>返回主窗</button></div>
    {:else if dialog==='updates'}
      <div class="modal-body">{#if view==='main'||utility==='updates'}<Updates visible blocked={busy||dataBusy} onbusy={value=>updateBusy=value}/>{/if}</div>
      <div class="modal-actions"><span></span><button type="button" disabled={modalLocked} onclick={returnMain}>返回主窗</button></div>
    {:else}
      <div class="modal-body"><Integration {connection} {diagnostics} onrefresh={refresh} onbusychange={value=>busy=value}/></div>
      <div class="modal-actions"><span></span><button disabled={modalLocked} onclick={returnMain}>返回主窗</button></div>
    {/if}
  </div></div>{/if}
{/if}
</div>
