import assert from 'node:assert/strict';

// Exercise the actual in-app menus after the workspace pill and clock consume
// their space. Coordinates come from the live DOM; no OS menu input is involved.
export async function checkHeaderControls({call, evaluate, settle}) {
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){if(${expression})resolve(true);
      else if(performance.now()-start>10000)reject(Error('Header timeout: '+${JSON.stringify(expression)}));else setTimeout(check,20);
    }check();
  })`);
  const click = async selector => {
    const point = await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)}),r=e.getBoundingClientRect();
      if(!r.width||getComputedStyle(e).visibility==='hidden')throw Error('Hidden menu target');return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    for (const type of ['mousePressed','mouseReleased'])
      await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const select = async label => {
    await evaluate(`(()=>{const e=[...document.querySelectorAll('.header-menu-overflow .popover button')]
      .find(e=>e.querySelector('.menu-label')?.textContent===${JSON.stringify(label)});
      if(!e)throw Error('Missing menu action');e.dataset.headerTarget='true';})()`);
    await click('[data-header-target="true"]');
  };
  const clock = async value => {
    await evaluate(`layerApp.dispatch({type:'preferences',action:{type:'edit',id:'show_clock',value:${value}}})`);
    await settle();
  };
  const resize = async width => {
    await call('Emulation.setDeviceMetricsOverride',{width,height:870,deviceScaleFactor:1,mobile:false});
    await settle();
  };
  const collapsed = value => wait(`document.querySelector('.header-menu-labels').inert===${value}
    && document.querySelector('.header-menu-overflow').hidden===${!value}`);
  const menus = await evaluate('layerApp.app.editor_models(innerWidth,innerHeight).application_menus');
  assert.equal(menus.length,8);
  await resize(744); await clock(2); await collapsed(false);
  await clock(1); await collapsed(true);
  const pill = await evaluate(`(()=>{const p=document.querySelector('.workspace-switcher'),r=p.getBoundingClientRect(),
    start=document.querySelector('#header-start').getBoundingClientRect(),end=document.querySelector('#header-end').getBoundingClientRect();
    return{height:r.height,buttons:[...p.children].map(e=>e.getBoundingClientRect().height),overlap:start.right>r.left||r.right>end.left};})()`);
  assert.equal(pill.height,34); assert.deepEqual(pill.buttons,[26,26,26]); assert.equal(pill.overlap,false);
  await click('.header-menu-overflow > summary');
  await wait("document.querySelectorAll('.header-menu-overflow .menu-label').length===8");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.header-menu-overflow .menu-label')].map(e=>e.textContent)"),menus.map(m=>m.label));
  for (const menu of menus) {
    await select(menu.label);
    assert.deepEqual(await evaluate("[...document.querySelectorAll('.header-menu-overflow .menu-label')].map(e=>e.textContent)"),
      menu.model.sections.flat().map(item=>item.label));
    await click('.header-menu-overflow .submenu-back');
  }
  const zoom = await evaluate('layerApp.state().camera.zoom');
  const view = menus.find(menu=>menu.id==='view');
  await select(view.label);
  await select(view.model.sections.flat().find(item=>item.action?.command==='zoom_in').label);
  await wait(`layerApp.state().camera.zoom>${zoom}`);
  assert.equal(await evaluate("document.querySelector('.header-menu-overflow').open"),false);
  await click('.header-menu-overflow > summary');
  await resize(1200); await collapsed(false);
  assert.equal(await evaluate("document.querySelectorAll('.header-menu[open]').length"),0,'Resize closes an obsolete popup');
  assert.equal(await evaluate("document.activeElement===document.querySelector('.header-menu-labels summary')"),true,'Focus follows the visible menus');
  await click('[data-menu="view"] > summary');
  assert.equal(await evaluate("document.querySelector('[data-menu=view]').open"),true);
  await resize(744); await collapsed(true);
  assert.equal(await evaluate("document.querySelectorAll('.header-menu[open]').length"),0);
  assert.equal(await evaluate("document.activeElement===document.querySelector('.header-menu-overflow summary')"),true);
  await clock(2); await collapsed(false);
  console.log('PASS: compact workspace/clock geometry, all eight overflow menus, real Zoom In action, and menu closure/restoration on resize');
}
