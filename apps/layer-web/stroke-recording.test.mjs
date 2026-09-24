import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {gunzipSync} from 'node:zlib';

export async function checkStrokeRecording({call, evaluate, settle}) {
  const action = async value => {await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle();};
  await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+30000;let clear=0;const timer=setInterval(()=>{const buttons=[...document.querySelectorAll('dialog[open] button')];const later=buttons.find(b=>b.textContent==='Keep for Later');if(later){later.click();clear=0;}else if(!document.querySelector('dialog[open]'))clear++;if(clear>=10){clearInterval(timer);resolve();}else if(performance.now()>deadline){clearInterval(timer);reject(Error('Startup dialog still open'));}},100);})`);
  const workspace = await evaluate('layerApp.state().workspace');
  const settings = await evaluate('layerApp.state().settings');
  await action({type:'customize', action:{type:'set_panel_visible',panel:'stats',visible:true}});
  await evaluate(`for(const column of layerApp.app.layout(innerWidth,innerHeight).collapsed){const group=column.groups.find(g=>g.icons.some(i=>i.panel==='stats'));if(group)layerApp.dispatch({type:'customize',action:{type:'set_column_collapsed',group:group.group,collapsed:false}});}`);
  await settle();
  await evaluate(`(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('stats'));if(g&&g.active!=='stats')layerApp.dispatch({type:'select_panel_tab',group:g.id,panel:'stats'});})()`);
  await settle();
  await evaluate(`window.strokeTest={picker:window.showSaveFilePicker};
    window.showSaveFilePicker=async()=>{throw new DOMException('Cancelled','AbortError')};`);
  const button = `document.querySelector('[data-control="stroke-recording"]')`;
  try {
    assert.equal(await evaluate(`${button} === ${button}.parentElement.lastElementChild`),true,'Recording is the last Diagnostics control');
    await evaluate(`${button}.click()`);
    assert.equal(await evaluate(`${button}.textContent`), 'Stop stroke recording');
    const origin = await evaluate(`(()=>{for(let y=200;y<innerHeight-100;y+=40)for(let x=350;x<innerWidth-200;x+=40)if([0,45,90].every(dx=>[-20,0,20].every(dy=>document.elementFromPoint(x+dx,y+dy)===layerApp.canvas)))return{x,y};throw Error('No uncovered canvas region');})()`);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...origin,button:'left',buttons:1,clickCount:1,pointerType:'pen',force:.3,tiltX:12,tiltY:24});
    for(let i=1;i<=30;i++) {
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:origin.x+i*3,y:origin.y+Math.sin(i/5)*20,buttons:1,pointerType:'pen',force:.3+i/100,tiltX:12,tiltY:24});
      await settle();
    }
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:origin.x+90,y:origin.y+Math.sin(6)*20,button:'left',buttons:0,clickCount:1,pointerType:'pen'});
    await settle();
    const captured=await evaluate('Number(layerApp.app.stroke_recording_status().raw_events)');
    assert.ok(captured>=32, `Expected all pen samples, captured ${captured}`);
    await evaluate(`${button}.click()`); await settle();
    assert.equal(await evaluate(`${button}.textContent`), 'Save stroke recording');
    assert.equal(await evaluate('layerApp.app.stroke_recording_status().ready'),true,'cancel retains recording');
    // Provider failure must retain the recording too.
    await evaluate(`window.showSaveFilePicker=async()=>({async createWritable(){throw Error('test provider failure')}}); ${button}.click()`);
    await settle();
    assert.equal(await evaluate('layerApp.app.stroke_recording_status().ready'),true);
    await evaluate(`window.showSaveFilePicker=async options=>{strokeTest.name=options.suggestedName;return {async createWritable(){return{async write(bytes){strokeTest.bytes=Array.from(bytes)},async close(){},async abort(){}}}}}; ${button}.click()`);
    await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+15000;function check(){if(strokeTest.bytes&&!layerApp.app.stroke_recording_status().ready)resolve();else if(performance.now()>deadline)reject(Error('Recording export timed out'));else setTimeout(check,20);}check();})`);
    const result = await evaluate('({name:strokeTest.name,bytes:strokeTest.bytes,state:{ready:layerApp.app.stroke_recording_status().ready}})');
    assert.equal(result.name,'stroke-recording.capystrokes'); assert.equal(result.state.ready,false);
    const bytes=Buffer.from(result.bytes);assert.equal(bytes.subarray(0,8).toString(),'CAPYPEN2');assert.ok(gunzipSync(bytes.subarray(8)).length>bytes.length);
    const output=process.env.LAYER_TEST_ARTIFACTS || 'artifacts/stroke-recording-web';
    await mkdir(output,{recursive:true});await writeFile(`${output}/web.capystrokes`,bytes);
    assert.equal(await evaluate(`${button}.textContent`),'Start stroke recording');
    console.log(`PASS: pen recording, cancellation, provider failure, retry and binary export (${bytes.length} bytes)`);
  } finally {
    await evaluate('window.showSaveFilePicker=strokeTest.picker;delete window.strokeTest');
    if(await evaluate("layerApp.state().commands.find(c=>c.id==='undo')?.enabled")) await action({type:'invoke',command:'undo'});
    await action({type:'restore_settings',settings});
    await action({type:'restore_workspace',workspace});
  }
}
