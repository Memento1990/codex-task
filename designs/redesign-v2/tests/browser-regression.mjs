// Run with ego-browser nodejs -e "await import('file:///D:/projects/codex-floatboard/designs/redesign-v2/tests/browser-regression.mjs?variant=a')"
// Uses this task's browser space; does not launch a native GUI or call the app bridge.
const fs=await import('node:fs/promises');
const variant=new URL(import.meta.url).searchParams.get('variant')||'a';
const file={a:'a-inspection-matrix.html',b:'b-focus-timeline.html',c:'c-swimlane-deck.html'}[variant];
const task=await taskSpace(8),page=task.page('p1');
const results=[],out='D:/projects/codex-floatboard/designs/redesign-v2/validation';
await fs.mkdir(out,{recursive:true});
async function check(name,fn){const value=await fn();results.push({name,pass:!!value});if(!value)throw new Error(name);}
const click=selector=>page.click(selector);
const nav=screen=>click(`#dock button[data-screen="${screen}"]`);
const state=fn=>page.evaluate(fn);
try {
  await page.goto('http://127.0.0.1:4311/redesign-v2/'+file+'?validation='+Date.now());
  await page.evaluate(()=>{window.mockErrors=[];window.addEventListener('error',e=>window.mockErrors.push(e.message));window.addEventListener('unhandledrejection',e=>window.mockErrors.push(String(e.reason)));});
  await page.selectOption('[name="mock-scenario"]','baseline');
  await nav('main');
  await check('baseline renders four active tasks',()=>state(()=>MOCK.tasks().length===4&&document.querySelector('.win')));
  await page.fill('[name="task-search"]','日志');
  await check('task search filters visible model',()=>state(()=>MOCK.tasks().length===1&&MOCK.tasks()[0].id==='log'));
  await page.fill('[name="task-search"]','不存在的搜索');
  await check('empty search has explicit empty state',()=>state(()=>MOCK.tasks().length===0&&/无匹配|没有匹配/.test(document.querySelector('#stage').textContent)));
  await page.fill('[name="task-search"]','');
  await page.selectOption('[name="task-filter"]','completed');
  await check('completed filter returns completed tasks',()=>state(()=>MOCK.tasks().length===2&&MOCK.tasks().every(t=>t.status==='completed')));
  await page.selectOption('[name="task-filter"]','active');
  const detailSelector=variant==='c'?'.lane[data-task="pay"]':'button[data-mock="detail"][data-task="pay"]';
  await click(detailSelector);
  await check('task detail shows checklist and owns focus',()=>state(()=>document.querySelectorAll('.m-drawer .m-step').length===7&&document.querySelector('.m-drawer').contains(document.activeElement)));
  await click('.m-tabs button[data-tab="blockers"]');
  await click('button[data-mock="resolve"][data-blocker="pay-b0"]');
  await click('button[data-mock="save-resolution"]');
  await check('blocker resolution requires verification result',()=>state(()=>document.querySelector('.m-dialog .m-error').textContent.length>0));
  await page.fill('[name="resolution"]','模拟条件满足，回归通过');
  await click('button[data-mock="save-resolution"]');
  await check('resolution updates blocker and creates evidence',()=>state(()=>!FB.tasks[0].blockers[0].open&&FB.tasks[0].evidence.length===3));
  await click('.m-tabs button[data-tab="evidence"]');
  await click('.m-drawer button[data-mock="evidence"][data-evidence="pay-e0"]');
  await check('evidence opens in a safe preview',()=>state(()=>document.querySelector('.m-dialog').textContent.includes('不访问真实路径')));
  await page.keyboard.press('Escape');
  await click('button[data-mock="close-drawer"]');
  await page.keyboard.press('Control+k');
  await page.fill('[name="command-search"]','不存在命令');
  await check('command search shows no-result state',()=>state(()=>document.querySelector('.m-command-list').textContent.includes('没有匹配')));
  await page.fill('[name="command-search"]','日志清洗');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await check('keyboard command selects task',()=>state(()=>document.querySelector('.m-drawer h2').textContent.includes('日志清洗')));
  for(let i=0;i<9&&await state(()=>FB.tasks.find(t=>t.id==='log').status!=='completed');i++)await click('.m-drawer button[data-mock="advance"]');
  await check('task advancement closes into completed record',()=>state(()=>{const t=FB.tasks.find(t=>t.id==='log');return t.status==='completed'&&t.plan.every(s=>s.status==='completed')&&t.evidence.length>0;}));
  await page.keyboard.press('Escape');
  for(const screen of ['completed','settings','integration','notifications','updates','repair']) {
    await nav(screen);
    await check(screen+' utility opens',()=>state(()=>!!document.querySelector('.m-window .m-body,.m-window .m-table-wrap')));
  }
  await page.selectOption('[name="mock-scenario"]','history');
  await nav('completed');
  await check('history defaults to 20 rows',()=>state(()=>document.querySelectorAll('.m-table tbody tr[data-task]').length===20));
  await page.selectOption('[name="history-size"]','50');
  await click('button[data-mock="page-next"]');
  await check('history pagination has final five records',()=>state(()=>document.querySelectorAll('.m-table tbody tr[data-task]').length===5));
  for(const scenario of ['partial','empty','states','failure','waiting','clean']) {
    await page.selectOption('[name="mock-scenario"]',scenario);
    await nav('main');
    await check(scenario+' scenario renders without bogus values',()=>state(()=>{const txt=document.querySelector('#stage').textContent;return !/NaN|Infinity|undefined/.test(txt)&&!!document.querySelector('.win');}));
  }
  await page.selectOption('[name="mock-scenario"]','baseline');
  await nav('main');
  await page.screenshot({path:out+'/'+variant+'-main.png'});
  await nav('narrow');
  await check('narrow viewport is 392 by 570 with internal overflow',()=>state(()=>{const el=document.querySelector('.win.narrow'),r=el.getBoundingClientRect();return Math.round(r.width)===392&&Math.round(r.height)===570&&el.scrollWidth<=el.clientWidth+1;}));
  await page.screenshot({path:out+'/'+variant+'-narrow.png'});
  await nav('dock');
  await check('dock summary starts hidden in hover mode',()=>state(()=>getComputedStyle(document.querySelector('.m-orb-summary')).display==='none'));
  await page.hover('.m-orb');
  await check('dock summary appears on hover',()=>state(()=>getComputedStyle(document.querySelector('.m-orb-summary')).display==='block'));
  await page.screenshot({path:out+'/'+variant+'-dock.png'});
  await click('.m-orb');
  await check('dock click restores the main window',()=>state(()=>!!document.querySelector('.win')));
  await check('browser has no observed runtime errors',()=>state(()=>window.mockErrors.length===0));
} catch(error) {
  results.push({name:'execution',pass:false,error:error.message});
  console.log(await page.snapshot());
} finally {
  await fs.writeFile(out+'/browser-'+variant+'.json',JSON.stringify({variant,results,passed:results.filter(r=>r.pass).length,total:results.length},null,2));
  console.log(JSON.stringify({variant,passed:results.filter(r=>r.pass).length,total:results.length,failures:results.filter(r=>!r.pass)}));
}
