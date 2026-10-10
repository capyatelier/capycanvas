import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkPressureCalibration({call,evaluate,settle}) {
  const read=()=>evaluate("JSON.parse(JSON.stringify(layerApp.state().pressure_calibration??null,(_,v)=>typeof v==='bigint'?Number(v):v))");
  const points=async()=> (await read())?.editor.points;
  const saved=()=>evaluate('layerApp.state().settings.pressure_curve');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();};
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+5000;function poll(){if(${condition})resolve();else if(performance.now()>deadline)reject(Error(${JSON.stringify(condition)}+': '+JSON.stringify({settingsOpen:layerApp.state().settings_open,settingsDialog:document.querySelector('#settings').open,calibration:!!layerApp.state().pressure_calibration,panel:!!document.querySelector('#pen-pressure-dialog'),closes:window.pressureSettingsCloseCount,busy:layerApp.documents.busy(),notice:document.querySelector('#status').textContent})));else setTimeout(poll,25)}poll()})`);
  const pointer=async(type,point,pointerType='mouse')=>call('Input.dispatchMouseEvent',{type,...point,pointerType,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,force:type==='mouseReleased'?0:.65});
  const graphPoint=async(x,y)=>evaluate(`(()=>{const g=document.querySelector('#pen-pressure-dialog .curve-editor'),v=layerApp.state().pressure_calibration.editor.controls,r=g.getBoundingClientRect(),w=r.width-2*v.inset,h=r.height-2*v.inset;return{x:r.left+v.inset+${x}*w,y:r.top+v.inset+(1-${y})*h}})()`);
  const stroke=async(start,end,pointerType='mouse')=>{await pointer('mousePressed',start,pointerType);await pointer('mouseMoved',end,pointerType);await pointer('mouseReleased',end,pointerType);await settle();};
  const assertNoNotice=async action=>assert.equal(await evaluate("document.querySelector('#status').textContent"),'',`${action} leaves no error notice`);
  const capture=async name=>{const dir='artifacts/pressure-calibration/rollout/web';await mkdir(dir,{recursive:true});const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${name}.png`,Buffer.from(shot.data,'base64'));};
  const open=async()=>{await evaluate('layerApp.app.wait_for_canvas()');await wait("!layerApp.documents.busy()&&!document.querySelector('#pen-pressure-dialog')&&!layerApp.state().settings_open&&!!document.querySelector('#header [data-command=settings]:not(:disabled)')?.getClientRects().length");await click('#header [data-command="settings"]');await wait("layerApp.state().settings_open&&document.querySelector('#settings').open");await click('[data-settings-page="input"]');const status=await evaluate("({state:layerApp.state().settings_open,dialog:document.querySelector('#settings').open,rows:layerApp.app.preferences()?.pages?.flatMap(p=>p.groups.flatMap(g=>g.rows)).map(r=>r.id)??[]})");assert.ok(status.rows.includes('pen_pressure'),`Preferences exposes Pen pressure action: ${JSON.stringify(status)}`);await wait('!layerApp.documents.busy()');const closes=await evaluate('window.pressureSettingsCloseCount');await click('#setting-pen-pressure');await wait(`window.pressureSettingsCloseCount>${closes}&&!!layerApp.state().pressure_calibration&&!!document.querySelector('#pen-pressure-dialog')&&!document.querySelector('#settings').open`);};
  await evaluate('layerApp.documents.startRecovery()');
  await evaluate("window.pressureSettingsCloseCount=0;document.querySelector('#settings').addEventListener('close',()=>window.pressureSettingsCloseCount++)");
  await click('[data-command="settings"]');
  await click('[data-settings-page="canvas"]');
  await click('#setting-pan-speed .number-value');
  await evaluate("(()=>{const n=document.querySelector('#setting-pan-speed .number-entry');n.value='1.5';n.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}))})()");await settle();
  assert.equal(await evaluate('layerApp.state().settings.pan_speed'),1.5,'Canvas retains a generic numeric preference');
  await click('#setting-pan-speed [aria-label="Increase Scroll pan speed"]');
  assert.ok(Math.abs(await evaluate('layerApp.state().settings.pan_speed')-1.55)<.001);
  await send({type:'preferences',action:{type:'reset',id:'pan_speed'}});
  await click('#close-settings');
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});
    const baseline=await saved();
    await open();
    await capture(`${theme}-default`);
    assert.equal(await evaluate("document.querySelector('#pen-pressure-dialog').getAttribute('aria-modal')"),'false');
    assert.equal(await evaluate("document.querySelectorAll('#pen-pressure-dialog .curve-coordinates, #pen-pressure-dialog .number-control').length"),0);
    assert.deepEqual(await evaluate("(()=>{const a=[...document.querySelectorAll('#pen-pressure-dialog .curve-axis-y span')];return a.map(n=>n.textContent)})()"),['100%','Output','0%']);
    assert.deepEqual(await evaluate("(()=>{const a=[...document.querySelectorAll('#pen-pressure-dialog .curve-axis-x span')];return a.map(n=>n.textContent)})()"),['0%','Input','100%']);
    const dialog=await evaluate("(()=>{const r=document.querySelector('#pen-pressure-dialog').getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()");
    const frame={x:dialog.x+80,y:dialog.y+17};
    await stroke(frame,{x:frame.x-35,y:frame.y+29});
    const moved=await evaluate("(()=>{const r=document.querySelector('#pen-pressure-dialog').getBoundingClientRect();return{x:r.x,y:r.y}})()");
    assert.ok(Math.abs(moved.x-dialog.x+35)<2&&Math.abs(moved.y-dialog.y-29)<2,'header moves the floating dialog');
    const first=await points();
    assert.equal(first.length,3);
    const fresh=await graphPoint(.1,.5);
    await stroke(fresh,fresh);
    assert.equal((await points()).length,4,'mouse inserts an interior point');
    const added=await points(),middle=added[1];
    const start=await graphPoint(...middle);
    await stroke(start,{x:start.x,y:start.y-12},'pen');
    assert.notDeepEqual(await points(),added,'pen drag edits a point');
    const edited=await points();
    const touchStart=await graphPoint(...edited[1]);
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...touchStart,id:91,radiusX:1,radiusY:1,force:.65}]});
    await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:touchStart.x,y:touchStart.y+10,id:91,radiusX:1,radiusY:1,force:.65}]});
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
    assert.notDeepEqual(await points(),edited,'touch drag edits a point');
    for(const [index,pointerType] of ['mouse','pen','touch'].entries()){
      if(index){const fresh=await graphPoint(.1,.5);await stroke(fresh,fresh);assert.equal((await points()).length,4,`${pointerType} removal starts with an interior point`);}
      const beforeRemoval=await points(),removal=await graphPoint(...beforeRemoval[1]);
      const outside=await evaluate("(()=>{const g=document.querySelector('#pen-pressure-dialog .curve-editor'),r=g.getBoundingClientRect(),inset=layerApp.state().pressure_calibration.editor.controls.inset;return{x:r.left+inset-20,far:r.left+inset-32,further:r.left+inset-48}})()");
      const move=async x=>{
        if(pointerType==='touch')await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x,y:removal.y,id:92+index,radiusX:1,radiusY:1,force:.65}]});
        else await pointer('mouseMoved',{x,y:removal.y},pointerType);
        await settle();
      };
      if(pointerType==='touch')await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...removal,id:92+index,radiusX:1,radiusY:1,force:.65}]});else await pointer('mousePressed',removal,pointerType);
      await settle();
      await move(outside.x);
      assert.equal((await points()).length,4,`${pointerType} keeps the point within 24 logical px of the plot`);
      await move(outside.far);
      assert.equal((await points()).length,3,`${pointerType} removes the interior point on Move beyond 24 logical px, before release`);
      await move(outside.further);
      await move(removal.x);
      assert.equal((await points()).length,3,`${pointerType} continued input does not restore the deleted point`);
      if(pointerType==='touch')await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});else await pointer('mouseReleased',{x:removal.x,y:removal.y},pointerType);
      await settle();
      assert.equal((await points()).length,3,`${pointerType} release retains the deletion`);
    }
    const end=await graphPoint(1,1);
    await stroke(end,{x:end.x-40,y:end.y+40});
    assert.deepEqual((await points()).at(-1),[1,1],'output end stays fixed');
    const initial=await points();
    await click('#pen-pressure-dialog .utility-sensitivity button:first-child');
    assert.ok((await points())[0][1]<initial[0][1],'Firmer lowers the start');
    await click('#pen-pressure-dialog .utility-sensitivity button:last-child');
    assert.deepEqual(await points(),initial,'Lighter restores the start');
    await click('#pen-pressure-dialog .utility-actions button:first-child');
    assert.deepEqual(await points(),[[0,.125],[.25,1],[1,1]],'Reset restores the default');
    const markerStart=await evaluate("(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()");
    await pointer('mousePressed',markerStart,'pen');await settle();
    assert.ok((await read()).editor.marker,'canvas pen pressure updates the live marker');
    await assertNoNotice('Canvas pen contact');
    await capture(`${theme}-live-marker`);
    await pointer('mouseReleased',markerStart,'pen');await settle();
    assert.equal((await read()).editor.marker??null,null);
    await click('#pen-pressure-dialog .utility-sensitivity button:first-child');
    const committed=await points();
    await click('#pen-pressure-dialog .utility-actions button:last-child');
    assert.equal(await read(),null,'Apply closes the dialog');
    await assertNoNotice('Apply');
    assert.deepEqual(await saved(),committed,'Apply commits the edited curve');
    assert.deepEqual(await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1')).pressure_curve"),committed,'Apply persists the edited curve');
    if(theme==='light'){
      const origin=await evaluate('performance.timeOrigin');
      await call('Page.reload');
      for(const deadline=Date.now()+20000;;await new Promise(resolve=>setTimeout(resolve,50))){
        assert.ok(Date.now()<deadline,'Pressure response reload timed out');
        if(await evaluate(`performance.timeOrigin!==${origin}&&!!window.layerApp&&document.body.dataset.gpu==='ready'&&layerApp.app.brush_ready()`).catch(()=>false))break;
      }
      await evaluate('layerApp.documents.startRecovery()');
      await evaluate("window.pressureSettingsCloseCount=0;document.querySelector('#settings').addEventListener('close',()=>window.pressureSettingsCloseCount++)");
      assert.deepEqual(await saved(),committed,'Reload restores the applied response');
    }
    await open();
    await click('#pen-pressure-dialog .utility-sensitivity button:first-child');
    await click('#pen-pressure-dialog .utility-actions button:nth-last-child(2)');
    assert.equal(await read(),null,'Cancel closes the dialog');
    await assertNoNotice('Cancel');
    assert.deepEqual(await saved(),committed,'Cancel restores saved response');
    await open();
    await click('#pen-pressure-dialog .utility-sensitivity button:first-child');
    await click('#pen-pressure-dialog .utility-close');
    assert.equal(await read(),null,'X closes the dialog');
    await assertNoNotice('X');
    assert.deepEqual(await saved(),committed);
    await open();
    await evaluate("document.querySelector('#pen-pressure-dialog .curve-editor').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}))");await settle();
    assert.equal(await read(),null,'Escape closes the dialog');
    await assertNoNotice('Escape');
    assert.deepEqual(await saved(),committed);
    await open();
    await click('#pen-pressure-dialog .utility-actions button:first-child');
    await click('#pen-pressure-dialog .utility-actions button:last-child');
    assert.deepEqual(await saved(),baseline);
  }
  console.log('PASS: Pen pressure calibration in light and dark themes');
}
