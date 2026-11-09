<script lang="ts">
  import { call, native } from './host';
  export let blocked=false;
  export let onbusychange:(value:boolean)=>void=()=>{};
  let busy=false, result='', error='';
  async function run(method:'backup'|'diagnostics'){
    busy=true;onbusychange(true);error='';
    try{if(!native)throw new Error('请在桌面程序中操作');const value=await call<Record<string,unknown>>(method);result=JSON.stringify(value,null,2);}
    catch(e){error=String(e);}finally{busy=false;onbusychange(false);}
  }
</script>
<section class="settings-section"><h3>一致性备份</h3><button disabled={blocked||busy} on:click={()=>run('backup')}>生成备份</button></section>
<section class="settings-section"><h3>脱敏诊断</h3><button disabled={blocked||busy} on:click={()=>run('diagnostics')}>读取诊断</button></section>
{#if error}<div class="form-note" role="alert">{error}</div>{/if}
{#if result}<pre class="result" aria-label="操作结果">{result}</pre>{/if}
<style>.result{font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere;user-select:text}</style>
