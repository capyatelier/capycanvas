import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {gunzipSync} from 'node:zlib';

export async function checkStrokeRecording({call, evaluate, settle}) {
  const action = async value => {await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle();};
  const output=process.env.LAYER_TEST_ARTIFACTS || 'artifacts/stroke-recording-web';await mkdir(output,{recursive:true});
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
  const button = `[...document.querySelectorAll('[data-control="stroke-recording"]')].find(node=>node.checkVisibility()&&node.getBoundingClientRect().width>0)`;
  const geometry=[];
  const compactControl=async label=>{
    const before=await evaluate('layerApp.state().workspace'),theme=await evaluate('layerApp.state().settings.theme');
    try{
      for(const current of ['light','dark'])for(const placement of ['dock','float','drawer']){
        await action({type:'restore_workspace',workspace:before});
        await action({type:'set_theme',theme:current});
        await action({type:'move_panel',panel:'stats',target:placement==='float'?{kind:'float',position:[350,140]}:{kind:'edge',edge:'right',outer:true}});
        if(placement==='drawer'){
          const group=await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(group=>group.panels.includes('stats')).id");
          await action({type:'customize',action:{type:'set_column_collapsed',group,collapsed:true}});
          const column=await evaluate("layerApp.app.layout(innerWidth,innerHeight).collapsed.find(column=>column.groups.some(group=>group.icons.some(icon=>icon.panel==='stats'))).id");
          await action({type:'customize',action:{type:'set_column_drawers',column,drawers:true}});
          await evaluate("document.querySelector('.collapsed-column [data-panel=stats]').click()");await settle();
        }
        await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+5000;function check(){const node=${button};if(node?.textContent===${JSON.stringify(label)}&&!node.disabled)resolve();else if(performance.now()>end)reject(Error('Visible recording control: '+${JSON.stringify(label)}));else setTimeout(check,20)}check()})`);
        await evaluate(`${button}.scrollIntoView({block:'nearest'})`);
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:await evaluate('innerWidth-2'),y:2,button:'none',buttons:0});await settle();
        const measured=await evaluate(`(()=>{const node=${button},style=getComputedStyle(node),bounds=node.getBoundingClientRect(),group=node.closest('.dock-group');return{label:node.textContent,height:bounds.height,width:bounds.width,padding:[style.paddingTop,style.paddingRight,style.paddingBottom,style.paddingLeft].map(parseFloat),border:[style.borderTopWidth,style.borderRightWidth,style.borderBottomWidth,style.borderLeftWidth].map(parseFloat),background:style.backgroundColor,shadow:style.boxShadow,drawer:!!node.closest('.content-drawer'),floating:group?.classList.contains('floating-panel')??false,metrics:layerApp.app.catalog().panel_metrics}})()`);
        geometry.push({theme:current,placement,...measured});
        assert.equal(measured.metrics.control_height,26,'Shared compact panel control height');
        assert.ok(Math.abs(measured.height-measured.metrics.control_height)<.5&&measured.width>0,`${current}/${placement}/${label}: compact recording geometry`);
        assert.deepEqual(measured.padding,[0,measured.metrics.field_padding,0,measured.metrics.field_padding],'Recording uses flat panel padding');
        assert.deepEqual(measured.border,[0,0,0,0]);assert.equal(measured.shadow,'none');assert.equal(measured.background,'rgba(0, 0, 0, 0)');
        assert.equal(measured.drawer,placement==='drawer');assert.equal(measured.floating,placement==='float');
      }
    }catch(error){const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${output}/control-failure.png`,Buffer.from(shot.data,'base64'));throw error;}
    finally{await action({type:'restore_workspace',workspace:before});await action({type:'set_theme',theme});}
  };
  try {
    assert.equal(await evaluate(`${button} === ${button}.parentElement.lastElementChild`),true,'Recording is the last Diagnostics control');
    await compactControl('Start stroke recording');
    await evaluate(`${button}.click()`);
    assert.equal(await evaluate(`${button}.textContent`), 'Stop stroke recording');
    await compactControl('Stop stroke recording');
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
    await compactControl('Save stroke recording');
    // Provider failure must retain the recording too.
    await evaluate(`window.showSaveFilePicker=async()=>({async createWritable(){throw Error('test provider failure')}}); ${button}.click()`);
    await settle();
    assert.equal(await evaluate('layerApp.app.stroke_recording_status().ready'),true);
    await evaluate(`window.showSaveFilePicker=async options=>{strokeTest.name=options.suggestedName;return {async createWritable(){return{async write(bytes){strokeTest.bytes=Array.from(bytes)},async close(){},async abort(){}}}}}; ${button}.click()`);
    await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+15000;function check(){if(strokeTest.bytes&&!layerApp.app.stroke_recording_status().ready)resolve();else if(performance.now()>deadline)reject(Error('Recording export timed out'));else setTimeout(check,20);}check();})`);
    const result = await evaluate('({name:strokeTest.name,bytes:strokeTest.bytes,state:{ready:layerApp.app.stroke_recording_status().ready}})');
    assert.equal(result.name,'stroke-recording.capystrokes'); assert.equal(result.state.ready,false);
    const bytes=Buffer.from(result.bytes);assert.equal(bytes.subarray(0,8).toString(),'CAPYPEN4');assert.ok(gunzipSync(bytes.subarray(8)).length>bytes.length);
    await writeFile(`${output}/web.capystrokes`,bytes);
    assert.equal(await evaluate(`${button}.textContent`),'Start stroke recording');
    console.log(`PASS: pen recording, cancellation, provider failure, retry and binary export (${bytes.length} bytes)`);
  } finally {
    await writeFile(`${output}/control-geometry.json`,JSON.stringify(geometry,null,2));
    await evaluate('window.showSaveFilePicker=strokeTest.picker;delete window.strokeTest');
    if(await evaluate("layerApp.state().commands.find(c=>c.id==='undo')?.enabled")) await action({type:'invoke',command:'undo'});
    await action({type:'restore_settings',settings});
    await action({type:'restore_workspace',workspace});
  }
}
