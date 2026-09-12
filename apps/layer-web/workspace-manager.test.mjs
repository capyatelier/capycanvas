import assert from "node:assert/strict";
import {mkdir, writeFile} from "node:fs/promises";

export async function checkWorkspaceManager({call, evaluate, settle, reload, touch = false}) {
  const wait = async predicate => {
    for (let n=0; n<150; n++) { if (await evaluate(predicate)) return; await new Promise(r=>setTimeout(r,100)); }
    throw Error(`Workspace timeout: ${predicate}\n${await evaluate('layerApp.app.workspace_view()')}`);
  };
  const view = () => evaluate('JSON.parse(layerApp.app.workspace_view())');
  const capture = () => evaluate('JSON.parse(layerApp.app.workspace_capture())');
  const idle = () => wait('JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
  const click = async selector => {
    // Only scroll the manager's list. Scrolling a centered native dialog while
    // Android's keyboard is active moves it between measurement and contact.
    await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing '+${JSON.stringify(selector)});if(e.closest('.workspace-list'))e.scrollIntoView({block:'nearest',behavior:'instant'});})()`);
    await settle();
    const point = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if (touch) {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...point}]});
      await new Promise(r=>setTimeout(r,60));
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    } else {
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
      await call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',buttons:0,clickCount:1});
    }
    await settle(); await new Promise(r=>setTimeout(r,200));
  };
  const menu = async label => {
    await click('[data-menu="window"] > summary');
    for (const text of ['Workspaces',label]) {
      await evaluate(`(()=>{const e=[...document.querySelectorAll('#workspace-menu button')].find(e=>e.textContent===${JSON.stringify(text)});if(!e)throw Error('Missing menu '+${JSON.stringify(text)});e.dataset.workspaceTest='target';})()`);
      await click('[data-workspace-test="target"]');
    }
    await idle();
  };
  const text = async (selector,value) => {
    await click(selector); await call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,nativeVirtualKeyCode:65,modifiers:2});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65,nativeVirtualKeyCode:65,modifiers:2});
    await call('Input.insertText',{text:value}); await settle();
  };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await new Promise(r=>setTimeout(r,500)); await idle(); };
  const normalized = value => JSON.parse(JSON.stringify(value,(key,v)=>key==='timestamp_ms'? 'date':v));
  const directory=process.env.LAYER_TEST_ARTIFACTS || '/tmp/capy-workspace-evidence/web'; await mkdir(directory,{recursive:true});
  const shot=async name=>{const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(s.data,'base64'));};
  await evaluate(`window.workspaceTestEvents=[];for(const type of ['pointerdown','pointerup','click'])window.addEventListener(type,e=>{workspaceTestEvents.push({type,target:e.target.outerHTML.slice(0,150),x:e.clientX,y:e.clientY,prevented:e.defaultPrevented,active:document.activeElement?.outerHTML.slice(0,80)});if(workspaceTestEvents.length>40)workspaceTestEvents.shift();},{capture:true});`);
  await idle(); const original = (await view()).id, originalCapture = await capture(); let created;
  try {
    console.log('Workspace UI: opening manager'); await menu('Manage Workspaces…');
    assert.equal(await evaluate('document.querySelector(".workspace-manager footer .suggested-action").disabled'),true);
    await shot('workspaces'); await click('.workspace-add');
    const name=`Web Painting ${Date.now()}`; await text('.workspace-form input',name); await shot('new-workspace');
    await click('.workspace-form .suggested-action'); await idle();
    console.log('Workspace UI: created', await view()); const candidate=(await view()).id; assert.notEqual(candidate,original); created=candidate; assert.equal((await view()).name,name);
    await send({type:'invoke',command:'eraser'});
    const beforeMove=await capture();
    await send({type:'move_panel',panel:'layers',target:{kind:'float',position:[480,220]}});
    const changed=await capture();
    assert.equal(changed.history.undo.length,beforeMove.history.undo.length+1);
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${original}"]`);
    assert.equal((await view()).id,created,'Selection only previews');
    assert.deepEqual(normalized(await capture()),normalized(changed),'Durable capture excludes preview');
    await shot('preview-original');
    // Filtering away the selected row cancels the preview and disables apply.
    await text('.workspace-filter','No matching workspace');
    await wait('JSON.parse(layerApp.app.workspace_view()).selected === null');
    assert.equal((await view()).enabled,false);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    // Search fields consume their first Escape to clear the query.
    if ((await view()).page) { await click('.workspace-manager footer button'); }
    await idle(); assert.deepEqual(normalized(await capture()),normalized(changed));
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${original}"]`);
    await click('.workspace-manager footer .suggested-action'); await idle();
    assert.equal((await view()).id,original);
    assert.deepEqual((await capture()).working,originalCapture.working,'Switch restores that workspace’s tool settings');
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${created}"]`); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual((await capture()).working,changed.working);
    await menu('Layout History…'); assert.equal((await view()).enabled,false); await shot('history');
    await click('.workspace-choice[data-id="r0"]'); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual((await capture()).working,changed.working,'History restores layout and preserves tools');
    await send({type:'invoke',command:'undo_workspace'});
    const beforeRestart=await capture();
    await reload(); await wait('window.layerApp?.startupTimes.complete != null'); await idle();
    assert.equal((await view()).id,created);
    assert.deepEqual(normalized(await capture()),normalized(beforeRestart),'Restart retains undo/redo and working settings');
    await menu('Manage Workspaces…');
    await click(`.workspace-choice[data-id="${created}"] + .workspace-options summary`);
    await click(`.workspace-choice[data-id="${created}"] + .workspace-options button`);
    await text('.workspace-form input','Web Inking Acceptance'); await click('.workspace-form .suggested-action'); await idle();
    assert.equal((await view()).name,'Web Inking Acceptance'); await click('.workspace-manager footer button');
    // Header switches address stable identities and preserve their arrangements.
    for (const row of (await view()).defaults) {
      await click(`.workspace-switcher button[data-workspace-id="${row.id}"]`); await idle();
      assert.equal((await view()).id,row.id);
      assert.equal(await evaluate(`document.querySelector('[data-workspace-id="${row.id}"]').getAttribute('aria-pressed')`),'true');
    }
    await menu('Manage Workspaces…');
    for (const row of (await view()).rows.filter(r=>r.id.startsWith('builtin:workspace:'))) { assert.equal(row.delete,false); assert.equal(row.options,true); }
    await click(`.workspace-choice[data-id="${created}"]`); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual(normalized(await capture()),normalized(beforeRestart));
    await send({type:'invoke',command:'brush'}); await send({type:'set_brush_size',value:73});
    await send({type:'invoke',command:'eraser'}); await send({type:'set_brush_size',value:91});
    const beforeReset=await capture();
    await menu('Reset All Brushes…'); await click('.workspace-form footer button'); await idle();
    assert.deepEqual(normalized(await capture()),normalized(beforeReset),'Cancelling brush reset preserves settings');
    await menu('Reset All Brushes…'); await shot('reset-brushes'); await click('.workspace-form .suggested-action'); await idle();
    const expectedReset=structuredClone(beforeReset); expectedReset.working.tools.overrides={};
    assert.deepEqual(normalized(await capture()),normalized(expectedReset),'Brush reset preserves selection, colors and layout history');
    await menu('Restore Starting Layout…'); await click('.workspace-form .suggested-action'); await idle();
    assert.deepEqual((await capture()).working,expectedReset.working);
    await reload(); await wait('window.layerApp?.startupTimes.complete != null'); await idle();
    assert.equal((await view()).id,created); assert.deepEqual((await capture()).working,expectedReset.working);
    console.log('PASS: real Web menu/dialog input, New, Rename, preview/filter/cancel, explicit switch, default pill, tools, history, undo, restart, starting layout and brush reset');
  } catch (error) { console.error("Workspace acceptance failed", error, await view()); await writeFile(`${directory}/input-events.json`,JSON.stringify(await evaluate('workspaceTestEvents'),null,2)); await shot("failure"); throw error; } finally {
    await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"cancel"}));null');
    await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"cancel"}));null');
    if (created) {
      await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:${JSON.stringify(original)}}));null`); await idle();
      await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'form',kind:'delete',id:${JSON.stringify(created)}}));null`);
      await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"submit",name:"",source:null}));null'); await idle();
    }
    assert.equal((await view()).id,original);
    assert.deepEqual(normalized(await capture()),normalized(originalCapture),'Original workspace is preserved');
  }
}
