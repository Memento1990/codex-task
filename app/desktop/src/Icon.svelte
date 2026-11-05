<script lang="ts">
  let {name, size = 16}: {name:'pin'|'more'|'minimize'|'maximize'|'restore'|'close'|'folder'|'details'|'dock'|'search'|'gear';size?:number} = $props();
</script>
<svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if name==='pin'}<path d="m7 3 7 0-1 5 3 3H4l3-3V3Zm3 8v6"/>
  {:else if name==='more'}<circle cx="4" cy="10" r="1" fill="currentColor" stroke="none"/><circle cx="10" cy="10" r="1" fill="currentColor" stroke="none"/><circle cx="16" cy="10" r="1" fill="currentColor" stroke="none"/>
  {:else if name==='minimize'}<path d="M4 10h12"/>
  {:else if name==='maximize'}<rect x="4" y="4" width="12" height="12" rx="1"/>
  {:else if name==='restore'}<path d="M7 6V3h10v10h-3"/><rect x="3" y="7" width="10" height="10" rx="1"/>
  {:else if name==='dock'}<circle cx="10" cy="10" r="7"/><path d="M7 8 9 10 7 12m4 0h2"/>
  {:else if name==='search'}<circle cx="9" cy="9" r="5"/><path d="m13 13 3.5 3.5"/>
  {:else if name==='gear'}<circle cx="10" cy="10" r="2.6"/><path d="M10 2.6v2M10 15.4v2M2.6 10h2M15.4 10h2M4.8 4.8l1.4 1.4M13.8 13.8l1.4 1.4M15.2 4.8l-1.4 1.4M6.2 13.8l-1.4 1.4"/>
  {:else if name==='details'}<rect x="3" y="4" width="14" height="12" rx="1"/><path d="M8 4v12m4-8 2 2-2 2"/>
  {:else if name==='folder'}<path d="M3 6V4h5l2 2h7v10H3V6Z"/>
  {:else}<path d="m5 5 10 10M15 5 5 15"/>{/if}
</svg>
