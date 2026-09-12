// Actual editor tab strips and pointer routing, paired with the direct SwiftUI
// fixture. Keep font/rasterization differences visible in the captures.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureWorkspaceTabs({fixture, output, evaluate, call}) {
  assert.equal(fixture.schema, 1);
  const prefix = `${fixture.platform}-${fixture.collapsed ? 'drawer' : 'docked'}-${fixture.theme}`;
  console.log(`Tab capture ${prefix}: restoring the fixture`);
  await evaluate(`{
    const fixture = ${JSON.stringify(fixture)};
    layerApp.dispatch({type:'restore_workspace', workspace:fixture.workspace});
    layerApp.dispatch({type:'set_theme', theme:fixture.theme});
    if (fixture.collapsed) layerApp.dispatch({type:'customize', action:{type:'toggle_column_drawer', group:fixture.group, panel:'toolbar'}});
    window.capyTabStrip = () => [...document.querySelectorAll('.tab-list,.drawer-tab-strip')].find(strip =>
      JSON.parse(strip.parentElement.dataset.workspaceDrag).item.group === fixture.group);
    window.capyTabMetrics = () => {
      const strip = capyTabStrip(), rect = node => { const b = node.getBoundingClientRect(); return {x:b.x,y:b.y,width:b.width,height:b.height}; };
      return {clip:rect(strip), frames:[...strip.children].map(rect), panels:[...strip.children].map(t=>t.dataset.panel),
        selected:[...strip.children].map(t=>t.getAttribute('aria-selected')), scale:devicePixelRatio};
    };
  }`);
  console.log(`Tab capture ${prefix}: waiting for geometry`);
  await evaluate(`document.fonts.ready.then(()=>new Promise((resolve,reject)=>{
    const start=performance.now();let last='',since=start;
    function check(){
      if(capyTabStrip()) {
        const value=JSON.stringify(capyTabMetrics());
        if(value!==last){last=value;since=performance.now();}
        if(performance.now()-since>200){resolve(true);return;}
      }
      if(performance.now()-start>10000)reject(new Error('Tab geometry did not settle'));else requestAnimationFrame(check);
    }check();
  }))`);
  const before = await evaluate('capyTabMetrics()');
  console.log(`Tab capture ${prefix}: sending the pointer gesture`);
  assert.equal(before.frames.length, 3);
  assert(before.frames[2].x + before.frames[2].width > before.clip.x + before.clip.width);
  async function capture(phase, metadata) {
    const shot = await call('Page.captureScreenshot', {format:'png', fromSurface:true, clip:{...before.clip, scale:1}});
    await writeFile(`${output}/web-${prefix}-${phase}.png`, Buffer.from(shot.data,'base64'));
    await writeFile(`${output}/web-${prefix}-${phase}.json`, JSON.stringify(metadata,null,2));
  }
  await capture('before', before);
  const press = {x:before.frames[1].x+3, y:before.frames[1].y+18};
  const position = {x:press.x+before.frames[2].width/2+1, y:press.y};
  await call('Input.dispatchMouseEvent', {type:'mousePressed', ...press, button:'left', buttons:1, clickCount:1});
  await call('Input.dispatchMouseEvent', {type:'mouseMoved', ...position, button:'left', buttons:1});
  const drag = await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){
      try {
      const preview=layerApp.app.tab_drag_preview([${position.x},${position.y}]);
      if(document.querySelector('.tab-slide-overlay')&&Number(preview?.insertion)===3)
        requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(JSON.parse(JSON.stringify({metrics:capyTabMetrics(),preview},(_,v)=>typeof v==='bigint'?Number(v):v)))));
      else if(performance.now()-start>5000)reject(new Error('Tab preview missing'));else requestAnimationFrame(check);
      } catch(error) { reject(error); }
    }check();
  })`);
  assert.deepEqual(drag.metrics, before, 'Animation must preserve the original DOM hit rectangles');
  assert.equal(drag.preview.offsets[2].x, -before.frames[1].width);
  await capture('drag', {...drag, press, position});
  await call('Input.dispatchMouseEvent', {type:'mouseReleased', ...position, button:'left', buttons:0, clickCount:1});
  await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){
      const find=node=>node.id===${fixture.group}?node:node.kind==='split'?(find(node.first)||find(node.second)):null;
      const group=layerApp.state().workspace.layout.bands.map(b=>find(b.root)).find(Boolean);
      if(!document.querySelector('.tab-slide-overlay')&&group?.panels.join(',')==='brushes,navigator,toolbar')resolve(true);
      else if(performance.now()-start>5000)reject(new Error('Reorder did not commit'));else requestAnimationFrame(check);
    }check();
  })`);
  console.log(`PASS: Chrome ${prefix}: natural widths, frozen hits, tab preview and release; matching-scale captures saved`);
}
