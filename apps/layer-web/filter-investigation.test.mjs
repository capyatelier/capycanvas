import {mkdir,writeFile} from 'node:fs/promises';

export async function investigate({call,evaluate,settle}) {
  const output=process.env.CAPY_FILTER_REPORT || 'artifacts/filter-investigation';
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){if(${condition})resolve();else if(performance.now()-start>120000)reject(Error('Timed out: '+${JSON.stringify(condition)}));else setTimeout(poll,50);}poll();})`);
  await wait('layerApp.startupTimes.complete!==null');
  await wait("layerApp.state().commands.find(c=>c.id==='open_document')?.enabled");
  await evaluate(`(async()=>{
    const response=await fetch('./filter-investigation-photo.jpg');
    if(!response.ok)throw Error('Missing filter-investigation-photo.jpg fixture');
    window.__filterPhoto=await response.arrayBuffer();
    window.showOpenFilePicker=async()=>[{name:'water.jpg',getFile:async()=>new File([__filterPhoto],'water.jpg',{type:'image/jpeg'})}];
    layerApp.dispatch({type:'invoke',command:'open_document'});
  })()`);
  await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
  await wait(`!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.state().tabs.some(t=>t.width===5184 && t.height===3456) && layerApp.app.brush_ready()`);
  await settle();
  // Allow the host's document adoption and background preparation to finish;
  // this warm-up is outside every recorded frame.
  await evaluate('new Promise(r=>setTimeout(r,5000))');
  await evaluate(`layerApp.dispatch({type:'effect',action:{op:'insert',effect:'gaussian_blur'}})`);
  await settle();
  const result=await evaluate(`(async()=>{
    const app=layerApp.app,id=layerApp.state().layer_properties.layer;
    if(id==null)throw Error('Gaussian layer was not selected');
    const frames=[];const pause=ms=>new Promise(r=>setTimeout(r,ms));
    // Drive untimed frames through deferred preparation, ending at radius 3.
    // Waiting only for a host callback can leave the initial filter deferred.
    let warmupFrames=0;
    for(;warmupFrames<20;warmupFrames++) {
      app.dispatch({type:'effect',action:{op:'set',layer:id,key:'sigma',value:{kind:'number',value:warmupFrames%2?3:4}}});
      app.frame(performance.now(),performance.now()+16.67);
      await app.wait_for_canvas();
      if(warmupFrames%2 && Number(app.renderer_stats().rows.find(r=>r.label==='Effect passes')?.value)>=2)break;
      await pause(250);
    }
    if(warmupFrames===20)throw Error('Initial Gaussian frame never completed');
    for(const sigma of [3.1,3,4,3,12,21,3.1,3]) {
      await pause(200);
      const before=Number(app.renderer_stats().rows.find(r=>r.label==='Frames').value);
      const start=performance.now();
      app.dispatch({type:'effect',action:{op:'set',layer:id,key:'sigma',value:{kind:'number',value:sigma}}});
      const dispatched=performance.now();
      app.frame(performance.now(),performance.now()+16.67);
      const cpu=performance.now()-dispatched;
      await app.wait_for_canvas();
      if(Number(app.renderer_stats().rows.find(r=>r.label==='Frames').value)<=before)throw Error('Filter frame was deferred; benchmark is not valid');
      frames.push({sigma,dispatch_ms:dispatched-start,frame_cpu_ms:cpu,complete_ms:performance.now()-dispatched});
      await pause(500);
    }
    const adapter=await navigator.gpu.requestAdapter();
    return JSON.parse(JSON.stringify({adapter:{vendor:adapter.info.vendor,architecture:adapter.info.architecture,description:adapter.info.description},warmupFrames:warmupFrames+1,frames,stats:app.renderer_stats(),color:app.document_color()},(_,v)=>typeof v==='bigint'?v.toString():v));
  })()`);
  console.log(JSON.stringify(result,(_,v)=>typeof v==='bigint'?v.toString():v));
  await mkdir(output,{recursive:true});
  await writeFile(`${output}/web.json`,JSON.stringify(result,null,2));
  const shot=await call('Page.captureScreenshot',{format:'png'});
  await writeFile(`${output}/web.png`,Buffer.from(shot.data,'base64'));
}
