import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {basename, dirname, join} from 'node:path';

const selectors={panel:'.color-wheel-square',wheel:'.color-wheel',readout:'.color-readout',
  foreground:'[data-color-slot="foreground"]',background:'[data-color-slot="background"]',
  transparent:'[data-color-slot="transparent"]',swap:'.color-swap',
  'shape-0':'.color-shape','shape-1':'.color-shape'};

// Both native manifests feed one production DOM renderer. Apple captures each
// complete panel; Windows captures a grid of panels with prefixed frame names.
function captureItems(fixture,grid) {
  const items=grid?fixture.items:[{...fixture,...fixture.resources,x:0,y:0,size:fixture.width}];
  assert.ok(items.length>0);
  return items.map(item=>({...item,frame_specs:[
    ...Object.entries(selectors).map(([id,selector])=>({
      key:grid?item.key+'/color-'+id:id,selector,
      // Shape buttons share a parent with the other controls, so their index is
      // within the shape collection, not CSS's collection of all button siblings.
      ...(id.startsWith('shape-')?{selector:'.color-shape',index:Number(id.slice(-1))}:{}),
    })),
    ...(!grid?['foreground','background','transparent'].map(id=>({key:'paint-'+id,selector:selectors[id]+' span'})):[]),
  ]}));
}

// Exact Rust models, layouts, stops and field bytes; no copied color policy.
export function observeColorWorkers(NativeWorker,modules,pending,failures,events) {
  const workers=new Set(),jobs=new Map();
  const settle=(worker,id,error,result)=>{
    const record=jobs.get(worker)?.get(id);if(!record)return;
    jobs.get(worker).delete(id);
    if(error){record.reject(error);return;}
    const {capture}=record,bytes=capture.bytes;
    if(!(result instanceof Uint8Array)||result.length!==bytes.length){record.reject(Error('Invalid production Color field result'));return;}
    for(let i=0;i<bytes.length;i++)if(result[i]!==bytes[i]){record.reject(Error('Production Color field differs at byte '+i+': '+result[i]+' expected '+bytes[i]));return;}
    capture.completed++;capture.lastCompleted=record.metadata;record.resolve();
  };
  class ObservedWorker extends NativeWorker {
    constructor(...args){
      super(...args);workers.add(this);jobs.set(this,new Map());let capture;
      const send=this.postMessage.bind(this);
      this.postMessage=(message,transfer)=>{
        if(message.module!==undefined){
          const owner=modules.get(message.module);
          if(!owner||(capture&&capture!==owner))throw Error('Color worker module has no unique capture owner');
          capture=owner;
        }
        const request=message.request;
        if(!capture||request?.operation!=='color-field'||request.metadata!==capture.item.field_request||!Array.isArray(request.buffers)||request.buffers.length)
          throw Error('Unexpected Color field request');
        if(!capture.bytes||capture.bytes.length!==capture.item.field_side**2*4||jobs.get(this).has(message.id))throw Error('Invalid Color worker request ownership');
        let resolve,reject;const job=new Promise((yes,no)=>{resolve=yes;reject=no;});
        jobs.get(this).set(message.id,{capture,metadata:request.metadata,resolve,reject});pending.add(job);
        job.then(()=>pending.delete(job),error=>{pending.delete(job);failures.push(String(error));});
        try{return send(message,transfer);}catch(error){settle(this,message.id,error);throw error;}
      };
      this.addEventListener('message',({data})=>{
        if(!jobs.has(this))return;
        if(data.gpu_event){events.push(data.gpu_event);return;}
        settle(this,data.id,data.error||data.color_feature_error?Error(String(data.error||data.color_feature_error)):null,data.result);
      });
      for(const name of ['error','messageerror'])this.addEventListener(name,event=>{
        if(!jobs.has(this))return;
        for(const id of jobs.get(this).keys())settle(this,id,Error(event.message||'Color worker transport failed'));
      });
    }
  }
  return {Worker:ObservedWorker,close(){
    for(const worker of workers){
      for(const id of jobs.get(worker).keys())settle(worker,id,Error('Color worker closed with a pending request'));
      worker.terminate();
    }
    workers.clear();jobs.clear();
  }};
}

export async function waitForColorCapture(captures,pending,failures,colorCaptureWorkerEvents) {
  captures.forEach(c=>c.panels.flushPaint());
  await Promise.all([...pending]);await Promise.resolve();
  captures.forEach(c=>c.panels.flushPaint());
  await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
  if(pending.size||failures.length||colorCaptureWorkerEvents.length||captures.some(c=>c.completed<1||c.lastCompleted!==c.lastRequested))throw Error('Color capture not ready: '+JSON.stringify({pending:pending.size,failures,worker_events:colorCaptureWorkerEvents,completed:captures.map(c=>c.completed)}));
}

export async function captureColorPanels({manifest,fixturePath,output,evaluate,call}) {
  assert.equal(manifest.schema,2);
  assert.ok(manifest.fixtures.length>0);
  const grid=Array.isArray(manifest.fixtures[0].items);
  assert.ok(manifest.fixtures.every(f=>Array.isArray(f.items)===grid),'Mixed fixture formats');
  if(!grid)assert.equal(manifest.fixtures.length,manifest.interaction_states===true?18:216);
  else assert.notEqual(manifest.interaction_states,true);
  await evaluate(`(async()=>{
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    const response=await fetch('/pkg/layer_web_bg.wasm');if(!response.ok)throw Error('Missing Color worker Wasm');
    window.colorCaptureModule=await WebAssembly.compileStreaming(response);
    window.colorCaptureWorkerEvents=[];
    window.colorCaptureIcons={};
    for(const name of ['color-circle','color-square','color-triangle','color-swap','pencil']){
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');
      if(!response.ok)throw Error('Missing color icon');colorCaptureIcons[name]=await response.text();
    }
  })()`);
  const reports=[];
  try {
  for(const source of manifest.fixtures){
    assert.match(source.name,grid?/^(dark|light)-(128|160|226|360)$/:/^[a-z0-9-]+$/);
    const fixture={...source,items:captureItems(source,grid)};
    for(const item of fixture.items){
      assert.ok(item.color_preview?.picker&&item.color_preview.view,'Missing shared Color preview; regenerate the native capture producer with the current query-record contract');
      assert.deepEqual(item.state?.colors,item.colors);
      assert.ok(item.state.layer_tools&&Object.hasOwn(item.state.layer_tools,'mask_editing'));
      assert.ok(item.query_context?.document_color&&item.query_context.language);
      assert.ok(Array.isArray(item.color_ui)&&item.color_ui.length>0,'Missing shared Color queries');
      assert.ok(item.field_file&&typeof item.field_request==='string','Missing shared Color field');
      if(grid)assert.equal(item.field_request_kind,'fixed_sdr_fixture_input');
      const request=JSON.parse(item.field_request);
      assert.equal(request.side,item.field_side);
      assert.deepEqual(request.colors,item.color_preview.colors);
    }
    for(const item of fixture.items){
      assert.equal(basename(item.field_file),item.field_file);
      if(!grid)assert.equal(item.field_file,'field-'+fixture.name+'.rgba');
      const bytes=await readFile(join(dirname(fixturePath),item.field_file));
      assert.equal(bytes.length,item.field_side**2*4);
      item.field_base64=bytes.toString('base64');
    }
    await evaluate('(async()=>{await window.colorCaptureDispose?.(); window.colorCaptureRoots=[]; document.body.replaceChildren()})()');
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    // Place the pointer outside all controls before constructing them, avoiding
    // stale hover state when a preceding fixture had a different size or state.
    const outside={x:fixture.width-2,y:2};
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...outside});
    const frames=await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)};
      const {createEditorPanels}=await import('/editor-panels.js');
      document.body.dataset.theme=fixture.theme;
      document.body.style.cssText='margin:0;padding:0;overflow:hidden;background:'+fixture.palette.panel;
      if(fixture.catalog)document.documentElement.style.setProperty('--ui-text-size',(fixture.catalog.text_size_pt*96/72)+'px');
      else document.documentElement.style.removeProperty('--ui-text-size');
      for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
        document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
      const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
      const button=(label,action,css)=>{const e=element('button',css,label);e.onclick=action;return e;};
      const icon=name=>{if(!colorCaptureIcons[name])throw Error('Missing color icon '+name);const template=document.createElement('template');template.innerHTML=colorCaptureIcons[name];return template.content.firstElementChild;};
      const captures=[],pending=new Set(),failures=[],originalError=console.error;
      const NativeWorker=window.Worker,modules=new WeakMap();
      const observer=(${observeColorWorkers.toString()})(NativeWorker,modules,pending,failures,colorCaptureWorkerEvents);
      const captureError=(...args)=>{if(args[0]==='Color field preview')failures.push(args.map(String).join(' '));originalError.apply(console,args);};
      console.error=captureError;
      const onError=event=>failures.push(event.message||String(event.reason));
      window.addEventListener('error',onError);window.addEventListener('unhandledrejection',onError);
      window.colorCaptureDispose=async()=>{
        try{await Promise.allSettled([...pending]);}
        finally{try{
          const results=await Promise.allSettled(colorCaptureRoots.map(root=>Promise.resolve().then(()=>root.disposeEditor())));
          const failed=results.find(result=>result.status==='rejected');if(failed)throw failed.reason;
        }finally{try{observer.close();}finally{window.Worker=NativeWorker;window.removeEventListener('error',onError);window.removeEventListener('unhandledrejection',onError);if(console.error===captureError)console.error=originalError;colorCaptureRoots=[];}}}
      };
      window.colorCaptureReady=()=>(${waitForColorCapture.toString()})(captures,pending,failures,colorCaptureWorkerEvents);
      window.Worker=observer.Worker;
      for(const item of fixture.items){
        const bytes=item.field_base64?Uint8Array.from(atob(item.field_base64),c=>c.charCodeAt(0)):null;
        const capture={completed:0,panels:null};
        const queryKey=request=>JSON.stringify(Object.entries(request).sort(([a],[b])=>a.localeCompare(b)));
        const app={catalog:()=>structuredClone(fixture.catalog),color_preview:()=>structuredClone(item.color_preview),
          color_ui:request=>{
            const records=item.color_ui.filter(record=>queryKey(record.request)===queryKey(request));
            if(!records.length)throw Error('Unexpected Color UI query '+JSON.stringify(request));
            if(records.some(record=>JSON.stringify(record.response)!==JSON.stringify(records[0].response)))throw Error('Conflicting Color UI records');
            return structuredClone(records[0].response);
          },color_field_request:side=>{if(side!==item.field_side)throw Error('Unexpected field raster size '+side);capture.lastRequested=item.field_request;return item.field_request;}};
        capture.item=item;capture.bytes=bytes;
        const wasmModule=structuredClone(colorCaptureModule);modules.set(wasmModule,capture);
        const panels=createEditorPanels({app,state:()=>structuredClone(item.state),wasmModule,element,button,icon,
          contentChanged:panel=>{if(panel!=='color')throw Error('Unexpected panel reflow '+panel);},
          dispatch(){throw Error('Unexpected edit during capture');}});
        capture.panels=panels;captures.push(capture);
        const root=panels.control('color_wheel');
        root.style.cssText='position:absolute;left:'+item.x+'px;top:'+item.y+'px;width:'+item.size+'px;height:'+item.size+'px';
        document.body.append(root);colorCaptureRoots.push(root);
      }
      await document.fonts.ready;
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      await colorCaptureReady();
      const frames={};
      fixture.items.forEach((item,i)=>{
        const root=colorCaptureRoots[i];
        for(const spec of item.frame_specs)
          frames[spec.key]=root.querySelectorAll(spec.selector)[spec.index||0].getBoundingClientRect().toJSON();
        for(const node of root.querySelectorAll('[data-color-slot]')){
          const swatch=item.color_preview.view.swatches.find(s=>s.slot===node.dataset.colorSlot);
          if(node.getAttribute('aria-pressed')!==String(swatch.selected))throw Error('Incorrect paint selection');
        }
      });
      return frames;
    })()`);
    assert.deepEqual(Object.keys(frames).sort(),Object.keys(fixture.frames).sort());
    const differences=Object.entries(frames).map(([key,web])=>({key,native:fixture.frames[key],web,
      error:Math.max(...['x','y','width','height'].map(p=>Math.abs(web[p]-fixture.frames[key][p])))}));
    const maximum=Math.max(...differences.map(d=>d.error));
    const report={name:fixture.name,frames,differences,maximum_error_points:maximum,within_one_point:maximum<=1};
    await writeFile(join(output,'geometry-'+fixture.name+'.json'),JSON.stringify(report,null,2));
    let target='',pressed=false,focus=false;
    if(manifest.interaction_states===true){
      ({target,pressed}=fixture.interaction);
      assert(['','foreground','background','transparent','shape-0','swap'].includes(target));
      assert.equal(typeof pressed,'boolean');
      assert.ok(target||!pressed);
    }else if(grid&&(fixture.capture_state||'default')!=='default'){
      assert(['color-readout','color-shape-0','color-background','color-swap'].includes(fixture.capture_target));
      target=fixture.capture_target.slice('color-'.length);
      focus=fixture.capture_state.endsWith('-focus');
      assert.ok(focus||fixture.capture_state.endsWith('-hover'),'Unknown color capture state');
    }
    if(target){
      const spec=fixture.items[0].frame_specs.find(s=>s.key===(grid?fixture.items[0].key+'/color-'+target:target));
      const node=`colorCaptureRoots[0].querySelectorAll(${JSON.stringify(spec.selector)})[${spec.index||0}]`;
      if(focus){
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
        await evaluate(`${node}.focus()`);
        assert.equal(await evaluate(`${node}.matches(':focus-visible')`),true);
      }else{
        const frame=frames[spec.key],point={
          x:!grid&&target==='background'?frame.x+frame.width-3:frame.x+frame.width/2,y:frame.y+frame.height/2};
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
        assert.equal(await evaluate(`${node}.matches(':hover')`),true);
        if(pressed){
          await call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
          assert.equal(await evaluate(`${node}.matches(':active')`),true);
        }
      }
      await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
    }
    await evaluate('colorCaptureReady()');
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(join(output,'web-'+fixture.name+'.png'),png);
    if(pressed){
      // Cancel outside the button so the reference fixture never dispatches an edit.
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...outside,button:'left',buttons:1});
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...outside,button:'left',buttons:0,clickCount:1});
    }
    reports.push({name:fixture.name,maximum_error_points:maximum,within_one_point:maximum<=1});
  }
  } finally {
    await evaluate('window.colorCaptureDispose?.()');
  }
  await writeFile(join(output,'color-panel-geometry.json'),JSON.stringify(reports,null,2));
  console.log('Captured '+reports.length+' production compact Color fixtures');
}
