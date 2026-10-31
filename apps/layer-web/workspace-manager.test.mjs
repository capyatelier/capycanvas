import {chooseDropdown,captureDropdowns} from './dropdown-fixture.mjs';
import assert from "node:assert/strict";
import {mkdir, writeFile} from "node:fs/promises";
import {checkTextEntry} from './control-style-fixture.mjs';

export async function checkWorkspaceManager({call, evaluate, settle, reload, touch = false}) {
  const wait = async (predicate, timeout = 15000) => {
    for (let n=0; n<Math.ceil(timeout/100); n++) { if (await evaluate(predicate)) return; await new Promise(r=>setTimeout(r,100)); }
    throw Error(`Workspace timeout: ${predicate}\n${await evaluate('layerApp.app.workspace_view()')}`);
  };
  const view = () => evaluate('JSON.parse(layerApp.app.workspace_view())');
  const editing = () => evaluate("JSON.parse(layerApp.app.editing_state())");
  const capture = () => evaluate('JSON.parse(layerApp.app.workspace_capture())');
  const idle = () => wait('!layerApp.documents.busy() && JSON.parse(layerApp.app.workspace_view()).ready && !JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
  const click = async selector => {
    await evaluate(`(()=>{const e=[...document.querySelectorAll(${JSON.stringify(selector)})].find(node=>node.checkVisibility());if(!e)throw Error('Missing '+${JSON.stringify(selector)});const scroll=e.closest('.workspace-list,.workspace-details');if(scroll){const row=e.getBoundingClientRect(),area=scroll.getBoundingClientRect();if(row.top<area.top)scroll.scrollTop+=row.top-area.top;else if(row.bottom>area.bottom)scroll.scrollTop+=row.bottom-area.bottom;}})()`);
    await settle();await wait("!document.querySelector('.document-transition')");
    const point = await evaluate(`(()=>{const node=[...document.querySelectorAll(${JSON.stringify(selector)})].find(node=>node.checkVisibility()),r=node.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2,hit=document.elementFromPoint(x,y);if(!node.contains(hit))throw Error('Obstructed '+${JSON.stringify(selector)}+': '+hit?.outerHTML);return{x,y}})()`);
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
    await click(selector);
    // Select the input contents directly, then use the browser's text insertion.
    // This avoids OS shortcut routing in headless Mac Chrome (Ctrl+A is not the
    // Mac select-all binding) while retaining real input events and app handling.
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).select()`);
    await call('Input.insertText',{text:value}); await settle();
  };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await new Promise(r=>setTimeout(r,500)); await idle(); };
  const normalized = value => JSON.parse(JSON.stringify(value,(key,v)=>key==='timestamp_ms'? 'date':v));
  const directory=process.env.LAYER_TEST_ARTIFACTS || 'artifacts/workspace-evidence/web'; await mkdir(directory,{recursive:true});
  const shot=async name=>{const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(s.data,'base64'));};
  await evaluate(`window.workspaceTestEvents=[];for(const type of ['pointerdown','pointerup','click'])window.addEventListener(type,e=>{workspaceTestEvents.push({type,target:e.target.outerHTML.slice(0,150),x:e.clientX,y:e.clientY,prevented:e.defaultPrevented,active:document.activeElement?.outerHTML.slice(0,80)});if(workspaceTestEvents.length>40)workspaceTestEvents.shift();},{capture:true});`);
  await send({type:"workspace_manager",command:{type:"switch",id:"builtin:workspace:illustrator"}});
  await idle(); await send({type:"set_theme",theme:process.env.CAPY_TEST_THEME || "dark"}); const original = (await view()).id, originalCapture = await capture(), originalLayout = await evaluate('layerApp.state().workspace.layout'); let created;
  async function advancedToolbars(){
    const count=await evaluate('layerApp.state().workspace.layout.panels.length'),name=`Web reusable ${Date.now()}`,description='Reusable brush and color controls';
    await send({type:'invoke',command:'manage_toolbars'});await wait('JSON.parse(layerApp.app.workspace_view()).page==="this_workspace"');
    await checkTextEntry(evaluate,'#workspace-manager-search',{padding:12,reference:'.preferences-search',capture:theme=>shot(`toolbar-search-${theme}`)});
    const toolbar=(await view()).rows[0];assert.ok(toolbar);
    await click(`.workspace-choice[data-id=${JSON.stringify(toolbar.id)}]`);
    await wait('!!document.querySelector(".workspace-details [data-action=save_toolbar]")');
    await click('.workspace-details [data-action=save_toolbar]');await text('#workspace-item-name',' ');
    assert.equal(await evaluate('document.querySelector(".workspace-form .suggested-action").disabled'),true);
    await checkTextEntry(evaluate,'#workspace-item-name');
    await text('#workspace-item-name',name);await click('.workspace-form .suggested-action');await idle();
    await click('.workspace-manager-tabs [data-page=toolbar_library]');await wait('JSON.parse(layerApp.app.workspace_view()).page==="toolbar_library"');
    let item=(await view()).rows.find(row=>row.title===name);assert.ok(item);
    await text('#workspace-manager-search',name);await wait(`JSON.parse(layerApp.app.workspace_view()).rows.length===1`);
    await click(`.workspace-choice[data-id=${JSON.stringify(item.id)}]`);await wait('!!document.querySelector(".workspace-details [data-action=rename]")');
    await click('.workspace-details [data-action=rename]');
    await checkTextEntry(evaluate,'#workspace-item-name,#workspace-item-description',{capture:theme=>shot(`toolbar-details-${theme}`)});
    await text('#workspace-item-description',description);await click('.workspace-form .suggested-action');await idle();
    await wait(`JSON.parse(layerApp.app.workspace_view()).details?.description===${JSON.stringify(description)}`);
    await click('.workspace-details [data-action=update_toolbar]');
    const options=(await view()).prompt.choices;assert.ok(options.length);
    for(const theme of ['light','dark']){const settings=await evaluate('layerApp.state().settings');await send({type:'restore_saved_settings',saved:JSON.stringify({...settings,theme})});await captureDropdowns({call,evaluate,settle},'.workspace-form[open]',directory,`workspace-choice-${theme}`);}
    const choice=options.at(-1).id;
    await evaluate(`(()=>{const field=document.querySelector('#workspace-item-choice');(${chooseDropdown})(field,${JSON.stringify(choice)});})()`);
    await click('.workspace-form .suggested-action');await idle();
    await click('.workspace-manager-tabs [data-page=this_workspace]');await click('.workspace-add');
    await wait('!!document.querySelector("#workspace-item-choice")');
    assert.ok((await view()).prompt.choices.some(value=>value.id===item.id));
    await evaluate(`(()=>{const field=document.querySelector('#workspace-item-choice');(${chooseDropdown})(field,${JSON.stringify(item.id)});})()`);
    await text('#workspace-item-name','Applied reusable toolbar');await click('.workspace-form .suggested-action');await idle();
    assert.equal(await evaluate('layerApp.state().workspace.layout.panels.length'),count+1);
    if((await view()).page)await click('.workspace-manager .dialog-close');await send({type:'invoke',command:'undo_workspace'});
    assert.equal(await evaluate('layerApp.state().workspace.layout.panels.length'),count);
    await send({type:'invoke',command:'manage_toolbars'});await click('.workspace-manager-tabs [data-page=toolbar_library]');
    await text('#workspace-manager-search',name);await click(`.workspace-choice[data-id=${JSON.stringify(item.id)}]`);await wait('!!document.querySelector(".workspace-details [data-action=delete]")');
    await click('.workspace-details [data-action=delete]');await click('.workspace-form .suggested-action');await idle();
    assert.equal((await view()).rows.some(row=>row.id===item.id),false);await click('.workspace-manager .dialog-close');
  }
  try {
    console.log('Workspace UI: opening manager'); await menu('Manage Workspaces…');
    assert.equal(await evaluate('[...document.querySelectorAll(".workspace-manager input[type=search]")].every(n=>n.hidden||!n.getClientRects().length)'),true);
    assert.equal(await evaluate('document.querySelector(".workspace-manager footer .suggested-action").disabled'),true);
    await shot('workspaces'); await click('.workspace-row-actions .workspace-add');
    const name=`Web Painting ${Date.now()}`; await text('.workspace-form input',name); await shot('new-workspace');
    await click('.workspace-form .suggested-action'); await idle();
    console.log('Workspace UI: created', await view()); const candidate=(await view()).id; assert.notEqual(candidate,original); created=candidate; assert.equal((await view()).name,name);assert.equal((await view()).page,'workspaces');assert.equal((await view()).selected,candidate);await click('.workspace-manager footer button');
    await advancedToolbars();
    await send({type:'invoke',command:'eraser'});
    await send({type:'set_brush_size',value:91});
    await send({type:'set_color',rgba:[0.2,0.3,0.4,1]});
    await send({type:'color',action:{op:'library',action:{op:'create_palette',name:'Shared colors'}}});
    await send({type:'color',action:{op:'library',action:{op:'store',palette:await evaluate('layerApp.state().color_library.active'),name:'Shared ink',color:await evaluate('layerApp.state().colors.foreground')}}});
    const sharedEditing=await editing();
    const beforeMove=await capture();
    await send({type:'move_panel',panel:'layers',target:{kind:'float',position:[480,220]}});
    const changed=await capture();
    assert.equal(changed.history.undo.length,beforeMove.history.undo.length+1);
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${original}"]`);
    assert.equal((await view()).id,created,'Selection only previews');
    assert.deepEqual(normalized(await capture()),normalized(changed),'Durable capture excludes preview');
    await shot('preview-original');
    assert.equal((await view()).enabled,true);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    assert.equal(await evaluate('document.querySelector(".workspace-manager").open'),false,'One Escape cancels the preview and closes the manager');
    await idle(); assert.deepEqual(normalized(await capture()),normalized(changed));
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${original}"]`);
    await click('.workspace-manager footer .suggested-action'); await idle();
    assert.equal((await view()).id,original);
    assert.deepEqual(await editing(),sharedEditing,"Switch retains brush, paints and palettes");
    assert.deepEqual((await capture()).working,originalCapture.working,'Switch restores workspace presentation');
    await menu('Manage Workspaces…'); await click(`.workspace-choice[data-id="${created}"]`); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual((await capture()).working,changed.working);
    await menu('Layout History…'); assert.equal((await view()).enabled,false); await shot('history');
    assert.equal(await evaluate('[...document.querySelectorAll(".workspace-manager input[type=search]")].every(n=>n.hidden||!n.getClientRects().length)'),true);
    const currentVersion=(await capture()).history.current;
    assert.equal(await evaluate('document.querySelector(\'.workspace-choice[data-id="r0"] .workspace-row-title\').textContent'),'Starting layout');
    assert.match(await evaluate(`document.querySelector('.workspace-choice[data-id="${currentVersion}"] .workspace-row-subtitle').textContent`),/^Current layout · /);
    await click('.workspace-choice[data-id="r0"]'); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual((await capture()).working,changed.working,'History restores layout and preserves tools');
    await send({type:'invoke',command:'undo_workspace'});
    const beforeRestart=await capture();
    await reload(); await wait('window.layerApp?.startupTimes.complete != null', 55000); await idle();
    assert.equal((await view()).id,created);
    assert.deepEqual(await editing(),sharedEditing,"Restart retains shared editing");
    assert.deepEqual(normalized(await capture()),normalized(beforeRestart),'Restart retains undo/redo and working settings');
    await menu('Manage Workspaces…');
    const renamePoint=await evaluate(`(()=>{const r=document.querySelector('.workspace-row[data-id="${created}"] .workspace-row-title').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const count of [1,2])for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...renamePoint,button:'left',buttons:type==='mousePressed'?1:0,clickCount:count});await settle();
    await text('.workspace-name-entry','Web Inking Acceptance');for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13,...(type==='keyDown'?{text:'\r',unmodifiedText:'\r'}:{})});await idle();
    assert.equal((await view()).name,'Web Inking Acceptance'); await click('.workspace-manager footer button');
    // Header switches address stable identities and preserve their arrangements.
    for (const id of ['builtin:workspace:painter','builtin:workspace:illustrator','builtin:workspace:photographer']) {
      await click(`.workspace-switcher button[data-workspace-id="${id}"]`); await idle();
      assert.equal((await view()).id,id);
      assert.deepEqual(await editing(),sharedEditing,"Default workspaces retain shared editing");
      assert.equal(await evaluate(`document.querySelector('[data-workspace-id="${id}"]').getAttribute('aria-pressed')`),'true');
    }
    await menu('Manage Workspaces…');
    for (const row of (await view()).rows.filter(r=>r.id.startsWith('builtin:workspace:'))) {
      assert.ok(!row.actions.some(button=>button.enabled&&['rename','delete'].includes(button.action.type)));
      assert.equal(await evaluate(`!!document.querySelector('.workspace-row[data-id="${row.id}"] .workspace-options')`),false);
      await click(`.workspace-row[data-id="${row.id}"] .workspace-grip`);
      assert.equal(await evaluate(`!!document.querySelector('.workspace-row-menu [data-action="rename"]')`),false);
      await click(`.workspace-row[data-id="${row.id}"] .workspace-grip`);
    }
    await click(`.workspace-choice[data-id="${created}"]`); await click('.workspace-manager footer .suggested-action'); await idle();
    assert.deepEqual(normalized(await capture()),normalized(beforeRestart));
    await send({type:'invoke',command:'brush'}); await send({type:'set_brush_size',value:73});
    await send({type:'invoke',command:'eraser'}); await send({type:'set_brush_size',value:91});
    const beforeReset=await capture();
    await menu('Reset All Brushes…'); await click('.workspace-form footer button'); await idle();
    assert.deepEqual(normalized(await capture()),normalized(beforeReset),'Cancelling brush reset preserves settings');
    await menu('Reset All Brushes…'); await shot('reset-brushes'); await click('.workspace-form .suggested-action'); await idle();
    const expectedReset=beforeReset;
    assert.notEqual((await evaluate("layerApp.state().brush")).diameter,91);
    await send({type:"invoke",command:"brush"});
    assert.notEqual((await evaluate("layerApp.state().brush")).diameter,73);
    assert.deepEqual(normalized(await capture()),normalized(expectedReset),'Brush reset preserves selection, colors and layout history');
    const beforeLayoutReset=await capture(), beforeLayoutResetView=await evaluate('layerApp.state().workspace.layout');
    assert.notDeepEqual(beforeLayoutReset.history.revisions[beforeLayoutReset.history.current].layout,originalCapture.history.revisions[originalCapture.history.current].layout);
    for(const confirm of [false,true]) {
      await menu('Restore Starting Layout…');
      assert.match(await evaluate('document.querySelector(".workspace-form[open] > p").textContent'),/arrangement shown behind this dialog.*Window → Undo Workspace/);
      assert.deepEqual(await evaluate('layerApp.state().workspace.layout'),originalLayout,'Opening Restore previews the starting layout');
      assert.deepEqual(normalized(await capture()),normalized(beforeLayoutReset),'Preview leaves durable history and tool settings untouched');
      await shot(confirm?'restore-starting-layout':'cancel-starting-layout');
      await click(confirm?'.workspace-form .suggested-action':'.workspace-form footer button');await idle();
      if(!confirm) {
        assert.deepEqual(normalized(await capture()),normalized(beforeLayoutReset));
        assert.deepEqual(await evaluate('layerApp.state().workspace.layout'),beforeLayoutResetView,'Cancel restores the layout on screen');
      }
    }
    assert.equal((await capture()).history.undo.length,beforeLayoutReset.history.undo.length+1);
    await send({type:'invoke',command:'undo_workspace'});
    assert.deepEqual(await evaluate('layerApp.state().workspace.layout'),beforeLayoutResetView,'One Undo Workspace reverses restoration');
    await send({type:'invoke',command:'redo_workspace'});
    assert.deepEqual((await capture()).working,expectedReset.working);
    await reload(); await wait('window.layerApp?.startupTimes.complete != null', 55000); await idle();
    assert.equal((await view()).id,created); assert.deepEqual((await capture()).working,expectedReset.working);
    const deleted=created, beforeDelete=await capture();
    const illustrator='builtin:workspace:illustrator';
    await menu('Manage Workspaces…');
    const rowsBeforeDelete=(await view()).rows.map(row=>row.id);
    const deleteForm=async()=>{
      await click('.workspace-delete');
      assert.equal(await evaluate('document.querySelector(".workspace-form[open] > p").textContent'),'Delete “Web Inking Acceptance”? This is permanent. This window will switch to an available default workspace.');
      assert.equal(await evaluate('document.querySelectorAll(".workspace-form[open] .dropdown").length'),0);
    };
    await deleteForm(); await click('.workspace-form footer button'); await idle();
    assert.equal((await view()).id,deleted);
    assert.deepEqual(normalized(await capture()),normalized(beforeDelete),'Cancelling deletion preserves the active workspace');
    await deleteForm(); await shot('delete-workspace');
    await click('.workspace-form .destructive-action'); await idle();
    assert.equal((await view()).id,illustrator,'Active deletion switches to the included Illustrator workspace');
    created=null;assert.equal((await view()).page,'workspaces');assert.equal((await view()).selected,illustrator);
    assert.equal(await evaluate(`document.querySelectorAll('.workspace-switcher [data-workspace-id="${deleted}"]').length`),0);
    assert.deepEqual((await view()).rows.map(row=>row.id).sort(),rowsBeforeDelete.filter(id=>id!==deleted).sort(),'Deletion removes the row without creating a replacement workspace');
    await click('.workspace-manager footer button');
    await reload(); await wait('window.layerApp?.startupTimes.complete != null', 55000); await idle();
    assert.equal((await view()).id,illustrator,'Reload retains the chosen default');
    await menu('Manage Workspaces…'); assert.ok(!(await view()).rows.some(row=>row.id===deleted),'Deleted workspace stays absent after reload');
    await click('.workspace-manager footer button');
    if(original!==illustrator){await click(`.workspace-switcher button[data-workspace-id="${original}"]`);await idle();}
    console.log('PASS: real Web menu/dialog input, search gating, reusable toolbar save/description/update/choice/apply/undo/delete, manager/history, New, Rename, preview/cancel, explicit switch, default pill, tools, history, undo, restart, starting layout, brush reset and active deletion/cancel/reload');
  } catch (error) { console.error("Workspace acceptance failed", error, await view()); await writeFile(`${directory}/input-events.json`,JSON.stringify(await evaluate('window.workspaceTestEvents || []'),null,2)); await shot("failure"); throw error; } finally {
    await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"cancel"}));null');
    await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"cancel"}));null');
    if (created) {
      await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:${JSON.stringify(original)}}));null`); await idle();
      await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'form',action:{type:'delete',value:${JSON.stringify(created)}}}));null`);
      await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"submit",name:""}));null'); await idle();
    }
    assert.equal((await view()).id,original);
    assert.deepEqual(normalized(await capture()),normalized(originalCapture),'Original workspace is preserved');
  }
}
