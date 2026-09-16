import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Unchanged full-size photo, ordinary app frame scheduling and WebGPU renderer.
// Gesture injection is synthetic; these timestamps are not hardware input latency.
export async function benchPhotoNavigation({evaluate,call},output) {
  await call('Runtime.evaluate',{expression:'document.documentElement.requestFullscreen()',awaitPromise:true,userGesture:true});
  await evaluate('new Promise(r=>setTimeout(r,1000))');
  const result=await evaluate(`(async()=>{
    const app=layerApp.app,raf=()=>new Promise(requestAnimationFrame),frame=app.frame.bind(app),report=[];
    layerApp.dispatch({type:'invoke',command:'fit_canvas'});await raf();await raf();
    const camera=app.camera(),area=camera.work_area,cx=area[0]+area[2]/2,cy=area[1]+area[3]/2;
    let measured=[];
    app.frame=(...args)=>{const start=performance.now();const result=frame(...args);measured.push([args[0],start,performance.now()-start]);return result;};
    try {
      for(let run=0;run<3;run++){
        measured=[];const inputs=[];let old={x:cx,y:cy,zoom:1,angle:0};
        for(let i=0;i<=360;i++){
          const time=await raf(),t=i/360,zoom=Math.pow(20,(1-Math.cos(t*2*Math.PI))/2),angle=.8*Math.sin(t*2*Math.PI),x=cx+90*Math.sin(t*4*Math.PI),y=cy+50*Math.sin(t*2*Math.PI);
          const start=performance.now();app.gesture(old.x,old.y,x,y,zoom/old.zoom,angle-old.angle);layerApp.wake();inputs.push([time,start,performance.now()-start]);old={x,y,zoom,angle};
        }
        await raf();await raf();report.push({run,inputs,frames:measured.slice(),stats:app.renderer_stats(),camera:app.camera()});
      }
    }finally{app.frame=frame;}
    return JSON.parse(JSON.stringify({viewport:camera.viewport,dpr:devicePixelRatio,ua:navigator.userAgent,report},(_,v)=>typeof v==='bigint'?Number(v):v));
  })()`);
  await writeFile(output,JSON.stringify(result,null,2));
  const q=(a,p)=>a.toSorted((a,b)=>a-b)[Math.floor((a.length-1)*p)];
  for(const r of result.report){assert.ok(r.frames.length>250);const cpu=r.frames.map(f=>f[2]),cadence=r.inputs.slice(1).map((f,i)=>f[0]-r.inputs[i][0]);console.log(JSON.stringify({run:r.run,frames:r.frames.length,cpu_p50:q(cpu,.5),cpu_p95:q(cpu,.95),cpu_p99:q(cpu,.99),cadence_p50:q(cadence,.5),cadence_p95:q(cadence,.95),cadence_p99:q(cadence,.99),over_12ms:cadence.filter(v=>v>12).length,stats:r.stats.rows}));}
}
