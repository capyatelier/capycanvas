import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Real Chrome pointer streams, retained DOM/pixels and durable IndexedDB state.
// Fixtures only select workspaces; editor journeys use visible controls.
export async function checkTitleBar({call,evaluate,settle,reload}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/title-bar/web'; await mkdir(dir,{recursive:true});
  const results=[];
  const pause=ms=>new Promise(r=>setTimeout(r,ms));
  const wait=async condition=>{
    for(let i=0;i<200;i++){if(await evaluate(condition))return;await pause(50);}
    throw Error(`Title bar timeout: ${condition}; ${await evaluate('document.querySelector("#status").textContent')}`);
  };
  const shot=async name=>{
    await settle(); const s=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${dir}/${name}.png`,Buffer.from(s.data,'base64'));
  };
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});const r=n.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  let device='mouse',point,pressed=false;
  const pointer=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd',cancel:'touchCancel'}[type],touchPoints:['up','cancel'].includes(type)?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.6});
    pressed=!['up','cancel'].includes(type);
  };
  const click=async selector=>{await pointer('down',center(await rect(selector)));await pointer('up');await settle();};
  const key=async(key,code=key,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,modifiers});await settle();};
  const model=()=>evaluate('layerApp.state().workspace.layout.header');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const selectWorkspace=async label=>{
    await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
    await evaluate(`(()=>{const b=[...document.querySelectorAll('.workspace-switcher button')].find(n=>n.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing workspace '+${JSON.stringify(label)});b.dataset.titleTestSwitch='true';})()`);
    await click('[data-title-test-switch]');
    await evaluate("document.querySelector('[data-title-test-switch]')?.removeAttribute('data-title-test-switch')");
    await wait('!JSON.parse(layerApp.app.workspace_view()).busy'); await settle();
  };
  const begin=async()=>{
    device='mouse';const p=center(await rect('#header'));
    await call('Input.dispatchMouseEvent',{type:'mousePressed',x:p.x,y:2,button:'right',buttons:2,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:p.x,y:2,button:'right',buttons:0,clickCount:1});
    await wait("document.querySelector('.panel-context-menu:popover-open')!==null");
    await evaluate(`(()=>{const b=[...document.querySelectorAll('.panel-context-menu button')].find(b=>b.textContent.includes('Customize Title Bar…'));if(!b)throw Error('Missing customization entry');b.dataset.titleTestEdit='true';})()`);
    await shot('entry-menu');
    await click('[data-title-test-edit]');await wait('layerApp.state().customization.header_editing');await settle();
  };
  const cancel=async()=>{device='mouse';await click('#header-edit-cancel');await wait('!layerApp.state().customization.header_editing');};
  const drop=async(selector,to)=>{await pointer('down',center(await rect(selector)));await pointer('move',to);await settle();await pointer('up');await settle();};
  const clean=()=>evaluate("!document.querySelector('.header-drag-preview')&&!document.querySelector('#workspace').hasAttribute('data-header-dragging')");
  try {
    await evaluate("window.__headerDevices=[];document.addEventListener('pointerdown',e=>{window.__headerPointer=e.pointerId;window.__headerDevices.push(e.pointerType)},true)");
    await wait('layerApp.startupTimes.complete!==null');
    console.log('title bar startup',await evaluate("[...document.querySelectorAll('.workspace-switcher button')].map(b=>b.textContent)"));
    await shot('startup');
    await selectWorkspace('Sketch');
    const original=await model();
    assert.equal(original.size,'medium');
    assert.deepEqual(original.zones[0].map(e=>e.item.kind),['capy','menu','tool','tool','tool']);
    assert.deepEqual(original.zones[1].map(e=>e.item.kind),['workspaces']);
    assert.deepEqual(await evaluate("[...document.querySelectorAll('.header-item .toolbar-controls')].length"),0);
    await begin();
    assert.equal(await evaluate("document.querySelectorAll('#header-editor h1,#header-editor h2,#header-editor #tool-picker').length"),0);
    await shot('editor');
    for(device of ['mouse','touch','pen']) {
      const before=await model();
      for(const selector of ['#header-component-clock','#header-component-tools']) {
        await click(selector);assert.deepEqual(await model(),before,`${device}: bank click inert`);
        assert.equal(await evaluate('layerApp.app.tool_picker()==null'),true);
      }
      const chip='#header-component-clock',from=center(await rect(chip));
      await pointer('down',from);await pointer('move',{x:from.x+12,y:from.y});await settle();
      assert.equal(await evaluate("!!document.querySelector('.header-drag-preview')"),true,`${device}: entire chip picks up without hold`);
      await pointer('up');await settle();assert.deepEqual(await model(),before,`${device}: bank outside cancels`);
      await drop(chip,{x:720,y:24});
      let added=(await model()).zones.flat().find(e=>e.item.kind==='clock');assert.ok(added,`${device}: bank drop adds`);
      assert.equal(await evaluate("document.querySelector('#header-component-clock').hidden"),true);
      const source=`[data-header-item="${added.id}"]`,r=await rect(source),press={x:r.x+4,y:r.y+10};
      await pointer('down',press);await pointer('move',{x:press.x+20,y:200});await settle();
      const held=await rect('.header-drag-preview');assert.ok(Math.abs(held.x-r.x-20)<.1&&Math.abs(held.y-190)<.1,`${device}: original grab offset`);
      await pointer('move',press);await settle();
      assert.equal(await evaluate("document.querySelector('.header-drag-preview').classList.contains('header-drag-remove')"),false,`${device}: re-entry`);
      await pointer('up');await settle();assert.ok((await model()).zones.flat().some(e=>e.id===added.id));
      await drop(source,{x:400,y:350});assert.ok(!(await model()).zones.flat().some(e=>e.id===added.id));
      assert.equal(await evaluate("document.querySelector('#header-component-clock').hidden"),false,`${device}: singleton returns`);
      assert.ok(await clean()); results.push(`${device}: inert bank clicks, immediate bank/body pickup, outside cancel, add, detach/grab offset/re-entry/remove`);
    }
    console.log('PASS: title-bar bank and body journeys for mouse/touch/pen');
    device='mouse';await cancel();assert.deepEqual(await model(),original);
    await shot('sketch');
    assert.deepEqual([...new Set(await evaluate('window.__headerDevices'))].sort(),['mouse','pen','touch']);
    await writeFile(`${dir}/results.json`,JSON.stringify({results,limitations:['CDP touch and pen streams in desktop Chrome; physical mobile browsers and pen hardware not attached.']},null,2));
  } finally {
    if(pressed)await pointer('up').catch(()=>{});
  }
}
