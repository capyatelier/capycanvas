import assert from 'node:assert/strict';

// Two real browser windows share IndexedDB but have independent document and
// workspace owners. This also exercises the recovery UI after a suspended owner
// loses its claim, without changing any system clock or production database.
export async function checkWorkspaceWindows({call,evaluate}) {
  const wait = async (run, predicate) => {
    for (let i=0;i<200;i++) { if (await run(predicate)) return; await new Promise(r=>setTimeout(r,100)); }
    throw Error(`Workspace window timed out: ${await run('layerApp.app.workspace_view()')}`);
  };
  const ready='window.layerApp?.startupTimes.complete != null && JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty';
  const view=run=>run('JSON.parse(layerApp.app.workspace_view())');
  const capture=run=>run('JSON.parse(layerApp.app.workspace_capture())');
  const checkpoint=run=>run('(async()=>{await layerApp.documents.startRecovery();await layerApp.documents.autosave(true)})()');
  const input=async(run,value)=>{await run(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`);await new Promise(r=>setTimeout(r,200));};
  await wait(evaluate,ready);
  const original=(await view(evaluate)).id,href=await evaluate('location.href'),workspaces=(await view(evaluate)).order.length;
  const navigating=async expression=>{try{return await evaluate(expression);}catch{return false;}};
  for(let i=0;i<3;i++){
    await checkpoint(evaluate);
    await call('Page.navigate',{url:'about:blank'});await wait(navigating,"location.href==='about:blank' && document.readyState==='complete'");
    await call('Page.navigate',{url:href});
    await wait(navigating,ready);
  }
  assert.equal((await view(evaluate)).id,original,'A document that navigated away releases its workspace without waiting for the lease');
  assert.equal((await view(evaluate)).order.length,workspaces,'Returning to the app does not create a workspace copy');
  await checkpoint(evaluate);
  const target=await call('Target.createTarget',{url:'about:blank',newWindow:true,width:1100,height:800},null);
  const {sessionId}=await call('Target.attachToTarget',{targetId:target.targetId,flatten:true},null);
  const other=async expression=>{
    const result=await call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},sessionId);
    if(result.exceptionDetails)throw Error(result.exceptionDetails.exception?.description||result.exceptionDetails.text);
    return result.result.value;
  };
  try {
    await call('Runtime.enable',{},sessionId); await call('Page.enable',{},sessionId);
    await call('Page.navigate',{url:await evaluate('location.href')},sessionId); await wait(other,ready);
    assert.notEqual((await view(other)).id,original,'Live windows start with independent workspaces');
    const defaults=['builtin:workspace:painter','builtin:workspace:illustrator','builtin:workspace:photographer'];
    assert.ok(defaults.includes(original));
    assert.ok(defaults.includes((await view(other)).id),'A second window reuses an available default');
    assert.deepEqual([...(await view(other)).order].sort(),[...defaults].sort(),'Opening a second window does not create a workspace');
    await other('layerApp.dispatch({type:"set_brush_size",value:91});null');
    await wait(other,ready);
    await evaluate('layerApp.dispatch({type:"set_color",rgba:[0.4,0.2,0.7,1]});null');
    await wait(evaluate,ready);
    const colour=await evaluate('layerApp.state().colors.foreground');
    await evaluate('layerApp.dispatch({type:"color",action:{op:"library",action:{op:"create_palette",name:"Window A palette"}}});layerApp.dispatch({type:"select_brush",id:5});layerApp.dispatch({type:"set_brush_size",value:53});null');
    await wait(evaluate,ready);
    await other('layerApp.dispatch({type:"select_brush",id:7});layerApp.dispatch({type:"set_brush_size",value:77});null');
    await wait(other,'layerApp.app.brush_ready()');
    const point=await other('(()=>{const r=layerApp.canvas.getBoundingClientRect();return{x:r.x+r.width*.5,y:r.y+r.height*.5}})()');
    for(const type of ['mousePressed','mouseReleased']) await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1},sessionId);
    await wait(other,'layerApp.state().color_library.history.length>0');
    await wait(other,ready);
    const history=await other('JSON.stringify(layerApp.state().color_library.history)');
    await evaluate('layerApp.dispatch({type:"invoke",command:"zen_mode"});null');
    await new Promise(r=>setTimeout(r,700)); await wait(evaluate,ready); await wait(other,ready);
    const ownCapture=await capture(evaluate), otherCapture=await capture(other), otherId=(await view(other)).id;
    assert.notDeepEqual(ownCapture.working,otherCapture.working);
    await checkpoint(other);
    await call('Page.reload',{},sessionId); await new Promise(r=>setTimeout(r,1000)); await wait(other,ready);
    assert.equal(await other('layerApp.state().brush.diameter'),77);
    assert.equal(await other('JSON.stringify(layerApp.state().color_library.history)'),history);
    assert.ok(await other('layerApp.state().color_library.palettes.some(p=>p.name==="Window A palette")'),'Painting keeps another window\'s palettes after restart');
    for(const [id,size] of [[2,91],[5,53],[7,77]]) {
      await other(`layerApp.dispatch({type:"select_brush",id:${id}});null`);
      assert.equal(await other('layerApp.state().brush.diameter'),size,`Brush ${id} keeps its independent override`);
    }
    assert.deepEqual(await other('layerApp.state().colors.foreground'),colour);
    assert.equal((await view(other)).id,otherId);
    assert.deepEqual([...(await view(other)).order].sort(),[...defaults].sort(),'Reloading does not create a workspace');
    assert.deepEqual((await capture(other)).working,otherCapture.working);
    await input(evaluate,{type:'suspend'}); await wait(evaluate,'!JSON.parse(layerApp.app.workspace_view()).busy');
    await input(other,{type:'switch',id:original}); await wait(other,ready);
    await other('layerApp.dispatch({type:"invoke",command:"zen_mode"});null'); await new Promise(r=>setTimeout(r,700)); await wait(other,ready);
    await input(evaluate,{type:'resume'});
    await wait(evaluate,'!!JSON.parse(layerApp.app.workspace_view()).error');
    assert.deepEqual((await capture(evaluate)).working,ownCapture.working,'Ownership loss preserves outgoing edits');
    await wait(evaluate,`[...document.querySelectorAll('dialog.workspace-form[open]')].some(dialog=>!dialog.querySelector('input')&&dialog.textContent.includes(JSON.parse(layerApp.app.workspace_view()).error))`);
    await evaluate('document.querySelector("dialog.workspace-form[open] .suggested-action").click()');
    await wait(evaluate,'JSON.parse(layerApp.app.workspace_view()).prompt_action?.type === "save_as_new"');
    await wait(evaluate,'!!document.querySelector(".workspace-form[open] input") && document.querySelector(".workspace-form[open] .suggested-action")?.disabled === false');
    await evaluate('document.querySelector(".workspace-form[open] input").value="Recovered browser window";document.querySelector(".workspace-form[open] .suggested-action").click()');
    await wait(evaluate,`${ready} && JSON.parse(layerApp.app.workspace_view()).name === "Recovered browser window"`);
    assert.notEqual((await view(evaluate)).id,original);
    assert.deepEqual((await capture(evaluate)).working,ownCapture.working);
    assert.notDeepEqual((await capture(other)).working,ownCapture.working,'Recovery never overwrites the new owner');
    console.log('PASS: two browser windows, released ownership after navigation, independent autosaves, owner identity on reload, suspension/takeover, stale-owner protection and Save as New recovery');
  } finally { await call('Target.closeTarget',{targetId:target.targetId},null); }
}
