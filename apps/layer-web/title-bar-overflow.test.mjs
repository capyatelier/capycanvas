import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkTitleBarOverflow({call,evaluate,settle}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/title-bar/overflow';await mkdir(dir,{recursive:true});
  const pause=ms=>new Promise(r=>setTimeout(r,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const edit=action=>send({type:'customize',action:{type:'header',action}});
  const model=()=>evaluate('layerApp.state().workspace.layout.header');
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await settle();await pause(150);};
  const shot=async name=>{const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${name}.png`,Buffer.from(s.data,'base64'));};
  let device='mouse',point,pressed=false;
  const pointer=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device,force:type==='up'?0:.6});
    pressed=type!=='up';await settle();
  };
  const click=async selector=>{
    const input=device;if(!selector.includes('overflow'))device='mouse';
    await pointer('down',center(await rect(selector)));await pointer('up');await pause(150);device=input;
  };
  const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();};
  const clean=async()=>assert.equal(await evaluate('!!document.querySelector(".header-drag-preview")||document.querySelector("#workspace").hasAttribute("data-header-dragging")'),false);
  const workspace=await evaluate('layerApp.app.workspace_persistence()');
  try {
    // A long menu-label component with a neighbor behind it reproduces the
    // whole-region collapse. Setup uses shared edits; all pickups are native.
    for(const e of (await model()).zones.flat())await edit({type:'remove',id:e.id});
    for(const kind of ['capy','menu_labels','space'])await edit({type:'add',zone:'left',before:null,item:{kind}});
    await edit({type:'add',zone:'right',before:null,item:{kind:'settings'}});
    const menu=(await model()).zones[0][1].id,neighbor=(await model()).zones[0][2].id;
    for(const size of ['small','medium','large'])for(device of ['mouse','touch','pen']) {
      await edit({type:'set_size',size});const baseline=await model();
      await edit({type:'edit',editing:true});await resize(1800);
      const body=`[data-header-item="${menu}"]`;
      await pointer('down',center(await rect(body)));await pointer('move',{x:900,y:230});
      assert.ok(await evaluate('!!document.querySelector(".header-drag-preview")'));
      await escape();await pointer('up');await clean();assert.deepEqual(await model(),baseline);
      await resize(880);
      assert.equal(await evaluate(`document.querySelector('${body}').hidden`),true);
      await click('#header-overflow-0 > summary');
      const row=`[data-header-overflow-item="${menu}"]`,from=center(await rect(row));
      await shot(`menu-${size}-${device}`);
      const hit=await evaluate(`(()=>{const n=document.elementFromPoint(${from.x},${from.y});return{inside:document.querySelector('${row}').contains(n),target:n?.outerHTML,open:document.querySelector('#header-overflow-0').open}})()`);
      assert.ok(hit.inside,`Overflow rows stay above the component bank: ${JSON.stringify(hit)}; ${JSON.stringify(await evaluate('__overflowEvents.slice(-12)'))}`);
      await click(row);assert.deepEqual(await model(),baseline,'A row tap selects without moving');
      assert.equal(await evaluate('document.querySelector("#header-overflow-0").open'),false);
      await click('#header-overflow-0 > summary');
      await pointer('down',from);await pointer('move',{x:from.x+12,y:from.y+30});
      await shot(`pickup-${size}-${device}`);
      assert.ok(await evaluate('!!document.querySelector(".header-drag-preview")'),`${size} ${device}: overflow row picks up immediately`);
      assert.equal(await evaluate('Number(document.querySelector(".header-drag-preview").dataset.headerOverflowItem)'),menu,'The actual overflow item owns pickup');
      assert.equal(await evaluate('document.querySelector("#header-overflow-0").open'),false,'Pickup closes its menu');
      await escape();await pointer('up');await clean();assert.deepEqual(await model(),baseline);
      if(size==='medium')for(const cancellation of ['capture','resize','source']) {
        await click('#header-overflow-0 > summary');await pointer('down',center(await rect(row)));
        await pointer('move',{x:300,y:260});
        assert.ok(await evaluate('!!document.querySelector(".header-drag-preview")'));
        if(cancellation==='capture')await evaluate('document.querySelector("#workspace").releasePointerCapture(__overflowEvents.filter(e=>e.type==="pointerdown").at(-1).id)');
        if(cancellation==='resize')await resize(760);
        if(cancellation==='source')await evaluate(`document.querySelector('${row}').remove()`);
        await settle();await pointer('up');await clean();assert.deepEqual(await model(),baseline,`${device}: ${cancellation} cancels overflow drag`);
        if(cancellation==='resize')await resize(880);
      }
      // Move the formerly hidden menu to another region with the same ID.
      await click('#header-overflow-0 > summary');await pointer('down',center(await rect(row)));
      await pointer('move',center(await rect('[data-zone="center"]')));await pointer('up');
      assert.equal((await model()).zones[1][0].id,menu);
      await click('#header-edit-cancel');assert.deepEqual(await model(),baseline);
      // Neighbor rows stay draggable too, and dropping onto collapsed menus
      // inserts before the hidden items using the frozen shared geometry.
      await edit({type:'edit',editing:true});await click('#header-overflow-0 > summary');
      await pointer('down',center(await rect(`[data-header-overflow-item="${neighbor}"]`)));
      await pointer('move',{x:8,y:24});await pointer('up');
      assert.equal((await model()).zones[0][0].id,neighbor);
      await click('#header-edit-cancel');await edit({type:'edit',editing:true});
      const overflow=await rect('#header-overflow-0');
      await pointer('down',center(await rect('#header-component-clock')));
      await pointer('move',{x:overflow.x+2,y:overflow.y+overflow.height/2});await pointer('up');
      assert.deepEqual((await model()).zones[0].map(e=>e.item.kind),['capy','clock','menu_labels','space']);
      await click('#header-edit-cancel');await edit({type:'edit',editing:true});
      await edit({type:'remove',id:neighbor});
      // With one hidden item, the collapsed button itself is unambiguous.
      await pointer('down',center(await rect('#header-overflow-0 > summary')));
      await pointer('move',{x:400,y:280});await pointer('up');
      assert.ok(!(await model()).zones.flat().some(e=>e.id===menu));
      assert.equal(await evaluate('document.querySelector("#header-component-menu_labels").hidden'),false);
      await click('#header-edit-done');const accepted=await model();
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await model(),baseline);
      await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await model(),accepted);
      await send({type:'invoke',command:'undo_workspace'});await clean();
      console.log(`PASS overflow ${size} ${device}: expanded body, collapsed rows/button, re-entry, destinations, Cancel and one-step undo/redo`);
    }
  } finally {
    if(pressed)await pointer('up');
    await edit({type:'cancel'});await resize(1440);
    await send({type:'restore_workspace',workspace});
  }
}
