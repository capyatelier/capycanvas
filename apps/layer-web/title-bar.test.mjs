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
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest',inline:'nearest'})`);await pointer('down',center(await rect(selector)));await pointer('up');await settle();};
  const key=async(key,code=key,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,modifiers,windowsVirtualKeyCode:({Backspace:8,Delete:46,ArrowLeft:37,ArrowRight:39,Escape:27,F10:121,s:83})[key]||0});await settle();};
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
    const bands=await evaluate('layerApp.state().workspace.layout.bands');
    assert.deepEqual(bands.map(b=>[b.edge,b.alignment,b.root.panels.length]),[['left','center',1]],'Sketch docks only its compact brush toolbar');
    const sliders=await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id===${JSON.stringify(bands[0].root.panels[0])}).content.tiles.map(t=>t.control.kind)`);
    assert.ok(sliders.includes('brush_size_slider')&&sliders.includes('brush_opacity_slider'),`compact brush sliders: ${sliders}`);
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

    // Slide uses Rust's frozen slots; no model edit is published during motion.
    for(const pointerType of ['mouse','touch','pen']) {
      await begin();device=pointerType;
      const before=await model(),id=before.zones[0][0].id,neighbor=before.zones[0][1].id;
      const source=`[data-header-item="${id}"]`,n=`[data-header-item="${neighbor}"]`;
      const r=await rect(source),nr=await rect(n),from=center(r);
      await pointer('down',from);await pointer('move',{x:nr.x+nr.width+16,y:from.y});await pause(160);
      assert.ok((await rect(n)).x<nr.x,`${device}: neighbors slide`);
      assert.deepEqual(await model(),before,`${device}: motion is only preview`);
      await shot(`slide-${device}`);await pointer('up');await settle();
      assert.notDeepEqual((await model()).zones[0],before.zones[0]);
      await cancel();assert.deepEqual(await model(),original);
      for(const mode of ['escape','capture','blur','source','resize','cancel']) {
        await begin();device=pointerType;
        await pointer('down',center(await rect(source)));await pointer('move',{x:400,y:250});await settle();
        if(mode==='escape')await key('Escape');
        if(mode==='capture')await evaluate("document.querySelector('#workspace').releasePointerCapture(window.__headerPointer)");
        if(mode==='blur')await evaluate("window.dispatchEvent(new Event('blur'))");
        if(mode==='source')await send({type:'customize',action:{type:'header',action:{type:'set_size',size:'large'}}});
        if(mode==='resize')await call('Emulation.setDeviceMetricsOverride',{width:1300,height:1000,deviceScaleFactor:1,mobile:false});
        if(mode==='cancel') {
          if(device==='touch')await pointer('cancel');
          else await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:window.__headerPointer,bubbles:true}))");
        }
        if(pressed)await pointer('up');await settle();assert.ok(await clean(),`${device}: ${mode} cleanup`);
        assert.deepEqual((await model()).zones,before.zones,`${device}: ${mode} no drop`);
        if(mode==='resize')await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
        await cancel();
      }
      results.push(`${pointerType}: frozen-slot neighbor slide and atomic drop; Escape, capture loss, blur, model invalidation, resize and pointer cancellation`);
    }
    console.log('PASS: slide and cancellation journeys');
    await begin();device='mouse';
    const last=original.zones[0].at(-1).id;
    await click(`[data-header-item="${last}"]`);await key('ArrowRight');
    assert.equal((await model()).zones[1][0].id,last,'Arrow crosses to center');
    await key('ArrowLeft');assert.equal((await model()).zones[0].at(-1).id,last);
    await key('F10','F10',8);await wait("!!document.querySelector('.panel-context-menu:popover-open')");
    assert.ok(await evaluate("document.querySelector('.panel-context-menu').textContent.includes('Remove from Title Bar')"));
    await key('Escape');await click(`[data-header-item="${last}"]`);await key('Backspace');
    assert.ok(!(await model()).zones.flat().some(e=>e.id===last));
    await cancel();assert.deepEqual(await model(),original);
    results.push('Keyboard Left/Right cross regions, Shift+F10 shared context menu, Backspace remove, Cancel restores');

    for(const pointerType of ['mouse','touch','pen']) {
      await begin();device=pointerType;
      await drop('#header-component-tools',{x:720,y:25});
      await wait("document.querySelector('#tool-picker').open");
      assert.ok(await evaluate('layerApp.state().customization.header_editing'));
      assert.deepEqual(await model(),original,'Tools drop only opens existing picker');
      device='mouse';await click('#tool-search');await call('Input.insertText',{text:'Undo'});await settle();
      await key('Backspace');assert.equal(await evaluate("document.querySelector('#tool-search').value"),'Und','Picker editing owns Backspace');
      assert.deepEqual(await model(),original);
      await click('#tool-picker .dialog-header button:first-child');
      await wait("!document.querySelector('#tool-picker').open");
      assert.ok(await evaluate('layerApp.state().customization.header_editing'));
      assert.deepEqual(await model(),original,'Picker Cancel preserves enclosing preview');
      device=pointerType;await drop('#header-component-tools',{x:720,y:25});await wait("document.querySelector('#tool-picker').open");
      device='mouse';await click('#tool-search');await call('Input.insertText',{text:'Undo'});await settle();
      await click('.tool-choice input');await click('#confirm-tools');
      await wait("!document.querySelector('#tool-picker').open");
      const added=(await model()).zones.flat().find(e=>!original.zones.flat().some(old=>old.id===e.id));assert.ok(added);
      device=pointerType;await drop(`[data-header-item="${added.id}"]`,{x:400,y:250});
      assert.ok(!(await model()).zones.flat().some(e=>e.id===added.id),'Outside tool drop removes');
      await cancel();assert.deepEqual(await model(),original);
      results.push(`${pointerType}: drop-only Tools, picker text ownership, nested Cancel, picker confirm and tool removal`);
    }
    console.log('PASS: keyboard, context and shared tool-picker journeys');

    // All native content families are added through the existing picker, then
    // their actual header buttons open the same retained drawers as toolbars.
    await begin();await drop('#header-component-tools',{x:450,y:25});await wait("document.querySelector('#tool-picker').open");
    const choices=await evaluate('layerApp.app.tool_picker().choices');
    const controls=choices.filter(c=>c.control.kind==='panel'||['opacity','brush','size'].includes(c.control.kind));
    const selectedChoices=[];
    for(const c of controls) {
      if(c.control.kind==='brush'&&selectedChoices.some(p=>p.kind==='brush')||c.control.kind==='size'&&selectedChoices.some(p=>p.kind==='size'))continue;
      const index=choices.findIndex(p=>JSON.stringify(p.control)===JSON.stringify(c.control));
      await click(`.tool-choice:nth-child(${index+1}) input`);selectedChoices.push(c.control);
    }
    await click('#confirm-tools');await click('#header-edit-done');await wait('!layerApp.state().customization.header_editing');
    const expanded=await model();
    // Put each test subject in the generous center region through shared moves
    // before native activation; content construction/activation stays real DOM.
    for(const control of selectedChoices) {
      let id=expanded.zones.flat().find(e=>JSON.stringify(e.item.control)===JSON.stringify(control)).id;
      await send({type:'customize',action:{type:'header',action:{type:'move',id,zone:'center',before:null}}});
      const selector=`[data-header-item="${id}"] .header-tool`;
      // Overflow activations are tested separately; make this content family visible.
      const all=await model();for(const e of all.zones[1])if(e.id!==id)await send({type:'customize',action:{type:'header',action:{type:'move',id:e.id,zone:'left',before:null}}});
      await click(selector);
      if(control.kind!=='size') {
        if(!await evaluate('!!layerApp.state().customization.drawer'))await click(selector);
        await wait("!!document.querySelector('.content-drawer:not([inert])')");await pause(200);
        assert.equal(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).borderBottomLeftRadius`),'0px');
        await shot(`drawer-${control.panel||control.kind}`);
        const explicit=await evaluate("layerApp.state().customization.drawer.dismissal==='explicit'");
        await pointer('down',{x:720,y:2});await pointer('up');await settle();
        if(explicit){assert.ok(await evaluate('!!layerApp.state().customization.drawer'),`${control.panel||control.kind}: explicit drawers ignore outside contact`);await click(selector);}
        await wait('!layerApp.state().customization.drawer');
      }
    }
    results.push(`All ${selectedChoices.length} picker content/brush/size families activate through header buttons; drawer corners and space dismissal`);
    console.log('PASS: content-family drawers');
    // Restore only the test fixture after exhaustive drawer coverage.
    await send({type:'restore_workspace',workspace:{...await evaluate('layerApp.state().workspace'),layout:{...await evaluate('layerApp.state().workspace.layout'),header:original}}});


    for(const pointerType of ['mouse','touch','pen']) {
      await begin();device=pointerType;const source='[data-header-item="1"]';
      await click(source);const before=await model();
      assert.deepEqual(before,original,`${device}: editing body click does not activate`);
      await pointer('down',center(await rect(source)));await pause(650);
      assert.equal(await evaluate("!!document.querySelector('.panel-context-menu:popover-open')"),device!=='mouse',`${device}: hold menu ownership`);
      await pointer('move',{x:50,y:200});await settle();
      assert.equal(await evaluate("!!document.querySelector('.panel-context-menu:popover-open')"),false,'Dragging closes held menu');
      await key('Escape');await pointer('up');await settle();assert.deepEqual(await model(),before);assert.ok(await clean());
      await pointer('down',center(await rect(source)));await pause(650);await pointer('up');await settle();
      assert.equal(await evaluate("!!document.querySelector('.panel-context-menu:popover-open')"),device!=='mouse',`${device}: hold release retains context`);
      await key('Escape');await cancel();
      results.push(`${pointerType}: inert body click, device-specific hold menu, same-contact drag dismissal, hold release and Escape cleanup`);
    }
    console.log('PASS: title-bar hold/context ownership');
    // Title-bar tool families preserve toolbar activation and drawer switching.
    for(const pointerType of ['mouse','touch','pen']) {
      device=pointerType;
      for(const command of ['drawing_brush','sculpt','eraser','select','scale_rotate']) {
        const entry=original.zones.flat().find(e=>e.item.control?.command===command),selector=`[data-header-item="${entry.id}"] .header-tool`;
        if(await evaluate(`document.querySelector(${JSON.stringify(selector)}).disabled`))continue;
        await click(selector);if(!await evaluate('!!layerApp.state().customization.drawer'))await click(selector);
        await wait('!!layerApp.state().customization.drawer');await pause(180);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-pressed')`),'true');
        assert.equal(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).borderBottomRightRadius`),'0px');
      }
      const color=original.zones.flat().find(e=>e.item.control?.kind==='color');
      await click(`[data-header-item="${color.id}"] .header-tool`);await pause(180);
      const background=await evaluate('JSON.stringify(layerApp.state().colors.background)');await click('.content-drawer .color-swap');
      assert.equal(await evaluate('JSON.stringify(layerApp.state().colors.foreground)'),background);
      await pointer('down',{x:720,y:2});await pointer('up');await wait('!layerApp.state().customization.drawer');
      results.push(`${pointerType}: Brush/Sculpt/Eraser/Select/Transform drawer activation and selected feedback; Color swap and title-space dismissal`);
    }
    device='mouse';await begin();await drop('#header-component-tools',{x:720,y:25});await wait("document.querySelector('#tool-picker').open");
    await click('#tool-search');await call('Input.insertText',{text:'Zoom In'});await settle();await click('.tool-choice input');await click('#confirm-tools');await click('#header-edit-done');
    const action=(await model()).zones.flat().find(e=>e.item.control?.command==='zoom_in'),selector=`[data-header-item="${action.id}"] .header-tool`;
    const zoom=await evaluate('layerApp.state().camera.zoom');await pointer('down',center(await rect(selector)));await settle();
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-pressed')`),'false','Action is not a selected tool');
    const color=await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).backgroundColor`);
    assert.ok(!color.includes('53, 132, 228'),'Action press uses grey feedback');await shot('action-pressed');await pointer('up');await settle();
    assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom);await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await model(),original);
    results.push('Zoom In action has grey pressed feedback, remains unselected, executes and is removable in one workspace undo');

    await shot('sketch');
    assert.deepEqual([...new Set(await evaluate('window.__headerDevices'))].sort(),['mouse','pen','touch']);
    await writeFile(`${dir}/results.json`,JSON.stringify({results,limitations:['CDP touch and pen streams in desktop Chrome; physical mobile browsers and pen hardware not attached.']},null,2));
  } finally {
    if(pressed)await pointer('up').catch(()=>{});
  }
}
