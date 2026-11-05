// Exercise the installed DOM event handlers without a browser or external DOM package.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const shared = path.resolve(__dirname, '../shared');

function boot(saved = new Map()) {
  let clock = 1800000000000, nextTimer = 0, selected = 'pay';
  const timers = new Map(), handlers = {}, nodes = new Map(), fields = new Map();
  const element = () => ({ dataset:{}, classList:{contains:()=>false,toggle(){}}, innerHTML:'', textContent:'',
    append(){}, remove(){}, focus(){ document.activeElement=this; }, setAttribute(){}, querySelectorAll(){return [];} });
  const document = { activeElement: element(), body:element(), documentElement:{dataset:{}},
    addEventListener(type, fn) { (handlers[type] ||= []).push(fn); },
    getElementById(id) { return nodes.get(id); },
    createElement() { const el=element(); Object.defineProperty(el,'id',{set(v){nodes.set(v,el);}}); return el; },
    querySelector(selector) { const m=selector.match(/\[name="([^"]+)"\]/); return m ? fields.get(m[1]) || null : null; },
    querySelectorAll(){return [];} };
  class MockDate extends Date { constructor(...args){super(...(args.length?args:[clock]));} static now(){return clock;} }
  const schedule = (fn, delay, interval) => {const id=++nextTimer;timers.set(id,{fn,due:clock+delay,interval});return id;};
  const context = vm.createContext({window:{},document,Date:MockDate,URL,console,CSS:{escape:v=>v},
    sessionStorage:{getItem:k=>saved.get(k)||null,setItem:(k,v)=>saved.set(k,v)},
    navigator:{clipboard:{writeText:async()=>{}}},history:{replaceState(){}},
    setTimeout:(fn,ms=0)=>schedule(fn,ms,0),setInterval:(fn,ms)=>schedule(fn,ms,ms),
    clearTimeout:id=>timers.delete(id),clearInterval:id=>timers.delete(id)});
  for(const file of ['data.js','kit.js','mock.js']) vm.runInContext(fs.readFileSync(path.join(shared,file),'utf8'),context,{filename:file});
  const F=context.window.FB,K=context.window.KIT,M=context.window.MOCK,state={screen:'main'};
  M.install({variant:'state-test',state,selected:()=>selected,select:id=>selected=id,render(){}});
  function event(type,target,extra={}) {const e={target,preventDefault(){},stopImmediatePropagation(){},...extra};for(const fn of handlers[type]||[])fn(e);}
  function click(action,data={}) { const el=element();el.dataset={mock:action,...data};el.closest=s=>s==='[data-mock]'?el:null;event('click',el); }
  function change(name,value) {event('change',{name,value,dataset:{}});}
  function field(name,value) {fields.set(name,{value});}
  function advance(ms) {const end=clock+ms;let work=0;while(true){const candidates=[...timers].filter(([,t])=>t.due<=end).sort((a,b)=>a[1].due-b[1].due);if(!candidates.length)break;assert.ok(++work<1000,'timer loop remains bounded');const[id,t]=candidates[0];clock=t.due;if(!t.interval)timers.delete(id);t.fn();if(t.interval&&timers.has(id))t.due=clock+t.interval;}clock=end;}
  function confirm(){click('confirm-next');click('confirm-next');click('confirm-next');}
  function resolve(id){const t=F.tasks.find(t=>t.id==='pay');click('resolve',{task:t.id,blocker:id});field('resolution','Mock verification passed');click('save-resolution');}
  return {F,K,M,click,change,field,advance,confirm,resolve,saved,layer:()=>nodes.get('mock-layer')?.innerHTML||''};
}

const results=[];
function test(name,fn){try{fn();results.push({name,pass:true});}catch(error){results.push({name,pass:false,error:error.message});}}

test('Open blockers prevent advancing a task',()=>{const b=boot(),t=b.F.tasks[0],before=b.K.doneSteps(t);b.click('advance',{task:t.id});assert.equal(b.K.doneSteps(t),before);assert.notEqual(t.status,'completed');});
test('Waiting resumes only after confirmation',()=>{const b=boot();b.change('mock-scenario','waiting');const t=b.F.tasks[1];b.click('advance',{task:t.id});assert.equal(t.status,'waiting');b.confirm();assert.equal(t.status,'running');assert.equal(t.waitingSince,null);});
test('Resolving blockers closes paused step segments and preserves recorded duration',()=>{const b=boot(),t=b.F.tasks[0],s=t.plan.find(s=>s.status==='blocked'),before=b.K.elapsed(s);for(const blocker of [...t.blockers])b.resolve(blocker.id);assert.ok(!s.segments.some(seg=>seg.endedAt===null),'pending step has no open segment');b.click('advance',{task:t.id});assert.equal(s.status,'inProgress');assert.ok(b.K.elapsed(s)>=before,'blocked duration remains recorded after resume');});
test('Reopening completed task excludes time after its completion',()=>{const b=boot(),t=b.F.tasks.find(t=>t.status==='completed'),duration=b.K.elapsed(t);b.advance(60000);b.click('reopen',{task:t.id});assert.ok(Math.abs(b.K.elapsed(t)-duration)<1,'frozen completed duration is preserved');assert.notEqual(t.currentStep,'已完成');});
test('Task completes only after all steps and evidence; completion survives refresh',()=>{const b=boot(),t=b.F.tasks[1];for(let i=0;i<12&&t.status!=='completed';i++){b.advance(1000);b.click('advance',{task:t.id});}assert.equal(t.status,'completed');assert.equal(b.K.doneSteps(t),t.plan.length);assert.ok(t.evidence.length);assert.equal(b.K.openBlockers(t).length,0);const next=boot(b.saved);assert.equal(next.F.tasks.find(x=>x.id===t.id).status,'completed');});
test('Reset cancels an in-flight repair instead of mutating the new scenario',()=>{const b=boot();b.click('repair');b.confirm();assert.equal(b.F.repair.status,'running');b.change('mock-scenario','baseline');b.advance(2000);assert.equal(b.F.repair.status,'not_started');});
test('Reset cancels an in-flight installation',()=>{const b=boot(),version=b.F.updates.currentVersion;b.F.updates.state='ready';b.click('install');b.confirm();b.change('mock-scenario','baseline');b.advance(1000);assert.equal(b.F.updates.currentVersion,version);assert.equal(b.F.updates.state,'available');});
test('Empty data keeps controls usable',()=>{const b=boot();b.change('mock-scenario','empty');assert.equal(b.M.tasks().length,0);b.click('advance');assert.match(b.M.utility('completed'),/暂无完成记录/);assert.match(b.M.dock('c'),/并发任务 0/);});
test('Cancelled task is excluded from intervention queue',()=>{const b=boot();b.change('mock-scenario','states');const t=b.F.tasks[1];assert.equal(t.status,'cancelled');assert.ok(!b.K.actionItems(b.F.tasks).some(it=>it.task.id===t.id));});

for(const r of results)console.log(`${r.pass?'PASS':'FAIL'} ${r.name}${r.error?' — '+r.error:''}`);
console.log(`${results.filter(r=>r.pass).length}/${results.length} passed`);
if(results.some(r=>!r.pass))process.exitCode=1;
