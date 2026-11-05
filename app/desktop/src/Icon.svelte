<script lang="ts">
  let {name, size = 16}: {name:'pin'|'more'|'minimize'|'maximize'|'restore'|'close'|'folder'|'details'|'dock'|'search'|'gear'|'sun'|'moon';size?:number} = $props();
</script>
<svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
  {#if name==='pin'}<path d="m7 3 7 0-1 5 3 3H4l3-3V3Zm3 8v6"/>
  {:else if name==='more'}<circle cx="4" cy="10" r="1" fill="currentColor" stroke="none"/><circle cx="10" cy="10" r="1" fill="currentColor" stroke="none"/><circle cx="16" cy="10" r="1" fill="currentColor" stroke="none"/>
  {:else if name==='minimize'}<path d="M4 10h12"/>
  {:else if name==='maximize'}<rect x="4" y="4" width="12" height="12" rx="1"/>
  {:else if name==='restore'}<path d="M7 6V3h10v10h-3"/><rect x="3" y="7" width="10" height="10" rx="1"/>
  {:else if name==='dock'}<circle cx="10" cy="10" r="7"/><path d="M7 8 9 10 7 12m4 0h2"/>
  {:else if name==='search'}<circle cx="9" cy="9" r="5"/><path d="m13 13 3.5 3.5"/>
  {:else if name==='sun'}<circle cx="10" cy="10" r="3"/><path d="M10 2v1.5M10 16.5V18M2 10h1.5M16.5 10H18M4.3 4.3l1.1 1.1M14.6 14.6l1.1 1.1M15.7 4.3l-1.1 1.1M5.4 14.6l-1.1 1.1"/>
  {:else if name==='moon'}<path d="M17 11.2A7.2 7.2 0 0 1 8.8 3a7.2 7.2 0 1 0 8.2 8.2Z"/>
  {:else if name==='gear'}<path d="M8.5 2h3l.5 2.2 1.5.9 2.2-.7 1.5 2.6-1.7 1.5v1.8l1.7 1.5-1.5 2.6-2.2-.7-1.5.9-.5 2.2h-3L8 14.6l-1.5-.9-2.2.7-1.5-2.6 1.7-1.5V8.5L2.8 7l1.5-2.6 2.2.7L8 4.2 8.5 2Z"/><circle cx="10" cy="9.4" r="2.5"/>
  {:else if name==='details'}<rect x="3" y="4" width="14" height="12" rx="1"/><path d="M8 4v12m4-8 2 2-2 2"/>
  {:else if name==='folder'}<path d="M3 6V4h5l2 2h7v10H3V6Z"/>
  {:else}<path d="m5 5 10 10M15 5 5 15"/>{/if}
</svg>
