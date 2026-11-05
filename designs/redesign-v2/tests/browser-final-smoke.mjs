const fs=await import('node:fs/promises');
const task=await taskSpace(8),page=task.page('p1'),results=[];
const out='D:/projects/codex-floatboard/designs/redesign-v2/validation';
for(const [variant,file] of Object.entries({a:'a-inspection-matrix.html',b:'b-focus-timeline.html',c:'c-swimlane-deck.html'})) {
  await page.goto('http://127.0.0.1:4311/redesign-v2/'+file+'?final='+Date.now());
  await page.selectOption('[name="mock-scenario"]','baseline');
  await page.click('#dock button[data-screen="settings"]');
  await page.selectOption('[data-option="theme"]','light');
  await page.click('#dock button[data-screen="main"]');
  results.push({name:variant+' light theme renders',pass:await page.evaluate(()=>document.documentElement.dataset.theme==='light'&&!!document.querySelector('.win'))});
  await page.screenshot({path:out+'/'+variant+'-light.png'});
  await page.click('#dock button[data-screen="narrow"]');
  results.push({name:variant+' narrow remains contained after CSS cleanup',pass:await page.evaluate(()=>{const e=document.querySelector('.win.narrow');return Math.round(e.getBoundingClientRect().height)===570&&e.scrollWidth<=e.clientWidth+1;})});
  await page.click('#dock button[data-screen="integration"]');
  await page.selectOption('[name="connection-state"]','conflict');
  results.push({name:variant+' conflict offers diagnostics without recovery overwrite',pass:await page.evaluate(()=>!!document.querySelector('button[data-mock="diagnostics"]')&&!document.querySelector('button[data-mock="recover"]'))});
  await page.selectOption('[name="connection-state"]','recovery_required');
  results.push({name:variant+' owned old connection has recovery preview',pass:await page.evaluate(()=>!!document.querySelector('button[data-mock="recover"]'))});
  await page.selectOption('[name="mock-scenario"]','baseline');
  await page.click('#dock button[data-screen="main"]');
}
await page.click('.lane[data-task="pay"] button[data-blocker="pay-b0"]');
await page.screenshot({path:out+'/c-blocker-detail.png'});
results.push({name:'C blocker marker opens correct detail and keyboard focus',pass:await page.evaluate(()=>document.querySelector('.m-highlight')?.dataset.blockerId==='pay-b0'&&document.querySelector('.m-drawer').contains(document.activeElement))});
await page.keyboard.press('Escape');
await fs.writeFile(out+'/browser-final-smoke.json',JSON.stringify({results,passed:results.filter(r=>r.pass).length,total:results.length},null,2));
console.log(JSON.stringify({passed:results.filter(r=>r.pass).length,total:results.length,failures:results.filter(r=>!r.pass)}));
