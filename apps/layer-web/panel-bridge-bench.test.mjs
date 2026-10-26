import assert from 'node:assert/strict';
import {mkdir,writeFile,open} from 'node:fs/promises';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {homedir} from 'node:os';
import {join} from 'node:path';
import {panelBridgeFixture} from './panel-bridge.test.mjs';

export async function benchPanelBridges({call,evaluate,settle,once}) {
  const photo=process.env.LAYER_BRIDGE_PHOTO,width=Number(process.env.LAYER_BRIDGE_PHOTO_WIDTH),height=Number(process.env.LAYER_BRIDGE_PHOTO_HEIGHT);
  assert.ok(photo&&Number.isSafeInteger(width)&&width>0&&Number.isSafeInteger(height)&&height>0,'Set the tier photo URL and exact dimensions');
  const modes=(process.env.LAYER_BRIDGE_MODES||'gap,panel').split(','),repeats=Number(process.env.LAYER_BRIDGE_REPEATS||3),inputHz=Number(process.env.LAYER_BRIDGE_INPUT_HZ||60);
  const osHelper=process.env.LAYER_BRIDGE_OS_HELPER;
  const displayPaced=inputHz===60&&process.env.LAYER_BRIDGE_INPUT_CLOCK!=='host';
  assert.ok(modes.every(m=>['gap','panel'].includes(m))&&Number.isInteger(repeats)&&repeats>0&&inputHz>0);
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/panel-bridges/web-benchmark';await mkdir(dir,{recursive:true});
  const saved=await evaluate('layerApp.state().workspace'),theme=process.env.LAYER_TEST_THEME||'light',runs=[];
  const wait=ms=>new Promise(r=>setTimeout(r,ms));
  const thermal=async()=>{
    if(!process.env.ANDROID_SERIAL)return null;
    const sdk=process.env.ANDROID_HOME||process.env.ANDROID_SDK_ROOT||join(homedir(),'Android','Sdk');
    const {stdout}=await promisify(execFile)(process.env.ADB||join(sdk,'platform-tools',process.platform==='win32'?'adb.exe':'adb'),['-s',process.env.ANDROID_SERIAL,'shell','dumpsys','thermalservice']);
    const match=stdout.match(/Thermal Status: (\d+)/);assert.ok(match,'Device thermal status is available');return Number(match[1]);
  };
  const osMotion=args=>{
    assert.ok(/^\/data\/local\/tmp\/[a-zA-Z0-9._-]+$/.test(osHelper)&&process.env.ANDROID_SERIAL,'Owned device helper path');
    const sdk=process.env.ANDROID_HOME||process.env.ANDROID_SDK_ROOT||join(homedir(),'Android','Sdk'),adb=process.env.ADB||join(sdk,'platform-tools',process.platform==='win32'?'adb.exe':'adb');
    const child=spawn(adb,['-s',process.env.ANDROID_SERIAL,'shell',`CLASSPATH=${osHelper} app_process -Xmx64m / AndroidWorkspaceMotion ${args.join(' ')}`]);
    let stdout='',stderr='',motionResolve;const motion=new Promise(resolve=>motionResolve=resolve);
    child.stdout.on('data',part=>{stdout+=part;if(stdout.includes('MOTION_BEGIN '))motionResolve();});child.stderr.on('data',part=>stderr+=part);
    const done=new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',code=>{motionResolve();const line=stdout.split('\n').find(l=>l.startsWith('{"contact_begin_ns"'));if(code!==0||!line)reject(Error(`OS motion failed ${code}: ${stderr} ${stdout}`));else resolve({timing:JSON.parse(line),stdout,stderr});});});done.catch(()=>motionResolve());
    return{motion,done};
  };
  const ready=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;const poll=()=>{if(${condition})resolve();else if(performance.now()>end)reject(Error('Bridge benchmark readiness'));else setTimeout(poll,50)};poll()})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(200);};
  const mouse=(type,p)=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  let tracing=false,held=false,enteredFullscreen=false;
  const finishTrace=async()=>{
    const done=once('Tracing.tracingComplete',180000);await call('Tracing.end');tracing=false;
    const {stream}=await done,output=await open(`${dir}/bridge-profile.trace.json`,'w');
    try {for(;;){const part=await call('IO.read',{handle:stream,size:1024*1024});await output.write(part.base64Encoded?Buffer.from(part.data,'base64'):part.data);if(part.eof)break;}}
    finally {await output.close();await call('IO.close',{handle:stream});}
  };
  try {
    if(!await evaluate('!!document.fullscreenElement')) {
      const response=await call('Runtime.evaluate',{expression:'document.documentElement.requestFullscreen().then(()=>true)',awaitPromise:true,returnByValue:true,userGesture:true});
      assert.ok(!response.exceptionDetails,response.exceptionDetails?.text||'Own page enters fullscreen');enteredFullscreen=true;await settle();await wait(500);
    }
    const dimensions=await evaluate(`(async()=>{const blob=await(await fetch(${JSON.stringify(photo)})).blob(),image=await createImageBitmap(blob);const size=[image.width,image.height];image.close();await layerApp.documents.openFiles([new File([blob],'tier-photo.jpg',{type:'image/jpeg'})]);return size})()`);
    assert.deepEqual(dimensions,[width,height]);await ready('!layerApp.documents.busy()&&layerApp.app.brush_ready()');
    await send({type:'invoke',command:'add_layer'});
    const document=await evaluate('layerApp.state().tabs.find(t=>t.active)');assert.equal(document.width,width);assert.equal(document.height,height);
    await send({type:'set_theme',theme});
    const fixture=panelBridgeFixture(saved);
    const info=await evaluate('(async()=>{const a=await navigator.gpu.requestAdapter();return{agent:navigator.userAgent,viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,adapter:{vendor:a.info.vendor,architecture:a.info.architecture,description:a.info.description,fallback:a.isFallbackAdapter??a.info.isFallbackAdapter??false},transparency:layerApp.state().settings.transparency}})()');assert.equal(info.adapter.fallback,false);
    const targets=await call('Target.getTargets');await writeFile(`${dir}/browser-targets-before.json`,JSON.stringify(targets.targetInfos.map(t=>({type:t.type,url:t.url,attached:t.attached})),null,2));
    if(process.env.ANDROID_SERIAL){const sdk=process.env.ANDROID_HOME||process.env.ANDROID_SDK_ROOT||join(homedir(),'Android','Sdk');const memory=await promisify(execFile)(process.env.ADB||join(sdk,'platform-tools','adb'),['-s',process.env.ANDROID_SERIAL,'shell','dumpsys','meminfo','com.android.chrome']);await writeFile(`${dir}/chrome-memory-before.txt`,memory.stdout);}
    await evaluate(`(()=>{window.bridgeContactListener=e=>{const p=window.bridgeContact;if(!p)return;const phase=e.type==='pointerdown'?'begin':'end';p[phase]=e.timeStamp;p[phase+'_event']={type:e.pointerType,x:e.clientX,y:e.clientY,buttons:e.buttons,trusted:e.isTrusted,target:e.target?.getAttribute?.('class'),action:e.target?.closest?.('[data-workspace-drag]')?.dataset.workspaceDrag};performance.mark(p.label+'-contact-'+phase,{startTime:e.timeStamp});};for(const type of ['pointerdown','pointerup'])window.addEventListener(type,bridgeContactListener,true);})()`);
    for(const mode of modes)for(let run=-1;run<repeats;run++) {
      await send({type:'restore_workspace',workspace:fixture});await evaluate('document.querySelector(".column-tab[data-panel=brushes]").click()');await settle();await wait(300);
      await ready("!!document.querySelector('.column-surface defs>path')?.getAttribute('d')&&document.querySelectorAll('.vector-panel').length===2");
      await send({type:'invoke',command:'fit_canvas'});await ready('layerApp.app.brush_ready()');
      const camera=await evaluate("JSON.parse(JSON.stringify(layerApp.app.camera(),(key,value)=>typeof value==='bigint'?Number(value):value))"),canvas=await evaluate('(()=>{const c=layerApp.canvas,r=c.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,pixel_width:c.width,pixel_height:c.height}})()');
      assert.ok(camera.zoom*width<=camera.work_area[2]+1&&camera.zoom*height<=camera.work_area[3]+1,'Exact tier photo remains at Fit');
      const geometry=await evaluate("layerApp.app.layout(document.querySelector('#workspace').clientWidth,document.querySelector('#workspace').clientHeight)"),anchor=geometry.collapsed.find(c=>c.id===45).groups.flatMap(g=>g.icons).find(i=>i.panel==='navigator').bounds;
      if(mode==='gap')await ready(`(()=>{const w=document.querySelector('#workspace'),d=layerApp.app.layout(w.clientWidth,w.clientHeight).dividers.find(d=>d.id===45),n=[...document.querySelectorAll('.divider')].find(n=>n.dragAction?.type==='drag_divider'&&n.dragAction.id===45);if(!d||!n)return false;const r=n.getBoundingClientRect();return ['x','y','width','height'].every(k=>Math.abs(r[k]-d.bounds[k])<1);})()`);
      const pick=await evaluate(`(()=>{const w=document.querySelector('#workspace'),n=${mode==='gap'?"[...document.querySelectorAll('.divider')].find(n=>n.dragAction?.type==='drag_divider'&&n.dragAction.id===45)":"document.querySelector('.dock-group[data-group=\"43\"] .panel-grip')"},b=n.getBoundingClientRect(),p={x:b.x+b.width/2,y:b.y+b.height/2},h=document.elementFromPoint(p.x,p.y),native_clips=[...document.querySelectorAll('.vector-panel .panel-preview,.vector-source')].map(n=>{const s=getComputedStyle(n),owner=n.closest('.dock-group')||n;return{class:n.className,published:owner.panelVectorRadii,clip:s.clipPath,radii:['borderTopLeftRadius','borderTopRightRadius','borderBottomRightRadius','borderBottomLeftRadius'].map(k=>parseFloat(s[k])),overflow:s.overflow,parent_overflow:n.classList.contains('panel-preview')?getComputedStyle(n.parentElement).overflow:null,bounds:n.getBoundingClientRect().toJSON(),owner_bounds:owner.getBoundingClientRect().toJSON()}});return{native_clips,squircle:CSS.supports('corner-shape','squircle'),bounds:b.toJSON(),point:p,hit:h?{tag:h.tagName,class:h.getAttribute('class'),action:h.closest('[data-workspace-drag]')?.dataset.workspaceDrag}:null,inner:[innerWidth,innerHeight],workspace:[w.clientWidth,w.clientHeight],workspace_bounds:w.getBoundingClientRect().toJSON(),visible:document.visibilityState,hidden:document.hidden,focus:document.hasFocus(),dialogs:[...document.querySelectorAll('dialog[open]')].map(n=>({class:n.className,text:n.textContent.slice(0,400)})),visual:visualViewport?{width:visualViewport.width,height:visualViewport.height,scale:visualViewport.scale}:null};})()`);
      await writeFile(`${dir}/pickup-${mode}-${run}.json`,JSON.stringify(pick,null,2));
      if(mode==='gap')assert.equal(JSON.parse(pick.hit?.action||'{}').id,45,'Actual visible pickup targets the shared divider');
      if(process.env.LAYER_EXPECT_NATIVE_CSS_CLIP==='1'){assert.ok(pick.squircle&&pick.native_clips.length===3,'NativeCSSclip experiment requires squircle and all three native surfaces');for(const n of pick.native_clips){assert.equal(n.clip,'none');assert.equal(n.overflow,'hidden');if(n.parent_overflow!==null)assert.equal(n.parent_overflow,'visible');assert.ok(n.radii.every((r,i)=>Math.abs(r-n.published[i])<.01),'Physical native radii match publishedTL/TR/BR/BL');for(const k of ['x','y','width','height'])assert.ok(Math.abs(n.bounds[k]-n.owner_bounds[k])<1,'Native clip extent matches original body bounds');}}
      const snapshot=()=>evaluate(`(()=>{const w=document.querySelector('#workspace'),l=layerApp.app.layout(w.clientWidth,w.clientHeight),u=layerApp.app.workspace_update();return JSON.parse(JSON.stringify({update:u,bands:layerApp.state().workspace.layout.bands,groups:l.groups.map(g=>({id:g.id,bounds:g.bounds})),dividers:l.dividers.filter(d=>d.id===45),native:[...document.querySelectorAll('.dock-group')].map(n=>({id:n.dataset.group,bounds:n.getBoundingClientRect().toJSON()})),svg:[...document.querySelectorAll('.column-surface defs>path')].map(n=>n.getAttribute('d')),busy:layerApp.documents.interactionBusy(),layers:layerApp.state().layers.map(l=>({id:l.id,visible:l.visible,editing:l.editing,label:l.label}))},(k,v)=>typeof v==='bigint'?String(v):v));})()`);
      const before=await snapshot(),b=pick.bounds;
      const start={x:b.x+b.width/2,y:b.y+b.height/2},thermal_before=await thermal();
      await evaluate(`window.bridgeContact={label:'bridge-${mode}-${run}',begin:null,end:null}`);
      const origin=mode==='gap'?{x:start.x,y:anchor.y-3}:{x:geometry.work_area.x+geometry.work_area.width*.5-60,y:geometry.work_area.y+geometry.work_area.height*.5};
      let started,middle;
      if(!osHelper){await mouse('mousePressed',start);held=true;await mouse('mouseMoved',origin);await wait(500);started=await snapshot();}
      const profiled=process.argv.includes('--trace')&&mode==='gap'&&run===0;
      if(profiled){assert.ok(once,'Trace completion listener');await call('Tracing.start',{categories:'-*,devtools.timeline,blink.user_timing,disabled-by-default-v8.cpu_profiler',transferMode:'ReturnAsStream'});tracing=true;}
      await evaluate(`(()=>{const p=window.bridgeProbe={begin:performance.now(),frames:[],native_heights:[],mutations:[],running:true,counts:{},cpu:{},actions:{},action_regions:{},action_samples:[],original:{},raf:requestAnimationFrame.bind(window)};
        for(const name of ['workspace_update','state','layout','frame','dispatch','input','cursor_input']){p.original[name]=layerApp.app[name].bind(layerApp.app);layerApp.app[name]=(...args)=>{const t=performance.now(),v=p.original[name](...args);if(name==='dispatch'||name==='input'){const a=args[0],key=[name,a?.type,a?.phase,a?.event?.kind,a?.action?.type,a?.action?.phase].filter(Boolean).join('/');p.actions[key]=(p.actions[key]||0)+1;p.action_regions[key]=(p.action_regions[key]||0)+Number(v?.regions||v?.change?.regions||0);if(p.action_samples.length<20)p.action_samples.push({type:a?.type,phase:a?.phase,position:a?.position,regions:v?.regions});}p.counts[name]=(p.counts[name]||0)+1;(p.cpu[name]||=[]).push(performance.now()-t);return v;};}
        p.gpu={};p.texture=GPUCanvasContext.prototype.getCurrentTexture;GPUCanvasContext.prototype.getCurrentTexture=function(...args){const result=p.texture.apply(this,args),kind=this.canvas===layerApp.canvas?'main':this.canvas.classList?.contains('navigator-surface')?'navigator':'other',key=kind+'/'+(p.inFrame?'app.frame':'outside_app.frame');p.gpu[key]=(p.gpu[key]||0)+1;return result;};const wrappedFrame=layerApp.app.frame;layerApp.app.frame=(...args)=>{p.inFrame=true;try{return wrappedFrame(...args)}finally{p.inFrame=false}};
        p.observer=new MutationObserver(changes=>p.mutations.push({time:performance.now(),styles:changes.filter(c=>c.attributeName==='style').length,vectors:changes.filter(c=>c.attributeName==='d'||c.attributeName==='viewBox').length}));p.observer.observe(document.querySelector('#workspace'),{attributes:true,subtree:true,attributeFilter:['style','d','viewBox']});
        const frame=t=>{if(!p.running)return;const nodes=[...document.querySelectorAll('.column-surface defs>path,.dock-group,.collapsed-column')],signature=nodes.map(n=>n.getAttribute('d')||n.getAttribute('style')).join('|');if(signature!==p.last){p.frames.push(t);p.last=signature;const receiver=nodes.find(n=>n.dataset.group==='43'),height=parseFloat(receiver?.style.height);if(Number.isFinite(height))p.native_heights.push({time:t,height});}p.raf(frame);};p.raf(frame);performance.mark('bridge-${mode}-${run}-begin');})()`);
      const duration=run<0?1000:6000,began=performance.now();let inputs=0,pending=[],lastPoint=origin;
      let os_timing=null;
      if(osHelper){
        const endpoint=mode==='gap'?{x:start.x,y:anchor.y+anchor.height+21}:{x:origin.x+120,y:origin.y};
        const scale=info.dpr,args=[start.x,start.y,origin.x,origin.y,endpoint.x,endpoint.y].map(v=>Math.round(v*scale));args.push(duration,500);
        const job=osMotion(args);held=true;await job.motion;started=await snapshot();await wait(duration*.37);middle=await snapshot();
        const result=await job.done;held=false;os_timing=result.timing;assert.ok(os_timing.motion_begin_ns>0&&os_timing.samples>0,'OS display clock produced actual motion');inputs=os_timing.samples;await writeFile(`${dir}/os-${mode}-${run}.json`,JSON.stringify(result,null,2));
      }else{
      while(performance.now()-began<duration) {
        if(displayPaced)await evaluate('new Promise(requestAnimationFrame)');
        if(performance.now()-began>=duration)break;
        const t=(performance.now()-began)/1000,cycle=(t%1)*2,phase=cycle<=1?cycle:2-cycle;
        const p=mode==='gap'?{x:start.x,y:anchor.y-3+phase*(anchor.height+24)}:{x:origin.x+phase*120,y:origin.y+Math.sin(t*Math.PI*2)*24};
        lastPoint=p;pending.push(mouse('mouseMoved',p));inputs++;if(!middle&&performance.now()-began>duration*.37)middle=await snapshot();if(!displayPaced)await wait(1000/inputHz);
      }
      }
      await Promise.all(pending);await settle();
      const probe=await evaluate(`(()=>{const p=bridgeProbe;p.running=false;p.end=performance.now();p.observer.disconnect();GPUCanvasContext.prototype.getCurrentTexture=p.texture;performance.mark('bridge-${mode}-${run}-end');performance.measure('bridge-${mode}-${run}','bridge-${mode}-${run}-begin','bridge-${mode}-${run}-end');for(const [name,original]of Object.entries(p.original))layerApp.app[name]=original;return{begin:p.begin,end:p.end,frames:p.frames,native_heights:p.native_heights,mutations:p.mutations,counts:p.counts,cpu:p.cpu,actions:p.actions,action_regions:p.action_regions,action_samples:p.action_samples,gpu:p.gpu};})()`);
      const ended=await snapshot();if(!osHelper){await mouse('mouseReleased',lastPoint);held=false;}await settle();const contact=await evaluate('({...bridgeContact})'),thermal_after=await thermal();await wait(300);await send({type:'invoke',command:'undo_workspace'});
      if(os_timing&&contact.begin!==null){probe.begin=contact.begin+(os_timing.motion_begin_ns-os_timing.contact_begin_ns)/1e6;probe.end=contact.begin+(os_timing.motion_end_ns-os_timing.contact_begin_ns)/1e6;}
      await writeFile(`${dir}/contact-${mode}-${run}.json`,JSON.stringify({pick,before,started,middle,ended,contact,os_timing,probe},null,2));
      if(run>=0) {
        assert.ok(probe.end-probe.begin>=5000&&probe.end-probe.begin<=10000,'Measured contact stays within the sustained 5–10 second window');
        assert.ok(contact.begin!==null&&contact.end>contact.begin,'Actual pointer down and up bound the contact');
        assert.ok(Object.entries(probe.actions).some(([key,count])=>key.startsWith('dispatch/drag_')&&key.endsWith('/move')&&count>0),'Shared workspace receives actual drag motion');
        if(mode==='gap'){const h=s=>s.groups.find(g=>g.id===43).bounds.height;assert.ok(Math.abs(h(started)-h(middle))>2,'Shared receiver geometry moves during the sustained contact');const heights=probe.native_heights.filter(v=>v.time>=probe.begin+100&&v.time<=probe.end-100).map(v=>v.height);assert.ok(heights.length>2&&Math.max(...heights)-Math.min(...heights)>6,'Native receiver geometry spans real motion throughout the measured contact');}
        const frames=probe.frames.filter(t=>t>=probe.begin+100&&t<=probe.end-100),gaps=frames.slice(1).map((t,i)=>t-frames[i]).sort((a,b)=>a-b);
        const result={mode,run,profiled,os_timing,input_hz:osHelper?60:inputHz,input_clock:osHelper?'Android OS Choreographer stylus':displayPaced?'browser requestAnimationFrame':'host timer',pick,before,started,middle,ended,camera,canvas,contact,thermal_before,thermal_after,measurement:'browser changed-style/path RAF callbacks; diagnostic, not presentations',duration_ms:probe.end-probe.begin,inputs,changed_frames:frames.length,callback_hz:frames.length>1?(frames.length-1)*1000/(frames.at(-1)-frames[0]):0,gap_p99_ms:gaps[Math.ceil(gaps.length*.99)-1],...probe};runs.push(result);
        await writeFile(`${dir}/bridge-benchmark.json`,JSON.stringify({info,photo,width,height,theme,input_hz:osHelper?60:inputHz,input_clock:osHelper?'Android OS Choreographer stylus':displayPaced?'browser requestAnimationFrame':'host timer',repeats,modes,runs},null,2));console.log(JSON.stringify({mode,run,callback_hz:result.callback_hz,gap_p99_ms:result.gap_p99_ms,counts:probe.counts}));
      }
      if(tracing)await finishTrace();
    }
    console.log(`RECORDED: Web bridge diagnostic ${repeats} six-second contacts per mode, ${inputHz}Hz declared input; priming undone`);
  } finally {
    await evaluate(`(()=>{const p=window.bridgeProbe;if(!p)return;p.running=false;p.observer?.disconnect();if(p.texture)GPUCanvasContext.prototype.getCurrentTexture=p.texture;for(const [name,original]of Object.entries(p.original))layerApp.app[name]=original;})()`).catch(()=>{});
    await evaluate(`(()=>{if(window.bridgeContactListener)for(const type of ['pointerdown','pointerup'])window.removeEventListener(type,bridgeContactListener,true);delete window.bridgeContactListener;delete window.bridgeContact;})()`).catch(()=>{});
    if(held)await mouse('mouseReleased',{x:500,y:400});
    if(tracing)await finishTrace();
    await send({type:'restore_workspace',workspace:saved});
    if(enteredFullscreen)await evaluate('document.fullscreenElement?document.exitFullscreen():null');
  }
}
