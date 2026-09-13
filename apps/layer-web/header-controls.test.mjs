import assert from 'node:assert/strict';

// Both full menu labels and whole-item overflow project the shared menu tree.
export async function checkHeaderControls({call,evaluate,settle}) {
  const click=async selector=>{const p=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)}),r=e.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const select=async(label,container)=>{await evaluate(`(()=>{const b=[...document.querySelectorAll(${JSON.stringify(container+' button')})].find(b=>(b.querySelector('.menu-label')?.textContent||b.textContent)===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.dataset.headerTest='true';})()`);await click('[data-header-test]');};
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:870,deviceScaleFactor:1,mobile:false});await settle();};
  await resize(1440);
  const menus=await evaluate('layerApp.app.editor_models(0,0).application_menus');assert.equal(menus.length,8);
  for(const m of menus){await click(`[data-menu="${m.id}"] > summary`);assert.equal(await evaluate(`document.querySelector('[data-menu="${m.id}"]').open`),true);}
  await resize(600);assert.equal(await evaluate("document.querySelectorAll('.header-item[hidden] details[open]').length"),0,'Resizing closes hidden menus');
  assert.ok(await evaluate("document.activeElement.closest('.header-overflow')!==null"),'Focus follows overflow');
  await click('#header-overflow-0 > summary');await select('Menu Labels','#header-overflow-0 .popover');
  assert.deepEqual(await evaluate("[...document.querySelectorAll('#header-overflow-0 .menu-label')].map(n=>n.textContent)"),menus.map(m=>m.label));
  for(const m of menus){await select(m.label,'#header-overflow-0 .popover');assert.deepEqual(await evaluate("[...document.querySelectorAll('#header-overflow-0 .menu-label')].map(n=>n.textContent)"),m.model.sections.flat().map(i=>i.label));await click('#header-overflow-0 .submenu-back');}
  const zoom=await evaluate('layerApp.state().camera.zoom');await select('View','#header-overflow-0 .popover');
  await select(menus.find(m=>m.id==='view').model.sections.flat().find(i=>i.action?.command==='zoom_in').label,'#header-overflow-0 .popover');
  assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom);assert.equal(await evaluate("document.querySelector('#header-overflow-0').open"),false);
  await click('#header-overflow-0 > summary');await resize(1440);
  assert.equal(await evaluate("document.querySelectorAll('.header-overflow[open]').length"),0);
  console.log('PASS: all eight full/overflow menus, real Zoom In, resize closure and focus restoration');
}
