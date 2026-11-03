import assert from 'node:assert/strict';
import {installSessionStoreFixture} from './session-store-fixture.mjs';
import {mkdir,writeFile} from 'node:fs/promises';
const summary=values=>{const sorted=values.toSorted((a,b)=>a-b),at=q=>sorted[Math.floor((sorted.length-1)*q)];return sorted.length?{count:sorted.length,p50:at(.5),p95:at(.95),p99:at(.99),max:sorted.at(-1)}:null;};

export async function measureSessionRestart({call,evaluate,settle,once}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/seamless-restart/web';await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+60000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status')?.textContent));else setTimeout(poll,10);}poll();})`);
  await wait('layerApp.startupTimes.complete!==null&&layerApp.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()');await evaluate('layerApp.documents.startRecovery()');
  await installSessionStoreFixture(evaluate);
  const instrumentation=`(()=>{
    window.sessionPerf={writes:[],transfers:[],operations:[],longTasks:[],checkpointCalls:[],frames:[],inputs:[],active:0};
    sessionPerf.invokeCheckpoint=(kind,invoke)=>{
      const call={kind,entry:performance.now()};let result;
      try{result=invoke();}finally{call.promise_return=performance.now();call.synchronous_ms=call.promise_return-call.entry;sessionPerf.checkpointCalls.push(call);}
      return {call,result};
    };
    sessionPerf.post=Worker.prototype.postMessage;
    Worker.prototype.postMessage=function(value,...args){
      const request=value.request;
      if(request)sessionPerf.operations.push({time:performance.now(),operation:request.operation,name:request.metadata?.name??null,bytes:(request.buffers??[]).reduce((sum,bytes)=>sum+bytes.byteLength,0),read_bytes:request.operation==='read_at'||request.operation==='legacy-range'?request.metadata.length:0});
      if(request?.operation==='write_at')sessionPerf.transfers.push({time:performance.now(),name:request.metadata.name,bytes:request.buffers[0].byteLength});
      return sessionPerf.post.call(this,value,...args);
    };
    if(PerformanceObserver.supportedEntryTypes.includes('longtask')){sessionPerf.observer=new PerformanceObserver(list=>sessionPerf.longTasks.push(...list.getEntries().map(({startTime,duration})=>({startTime,duration}))));sessionPerf.observer.observe({type:'longtask'});}
    sessionPerf.api=typeof layerApp.app.checkpoint_session==='function'?'OPFS':'released-IDB';
    if(sessionPerf.api==='OPFS') {
    sessionPerf.checkpoint=layerApp.app.checkpoint_session.bind(layerApp.app);
    layerApp.app.checkpoint_session=async(id,...args)=>{
      const requested=performance.now(),key=layerApp.app.session_binding(id,null),begin=performance.now();sessionPerf.active++;
      const invoked=sessionPerf.invokeCheckpoint('checkpoint_session',()=>sessionPerf.checkpoint(id,...args));
      let result,end;try{result=await invoked.result;end=performance.now();}finally{invoked.call.settled=performance.now();invoked.call.awaited_ms=invoked.call.settled-invoked.call.promise_return;sessionPerf.active--;}
      if(result){
        const after=await sessionStoreFixture.snapshot(key),transfers=sessionPerf.transfers.filter(write=>write.time>=begin&&write.time<=end&&write.name.startsWith('drawings/'+key+'/'));
        const resources=transfers.filter(write=>write.name.includes('/packs/')||write.name.includes('/resources/')).reduce((sum,write)=>sum+write.bytes,0);
        const metadata=transfers.filter(write=>write.name.includes('/generations/')).reduce((sum,write)=>sum+write.bytes,0);
        sessionPerf.writes.push({requested,begin,end,resources,metadata,logical_bytes:resources+metadata,publication_ms:end-begin,durable_age_ms:end-requested,tile_count:after.current.metadata.resources.filter(resource=>resource.type==='capy.raster-tile/1').length,canvas:after.current.metadata.objects.filter(object=>object.record.type==='capy.composition/2').map(object=>object.record.data)});
      }
      return result;
    };
    }else{
      sessionPerf.legacyRead=async(store,key)=>{
        sessionPerf.database??=new Promise((resolve,reject)=>{const open=indexedDB.open('capy-session-restart');open.onupgradeneeded=()=>{open.transaction.abort();reject(Error('Missing control recovery database'));};open.onsuccess=()=>resolve(open.result);open.onerror=()=>reject(open.error);});
        const database=await sessionPerf.database;
        return new Promise((resolve,reject)=>{const tx=database.transaction(store,'readonly'),read=tx.objectStore(store).get(key);let value;read.onsuccess=()=>{value=read.result;};tx.oncomplete=()=>resolve(value);tx.onabort=()=>reject(tx.error);});
      };
      sessionPerf.capture=layerApp.app.capture_tab_session.bind(layerApp.app);
      layerApp.app.capture_tab_session=id=>{
        const requested=performance.now(),captured=sessionPerf.invokeCheckpoint('capture_tab_session',()=>sessionPerf.capture(id)),capture=captured.result,write=capture.write.bind(capture);
        captured.call.settled=captured.call.promise_return;captured.call.awaited_ms=0;
        capture.write=async(key,...args)=>{
          const before=await sessionPerf.legacyRead('sessions',key),retained=new Set([...(before?.current.resources??[]),...(before?.previous?.resources??[])]),begin=performance.now();sessionPerf.active++;
          const invoked=sessionPerf.invokeCheckpoint('capture.write',()=>write(key,...args));
          let result,end;try{result=await invoked.result;end=performance.now();}finally{invoked.call.settled=performance.now();invoked.call.awaited_ms=invoked.call.settled-invoked.call.promise_return;sessionPerf.active--;}
          const after=await sessionPerf.legacyRead('sessions',key),payloads=await Promise.all(after.current.resources.filter(id=>!retained.has(id)).map(id=>sessionPerf.legacyRead('resources',[key,id])));
          const resources=payloads.reduce((sum,bytes)=>sum+bytes.byteLength,0),metadata=new TextEncoder().encode(JSON.stringify(after)).byteLength,record=JSON.parse(after.current.metadata);
          sessionPerf.writes.push({requested,begin,end,resources,metadata,logical_bytes:resources+metadata,publication_ms:end-begin,durable_age_ms:end-requested,tile_count:record.resources.filter(resource=>resource.type==='capy.raster-tile/1').length,canvas:record.objects.filter(object=>object.record.type==='capy.composition/2').map(object=>object.record.data)});return result;
        };return capture;
      };
    }
    sessionPerf.frame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{const begin=performance.now();try{return sessionPerf.frame(...args)}finally{sessionPerf.frames.push({time:args[0],duration:performance.now()-begin,writing:sessionPerf.active>0});}};
    sessionPerf.input=layerApp.app.input.bind(layerApp.app);layerApp.app.input=(...args)=>{const begin=performance.now();try{return sessionPerf.input(...args)}finally{sessionPerf.inputs.push(performance.now()-begin);}};
  })()`;
  await evaluate(instrumentation);
  const stopInstrumentation=()=>evaluate('if(window.sessionPerf){sessionPerf.observer?.disconnect();Worker.prototype.postMessage=sessionPerf.post;if(sessionPerf.checkpoint)layerApp.app.checkpoint_session=sessionPerf.checkpoint;else layerApp.app.capture_tab_session=sessionPerf.capture;layerApp.app.frame=sessionPerf.frame;layerApp.app.input=sessionPerf.input;sessionPerf.database?.then(database=>database.close());}');
  const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.5)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.5)*r.height/c.viewport[1]};})()`);
  const pointer=(type,x,y)=>call('Input.dispatchMouseEvent',{type,x,y,button:'left',buttons:type==='mouseReleased'?0:1,pointerType:'pen',force:type==='mouseReleased'?0:.65,clickCount:1});
  const gesture=async duration=>{
    const begin=await evaluate('performance.now()');await pointer('mousePressed',point.x,point.y);const start=performance.now(),pending=[];let failure;
    while(performance.now()-start<duration){const t=performance.now()-start;pending.push(pointer('mouseMoved',point.x+75*Math.sin(t/250),point.y+40*Math.cos(t/310)).catch(error=>failure??=error));await new Promise(resolve=>setTimeout(resolve,8));}
    await Promise.all(pending);await pointer('mouseReleased',point.x,point.y);if(failure)throw failure;return{begin,end:await evaluate('performance.now()')};
  };
  const report={date:new Date().toISOString(),commit:process.env.LAYER_TEST_SOURCE_REVISION??null,source:'Prebuilt runtime',profile:process.env.LAYER_TEST_BUILD_PROFILE??'prebuilt',runtime:process.env.LAYER_WEB_URL??null,brush:await evaluate("JSON.parse(JSON.stringify(layerApp.state().brush,(_,value)=>typeof value==='bigint'?Number(value):value))"),qualification:false,reason:'Desktop diagnostic; not reference tablet hardware or tier photo',gpu:(await call('SystemInfo.getInfo',{},null)).gpu.devices,startup:await evaluate('layerApp.startupTimes'),checkpoint_api:await evaluate('sessionPerf.api'),runs:[],pans:[]};
  let reloadProbe;
  try {
    const checkpoint=async()=>{
      const boundary=await evaluate(`(async()=>{const begin=performance.now();await layerApp.documents.autosave();if(sessionPerf.api==='OPFS')for(const tab of layerApp.app.document_tabs(0).tabs)if(!layerApp.app.session_current(tab.id))await layerApp.app.checkpoint_session(tab.id,true);return {begin,end:performance.now()};})()`);
      const tasks=await evaluate(`sessionPerf.longTasks.filter(task=>task.startTime<${boundary.end}&&task.startTime+task.duration>${boundary.begin})`);
      const calls=await evaluate(`sessionPerf.checkpointCalls.filter(call=>call.entry<${boundary.end}&&call.promise_return>=${boundary.begin})`);
      return {...boundary,checkpoint_calls:calls,duration_ms:boundary.end-boundary.begin,longest_main_task_ms:tasks.length?Math.max(...tasks.map(task=>task.duration)):null,longest_main_task_observation:tasks.length?'observed long task':'<50ms',longTasks:tasks};
    };
    await gesture(600);await wait('layerApp.app.document_park_ready()');
    report.first_checkpoint_main=await checkpoint();await settle();
    report.first_checkpoint=await evaluate('sessionPerf.writes[0]??null');
    assert.ok(report.first_checkpoint,'The initial painted drawing publishes an observed checkpoint');
    const cleanWrites=await evaluate('sessionPerf.writes.length');report.clean_checkpoint=await checkpoint();
    report.clean_checkpoint_ms=report.clean_checkpoint.duration_ms;assert.equal(await evaluate('sessionPerf.writes.length'),cleanWrites,'A clean checkpoint publishes no generation');
    for(let run=0;run<3;run++) {
      await evaluate('sessionPerf.frames=[];sessionPerf.inputs=[];sessionPerf.writes=[];sessionPerf.transfers=[];sessionPerf.operations=[];sessionPerf.longTasks=[];sessionPerf.checkpointCalls=[];');
      await evaluate(`layerApp.dispatch({type:'invoke',command:'add_layer'})`);await wait('layerApp.app.document_park_ready()');
      await evaluate('window.sessionPerfSave=layerApp.documents.autosave()');
      await wait('sessionPerf.active>0||sessionPerf.writes.length>0');
      const motion=await gesture(6000);await wait('layerApp.app.document_park_ready()');await evaluate('sessionPerfSave');await evaluate('layerApp.documents.autosave()');
      const observed=await evaluate('({frames:sessionPerf.frames,inputs:sessionPerf.inputs,writes:sessionPerf.writes,operations:sessionPerf.operations,longTasks:sessionPerf.longTasks,checkpoint_calls:sessionPerf.checkpointCalls})');
      observed.frames=observed.frames.filter(frame=>frame.time>=motion.begin&&frame.time<=motion.end);
      const intervals=observed.frames.slice(1).map((frame,index)=>frame.time-observed.frames[index].time);
      const tail=summary(intervals).p99,tailGaps=observed.frames.slice(1).map((frame,index)=>({begin:observed.frames[index].time,end:frame.time,duration_ms:intervals[index]})).filter(gap=>gap.duration_ms>=tail).map(gap=>({...gap,checkpoint_sync_overlap:observed.checkpoint_calls.filter(call=>call.entry<gap.end&&call.promise_return>gap.begin).map(call=>({...call,overlap_ms:Math.min(call.promise_return,gap.end)-Math.max(call.entry,gap.begin)})),checkpoint_await_overlap:observed.checkpoint_calls.filter(call=>call.promise_return<gap.end&&call.settled>gap.begin).map(call=>({kind:call.kind,promise_return:call.promise_return,settled:call.settled}))}));
      report.runs.push({duration_ms:6000,motion,tail_gaps:tailGaps,checkpoint_synchronous_ms:summary(observed.checkpoint_calls.map(call=>call.synchronous_ms)),checkpoint_awaited_ms:summary(observed.checkpoint_calls.map(call=>call.awaited_ms)),longest_main_task_ms:Math.max(0,...observed.longTasks.filter(task=>task.startTime>=motion.begin&&task.startTime<=motion.end).map(task=>task.duration)),callback_intervals_ms:summary(intervals),host_frame_ms:summary(observed.frames.map(frame=>frame.duration)),input_submission_ms:summary(observed.inputs),frames_while_writing:observed.frames.filter(frame=>frame.writing).length,write_bytes:summary(observed.writes.map(write=>write.logical_bytes)),durable_age_ms:summary(observed.writes.map(write=>write.durable_age_ms)),...observed});
    }
    assert.ok(report.runs.every(run=>run.writes.length>0),'Each motion run publishes a complete checkpoint');
    assert.ok(report.runs.every(run=>run.writes.every(write=>write.canvas.length===1&&write.canvas[0].size?.length===2)),'Each checkpoint records its composition');
    assert.ok(report.runs.every(run=>run.frames.length>100&&run.inputs.length>400),'Real pen input and rendering continue throughout each motion window');
    report.checkpoint_overlap_frames=report.runs.reduce((count,run)=>count+run.frames_while_writing,0);
    const before=await evaluate('sessionPerf.writes.length');await evaluate('layerApp.documents.autosave()');assert.equal(await evaluate('sessionPerf.writes.length'),before,'Unchanged drawing does not write another checkpoint');
    const pan=async label=>{
      await wait('layerApp.app.document_park_ready()&&!layerApp.app.canvas_work_pending()');
      const observed=await evaluate(`(async()=>{
        sessionPerf.frames=[];sessionPerf.operations=[];sessionPerf.longTasks=[];
        const camera=layerApp.app.camera(),area=camera.work_area,cx=area[0]+area[2]/2,cy=area[1]+area[3]/2,begin=performance.now(),inputs=[];
        let old={x:cx,y:cy};
        for(;;){const time=await new Promise(requestAnimationFrame),t=Math.min(1,(time-begin)/1200),x=cx+180*Math.sin(Math.PI*t),y=cy+60*Math.sin(Math.PI*t),start=performance.now();layerApp.app.gesture(old.x,old.y,x,y,1,0);layerApp.wake();inputs.push({time,duration:performance.now()-start});old={x,y};if(t===1)break;}
        return {begin,end:performance.now(),inputs};
      })()`);
      const readyBegin=performance.now();await wait('layerApp.app.brush_ready()&&layerApp.app.document_park_ready()&&!layerApp.app.canvas_work_pending()');await settle();
      const data=await evaluate('({frames:sessionPerf.frames,operations:sessionPerf.operations,longTasks:sessionPerf.longTasks,camera:JSON.parse(JSON.stringify(layerApp.app.camera(),(_,value)=>typeof value==="bigint"?Number(value):value))})');
      const frames=data.frames.filter(frame=>frame.time>=observed.begin&&frame.time<=observed.end),intervals=frames.slice(1).map((frame,index)=>frame.time-frames[index].time);
      assert.ok(observed.inputs.length>20&&frames.length>10,'Each pan includes moving input and actual app frame callbacks');
      report.pans.push({label,ready_after_input_ms:performance.now()-readyBegin,ready_boundary:'brush_ready && document_park_ready && !canvas_work_pending, then existing settle',callback_intervals_ms:summary(intervals),host_frame_ms:summary(frames.map(frame=>frame.duration)),input_submission_ms:summary(observed.inputs.map(input=>input.duration)),longest_main_task_ms:Math.max(0,...data.longTasks.map(task=>task.duration)),...observed,...data,frames});
    };
    await pan('warm-before-reload');await evaluate('layerApp.documents.autosave()');
    await stopInstrumentation();
    reloadProbe=await call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{
      const state=window.sessionReload={operations:[],longTasks:[]},post=Worker.prototype.postMessage;
      Worker.prototype.postMessage=function(value,...args){const request=value.request;if(request)state.operations.push({time:performance.now(),operation:request.operation,name:request.metadata?.name??null,read_bytes:request.operation==='read_at'||request.operation==='legacy-range'?request.metadata.length:0});return post.call(this,value,...args);};
      if(PerformanceObserver.supportedEntryTypes.includes('longtask')){state.observer=new PerformanceObserver(list=>state.longTasks.push(...list.getEntries().map(({startTime,duration})=>({startTime,duration}))));state.observer.observe({type:'longtask',buffered:true});}
      state.stop=()=>{Worker.prototype.postMessage=post;state.observer?.disconnect();};
    })()`});
    const old=await evaluate('performance.timeOrigin'),pageLoaded=once('Page.loadEventFired'),reloadBegin=performance.now();await call('Page.reload');await pageLoaded;
    await wait(`performance.timeOrigin!==${old}&&window.layerApp?.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()`);
    await evaluate('layerApp.documents.startRecovery()');await wait('!layerApp.app.canvas_work_pending()');await settle();
    report.reload_ready_ms=performance.now()-reloadBegin;report.restored_camera=await evaluate("JSON.parse(JSON.stringify(layerApp.app.camera(),(_,value)=>typeof value==='bigint'?Number(value):value))");
    report.reload=await evaluate('({operations:sessionReload.operations,longTasks:sessionReload.longTasks,longest_main_task_ms:Math.max(0,...sessionReload.longTasks.map(task=>task.duration))})');
    await evaluate('sessionReload.stop()');await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:reloadProbe.identifier});reloadProbe=null;
    await installSessionStoreFixture(evaluate);await evaluate(instrumentation);
    await pan('first-after-reload');await pan('warm-repeat-after-reload');
    console.log('PASS checkpoint motion diagnostics',JSON.stringify(report.runs.map(({callback_intervals_ms,host_frame_ms,frames_while_writing,write_bytes,durable_age_ms})=>({callback_intervals_ms,host_frame_ms,frames_while_writing,write_bytes,durable_age_ms}))));
  } finally {
    if(reloadProbe){await call('Page.removeScriptToEvaluateOnNewDocument',{identifier:reloadProbe.identifier});await evaluate('window.sessionReload?.stop()');}
    await stopInstrumentation();await writeFile(`${directory}/checkpoint-performance.json`,JSON.stringify(report,null,2)+'\n');
  }
}
