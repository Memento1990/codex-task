const fs=await import('node:fs/promises');
const task=await taskSpace(8),page=task.page('p1'),results=[];
try {
  for(const [variant,file] of Object.entries({a:'a-inspection-matrix.html',b:'b-focus-timeline.html',c:'c-swimlane-deck.html'})) {
    await page.goto('http://127.0.0.1:4311/redesign-v2/'+file+'?delivery='+Date.now());
    await page.selectOption('[name="mock-scenario"]','baseline');
    await page.click('#dock button[data-screen="settings"]');
    await page.click('input[data-option="notifications"]');
    await page.click('button[data-mock="test-notice"]');
    results.push({name:variant+' disabled notification preview explains prerequisite',pass:await page.evaluate(()=>!FB.options.notifications&&document.querySelector('#mock-toast')?.textContent.includes('请先开启'))});
    await page.click('input[data-option="notifications"]');
    await page.click('button[data-mock="test-blocker-notice"]');
    results.push({name:variant+' enabled preview produces notification with task action',pass:await page.evaluate(()=>FB.options.notifications&&!!document.querySelector('#mock-notices button[data-mock="detail"]'))});
    await page.selectOption('[name="mock-scenario"]','empty');
    await page.click('button[data-mock="test-blocker-notice"]');
    results.push({name:variant+' empty data notification has no invalid task action',pass:await page.evaluate(()=>!!document.querySelector('.m-notice')&&!document.querySelector('#mock-notices button[data-mock="detail"]'))});
    await page.selectOption('[name="mock-scenario"]','baseline');
    await page.click('#dock button[data-screen="main"]');
  }
  await page.goto('http://127.0.0.1:4311/redesign-v2/index.html?delivery='+Date.now());
  const links=await page.evaluate(()=>[...document.querySelectorAll('a[href]')].map(a=>({label:a.textContent.trim(),href:a.href})).filter(a=>a.href.startsWith(location.origin)));
  for(const link of links){const response=await page.fetch(link.href);results.push({name:'index link '+link.label,pass:response.ok});}
  results.push({name:'index copy states Mock and adaptation boundaries',pass:await page.evaluate(()=>!document.body.textContent.includes('逐组件替换即可')&&!document.body.textContent.includes('完全一致')&&document.body.textContent.includes('最小事件投影'))});
} catch(error) {
  results.push({name:'execution',pass:false,error:error.message});
} finally {
  await fs.writeFile('D:/projects/codex-floatboard/designs/redesign-v2/validation/browser-delivery-smoke.json',JSON.stringify({results,passed:results.filter(r=>r.pass).length,total:results.length},null,2));
  console.log(JSON.stringify({passed:results.filter(r=>r.pass).length,total:results.length,failures:results.filter(r=>!r.pass)}));
  console.log(await page.snapshot());
}
