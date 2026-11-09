<script lang="ts">
  import { onMount } from 'svelte';
  import { call, native, action } from './host';
  import { runningTaskSyncPrompt, connectionStatus, evidenceTime, type Connection } from './model';
  import UtilityDialog from './UtilityDialog.svelte';
  export let connection:Connection;
  export let diagnostics='';
  export let onbusychange:(value:boolean)=>void=()=>{};
  $: onbusychange(busy);
  export let onrefresh:()=>Promise<void>;
  type Change={title:string;key:string;before:string;after:string;diff:string};
  type Pairing={id:string;client_id:string;redirect_uri:string;expires_at:number;approved:boolean};
  let busy=false,error='',changes:Change[]=[],beforeHash='',confirm:'connect'|'disconnect'|'revoke'|null=null;
  let pending:Pairing[]=[],pairing:Pairing|null=null;
  async function preview(remove=false){
    busy=true;error='';
    try{const value=await call<{before_hash:string;changes:Change[]}>('preview',{remove});changes=value.changes;beforeHash=value.before_hash;confirm=remove?'disconnect':'connect';}
    catch(e){error=String(e);}finally{busy=false;}
  }
  async function execute(){
    if(!confirm)return;busy=true;error='';
    try{await call(confirm==='revoke'?'pairing_revoke':confirm,{confirmed:true,before_hash:beforeHash});confirm=null;await onrefresh();await loadPairing();}
    catch(e){error=String(e);}finally{busy=false;}
  }
  async function loadPairing(){if(!native)return;try{pending=await call<Pairing[]>('pairing_list');}catch(e){error=String(e);}}
  async function approve(){if(!pairing)return;busy=true;error='';try{await call('pairing_approve',{id:pairing.id,confirmed:true});pairing=null;await loadPairing();}catch(e){error=String(e);}finally{busy=false;}}
  onMount(()=>{let stopped=false;let timer:ReturnType<typeof setTimeout>;async function poll(){await loadPairing();if(!stopped)timer=setTimeout(poll,1500);}void poll();return()=>{stopped=true;clearTimeout(timer);};});
</script>
<section class="settings-section"><h3>Codex 接入 <span>{connectionStatus(connection)}</span></h3>
  <div class="settings-row"><code class="data-current">{connection.codex_home||'目录不可用'}</code></div>
  <div class="utility-actions"><button class="primary" disabled={busy||!native} on:click={()=>preview()}>预览接入配置</button><button disabled={busy||connection.state==='missing'} on:click={()=>preview(true)}>断开连接…</button><button disabled={busy} on:click={onrefresh}>刷新</button></div>
</section>
<section class="settings-section"><h3>本地 OAuth 配对</h3>
  {#each pending as item}<div class="pairing"><code>{item.client_id}</code><small>{item.redirect_uri}</small><button disabled={busy||item.approved} on:click={()=>pairing=item}>{item.approved?'等待 Codex 完成':'确认此客户端…'}</button></div>{/each}
  {#if !pending.length}<div class="empty small">暂无待确认配对</div>{/if}
  <div class="utility-actions"><button disabled={busy} on:click={()=>confirm='revoke'}>撤销全部授权…</button></div>
</section>
<section class="settings-section"><h3>任务同步</h3><div class="utility-actions"><button on:click={()=>action('copy',runningTaskSyncPrompt)}>复制同步指令</button><span>{connection.sync_verified_at?evidenceTime(connection.sync_verified_at):'尚未收到任务汇报'}</span></div></section>
{#if connection.error||error}<div class="form-note" role="alert">{error||connection.error}</div>{/if}
<details class="connection-diagnostics"><summary>诊断</summary><pre>{diagnostics||'暂无额外诊断'}</pre></details>
{#if confirm}<UtilityDialog title={confirm==='revoke'?'撤销全部授权':confirm==='disconnect'?'确认断开连接':'确认接入配置'} locked={busy} onClose={()=>confirm=null}>
  {#if confirm==='revoke'}<p>当前 profile 的全部令牌和待确认配对将失效。</p>{:else}{#each changes as change}<details open><summary>{change.title}</summary><pre>{change.diff}</pre></details>{/each}{/if}
  <div class="utility-actions"><button disabled={busy} on:click={()=>confirm=null}>取消</button><button class="primary" disabled={busy} on:click={execute}>确认</button></div>{#if error}<p role="alert">{error}</p>{/if}
</UtilityDialog>{/if}
{#if pairing}<UtilityDialog title="确认本地配对" locked={busy} onClose={()=>pairing=null}><p>仅在你刚刚发起 Codex 登录且回调地址相符时确认。</p><pre>{pairing.client_id}{'\n'}{pairing.redirect_uri}</pre><button class="primary" disabled={busy} on:click={approve}>授权此客户端</button>{#if error}<p role="alert">{error}</p>{/if}</UtilityDialog>{/if}
<style>.pairing{display:grid;gap:6px;padding:8px 0;border-bottom:1px solid var(--divider)}code,pre{overflow-wrap:anywhere;white-space:pre-wrap;font-size:11px;user-select:text}</style>
