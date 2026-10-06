import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const summary=values=>{const sorted=values.toSorted((a,b)=>a-b),at=q=>sorted[Math.floor((sorted.length-1)*q)];return sorted.length?{count:sorted.length,p50:at(.5),p95:at(.95),p99:at(.99),max:sorted.at(-1)}:null;};

export async function measureSessionRestart({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/seamless-restart/web';await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+60000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status')?.textContent));else setTimeout(poll,10);}poll();})`);
  await wait('layerApp.startupTimes.complete!==null&&layerApp.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()');await evaluate('layerApp.documents.startRecovery()');
  await evaluate(`(async()=>{
    const store=(await import('./restart-store.js')).createRestartStore();
    window.sessionPerf={store,writes:[],frames:[],inputs:[],active:0};
    sessionPerf.capture=layerApp.app.capture_tab_session.bind(layerApp.app);
    layerApp.app.capture_tab_session=id=>{const captured=performance.now(),capture=sessionPerf.capture(id),write=capture.write.bind(capture);capture.write=async(key,...args)=>{
      let completed=false;const before=await store.read(key),retained=new Set([...(before?.current.resources??[]),...(before?.previous?.resources??[])]),begin=performance.now();sessionPerf.active++;
      try {const result=await write(key,...args),end=performance.now();sessionPerf.active--;completed=true;const after=await store.read(key),newIds=after.current.resources.filter(id=>!retained.has(id));
        const blocks=await store.resources(key,newIds),resources=blocks.reduce((sum,bytes)=>sum+bytes.byteLength,0),metadata=new TextEncoder().encode(JSON.stringify(after)).byteLength;
        sessionPerf.writes.push({captured,begin,end,resources,metadata,logical_bytes:resources+metadata,publication_ms:end-begin,durable_age_ms:end-captured,canvas:JSON.parse(after.current.metadata).objects.filter(object=>object.record.type==='capy.composition/2').map(object=>object.record.data)});return result;
      }finally{if(!completed)sessionPerf.active--;}
    };return capture;};
    sessionPerf.frame=layerApp.app.frame.bind(layerApp.app);layerApp.app.frame=(...args)=>{const begin=performance.now();try{return sessionPerf.frame(...args)}finally{sessionPerf.frames.push({time:args[0],duration:performance.now()-begin,writing:sessionPerf.active>0});}};
    sessionPerf.input=layerApp.app.input.bind(layerApp.app);layerApp.app.input=(...args)=>{const begin=performance.now();try{return sessionPerf.input(...args)}finally{sessionPerf.inputs.push(performance.now()-begin);}};
  })()`);
  const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.5)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.5)*r.height/c.viewport[1]};})()`);
  const pointer=(type,x,y)=>call('Input.dispatchMouseEvent',{type,x,y,button:'left',buttons:type==='mouseReleased'?0:1,pointerType:'pen',force:type==='mouseReleased'?0:.65,clickCount:1});
  const gesture=async duration=>{
    const begin=await evaluate('performance.now()');await pointer('mousePressed',point.x,point.y);const start=performance.now(),pending=[];let failure;
    while(performance.now()-start<duration){const t=performance.now()-start;pending.push(pointer('mouseMoved',point.x+75*Math.sin(t/250),point.y+40*Math.cos(t/310)).catch(error=>failure??=error));await new Promise(resolve=>setTimeout(resolve,8));}
    await Promise.all(pending);await pointer('mouseReleased',point.x,point.y);if(failure)throw failure;return{begin,end:await evaluate('performance.now()')};
  };
  const commit=(await promisify(execFile)('git',['rev-parse','HEAD'])).stdout.trim();
  const report={date:new Date().toISOString(),commit,source:'Working tree implementation',profile:'dev-perf',brush:await evaluate("JSON.parse(JSON.stringify(layerApp.state().brush,(_,value)=>typeof value==='bigint'?Number(value):value))"),qualification:false,reason:'Desktop diagnostic; not reference tablet hardware or tier photo',gpu:(await call('SystemInfo.getInfo',{},null)).gpu.devices,startup:await evaluate('layerApp.startupTimes'),runs:[]};
  try {
    await gesture(600);await wait('layerApp.app.document_park_ready()');await evaluate('layerApp.documents.autosave()');await settle();
    for(let run=0;run<3;run++) {
      await evaluate('sessionPerf.frames=[];sessionPerf.inputs=[];sessionPerf.writes=[];');
      await evaluate(`layerApp.dispatch({type:'invoke',command:'add_layer'})`);await wait('layerApp.app.document_park_ready()');
      await evaluate('window.sessionPerfSave=layerApp.documents.autosave()');
      await wait('sessionPerf.active>0||sessionPerf.writes.length>0');
      const motion=await gesture(6000);await wait('layerApp.app.document_park_ready()');await evaluate('sessionPerfSave');await evaluate('layerApp.documents.autosave()');
      const observed=await evaluate('({frames:sessionPerf.frames,inputs:sessionPerf.inputs,writes:sessionPerf.writes})');
      observed.frames=observed.frames.filter(frame=>frame.time>=motion.begin&&frame.time<=motion.end);
      const intervals=observed.frames.slice(1).map((frame,index)=>frame.time-observed.frames[index].time);
      report.runs.push({duration_ms:6000,callback_intervals_ms:summary(intervals),host_frame_ms:summary(observed.frames.map(frame=>frame.duration)),input_submission_ms:summary(observed.inputs),frames_while_writing:observed.frames.filter(frame=>frame.writing).length,write_bytes:summary(observed.writes.map(write=>write.logical_bytes)),durable_age_ms:summary(observed.writes.map(write=>write.durable_age_ms)),...observed});
    }
    assert.ok(report.runs.every(run=>run.writes.length>0),'Each motion run publishes a complete checkpoint');
    assert.ok(report.runs.every(run=>run.writes.every(write=>write.canvas.length===1&&write.canvas[0].size?.length===2)),'Each checkpoint records its composition');
    assert.ok(report.runs.every(run=>run.frames.length>100&&run.inputs.length>400),'Real pen input and rendering continue throughout each motion window');
    report.checkpoint_overlap_frames=report.runs.reduce((count,run)=>count+run.frames_while_writing,0);
    const before=await evaluate('sessionPerf.writes.length');await evaluate('layerApp.documents.autosave()');assert.equal(await evaluate('sessionPerf.writes.length'),before,'Unchanged drawing does not write another checkpoint');
    await writeFile(`${directory}/checkpoint-performance.json`,JSON.stringify(report,null,2)+'\n');
    console.log('PASS checkpoint motion diagnostics',JSON.stringify(report.runs.map(({callback_intervals_ms,host_frame_ms,frames_while_writing,write_bytes,durable_age_ms})=>({callback_intervals_ms,host_frame_ms,frames_while_writing,write_bytes,durable_age_ms}))));
  } finally {await evaluate('layerApp.app.capture_tab_session=sessionPerf.capture;layerApp.app.frame=sessionPerf.frame;layerApp.app.input=sessionPerf.input');}
}
