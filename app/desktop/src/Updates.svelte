<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { action, native } from './host';
  import { appVersion } from './version';
  import { safeUpdateUrl, updateBytes, updatePresentation, updateProgress, type UpdateSnapshot } from './model';
  let {visible=false, compact=false, blocked=false, onbusy=(_value:boolean)=>{}}:{visible?:boolean;compact?:boolean;blocked?:boolean;onbusy?:(value:boolean)=>void}=$props();
  let snapshot=$state<UpdateSnapshot>({current_version:appVersion,state:'idle',downloaded_bytes:0,preferences:{auto_check:true,auto_download:true},supported:true});
  let loaded=$state(false), pending=$state(false), requestError=$state(''), feedback=$state(''), confirmInstall=$state(false);
  let revision=0;
  let presentation=$derived(updatePresentation(snapshot));
  let percent=$derived(updateProgress(snapshot.downloaded_bytes,snapshot.total_bytes));
  let releaseUrl=$derived(safeUpdateUrl(snapshot.release_url)?snapshot.release_url!:'https://github.com/Memento1990/codex-task/releases/latest');
  let locked=$derived(pending || snapshot.state==='installing');
  $effect(()=>{onbusy(locked);});
  $effect(()=>{if(snapshot.state!=='ready')confirmInstall=false;});
  async function request(name:string,params:Record<string,unknown>={}) {
    if(pending)return;
    ++revision;pending=true;requestError='';feedback='';
    try{
      if(native)snapshot=await invoke<UpdateSnapshot>('update_action',{action:name,params});
      else snapshot={...snapshot,state:'unsupported',supported:false};
      loaded=true;
      if(name==='preferences')feedback='更新偏好已保存';
    }catch(e){requestError='更新操作失败：'+String(e);}finally{pending=false;}
  }
  async function poll(){
    if(pending)return;
    const observed=revision;
    try{const next=native?await invoke<UpdateSnapshot>('update_action',{action:'snapshot',params:{}}):{...snapshot,state:'unsupported',supported:false};if(observed!==revision)return;snapshot=next;loaded=true;}
    catch(e){if(observed===revision&&!loaded)requestError='暂时无法读取更新状态：'+String(e);}
  }
  function escapeConfirmation(event:KeyboardEvent){if(event.key==='Escape'&&confirmInstall){event.preventDefault();event.stopPropagation();if(!locked)confirmInstall=false;}}
  function focusConfirmation(node:HTMLDivElement){queueMicrotask(()=>node.querySelector<HTMLButtonElement>('button')?.focus());}
  async function primary(){if(presentation.action==='install')confirmInstall=true;else if(presentation.action)await request(presentation.action);}
  async function openRelease(){try{if(native)await action('open_reference',releaseUrl);else window.open(releaseUrl,'_blank','noopener,noreferrer');}catch(e){requestError='无法打开发布页面：'+String(e);}}
  onMount(()=>{let stopped=false;let timer:ReturnType<typeof setTimeout>;async function loop(){await poll();if(!stopped)timer=setTimeout(loop,visible?1500:10000);}void loop();return()=>{stopped=true;clearTimeout(timer);onbusy(false);};});
</script>
{#if visible}
<section class="updates" class:compact aria-label="版本更新">
  <div class="updates-heading"><strong>版本更新</strong><span>当前版本 {snapshot.current_version || appVersion}</span></div>
  <div class="update-status" role="status"><strong>{!loaded&&!requestError?'正在读取更新状态…':presentation.title}</strong>{#if snapshot.latest_version}<span class="update-version">{snapshot.latest_version}</span>{/if}<p>{presentation.description}</p></div>
  {#if snapshot.state==='downloading'}<div class="update-download"><progress max="100" value={percent===null?undefined:percent} aria-label="更新下载进度"></progress><span>{updateBytes(snapshot.downloaded_bytes)}{snapshot.total_bytes?` / ${updateBytes(snapshot.total_bytes)}`:''}{percent!==null?` · ${percent}%`:''}</span></div>{/if}
  {#if snapshot.release_notes}<details class="update-notes"><summary>发布说明</summary><div>{snapshot.release_notes}</div></details>{/if}
  {#if snapshot.installation_result?.status==='rolled_back'}<p class="update-error" role="status">上次更新未完成启动确认，已恢复旧版本入口。任务数据未回退，可以重新检查并重试。</p>{:else if snapshot.installation_result?.status==='awaiting_health'}<p class="update-error" role="alert">新版仍运行但未完成启动确认。请正常退出看板后重新启动，以恢复旧版本；不会强制结束进程。</p>{:else if snapshot.installation_result?.status==='completed'}<p class="update-meta" role="status">已完成更新至 {snapshot.installation_result.version}</p>{/if}
  {#if snapshot.error}<p class="update-error" role="status">{snapshot.error}</p>{/if}
  {#if requestError}<p class="update-error" role="alert">{requestError}</p>{/if}
  {#if confirmInstall}<div use:focusConfirmation class="confirm-box update-confirm" role="group" aria-label="安装更新确认"><strong>安装并重启看板？</strong><p>仅关闭并重启看板，不会结束 Codex 或正在进行的任务。任务数据将保留。未保存的看板设置不会自动保存。</p>{#if blocked}<p>请先完成数据迁移后的重启或当前设置操作，再安装更新。</p>{/if}<div><button type="button" disabled={locked} onkeydown={escapeConfirmation} onclick={()=>confirmInstall=false}>稍后安装</button><button type="button" class="primary" onkeydown={escapeConfirmation} disabled={locked||blocked} onclick={()=>request('install',{confirmed:true})}>确认安装并重启</button></div></div>
  {:else}<div class="update-actions">{#if presentation.action}<button type="button" class="primary" disabled={!loaded||locked||blocked} onclick={primary}>{pending?'正在处理…':presentation.button}</button>{/if}{#if snapshot.state==='downloading'}<button type="button" disabled={locked} onclick={()=>request('cancel')}>取消下载</button>{/if}<button type="button" disabled={snapshot.state==='installing'} onclick={openRelease}>官方发布页面 ↗</button></div>{/if}
  <div class="update-preferences"><label class="check"><input type="checkbox" checked={snapshot.preferences.auto_check} disabled={!loaded||locked||!snapshot.supported} onchange={event=>request('preferences',{auto_check:event.currentTarget.checked,auto_download:snapshot.preferences.auto_download})}>自动检查更新</label><label class="check"><input type="checkbox" checked={snapshot.preferences.auto_download} disabled={!loaded||locked||!snapshot.supported} onchange={event=>request('preferences',{auto_check:snapshot.preferences.auto_check,auto_download:event.currentTarget.checked})}>自动下载更新</label><p>偏好即时保存；自动下载不会自动安装。安装前始终需要确认。</p></div>
  <div class="update-meta" role="status">{feedback || (snapshot.last_checked?`上次检查：${new Date(snapshot.last_checked*1000).toLocaleString('zh-CN',{hour12:false})}`:'尚未完成版本检查')}</div>
</section>
{/if}
