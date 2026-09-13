import assert from 'node:assert/strict';

// Full labels, the retained compact menu, and whole-item overflow share menus.
export async function checkHeaderControls({call,evaluate,settle}) {
  const click=async selector=>{const p=await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)}),r=e.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const select=async(label,container)=>{await evaluate(`(()=>{const b=[...document.querySelectorAll(${JSON.stringify(container+' button')})].find(b=>(b.querySelector('.menu-label')?.textContent||b.textContent)===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.dataset.headerTest='true';})()`);await click('[data-header-test]');};
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:870,deviceScaleFactor:1,mobile:false});await settle();};
  await resize(1440);
  const menus=await evaluate('layerApp.app.editor_models(0,0).application_menus');assert.equal(menus.length,8);
  for(const m of menus){await click(`[data-menu="${m.id}"] > summary`);assert.equal(await evaluate(`document.querySelector('[data-menu="${m.id}"]').open`),true);}
  for(const [width,selector] of [[600,'.header-menu-labels-compact'],[240,'#header-overflow-0']]) {
    await resize(width);
    assert.equal(await evaluate("document.querySelectorAll('.header-item[hidden] details[open],.header-menu-labels[hidden] details[open]').length"),0,'Resizing closes hidden menus');
    assert.ok(await evaluate(`document.activeElement.closest('${selector}')!==null`),'Focus follows the visible menu');
    await click(`${selector} > summary`);
    if(width===240)await select('Menu Labels',`${selector} .popover`);
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('${selector} .menu-label')].map(n=>n.textContent)`),menus.map(m=>m.label));
    for(const m of menus){await select(m.label,`${selector} .popover`);assert.deepEqual(await evaluate(`[...document.querySelectorAll('${selector} .menu-label')].map(n=>n.textContent)`),m.model.sections.flat().map(i=>i.label));await click(`${selector} .submenu-back`);}
    const zoom=await evaluate('layerApp.state().camera.zoom');await select('View',`${selector} .popover`);
    await select(menus.find(m=>m.id==='view').model.sections.flat().find(i=>i.action?.command==='zoom_in').label,`${selector} .popover`);
    assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom);assert.equal(await evaluate(`document.querySelector('${selector}').open`),false);
    await click(`${selector} > summary`);await resize(1440);
    assert.equal(await evaluate("document.querySelectorAll('.header-overflow[open],.header-menu-labels-compact[open]').length"),0);
    assert.ok(await evaluate(`document.activeElement.closest('${width===600?'.header-menu-labels':'.header-item:not([hidden])'}')!==null`),'Expansion restores a visible control');
  }
  console.log('PASS: all eight full/compact/overflow menus, real Zoom In, resize closure and focus restoration');
}
