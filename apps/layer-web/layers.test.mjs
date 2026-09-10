import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkLayers({ call, evaluate, settle }) {
  const send = action => evaluate(`layerApp.dispatch({type:'layer',action:${JSON.stringify(action)}})`);
  const screenshot = async name => {
    await settle(); const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`artifacts/ui/layers-web/${name}.png`, Buffer.from(shot.data,"base64"));
  };
  await mkdir("artifacts/ui/layers-web", {recursive:true});
  // Menus and hover tips resolve typed actions, including remapped/custom keys.
  await evaluate(`(() => {
    window.originalLayerTestSettings = layerApp.state().settings;
    layerApp.dispatch({type:'restore_settings',settings:{...originalLayerTestSettings,
      custom_actions:[{id:'custom.alpha',label:'An unrelated action label',repeat:false,
        action:{kind:'action',action:{type:'layer',action:{op:'alpha_lock',id:1,value:true}}}}],
      shortcuts:{...originalLayerTestSettings.shortcuts,
        'command.ZenMode':[{key:'j',command:true,alt:true,shift:false}],
        'command.AddLayer':[{key:'n',command:true,alt:true,shift:false}],
        'custom.alpha':[{key:'l',command:false,alt:true,shift:false}]}}});
  })()`);
  assert.equal(await evaluate("document.querySelector('#zen-button').title"), "Zen mode (Ctrl+Alt+J)");
  await evaluate(`document.querySelector('.layer-footer [aria-label="New layer"]').dispatchEvent(new PointerEvent('pointerenter'))`);
  assert.equal(await evaluate(`document.querySelector('.layer-footer [aria-label="New layer"]').title`), "New layer (Ctrl+Alt+N)");
  assert.deepEqual(await evaluate(`(() => { const item=layerApp.app.layer_menu(1n,false).sections.flat().find(i=>i.label==='Alpha lock'); return [item.hint,item.selected]; })()`), ["Alt+L", false]);
  await evaluate("layerApp.dispatch({type:'restore_settings',settings:originalLayerTestSettings}); delete window.originalLayerTestSettings");
  await evaluate(`layerApp.dispatch({type:'set_theme',theme:'dark'})`);
  const thumb = await evaluate(`new Promise((resolve,reject)=>{ const start=performance.now(); function check(){
    const c=document.querySelector('.layer-thumbnail canvas'); const data=c.getContext('2d',{willReadFrequently:true}).getImageData(0,0,32,32).data;
    if(data[3])resolve(Array.from(data.slice(0,20))); else if(performance.now()-start>10000)reject(Error('No thumbnail: '+document.querySelector('#status').textContent)); else setTimeout(check,120); } check(); })`);
  assert.equal(thumb[3],255);
  await screenshot("01-empty-dark");
  await send({op:"rename",id:1,name:"Linework"});
  await send({op:"reference_selection"});
  assert.equal(await evaluate("layerApp.state().layers.find(l=>l.editing).selection_icon"),"layer-reference-symbolic");
  await send({op:"new",group:false,clipped:false});
  const second = await evaluate("Number(layerApp.state().layers.find(l=>l.editing).id)");
  await send({op:"rename",id:second,name:"Color wash"});
  await send({op:"toggle_selection",id:1});
  await send({op:"reference_selection"});
  assert.equal(await evaluate("layerApp.state().layers.filter(l=>l.selected).length"),1);
  assert.equal(await evaluate("layerApp.state().layers.filter(l=>l.reference).length"),2);
  await send({op:"tool",tool:"lasso_fill"});
  await evaluate("layerApp.dispatch({type:'set_color',rgba:[.8,.2,.12,1]})");
  const points = [[440,360],[740,400],[810,620],[560,690],[440,360]];
  await call("Input.dispatchMouseEvent",{type:"mouseMoved",x:440,y:360});
  await call("Input.dispatchMouseEvent",{type:"mousePressed",x:440,y:360,button:"left",buttons:1,clickCount:1});
  for (const [x,y] of points.slice(1)) await call("Input.dispatchMouseEvent",{type:"mouseMoved",x,y,button:"left",buttons:1});
  await call("Input.dispatchMouseEvent",{type:"mouseReleased",x:440,y:360,button:"left",buttons:0,clickCount:1});
  await settle(); await send({op:"add_mask",id:second,replace:false});
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){
    const c=document.querySelector('[data-layer="${second}"] .layer-thumbnail canvas'),data=c.getContext('2d',{willReadFrequently:true}).getImageData(0,0,32,32).data;
    if(Array.from({length:1024},(_,i)=>i*4).some(i=>data[i]>150&&data[i]>data[i+1]*2))resolve(true);
    else if(performance.now()-start>10000)reject(Error('Paint thumbnail did not update'));else setTimeout(check,120);}check();})`);
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){
    const previews=[...document.querySelectorAll('.layer-thumbnail:not([hidden]) canvas')].map(c=>[c.closest('[data-layer]').dataset.layer,Array.from(c.getContext('2d',{willReadFrequently:true}).getImageData(16,16,1,1).data)]);
    if(previews.every(([,p])=>p[3]===255))resolve(true);
    else if(performance.now()-start>10000)reject(Error('Unfinished thumbnails '+JSON.stringify(previews)));else setTimeout(check,120);}check();})`);
  await screenshot("02-paint-mask-dark");
  // The row padding is selectable, not just its name/thumbnail.
  const rect=await evaluate(`(()=>{const r=document.querySelector('[data-layer="1"]').getBoundingClientRect();return{x:r.x+1,y:r.y+1}})()`);
  await call("Input.dispatchMouseEvent",{type:"mousePressed",...rect,button:"left",buttons:1,clickCount:1});
  await call("Input.dispatchMouseEvent",{type:"mouseReleased",...rect,button:"left",buttons:0,clickCount:1});
  await settle(); assert.equal(await evaluate("Number(layerApp.state().layers.find(l=>l.editing).id)"),1);
  await send({op:"select",id:second,mask:true});
  await evaluate("document.querySelector('.layer-more').click()"); await settle();
  assert.ok(await evaluate("document.querySelector('.panel-context-menu').textContent.includes('Delete mask')"));
  await screenshot("03-mask-menu-dark");
  await call("Input.dispatchKeyEvent",{type:"keyDown",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
  await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'})");
  await screenshot("04-paint-mask-light");
  for (const command of ["lasso","move","brush"]) {
    await evaluate(`layerApp.dispatch({type:'invoke',command:'${command}'})`);
    assert.equal(await evaluate(`layerApp.state().commands.find(c=>c.id==='${command}').selected`),true);
  }
  await evaluate("layerApp.dispatch({type:'move_panel',panel:'sizes',viewport:[innerWidth,innerHeight],target:{kind:'float',position:[550,430]}})");
  await settle();
  const grip = await evaluate(`(() => { const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('sizes'));
    if(g.tabs_visible)throw Error('Floating panel must hide its tab');
    const r=document.querySelector('[data-group="'+g.id+'"] .panel-grip').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  const edge=await evaluate("({x:innerWidth-2,y:innerHeight*.5})");
  await call("Input.dispatchMouseEvent",{type:"mouseMoved",...grip});
  await call("Input.dispatchMouseEvent",{type:"mousePressed",...grip,button:"left",buttons:1,clickCount:1});
  await call("Input.dispatchMouseEvent",{type:"mouseMoved",...edge,button:"left",buttons:1});
  await call("Input.dispatchMouseEvent",{type:"mouseReleased",...edge,button:"left",buttons:0,clickCount:1});
  await settle();
  assert.deepEqual(await evaluate("(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('sizes'));return[g.floating,g.tabs_visible]})()"),[false,true]);
  assert.ok(await evaluate("document.querySelector('.dock-tab[data-panel=sizes]')!==null"));
  await screenshot("05-tab-shown-after-docking");
  await evaluate("layerApp.dispatch({type:'invoke',command:'undo_workspace'})");
  assert.deepEqual(await evaluate("(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('sizes'));return[g.floating,g.tabs_visible]})()"),[true,false]);
  console.log("PASS: layer thumbnails, references, whole-row selection, mask menu, shared shortcuts/tools and docking tab visibility");
}
