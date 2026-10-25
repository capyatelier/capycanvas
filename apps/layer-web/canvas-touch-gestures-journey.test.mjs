import assert from 'node:assert/strict';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkCanvasTouchGestures({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const camera=async()=>{const {translation,zoom,rotation,flipped}= (await state()).camera;return {translation,zoom,rotation,flipped};};
  const near=(actual,expected,label)=>{
    if(Array.isArray(expected))expected.forEach((value,index)=>near(actual[index],value,label));
    else if(typeof expected==='number')assert.ok(Math.abs(actual-expected)<.002,`${label}: ${actual} != ${expected}`);
    else assert.deepEqual(actual,expected,label);
  };
  const expectCamera=async(expected,label)=>{const actual=await camera();for(const key of Object.keys(expected))near(actual[key],expected[key],`${label}/${key}`);};
  let nextId=1000;
  const touch=(type,points=[])=>call('Input.dispatchTouchEvent',{type,touchPoints:points,timestamp:Date.now()/1000});
  await invoke('new_document');
  await evaluate(createDrawing({width:1000,height:700}));
  await evaluate('new Promise((resolve,reject)=>{const end=performance.now()+25000;function check(){if(layerApp.app.brush_ready()&&!layerApp.documents.busy())resolve();else if(performance.now()>end)reject(Error("drawing readiness"));else setTimeout(check,20)}check()})');
  await invoke('reset_view');
  const center=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  const points=(count,spacing=55)=>Array.from({length:count},(_,index)=>({id:++nextId,x:center.x+(index-(count-1)/2)*spacing,y:center.y}));
  const pinch=async(outward,{slow=false,cancel=false}={})=>{
    const contacts=points(2,outward?100:200);
    await touch('touchStart',contacts);
    for(let step=1;step<=4;step++){
      await pause(slow?140:18);
      const factor=outward?1+step/4:1-step/8;
      await touch('touchMove',contacts.map(point=>({...point,x:center.x+(point.x-center.x)*factor})));
    }
    await touch(cancel?'touchCancel':'touchEnd');
    await settle();
  };
  const layerIds=async()=>(await state()).layers.map(layer=>layer.id);
  const releaseHold=async()=>{
    await evaluate(`window.__historyRelease=[];window.__historyReleaseCapture=event=>{if(event.pointerType==='touch')window.__historyReleaseBefore=layerApp.state().layers.map(layer=>String(layer.id));};window.__historyReleaseBubble=event=>{if(event.pointerType==='touch')__historyRelease.push({before:__historyReleaseBefore,after:layerApp.state().layers.map(layer=>String(layer.id))});};layerApp.canvas.addEventListener('pointerup',__historyReleaseCapture,true);layerApp.canvas.addEventListener('pointerup',__historyReleaseBubble)`);
    await touch('touchEnd');
    const releases=await evaluate(`(()=>{layerApp.canvas.removeEventListener('pointerup',__historyReleaseCapture,true);layerApp.canvas.removeEventListener('pointerup',__historyReleaseBubble);return __historyRelease})()`);
    assert.ok(releases.length>=2,'the browser delivers real touch releases');
    for(const release of releases)assert.deepEqual(release.after,release.before,'releasing a history hold does not add a tap');
    await settle();
  };
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await invoke('reset_view');
    const fitted=await camera();
    await send({type:'set_zoom',zoom:fitted.zoom*1.8});
    await send({type:'set_rotation',rotation:.7});
    await invoke('flip_horizontal');
    const previous=await camera();
    const layers=await layerIds();
    await pinch(false);
    await expectCamera(fitted,`${theme}: quick inward pinch fits upright`);
    await pinch(true);
    await expectCamera(previous,`${theme}: quick outward pinch restores complete previous view`);
    assert.deepEqual(await layerIds(),layers,'quick pinches preserve artwork history');

    for(const options of [{slow:true},{cancel:true}]) {
      await send({type:'set_rotation',rotation:.7});
      const before=await camera();
      await pinch(false,options);
      const after=await camera();
      near(after.rotation,before.rotation,`${theme}: rejected quick pinch preserves rotation`);
      assert.ok(after.zoom<before.zoom*.8,`${theme}: ${JSON.stringify(options)} remains ordinary zoom navigation`);
      await pause(650);
      assert.deepEqual(await layerIds(),layers,'navigation never starts held Undo');
    }

    await invoke('reset_view');
    for(let index=0;index<12;index++)await invoke('add_layer');
    const complete=await layerIds();
    const view=await camera();
    await touch('touchStart',points(2));
    await pause(760);
    const undone=await layerIds();
    assert.ok(undone.length<=complete.length-2,`${theme}: stationary two-finger hold repeats Undo before lift`);
    assert.ok(undone.length>=complete.length-8,'hold does not exhaust the history fixture');
    await releaseHold();
    const released=await layerIds();
    await pause(350);
    assert.deepEqual(await layerIds(),released,'Undo repeat stops after release');
    await expectCamera(view,'history hold preserves camera');

    await touch('touchStart',points(3));
    await pause(760);
    assert.ok((await layerIds()).length>=released.length+2,`${theme}: stationary three-finger hold repeats Redo before lift`);
    await releaseHold();
    const redone=await layerIds();
    await pause(350);
    assert.deepEqual(await layerIds(),redone,'Redo repeat stops after release');
    await expectCamera(view,'Redo hold preserves camera');
    for(let remaining=complete.length;(await layerIds()).length<complete.length && remaining>0;remaining--)await invoke('redo');
    assert.deepEqual(await layerIds(),complete,'Redo restores the exact layer order');

    for(const count of [2,3]) {
      const before=await layerIds();
      await touch('touchStart',points(count));
      await pause(100);
      await touch('touchCancel');
      await pause(750);
      assert.deepEqual(await layerIds(),before,`${theme}: cancelled ${count}-finger hold cannot tap or repeat`);
    }
  }
  console.log('PASS: quick pinch fit/restore, slow and cancelled pinch navigation, held Undo/Redo, release and cancellation in both themes');
}
