import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';

export async function checkSavedDataUx({call,evaluate,reload,settle}) {
  const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/saved-data-compat/ux-shots';
  await mkdir(output,{recursive:true});
  const messages={
    newer:"Your workspaces were saved by a newer version of Capy Canvas. Update Capy Canvas to save changes to workspaces, palettes and brush settings.",
    unavailable:"Capy Canvas can't open your saved workspaces. Restart Capy Canvas to save changes to workspaces, palettes and brush settings.",
    damaged:"Your saved workspaces were damaged. Capy Canvas started new ones.",
    editing:"Some palettes or brush settings can't be opened in this version of Capy Canvas. Update Capy Canvas to open them.",
    workspace:"This version of Capy Canvas can't open your saved workspaces. Update Capy Canvas to open them.",
    choice:"This version of Capy Canvas can't open this workspace.",
    'drawing-unsupported':"It has layers or settings that this version of Capy Canvas can't edit.",
    'drawing-damaged':"Part of this file is damaged.",
  };
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20);}poll();})`);
  const ready=()=>wait('window.layerApp?.startupTimes.complete!=null&&JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
  const storage=operation=>evaluate(`(async()=>{const db=await new Promise((resolve,reject)=>{const request=indexedDB.open('capycanvas.workspaces',1);request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error)});try{return await new Promise((resolve,reject)=>{const tx=db.transaction('workspace','readwrite'),store=tx.objectStore('workspace');let result;${operation}tx.oncomplete=()=>resolve(result);tx.onabort=()=>reject(tx.error);});}finally{db.close();}})()`);
  const shot=async name=>{
    await settle();
    const {data}=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${output}/${name}.png`,Buffer.from(data,'base64'));
  };
  const template=JSON.parse(await readFile(new URL('../../crates/layer-workspace/tests/compat/1.0.15/editing.json',import.meta.url),'utf8'));
  await ready();
  const baseline=await storage("const read=store.get('database');read.onsuccess=()=>{result=read.result.snapshot;};");
  const evidence=[];
  for(const theme of ['light','dark']) {
    for(const [kind,expected] of Object.entries(messages)) {
      await wait('!JSON.parse(layerApp.app.workspace_view()).dirty&&!JSON.parse(layerApp.app.workspace_view()).saving&&!layerApp.documents.busy()');
      const database=JSON.parse(baseline);
      if(kind==='newer')database.schema++;
      if(kind==='editing'){const editing=structuredClone(template);editing.preset='unreadable';database.editing=JSON.stringify(editing);}
      if(kind==='workspace')for(const record of Object.values(database.items)){if(record.entity.metadata.kind==='workspace'){record.entity.content={};record.claim=null;}}
      if(kind==='choice'){database.items['builtin:workspace:painter'].entity.content={};database.items['builtin:workspace:painter'].claim=null;}
      await storage(`store.put({id:'database',snapshot:${JSON.stringify(JSON.stringify(database))}});`);
      await evaluate(`localStorage.setItem('layer.preferences.v1',JSON.stringify({...layerApp.state().settings,language:{Explicit:'en'},theme:${JSON.stringify(theme)}}))`);
      let injected;
      if(kind==='unavailable'||kind==='damaged') {
        const source=`(()=>{const Base=Worker;window.Worker=class extends Base{constructor(url,options){super(url,options);this.savedDataStorage=options?.name==='workspace-storage';this.savedDataFailed=false;}postMessage(data,transfer){if(this.savedDataStorage&&data.request){const request=JSON.parse(data.request);if(request.type==='reopen'&&!this.savedDataFailed){this.savedDataFailed=true;queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:{id:data.id,error:JSON.stringify({kind:${JSON.stringify(kind==='damaged'?'damaged_storage':'unavailable')},message:'PRIVATE reader details'})}})));return;}if(request.type==='set_aside'){queueMicrotask(()=>this.dispatchEvent(new MessageEvent('message',{data:{id:data.id,response:'{"type":"done"}'}})));return;}}super.postMessage(data,transfer);}}})()`;
        injected=(await call('Page.addScriptToEvaluateOnNewDocument',{source})).identifier;
      }
      await reload();
      await ready();
      if(injected)await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:injected});
      if(kind==='choice') {
        await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'open',page:'workspaces'}));layerApp.app.workspace_input(JSON.stringify({type:'select',id:'builtin:workspace:painter'}));null`);
        await wait(`document.querySelector('.workspace-manager[open]')?.textContent.includes(${JSON.stringify(expected)})`);
        assert.equal(await evaluate('JSON.parse(layerApp.app.workspace_view()).error'),expected);
      } else if(kind.startsWith('drawing-')) {
        const fixture=await readFile(new URL(`../../crates/layer-core/tests/compat/1.0.15/${kind==='drawing-unsupported'?'unsupported-pass-through':'samples-U8'}.capy`,import.meta.url));
        const bytes=kind==='drawing-damaged'?fixture.subarray(0,Math.floor(fixture.length/2)):fixture;
        await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()');
        await evaluate(`(()=>{const bytes=Uint8Array.from(atob(${JSON.stringify(bytes.toString('base64'))}),c=>c.charCodeAt(0));window.savedDataOpening=layerApp.documents.openFiles([{file:new File([bytes],'fixture.capy')}]);return true;})()`);
        await wait(`document.querySelector('dialog[open].document-dialog')?.textContent.includes(${JSON.stringify(expected)})`);
      } else {
        await wait(`layerApp.state().notice?.text===${JSON.stringify(expected)}`);
        await wait(`(()=>{const notice=document.querySelector('.canvas-notice');return notice&&!notice.hidden&&notice.textContent.includes(${JSON.stringify(expected)});})()`);
        assert.equal(await evaluate('document.querySelector(".canvas-notice").textContent.includes("PRIVATE")'),false);
      }
      const name=`web-${kind}-${theme}`;
      await shot(name);
      evidence.push({name,expected,source:kind==='damaged'?'SQLite corruption reply injected at the worker transport':'shared startup or file reader'});
      if(kind.startsWith('drawing-')) {
        await evaluate(`[...document.querySelectorAll('dialog[open].document-dialog footer button')].find(button=>button.textContent==='Close').click()`);
        await evaluate('savedDataOpening');
      }
      if(kind==='choice')await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"cancel"}));null');
      if(kind==='newer')assert.equal(JSON.parse(await storage("const read=store.get('database');read.onsuccess=()=>{result=read.result.snapshot;};")).schema,database.schema);
    }
  }
  await writeFile(`${output}/web-messages.json`,JSON.stringify(evidence,null,2));
  console.log('PASS saved-data messages: all approved U1 messages rendered in both themes; damaged SQLite transport fixture; real newer schema, discarded editing, unreadable workspaces and package readers');
}
