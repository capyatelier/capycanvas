// Physical-browser UI startup/response benchmark. Use a dedicated test origin.
// Example: LAYER_DEVICE_CDP=http://127.0.0.1:9246 LAYER_WEB_URL=http://127.0.0.1:4196/ node tools/performance/web-ui-startup.mjs
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
const endpoint=process.env.LAYER_DEVICE_CDP||'http://127.0.0.1:9246';
const url=process.env.LAYER_WEB_URL||'http://127.0.0.1:4196/';
const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/ui-startup';
const count=Number(process.env.LAYER_UI_RUNS||3);
await mkdir(output,{recursive:true});
let tab;
for (const deadline=Date.now()+10000; !tab && Date.now()<deadline;) {
 const tabs=await(await fetch(`${endpoint}/json/list`)).json();
 tab=tabs.find(x=>x.url===url);
 if(!tab)await new Promise(resolve=>setTimeout(resolve,100));
}
assert(tab,`Open the dedicated test origin ${url}`);
const ws=new WebSocket(tab.webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j;});
let onLoad;
let sequence=0;const pending=new Map(),errors=[];
ws.onmessage=e=>{const m=JSON.parse(e.data);if(m.id){const p=pending.get(m.id);if(!p)return;pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(JSON.stringify(m.error))):p.resolve(m.result);}else if(m.method==='Page.loadEventFired'){onLoad?.();onLoad=null;}else if(m.method==='Page.javascriptDialogOpening'&&m.params.type==='beforeunload'){call('Page.handleJavaScriptDialog',{accept:true}).catch(()=>{});}else if(m.method==='Runtime.exceptionThrown')errors.push(m.params);};
const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence,timer=setTimeout(()=>{pending.delete(id);reject(Error(method));},150000);pending.set(id,{resolve,reject,timer});ws.send(JSON.stringify({id,method,params}));});
const evaluate=async expression=>{const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value;};
const waitFor=(condition,timeout=120000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${condition})resolve();else if(performance.now()>end)reject(Error('UI startup timed out'));else setTimeout(check,25)}check()})`);
const reload=async(ignoreCache=false)=>{const loaded=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Navigation timed out')),20000);onLoad=()=>{clearTimeout(timer);resolve();};});await call('Page.reload',{ignoreCache});await loaded;};
const median=values=>values.sort((a,b)=>a-b)[Math.floor(values.length/2)];
let preload;
try {
 for(const domain of ['Page','Runtime','Network'])await call(domain+'.enable');
 await call('Page.bringToFront');await call('Network.setCacheDisabled',{cacheDisabled:false});
 if(process.argv.includes("--refresh-assets"))await reload(true);
 // Warm the normal browser/driver caches before measured reloads.
 await waitFor('window.layerApp?.startupTimes.complete != null');
 assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"),0,'Use a clean fixture without recovery or other open dialogs');
 errors.length=0;
 preload=(await call('Page.addScriptToEvaluateOnNewDocument',{source:`window.uiAudit={longtasks:[],frames:[]};new PerformanceObserver(l=>uiAudit.longtasks.push(...l.getEntries().map(x=>({start:x.startTime,duration:x.duration})))).observe({type:'longtask',buffered:true});let previous;function frame(t){if(previous)uiAudit.frames.push([t,t-previous]);previous=t;if(performance.now()<15000)requestAnimationFrame(frame)}requestAnimationFrame(frame);`})).identifier;
 const results=[];
 for(let run=0;run<count;run++) {
  await reload();await waitFor("performance.getEntriesByName('capy.startup.workspace').length > 0",10000);
  const measureSettings=()=>evaluate(`(async()=>{const results=[];for(let i=0;i<4;i++){await new Promise(r=>setTimeout(r,300));const start=performance.now();document.querySelector('[data-command="settings"]').click();const sync=performance.now()-start;await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));if(!document.querySelector('#settings').open)throw Error('Settings not open');results.push({sync_ms:sync,frame_ms:performance.now()-start,compiling:layerApp.startupTimes.complete==null});layerApp.dispatch({type:'close_settings'});await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));}return results})()`);
  const settings=await measureSettings();
  await waitFor('layerApp.startupTimes.complete != null');
  const steadySettings=await measureSettings();
  // Tab selection is persisted in layout history. Measure it only after the
  // final startup sample so earlier probes don't enlarge the next fixture.
  const panels=run===count-1?await evaluate(`(async()=>{
   const results=[],groups=layerApp.app.layout(innerWidth,innerHeight).groups.filter(g=>g.panels.length>1);
   const frame=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
   try { for(const group of groups)for(const panel of [group.panels.find(p=>p!==group.active),group.active]) {
    await new Promise(r=>setTimeout(r,300));
    const button=document.querySelector('.dock-group[data-group="'+group.id+'"] .dock-tab[data-panel="'+panel+'"]');
    if(!button?.getBoundingClientRect().width)continue;
    const start=performance.now();button.click();const sync=performance.now()-start;await frame();
    results.push({panel,sync_ms:sync,frame_ms:performance.now()-start});
   }} finally {for(const group of groups)layerApp.dispatch({type:'select_panel_tab',group:group.id,panel:group.active});await frame();}
   return results;
  })()`):[];
  const sample=await evaluate(`({...uiAudit,navigation:performance.getEntriesByType('navigation')[0]?.toJSON(),marks:Object.fromEntries(performance.getEntriesByType('mark').map(x=>[x.name,x.startTime])),resources:performance.getEntriesByType('resource').map(x=>({name:x.name.split('/').pop(),start:x.startTime,end:x.responseEnd,transfer:x.transferSize,bytes:x.decodedBodySize})),dom:document.querySelectorAll('*').length,theme:layerApp.state().theme,viewport:[innerWidth,innerHeight,devicePixelRatio],workspace:JSON.parse(layerApp.app.workspace_view()).name})`);
  sample.database_bytes=await evaluate(`new Promise((resolve,reject)=>{const open=indexedDB.open('capycanvas.workspaces');open.onerror=()=>reject(open.error);open.onsuccess=()=>{const db=open.result,read=db.transaction('workspace').objectStore('workspace').get('database');read.onsuccess=()=>{resolve(read.result?.snapshot?.length??0);db.close();};read.onerror=()=>{db.close();reject(read.error);};};})`);
  sample.settings=settings;sample.steadySettings=steadySettings;sample.panels=panels;results.push(sample);await writeFile(`${output}/web-ui.json`,JSON.stringify(results,null,2));
  console.log(JSON.stringify({run,ui:sample.marks['capy.startup.ui'],workspace:sample.marks['capy.startup.workspace'],settings,steadySettings,longtasks:sample.longtasks}));
  // Don't overlap the next navigation with unfinished pipeline work from this one.
  await waitFor('layerApp.startupTimes.complete != null');
 }
 assert.deepEqual(errors,[]);
 const panelTimes=results.flatMap(x=>x.panels.map(p=>p.frame_ms));
 const summary={ui_ms:median(results.map(x=>x.marks['capy.startup.ui'])),workspace_ms:median(results.map(x=>x.marks['capy.startup.workspace'])),settings_ms:median(results.flatMap(x=>x.steadySettings.map(s=>s.frame_ms))),settings_during_startup_ms:median(results.flatMap(x=>x.settings.slice(1).map(s=>s.frame_ms))),panels_ms:panelTimes.length?median(panelTimes):null};
 console.log('Medians',summary);await writeFile(`${output}/web-summary.json`,JSON.stringify(summary,null,2));
 if(process.argv.includes('--assert-targets')){assert(summary.ui_ms<=700,'UI <=700 ms');assert(summary.workspace_ms<1000,'workspace <1000 ms');assert(summary.settings_ms<75,'Settings <75 ms');if(panelTimes.length)assert(summary.panels_ms<75,'Panel tabs <75 ms');}
} finally {if(preload)await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:preload});ws.close();for(const p of pending.values())clearTimeout(p.timer);}
