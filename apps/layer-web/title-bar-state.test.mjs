import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';

export async function checkTitleBarState({call,evaluate,settle,reload,windowId}) {
  const dir=resolve(process.env.LAYER_TEST_ARTIFACTS||'artifacts/title-bar/state');await mkdir(dir,{recursive:true});
  const results=[];
  const pause=ms=>new Promise(r=>setTimeout(r,ms));
  const wait=async expression=>{for(let i=0;i<400;i++){if(await evaluate(expression))return;await pause(50);}throw Error(`Title bar timeout: ${expression}`);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const rect=selector=>evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing '+${JSON.stringify(selector)});const r=e.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const input=async(type,p)=>call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest',inline:'nearest'})`);const r=await rect(selector),p={x:r.x+r.width/2,y:r.y+r.height/2};await input('mousePressed',p);await input('mouseReleased',p);await settle();};
  const key=async(key,code=key,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,modifiers,windowsVirtualKeyCode:({Backspace:8,Delete:46,ArrowLeft:37,ArrowRight:39,Escape:27,F10:121,s:83})[key]||0});await settle();};
  const shot=async name=>{await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:innerWidthSafe-1,y:2,buttons:0});await settle();const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${name}.png`,Buffer.from(s.data,'base64'));return s;};
  let innerWidthSafe=1440;
  const resize=async(width,height=1000,scale=1)=>{innerWidthSafe=width;await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:scale,mobile:false});await settle();await pause(100);};
  const header=()=>evaluate('layerApp.state().workspace.layout.header');
  const capture=()=>evaluate('JSON.parse(layerApp.app.workspace_capture()).history');
  const idle=async()=>{await pause(200);await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy&&!JSON.parse(layerApp.app.workspace_view()).dirty');};
  const begin=async()=>{
    await call('Input.dispatchMouseEvent',{type:'mousePressed',x:720,y:2,button:'right',buttons:2,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:720,y:2,button:'right',buttons:0,clickCount:1});
    await wait("!!document.querySelector('.panel-context-menu:popover-open')");
    await evaluate("[...document.querySelectorAll('.panel-context-menu button')].find(b=>b.textContent.includes('Customize Title Bar…')).dataset.stateEdit='true'");
    await click('[data-state-edit]');await wait('layerApp.state().customization.header_editing');
  };
  const drop=async(kind,x,y)=>{const r=await rect(`#header-component-${kind}`),p={x:r.x+r.width/2,y:r.y+r.height/2};await input('mousePressed',p);await input('mouseMoved',{x,y});await settle();await input('mouseReleased',{x,y});await settle();};
  const original=await header(),originalInfo=await evaluate('layerApp.state().workspace.layout.canvas_info');
  await wait('layerApp.startupTimes.complete!==null');await idle();
  const originalCapture=await capture();
  await begin();await click('[data-header-size="large"]');await click('#header-canvas-info');
  assert.deepEqual(await capture(),originalCapture,'Capture during customization uses the committed baseline');
  assert.equal(await evaluate('document.querySelector("#canvas-status").hidden'),originalInfo.visible);
  // Explicit project save uses the real browser download fallback and controls.
  await call('Browser.setDownloadBehavior',{behavior:'allow',downloadPath:dir},null);
  await evaluate('window.showSaveFilePicker=undefined');await click('.header-item:not([hidden])');await key('s','KeyS',2);
  await wait("!![...document.querySelectorAll('dialog[open]')].find(d=>d.textContent.includes('Download file'))");
  for(const label of ['Download','File saved']) {
    await evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});b.dataset.stateDownload='true';})()`);
    await click('[data-state-download]');await evaluate("document.querySelector('[data-state-download]')?.removeAttribute('data-state-download')");
  }
  await wait("document.querySelectorAll('dialog[open]').length===0");
  assert.deepEqual(await capture(),originalCapture,'Save cannot publish title preview');
  await click('#header-edit-cancel');assert.deepEqual(await header(),original);assert.deepEqual(await evaluate('layerApp.state().workspace.layout.canvas_info'),originalInfo);
  await begin();await click('[data-header-size="large"]');await click('#header-canvas-info');
  const accepted=await header();await click('#header-edit-done');await idle();
  const acceptedCapture=await capture();assert.notDeepEqual(acceptedCapture,originalCapture);
  await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await header(),original);
  await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await header(),accepted);
  await idle();await reload();await wait('window.layerApp?.startupTimes.complete!=null');await idle();assert.deepEqual(await header(),accepted);
  await begin();await click('[data-header-size="small"]');await idle();
  await reload();await wait('window.layerApp?.startupTimes.complete!=null');await idle();
  assert.deepEqual(await header(),accepted,'Reload discards temporary header');
  assert.equal(await evaluate('layerApp.state().customization.header_editing'),false);
  // Browser workspace switching while the title-bar selector is inert uses the
  // existing manager (as keyboard/application-menu switching does on GTK).
  const active=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  const otherWorkspace=await evaluate('JSON.parse(layerApp.app.workspace_view()).switcher_display.find(w=>w.id!==JSON.parse(layerApp.app.workspace_view()).id).id');
  await begin();await click('[data-header-size="small"]');
  await send({type:'workspace_manager',command:{type:'manage'}});
  await wait("document.querySelector('.workspace-manager').open");
  await click(`.workspace-choice[data-id="${otherWorkspace}"]`);await click('.workspace-manager footer .suggested-action');await idle();
  await click(`.workspace-switcher button[data-workspace-id="${active}"]`);await idle();assert.deepEqual(await header(),accepted,'Switch discards preview');
  results.push('Project Save via browser download, committed captures, Cancel, Done, one-step workspace Undo/Redo, autosave/reload and manager switching during customization');
  console.log('PASS: title bar Done/Cancel/save/switch/reload');

  // Both themes, all native item sizes and real backing-store scale two.
  await send({type:'restore_workspace',workspace:{...await evaluate('layerApp.state().workspace'),layout:{...await evaluate('layerApp.state().workspace.layout'),header:original,canvas_info:originalInfo}}});
  await begin();
  for(const theme of ['dark','light'])for(const size of ['small','medium','large'])for(const scale of [1,2]) {
    await resize(1440,1000,scale);await send({type:'set_theme',theme});await click(`[data-header-size="${size}"]`);
    const spec=await evaluate('layerApp.app.header_view().sizes.find(s=>s.id===layerApp.state().workspace.layout.header.size)');
    assert.equal(await evaluate('devicePixelRatio'),scale);
    const items=await evaluate("[...document.querySelectorAll('#header .header-item:not([hidden])')].map(n=>({r:n.getBoundingClientRect().toJSON(),grip:n.querySelector('.header-item-grip').getBoundingClientRect().toJSON()}))");
    for(const item of items) {
      assert.equal(item.r.height,spec.tile);assert.equal(item.r.y,6);
      assert.ok(Math.abs(item.grip.y+item.grip.height/2-item.r.y-item.r.height/2)<.1,'Grips vertically centered');
    }
    const s=await shot(`${theme}-${size}-${scale}x`);
    const width=Buffer.from(s.data,'base64').readUInt32BE(16);assert.equal(width,1440*scale,'Capture has physical scale, not a CSS transform');
  }
  await resize(1440);await click('#header-edit-cancel');
  await begin();
  const centerIds=(await header()).zones[1].map(e=>e.id);
  for(const id of centerIds){await click(`[data-header-item="${id}"]`);await key('Delete');}
  const centerZone=await rect('[data-zone="center"]');assert.ok(centerZone.width>=240,'Empty center is an easy drop target');
  await drop('space',centerZone.x+centerZone.width/2,24);assert.equal((await header()).zones[1][0].item.kind,'space');
  await click('[data-header-size="large"]');
  for(const width of [744,480,360]) {
    await resize(width,640);
    assert.ok(await evaluate("document.querySelector('#header-editor').scrollWidth<=document.querySelector('#header-editor').clientWidth+1"),'Small editor fits horizontally');
    assert.ok(await evaluate("[...document.querySelectorAll('.header-item:not([hidden]),.header-overflow:not([hidden])')].every(n=>{const r=n.getBoundingClientRect();return r.x>=0&&r.right<=innerWidth+.1})"));
    const selector=await evaluate("[...document.querySelectorAll('.header-overflow:not([hidden])')].map(n=>'#'+n.id)[0]");
    assert.ok(selector);await click(`${selector} > summary`);assert.ok(await evaluate("document.querySelectorAll('.header-overflow[open] button').length>0"));
    await click(`${selector} .popover button`);await key('ArrowRight');
    await shot(`small-${width}`);
  }
  await resize(1440);await click('#header-edit-cancel');
  results.push('Both themes × Small/Medium/Large × physical 1×/2×; centered grips; 744/480/360px wrapping/overflow and keyboard access; generous empty-center drop');
  console.log('PASS: title bar themes/sizes/2x/overflow/empty center');

  // Status positions remain in the workspace while visibility follows actual
  // DOM or browser fullscreen, independent of the retired preference.
  await send({type:'set_theme',theme:'dark'});
  assert.equal(await evaluate("document.querySelector('#system-clock').hidden"),true);
  await click('#fullscreen');await wait('!!document.fullscreenElement&&layerApp.state().fullscreen');await settle();
  await wait("!document.querySelector('#system-clock').hidden&&!document.querySelector('#system-battery').hidden");
  assert.equal(await evaluate("document.querySelector('#system-battery').getAttribute('aria-label')"),'Battery 72%, charging');
  for(const theme of ['dark','light']){await send({type:'set_theme',theme});await shot(`fullscreen-${theme}`);}
  await evaluate("window.__statusBattery.level=.08;window.__statusBattery.charging=false;window.__statusBattery.dispatchEvent(new Event('levelchange'))");
  assert.equal(await evaluate("document.querySelector('#system-battery').getAttribute('aria-label')"),'Battery 8%, low');
  await click('#fullscreen');await wait('!document.fullscreenElement&&!layerApp.state().fullscreen');
  assert.deepEqual(await header(),original,'Fullscreen preserves saved status positions');
  for(const theme of ['dark','light']) {
    await send({type:'set_theme',theme});await click('#zen-button');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:720,y:500,buttons:0});await settle();
    assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"),true);
    assert.equal(await evaluate("getComputedStyle(document.querySelector('#header')).pointerEvents"),'none');
    assert.equal(await evaluate("!!document.querySelector('.zen-toolbar')"),false);
    const s=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/zen-${theme}.png`,Buffer.from(s.data,'base64'));
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:6,y:6,buttons:0});await settle();
    await click('#zen-button');assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),false);
  }
  results.push('Actual fullscreen entry/exit, retained clock/battery positions and low/charging battery; footer ownership; full Zen and edge reveal in both themes');
  console.log('PASS: title bar fullscreen/status/full Zen');

  // Close an actual second browser window while its title preview is active;
  // read the same origin's durable record from the surviving window afterward.
  await idle();
  const target=await call('Target.createTarget',{url:'about:blank',newWindow:true,width:1440,height:1000},null);
  const {sessionId}=await call('Target.attachToTarget',{targetId:target.targetId,flatten:true},null);
  const other=async expression=>{const r=await call('Runtime.evaluate',{expression,returnByValue:true,awaitPromise:true},sessionId);if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description||r.exceptionDetails.text);return r.result.value;};
  const stored=id=>evaluate(`new Promise((resolve,reject)=>{const r=indexedDB.open('capycanvas.workspaces',1);r.onerror=()=>reject(r.error);r.onsuccess=()=>{const db=r.result,q=db.transaction('workspace').objectStore('workspace').get('database');q.onsuccess=()=>{const content=JSON.parse(q.result.snapshot).items[${JSON.stringify(id)}].entity.content;db.close();resolve(content)};q.onerror=()=>reject(q.error)}})`);
  try {
    await call('Runtime.enable',{},sessionId);await call('Page.enable',{},sessionId);
    await call('Page.navigate',{url:await evaluate('location.href')},sessionId);
    for(let i=0;i<500;i++){if(await other('window.layerApp?.startupTimes.complete!=null&&JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy'))break;await pause(100);}
    const id=await other('JSON.parse(layerApp.app.workspace_view()).id');await pause(400);const before=await stored(id);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:550,y:2,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1},sessionId);
    const choose=async selector=>{const p=await other(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1},sessionId);await pause(150);};
    await other("[...document.querySelectorAll('.panel-context-menu button')].find(b=>b.textContent.includes('Customize Title Bar…')).dataset.closeEdit='true'");
    await choose('[data-close-edit]');await choose('[data-header-size="large"]');
    assert.equal(await other('layerApp.state().customization.header_editing'),true);
    await pause(400);await call('Target.closeTarget',{targetId:target.targetId},null);await pause(300);
    assert.deepEqual(await stored(id),before,'Closing the window cannot store its temporary title bar');
    results.push('Actual second browser window closed during customization; IndexedDB retained its committed layout/history');
  } finally {await call('Target.closeTarget',{targetId:target.targetId},null).catch(()=>{});}
  await call('Page.bringToFront');
  await writeFile(`${dir}/results.json`,JSON.stringify({results,limitations:['Desktop Chrome with CDP input; physical tablet/pen and mobile browser coverage remains open.']},null,2));
}
