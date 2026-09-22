import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

export async function checkBrushDrawers({call, evaluate, settle}) {
  const pause = () => new Promise(resolve => setTimeout(resolve, 220));
  const send = async action => {
    if(action.type==='workspace_manager') await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(action.command))});null`);
    else await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
    await settle(); await pause();
  };
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,50);}check();})`);
  const original = await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  await send({type:'workspace_manager',command:{type:'switch',id:'builtin:workspace:painter'}});
  await wait('JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:painter" && !JSON.parse(layerApp.app.workspace_view()).busy');
  await send({type:'workspace_manager',command:{type:'form',kind:'new'}});
  await send({type:'workspace_manager',command:{type:'submit',name:`Brush drawer test ${Date.now()}`}});
  await wait('!JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
  const created = await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  assert.notEqual(created,'builtin:workspace:painter');
  const theme = await evaluate('layerApp.state().settings.theme ?? null');
  const headers = await evaluate('layerApp.state().workspace.layout.header.zones.flat()');
  const header = command => `[data-header-item="${headers.find(e=>e.item.control?.command===command).id}"] .header-tool`;
  const drawer = () => evaluate('layerApp.state().customization.drawer');
  const contact = async (selector, device='mouse') => {
    const p = await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...p}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    } else {
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0,pointerType:device});
      await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',buttons:1,clickCount:1,pointerType:device,force:.7});
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'left',buttons:0,clickCount:1,pointerType:device});
    }
    await settle(); await pause();
  };
  const set = (panel,label) => `.content-drawer [data-control="${panel}"] [data-tool-choice="${label}"]`;
  const check = async first => {
    assert.deepEqual((await drawer()).columns,[[first],['tools'],['tool_settings']]);
    assert.equal(await evaluate('document.querySelectorAll(".content-drawer > .drawer-column").length'),3);
    const widths = await evaluate('[...document.querySelectorAll(".content-drawer > .drawer-column")].map(n=>n.getBoundingClientRect().width)');
    assert.ok(widths[0]<widths[1] && widths[1]<widths[2],`Narrow sets, normal tools, wider settings: ${widths}`);
    assert.equal(await evaluate('document.querySelectorAll(".content-drawer .tools-control .tool-groups").length'),0);
    assert.ok(await evaluate('document.querySelectorAll(".content-drawer [data-tool-setting]").length')>0);
  };
  const capture = async name => {
    await mkdir('artifacts/brush-sculpt-web',{recursive:true});
    const shot = await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`artifacts/brush-sculpt-web/${name}.png`,Buffer.from(shot.data,'base64'));
  };
  try {
    await send({type:'invoke',command:'drawing_brush'});
    await contact(header('drawing_brush'));
    await check('brush_sets');
    assert.deepEqual(await evaluate('layerApp.state().tool_panels.brush_sets.groups.map(s=>s.label)'),['Pen','Marker','Pencil','Pastel','Paint','Watercolor','Oil paint','Airbrush','Spray','Texture']);
    await evaluate('window.brushSetRoot=document.querySelector(".content-drawer .brush-sets-control")');
    for(const [device,label] of [['mouse','Pencil'],['touch','Pastel'],['pen','Paint']]) {
      assert.ok(await evaluate(`document.querySelector(${JSON.stringify(set('brush_sets',label))}).getBoundingClientRect().height>=44`));
      await contact(set('brush_sets',label),device); await check('brush_sets');
      assert.ok(await evaluate('brushSetRoot===document.querySelector(".content-drawer .brush-sets-control")'),'Set list is retained');
      const choice=await evaluate('layerApp.state().tool_set.subtools[0]');
      await contact(`.content-drawer .tools-control [data-brush="${choice.preview}"]`,device);
      assert.equal(await evaluate('layerApp.state().brush.preset'),choice.preview);
    }
    await send({type:'set_brush_size',value:37});
    const drawing=await evaluate('layerApp.state().brush.preset');
    for(const theme of ['light','dark']) {await send({type:'set_theme',theme}); await capture(`brush-${theme}`);}
    await contact(header('sculpt'),'pen'); await check('sculpt_sets');
    assert.deepEqual(await evaluate('layerApp.state().tool_panels.sculpt_sets.groups.map(s=>s.label)'),['Blend','Liquify']);
    for(const device of ['mouse','touch','pen']) for(const label of ['Liquify','Blend']) {
      await contact(set('sculpt_sets',label),device); await check('sculpt_sets');
      assert.equal(await evaluate('layerApp.state().brush.tool'),label.toLowerCase());
    }
    await contact(set('sculpt_sets','Liquify'),'touch');
    await send({type:'set_brush_size',value:79});
    const sculpt=await evaluate('layerApp.state().brush.preset');
    for(const theme of ['light','dark']) {await send({type:'set_theme',theme}); await capture(`sculpt-${theme}`);}
    await contact(header('drawing_brush'));
    assert.deepEqual(await evaluate('[layerApp.state().brush.preset,layerApp.state().brush.diameter]'),[drawing,37]);
    await contact(header('eraser'),'touch');
    assert.deepEqual((await drawer()).columns,[['tools'],['tool_settings']]);
    assert.equal(await evaluate('document.querySelectorAll(".content-drawer .tool-groups").length'),0);
    await capture('eraser');
    assert.ok(await evaluate('layerApp.state().commands.filter(c=>["drawing_brush","sculpt"].includes(c.id)).every(c=>!c.selected)'));
    await contact(header('sculpt'),'pen');
    assert.deepEqual(await evaluate('[layerApp.state().brush.preset,layerApp.state().brush.diameter]'),[sculpt,79]);
    await contact(header('sculpt')); assert.equal(await drawer(),undefined);
    await contact(header('drawing_brush')); assert.equal(await drawer(),undefined,'First contact selects Brush');
    await contact(header('drawing_brush')); await check('brush_sets');
    console.log('PASS: Brush/Sculpt three-panel drawers, mouse/touch/pen contacts, filtered sets, retained lists, independent selections, settings, light/dark');
  } finally {
    await send({type:'set_theme',theme});
    await send({type:'workspace_manager',command:{type:'switch',id:original}});
    await wait('!JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
    await send({type:'workspace_manager',command:{type:'form',kind:'delete',id:created}});
    await send({type:'workspace_manager',command:{type:'submit',name:''}});
    await evaluate('delete window.brushSetRoot');
  }
}
