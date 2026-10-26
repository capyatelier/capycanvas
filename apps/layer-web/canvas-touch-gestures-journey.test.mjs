import assert from 'node:assert/strict';
import {createDrawing} from './new-drawing-fixture.mjs';

export async function checkCanvasTouchGestures({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+25000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,20)}check()})`);
  const click=async selector=>{
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'center'})`);await settle();
    const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n.getBoundingClientRect();if(!r.width||!r.height||n.disabled)throw Error('Unavailable '+${JSON.stringify(selector)}+' '+JSON.stringify({open:layerApp.state().settings_open,dialog:document.querySelector('#settings').open,startup:layerApp.startupTimes}));return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const preferences=async(fit,restore)=>{
    await send({type:'open_settings',page:'canvas'});
    assert.ok(await evaluate("document.querySelector('#settings').open"),'Canvas Preferences opens');
    assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"),'canvas');
    for(const [name,value] of [['fit',fit],['restore',restore]]) {
      const selector=`#setting-quick-pinch-${name}`,key=`quick_pinch_${name}`;
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).type`),'checkbox');
      if(await evaluate(`document.querySelector(${JSON.stringify(selector)}).checked`)!==value)await click(selector);
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).checked`),value);
      await wait(`layerApp.state().settings.${key}===${value}&&JSON.parse(localStorage.getItem('layer.preferences.v1')??'{}').${key}===${value}`);
    }
    await click('#close-settings');
  };
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
  let center;
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
  assert.deepEqual(await evaluate('[layerApp.state().settings.quick_pinch_fit,layerApp.state().settings.quick_pinch_restore]'),[true,true],'quick-pinch actions default to enabled');
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await preferences(false,false);
    const origin=await evaluate('performance.timeOrigin');
    await call('Page.reload');
    for(const deadline=Date.now()+30000;;await pause(50)){
      if(await evaluate(`performance.timeOrigin!==${origin}&&!!window.layerApp&&document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready()`).catch(()=>false))break;
      assert.ok(Date.now()<deadline,`${theme}: preferences reload reaches hardware canvas readiness`);
    }
    await wait("layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy");
    assert.deepEqual(await evaluate('[layerApp.state().settings.quick_pinch_fit,layerApp.state().settings.quick_pinch_restore]'),[false,false],`${theme}: both disabled preferences persist across reload`);
    await preferences(false,false);
    await preferences(true,true);
    await invoke('new_document');await evaluate(createDrawing({width:1000,height:700}));
    await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()');
    await invoke('reset_view');
    center=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
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

    await preferences(false,true);
    const fitDisabled=await camera();await pinch(false);
    const ordinaryInward=await camera();
    assert.ok(Math.abs(ordinaryInward.zoom/fitDisabled.zoom-.5)<.01,`${theme}: disabled fit keeps ordinary inward zoom`);
    near(ordinaryInward.rotation,fitDisabled.rotation,'disabled fit preserves rotation');
    assert.deepEqual(ordinaryInward.flipped,fitDisabled.flipped,'disabled fit preserves mirrored view');
    await preferences(true,false);
    await send({type:'set_zoom',zoom:fitted.zoom*1.8});await send({type:'set_rotation',rotation:.7});
    await pinch(false);await expectCamera(fitted,`${theme}: fit remains enabled when restore is disabled`);
    await pinch(true);const ordinaryOutward=await camera();
    assert.ok(Math.abs(ordinaryOutward.zoom/fitted.zoom-2)<.02,`${theme}: disabled restore keeps ordinary outward zoom`);
    near(ordinaryOutward.rotation,0,'disabled restore retains fitted rotation');
    assert.deepEqual(ordinaryOutward.flipped,fitted.flipped,'disabled restore retains fitted orientation');
    assert.deepEqual(await layerIds(),layers,'preference-controlled gestures preserve artwork history');
    await preferences(true,true);

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
  console.log('PASS: native quick-pinch preferences persist and independently preserve ordinary zoom when disabled; enabled fit/restore, slow/cancelled navigation and held Undo/Redo pass in both themes');
}
