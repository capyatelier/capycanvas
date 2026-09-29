import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

// Real Wasm/GPU and native presentation; observe the existing bridge without
// adding production instrumentation or driving a second preview producer.
export async function checkFilterPreviews({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{
    const end=performance.now()+45000;function check(){
      if(${condition})resolve(true);
      else if(performance.now()>end)reject(Error('Preview timeout: '+${JSON.stringify(condition)}));
      else setTimeout(check,40);
    }check();})`);
  const histogram=()=>evaluate(`(async()=>{const control=layerApp.app.capture_control();try{
    return JSON.parse(JSON.stringify((await layerApp.app.histogram(control)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v));
  }finally{control.free();}})()`);
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/filter-memory';
  await mkdir(directory,{recursive:true});
  const capture=async name=>{
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,'base64'));
  };
  const blank=await histogram();
  await evaluate(`(async()=>{
    const canvas=new OffscreenCanvas(256,256),context=canvas.getContext('2d',{willReadFrequently:true});
    for(let y=0;y<8;y++)for(let x=0;x<8;x++){context.fillStyle=(x+y)%2?'rgb(30,160,220)':'rgb(230,50,80)';context.fillRect(x*32,y*32,32,32);}
    const pixel=Array.from(context.getImageData(0,0,1,1).data);
    if(pixel.join(',')!=='230,50,80,255')throw Error('Invalid generated checker pixel: '+pixel);
    const file=new File([await canvas.convertToBlob()],'Preview checker.png',{type:'image/png'});
    window.previewPicker=window.showOpenFilePicker;window.showOpenFilePicker=async()=>[{getFile:async()=>file}];
    layerApp.dispatch({type:'invoke',command:'import_image'});
  })()`);
  await wait(`layerApp.state().commands.find(c=>c.id==='placement_original_size').enabled`);
  await evaluate(`window.showOpenFilePicker=previewPicker;delete window.previewPicker;layerApp.dispatch({type:'invoke',command:'apply_transform'})`);
  await wait(`layerApp.state().layer_tools.editing_layer.label==='Preview checker'&&!layerApp.state().commands.find(c=>c.id==='placement_original_size').enabled`);
  await wait('layerApp.app.brush_ready()');
  await capture('web-preview-import');
  const imported=await histogram();
  assert.notDeepEqual(imported,blank,'The imported checker changes document pixels');
  await evaluate(`(()=>{
    const app=layerApp.app,original=app.poll_filter_previews;
    const pipeline=GPUDevice.prototype.createRenderPipelineAsync;
    window.previewCheck={pipelineCalls:0,restore(){app.poll_filter_previews=original;GPUDevice.prototype.createRenderPipelineAsync=pipeline;}};
    GPUDevice.prototype.createRenderPipelineAsync=function(descriptor){
      if(descriptor.label==='pointwise effect chain')previewCheck.pipelineCalls++;
      return pipeline.call(this,descriptor);
    };
    // Hold a host pen contact outside the canvas. Preview requests may queue,
    // but optional compiler admission must wait for release and quiet time.
    document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerId:987,pointerType:'pen'}));
    app.poll_filter_previews=function(...args){
      const result=original.apply(this,args);
      previewCheck.status=result;previewCheck.ids=args[0];return result;
    };
    previewCheck.pixels=id=>{
      const c=[...document.querySelectorAll('[data-effect="'+id+'"] canvas')].find(c=>c.getBoundingClientRect().height>0);
      return c?.width>0&&c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0);
    };
    layerApp.dispatch({type:'filter_picker',action:{op:'category',category:null}});
  })()`);
  const show=async()=>{
    await evaluate(`(()=>{if(![...document.querySelectorAll('.filter-picker')].some(p=>p.getBoundingClientRect().height>0))document.querySelector('.dock-tab[data-panel=adjustments]').click();})()`);
    await settle();
  };
  const status=()=>evaluate(`(()=>{const {atlas,bytes,...status}=previewCheck.status;return JSON.parse(JSON.stringify(status,(_,v)=>typeof v==='bigint'?Number(v):v));})()`);
  try {
    await settle();await show();
    await wait('layerApp.app.shader_work_pending(true)');
    await new Promise(resolve=>setTimeout(resolve,350));
    assert.equal(await evaluate('previewCheck.pipelineCalls'),0,'Held contact defers optional preview pipelines');
    await evaluate("document.body.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,pointerId:987,pointerType:'pen'}));undefined");
    await wait(`previewCheck.status?.retained.includes('curves')&&!previewCheck.status.pending&&previewCheck.pixels('curves')`);
    assert.ok(await evaluate('previewCheck.pipelineCalls>0'),'Queued previews resume after release');
    const first=await status();assert.equal(first.error??null,null);
    await evaluate(`document.querySelector('.dock-tab[data-panel=properties]').click()`);await settle();
    await wait(`previewCheck.ids.length===0&&!previewCheck.status.pending`);
    await show();
    await wait(`previewCheck.ids.includes('curves')&&previewCheck.pixels('curves')`);
    const reopened=await status();
    assert.equal(reopened.key,first.key);assert.equal(reopened.requests,first.requests,"Reopening reuses the atlas");
    await evaluate(`layerApp.dispatch({type:'set_layer_opacity',opacity:.63})`);
    await wait(`previewCheck.status.key!==${JSON.stringify(first.key)}&&!previewCheck.status.pending&&previewCheck.pixels('curves')`);
    const edited=await status();assert.ok(edited.requests>first.requests);assert.equal(edited.error??null,null);
    await evaluate(`layerApp.dispatch({type:'filter_picker',action:{op:'category',category:'color'}})`);
    await wait(`previewCheck.ids.includes('hue_saturation')&&previewCheck.pixels('hue_saturation')&&!previewCheck.status.pending`);
    const category=await status();assert.ok(category.retained.length<=64);assert.equal(category.error??null,null);
    const beforeRestart=await histogram();
    await capture('web-preview-before-restart');
    await evaluate(`layerApp.restartGpu()`);
    await wait('layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null');
    await wait(`previewCheck.status.key!==${JSON.stringify(category.key)}&&previewCheck.pixels('hue_saturation')&&!previewCheck.status.pending`);
    const restarted=await status();assert.equal(restarted.error??null,null);
    const afterRestart=await histogram();
    await capture('web-preview-lifecycle');
    assert.deepEqual(afterRestart,beforeRestart,'GPU replacement preserves imported artwork');
    await writeFile(`${directory}/web-preview-lifecycle.json`,JSON.stringify({first,reopened,edited,category,restarted},null,2));
    console.log('PASS: GPU preview pixels, hide/reopen cache reuse, source/category changes and GPU replacement');
  } finally { await evaluate(`document.body.dispatchEvent(new PointerEvent('pointercancel',{bubbles:true,pointerId:987,pointerType:'pen'}));previewCheck.restore();delete window.previewCheck`); }
}
