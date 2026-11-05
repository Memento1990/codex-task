<script lang="ts">
  import type { Snippet } from 'svelte';
  import { modalFocus } from './matrix-format';
  import Icon from './Icon.svelte';
  let { title, locked=false, onClose, children }:{title:string;locked?:boolean;onClose:()=>void;children:Snippet}=$props();
  function close(){if(!locked)onClose();}
  function keys(event:KeyboardEvent){if(event.key==='Escape'){event.preventDefault();event.stopPropagation();close();}}
</script>

<div class="utility-dialog-overlay">
  <button class="utility-dialog-backdrop" tabindex="-1" aria-label={`关闭${title}`} disabled={locked} onclick={close}></button>
  <div class="utility-dialog" role="dialog" aria-modal="true" aria-label={title} aria-busy={locked} tabindex="-1" use:modalFocus onkeydown={keys}>
    <header><h2>{title}</h2><button type="button" aria-label={`关闭${title}`} disabled={locked} onclick={close}><Icon name="close" size={14}/></button></header>
    <div class="utility-dialog-body">{@render children()}</div>
  </div>
</div>

<style>
  .utility-dialog-overlay{position:fixed;inset:0;z-index:110;display:flex;align-items:center;justify-content:center;padding:20px}
  .utility-dialog-backdrop{position:absolute;inset:0;width:100%;height:100%;border:0;border-radius:0;background:#0009;cursor:default}
  .utility-dialog{position:relative;display:flex;flex-direction:column;min-height:0;max-height:100%;width:460px;max-width:100%;color:var(--mx-fg);background:var(--mx-panel);border:1px solid var(--mx-line2);border-radius:7px;box-shadow:0 18px 64px #0009;overflow:hidden;font-size:12px}
  header{display:flex;align-items:center;justify-content:space-between;flex:none;gap:12px;padding:11px 14px;border-bottom:1px solid var(--mx-line)}
  h2{margin:0;color:var(--mx-fg);font-size:13px;font-weight:600}
  header button{display:flex;align-items:center;justify-content:center;width:26px;height:26px;padding:4px;background:transparent;border:0;color:var(--mx-dim)}
  .utility-dialog-body{min-height:0;overflow:auto;padding:14px;line-height:1.65}
</style>
