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
  const input=async(run,value)=>{await run(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`);await new Promise(r=>setTimeout(r,200));};
  await wait(evaluate,ready);
  const original=(await view(evaluate)).id;
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
    await evaluate('layerApp.dispatch({type:"set_brush_size",value:53});null');
    await other('layerApp.dispatch({type:"set_brush_size",value:87});null');
    await new Promise(r=>setTimeout(r,700)); await wait(evaluate,ready); await wait(other,ready);
    const ownCapture=await capture(evaluate), otherCapture=await capture(other), otherId=(await view(other)).id;
    assert.notDeepEqual(ownCapture.working,otherCapture.working);
    await call('Page.reload',{},sessionId); await new Promise(r=>setTimeout(r,1000)); await wait(other,ready);
    assert.equal((await view(other)).id,otherId);
    assert.deepEqual((await capture(other)).working,otherCapture.working);
    await input(evaluate,{type:'suspend'}); await wait(evaluate,'!JSON.parse(layerApp.app.workspace_view()).busy');
    await input(other,{type:'switch',id:original}); await wait(other,ready);
    await other('layerApp.dispatch({type:"set_brush_size",value:33});null'); await new Promise(r=>setTimeout(r,700)); await wait(other,ready);
    await input(evaluate,{type:'resume'});
    await wait(evaluate,'!!JSON.parse(layerApp.app.workspace_view()).error');
    assert.deepEqual((await capture(evaluate)).working,ownCapture.working,'Ownership loss preserves outgoing edits');
    await wait(evaluate,`[...document.querySelectorAll('dialog')].some(d=>d.getAttribute('aria-label')==='Workspace could not be saved'&&d.open)`);
    await evaluate('[...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Save as New Workspace…").click()');
    await wait(evaluate,'JSON.parse(layerApp.app.workspace_view()).form?.kind === "recover"');
    await evaluate('document.querySelector(".workspace-form[open] input").value="Recovered browser window";document.querySelector(".workspace-form[open] .suggested-action").click()');
    await wait(evaluate,ready);
    assert.notEqual((await view(evaluate)).id,original);
    assert.deepEqual((await capture(evaluate)).working,ownCapture.working);
    assert.notDeepEqual((await capture(other)).working,ownCapture.working,'Recovery never overwrites the new owner');
    console.log('PASS: two browser windows, independent autosaves, owner identity on reload, suspension/takeover, stale-owner protection and Save as New recovery');
  } finally { await call('Target.closeTarget',{targetId:target.targetId},null); }
}
