import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import {execFile} from "node:child_process";
import {promisify} from "node:util";

export async function checkSelectedPainting({call, evaluate, settle}) {
  const action = value => evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);
  const layer = value => action({type:"layer",action:value});
  const path = async points => {
    for (let i=0;i<points.length;i++) {
      const [x,y]=points[i];
      await call("Input.dispatchMouseEvent",{type:i===0?"mousePressed":i===points.length-1?"mouseReleased":"mouseMoved",
        x,y,button:"left",buttons:i===points.length-1?0:1,clickCount:1});
      await settle();
    }
    await settle();
  };
  const brushReady = () => evaluate("new Promise(resolve=>{(function poll(){if(layerApp.app.brush_ready())resolve();else setTimeout(poll,20);})();})");
  const pixels = async () => {
    const {data}=await call("Page.captureScreenshot",{format:"png"});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();
      const canvas=document.createElement('canvas');canvas.width=image.width;canvas.height=image.height;
      const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);
      return [600,750,900].map(x=>Array.from(ctx.getImageData(x,500,1,1).data));})()`);
  };
  await layer({op:"tool",tool:"select"});
  await path([[650,350],[850,350],[850,650],[650,650],[650,350]]);
  assert.ok(await evaluate("layerApp.state().layer_tools.has_selection"));
  const before=await pixels();
  assert.ok(before.every(p=>p.slice(0,3).every(v=>v>245)),"Sample points start on blank paper");
  await action({type:"select_brush",id:1});
  await action({type:"set_brush_size",value:80});
  await action({type:"set_color",rgba:[.9,0,0,1]});
  const line=Array.from({length:21},(_,i)=>[550+i*20,500]);
  await brushReady();
  await path(line);
  const selected=await pixels();
  assert.deepEqual(selected[0],before[0]);
  assert.deepEqual(selected[2],before[2]);
  assert.ok(selected[1][0]>150&&selected[1][1]<100,"GPU paint fills selected interior");
  await layer({op:"invert_selection"});
  await action({type:"set_color",rgba:[0,0,.9,1]});
  await brushReady();
  await path(line);
  const inverse=await pixels();
  assert.deepEqual(inverse[1],selected[1]);
  assert.ok([inverse[0],inverse[2]].every(p=>p[2]>150&&p[0]<100),"Inversion paints only the exterior");
  await layer({op:"deselect"});
  for (const command of ["undo","undo","redo","redo"]) {
    await action({type:"invoke",command}); await settle();
  }
  assert.deepEqual(await pixels(),inverse,"Replay retains each stroke's selection after deselecting");
  console.log("PASS: WebGPU selected/inverted painting, deselect and stroke replay");
}

export async function checkLayers({ call, evaluate, settle }) {
  const send = action => evaluate(`layerApp.dispatch({type:'layer',action:${JSON.stringify(action)}})`);
  const screenshot = async name => {
    await settle(); const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`artifacts/ui/layers-web/${name}.png`, Buffer.from(shot.data,"base64"));
  };
  await mkdir("artifacts/ui/layers-web", {recursive:true});
  assert.equal(await evaluate("document.querySelector('.layer-more svg').dataset.asset"),"more-small");
  const gripSize=await evaluate("(()=>{const n=document.querySelector('.layer-grip'),r=n.getBoundingClientRect(),s=n.querySelector('svg').getBoundingClientRect();return{width:r.width,iconWidth:s.width,iconHeight:s.height}})()");
  assert.deepEqual(gripSize,{width:16,iconWidth:16,iconHeight:16},"layer-row grip retains the shared icon canvas");
  const theme=await evaluate('layerApp.state().settings.theme');
  const space=type=>call('Input.dispatchKeyEvent',{type,key:' ',code:'Space',windowsVirtualKeyCode:32,nativeVirtualKeyCode:32});
  try { for(const value of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${value}'});layerApp.canvas.focus()`);await settle();
    await space('keyDown');await settle();
    assert.equal(await evaluate('layerApp.canvas.style.cursor'),'grab',`${value}: Space holds canvas pan`);
    const point=await evaluate(`(()=>{const node=[...document.querySelectorAll('.layer-row.selected .layer-name')].find(n=>n.getBoundingClientRect().width>0),r=node.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point,button:'none'});
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:2});await settle();
    assert.equal(await evaluate(`document.activeElement===document.querySelector('.layer-name-entry')`),true,`${value}: native double-click starts layer rename`);
    assert.equal(await evaluate('layerApp.canvas.style.cursor'),'grab',`${value}: native editor focus retains the existing hold`);
    await evaluate(`document.activeElement.dispatchEvent(new CompositionEvent('compositionstart',{data:'１',bubbles:true}))`);
    await space('keyUp');await settle();
    assert.equal(await evaluate('layerApp.canvas.style.cursor'),'',`${value}: composition cannot swallow an existing canvas hold release`);
    await evaluate(`document.activeElement.dispatchEvent(new CompositionEvent('compositionend',{data:'１',bubbles:true}));document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',code:'Escape',bubbles:true,cancelable:true}));layerApp.canvas.focus()`);await settle();
    assert.equal(await evaluate('layerApp.canvas.style.cursor'),'',`${value}: returning to the canvas leaves no retired hold`);
  }} finally {await space('keyUp');await send({op:'cancel_rename'});await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);}
  const initialCount = await evaluate("layerApp.state().layers.length");
  await send({op:"new",group:false,clipped:false});
  await evaluate(`document.querySelector('.layer-footer [aria-label="Delete selected layers"]').click()`);
  assert.equal(await evaluate("layerApp.state().layers.length"),initialCount);
  await send({op:"select",id:2,mask:false});
  assert.equal(await evaluate(`document.querySelector('.layer-footer [aria-label="Delete selected layers"]').disabled`),false,"Paper is deletable");
  await send({op:"select",id:1,mask:false});
  // Menus and hover tips resolve typed actions, including remapped keys.
  await evaluate(`(() => {
    window.originalLayerTestSettings = layerApp.state().settings;
    layerApp.dispatch({type:'restore_settings',settings:{...originalLayerTestSettings,
      shortcuts:{...originalLayerTestSettings.shortcuts,
        'command.ZenMode':[{key:'j',command:true,alt:true,shift:false}],
        'command.AddLayer':[{key:'n',command:true,alt:true,shift:false}]}}});
  })()`);
  assert.equal(await evaluate("document.querySelector('#zen-capy').title"), "Zen mode (Ctrl+Alt+J)");
  await evaluate(`document.querySelector('.layer-footer [aria-label="New layer"]').dispatchEvent(new PointerEvent('pointerenter'))`);
  assert.equal(await evaluate(`document.querySelector('.layer-footer [aria-label="New layer"]').title`), "New layer (Ctrl+Alt+N)");
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
  await settle();
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){
    if(layerApp.app.document_park_ready())resolve();else if(performance.now()>end)reject(Error('Lasso contact did not finish'));else requestAnimationFrame(check);}check();})`);
  await send({op:"add_mask",id:second,replace:false});
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${second}).has_mask`),true);
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
    if(!g.tabs_visible)throw Error('Floating preserves the visible tab bar');
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
  assert.deepEqual(await evaluate("(()=>{const g=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('sizes'));return[g.floating,g.tabs_visible]})()"),[true,true]);
  console.log("PASS: layer thumbnails, references, whole-row selection, mask menu, shared shortcuts/tools and docking tab visibility");
  await checkLayerRelationships({call,evaluate,settle});
}

export async function checkLayerRelationships({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/layer-attachment-web';
  await mkdir(directory,{recursive:true});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const applied=()=>wait('Number(layerApp.app.gpu_diagnostics().session.deferred_edits)===0');
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();await applied();};
  const layer=action=>send({type:'layer',action});
  const invoke=command=>send({type:'invoke',command});
  const active=()=>evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  const row=id=>`#layer-rows .layer-row[data-layer="${id}"]`;
  const attachment=()=>evaluate(`(()=>{const n=document.querySelector('.layer-attachment');return{label:n.ariaLabel,description:n.getAttribute('aria-description'),icon:n.firstChild.dataset.asset,checked:n.getAttribute('aria-pressed'),disabled:n.disabled}})()`);
  const rect=async selector=>{
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);await settle();
    return evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  };
  const mouse=async(type,p)=>{await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});await settle();await applied();};
  const attach=async()=>{await evaluate("document.querySelector('.layer-attachment').click()");await settle();await applied();};
  const insert=async effect=>{await send({type:'effect',action:{op:'insert',effect}});return active();};
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
  if(await evaluate("layerApp.state().commands.find(c=>c.id==='reset_layout').enabled")) {
    await invoke('reset_layout');
    await wait("!!document.querySelector('.workspace-form[open] .suggested-action')");
    await evaluate("document.querySelector('.workspace-form[open] .suggested-action').click()");
    await wait("!document.querySelector('.workspace-form[open]')&&!JSON.parse(layerApp.app.workspace_view()).busy");
  }
  await layer({op:'select',id:await evaluate('Number(layerApp.state().layers[0].id)'),mask:false});
  await layer({op:'new',group:false,clipped:false});const base=await active();
  await layer({op:'rename',id:base,name:'Base {ink} 🎨'});
  const blur=await insert('gaussian_blur');await attach();
  const outer=await insert('curves');await attach();
  const sourceBox=await rect(`${row(blur)} .layer-name`),outerBox=await rect(row(outer));
  await mouse('mousePressed',{x:sourceBox.x+sourceBox.width/2,y:sourceBox.y+sourceBox.height/2});
  await mouse('mouseMoved',{x:outerBox.x+outerBox.width/2,y:outerBox.y+1});
  assert.equal(await evaluate(`document.querySelector('${row(outer)}').classList.contains('layer-drop-before')`),true);
  await mouse('mouseReleased',{x:outerBox.x+outerBox.width/2,y:outerBox.y+1});
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${blur}).relationship??null`),null,'outer drop above the first FX chain detaches');
  await invoke('undo');
  assert.equal(await evaluate(`Number(layerApp.state().layers.find(l=>Number(l.id)===${blur}).relationship.target)`),base,'one Undo restores the effect owner');
  await layer({op:'delete',id:outer});
  await layer({op:'select',id:base,mask:false});
  await layer({op:'new',group:false,clipped:true});const owner=await active();
  await layer({op:'rename',id:owner,name:'Shadows'});
  await layer({op:'new',group:false,clipped:true});const gapTop=await active();
  await layer({op:'select',id:base,mask:false});const gapFilter=await insert('exposure');
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for(const target of [owner,base]) {
      await layer({op:'select',id:gapFilter,mask:false});
      await rect(`${row(gapFilter)} .layer-name`);await rect(row(target));
      const points=await evaluate(`(()=>{const a=document.querySelector('${row(gapFilter)} .layer-name').getBoundingClientRect(),b=document.querySelector('${row(target)}').getBoundingClientRect();return{source:{x:a.x+a.width/2,y:a.y+a.height/2},target:{x:b.x+b.width*.7,y:b.y+2}}})()`);
      await mouse('mousePressed',points.source);await mouse('mouseMoved',points.target);
      assert.equal(await evaluate(`document.querySelector('${row(target)} .layer-thumbnail').classList.contains('layer-drop-attach')`),true,'gap preview identifies the filter owner');
      assert.equal(await evaluate(`document.querySelector('${row(target)}').classList.contains('layer-drop-before')`),true);
      await mouse('mouseReleased',points.target);
      assert.equal(await evaluate(`Number(layerApp.state().layers.find(l=>Number(l.id)===${gapFilter}).relationship.target)`),target);
      await invoke('undo');await invoke('redo');await invoke('undo');
    }
  }
  await layer({op:'delete',id:gapFilter});await layer({op:'delete',id:gapTop});
  await layer({op:'select',id:owner,mask:false});

  await layer({op:'add_mask',id:owner,replace:false});
  await layer({op:'select',id:owner,mask:false});
  const first=await insert('curves');await attach();
  const top=await insert('exposure');await attach();
  assert.deepEqual(await attachment(),{label:'Apply to layers below',description:'Applied to Shadows',icon:'effect-link',checked:'true',disabled:false});
  assert.deepEqual(await evaluate(`layerApp.state().layers.filter(l=>[${first},${top}].includes(Number(l.id))).map(l=>[l.relationship.kind,Number(l.relationship.target)])`),[['effect',owner],['effect',owner]]);
  await layer({op:'visibility',id:first,value:false});await layer({op:'visibility',id:owner,value:false});
  assert.deepEqual(await evaluate(`(()=>{const r=document.querySelector('${row(top)}'),l=layerApp.state().layers.find(l=>Number(l.id)===${top});return[l.visible,l.visibility_blocked,r.querySelector('.layer-icon').firstChild.dataset.asset,getComputedStyle(r.querySelector('.layer-icon')).opacity]})()`),[true,true,'eye-hidden','0.35']);
  await layer({op:'visibility',id:owner,value:true});
  assert.deepEqual(await evaluate(`layerApp.state().layers.filter(l=>[${first},${top}].includes(Number(l.id))).map(l=>[l.visible,l.visibility_blocked])`),[[true,false],[false,false]]);
  await layer({op:'new',group:true,clipped:false});const group=await active();
  await layer({op:'blend',id:group,value:0});
  await layer({op:'new',group:false,clipped:false});const child=await active();
  await layer({op:'select',id:group,mask:false});const groupFx=await insert('curves');await attach();
  await layer({op:'collapse',id:group});
  assert.equal(await evaluate(`document.querySelector('${row(child)}')===null`),true);
  assert.equal(await evaluate(`layerApp.state().layer_tools.connections.some(e=>e.kind==='effect'&&Number(e.from)===${groupFx}&&Number(e.to)===${group})`),true);
  await invoke('new_selection_layer');const saved=await active();
  await layer({op:'cancel_rename'});
  await layer({op:'drop',id:saved,target:owner,fraction:1,surface:'row'});
  await rect(row(owner));await rect(`${row(saved)} .layer-name`);
  const {source,target}=await evaluate(`(()=>{const box=n=>{const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}};return{source:box(document.querySelector('${row(saved)} .layer-name')),target:box(document.querySelector('${row(owner)}'))}})()`);
  await mouse('mousePressed',{x:source.x+source.width/2,y:source.y+source.height/2});
  await mouse('mouseMoved',{x:target.x+target.width/2,y:target.y+target.height*.15});
  assert.equal(await evaluate(`document.querySelector('${row(top)}').classList.contains('layer-drop-before')`),true,'saved selection drop is painted at normalized top FX');
  await mouse('mouseReleased',{x:target.x+target.width/2,y:target.y+target.height*.15});
  assert.equal(await evaluate(`(()=>{const rows=layerApp.state().layers;return rows.findIndex(l=>Number(l.id)===${top})===rows.findIndex(l=>Number(l.id)===${saved})+1})()`),true);
  await invoke('undo');await invoke('redo');
  await invoke('return_to_artwork');await layer({op:'select',id:top,mask:false});
  const added=await insert('curves');
  await layer({op:'drop',id:added,target:saved,fraction:0,surface:'row'});await attach();
  assert.equal(await evaluate(`(()=>{const rows=layerApp.state().layers;return rows.findIndex(l=>Number(l.id)===${added})===rows.findIndex(l=>Number(l.id)===${saved})+1})()`),true,'attachment gathers contiguous FX below saved Selection');
  const extra=await insert('curves');
  for(const thumbnail of [true,false]) {
    const selector=thumbnail?`${row(group)} .layer-thumbnail`:row(group);
    await rect(`${row(extra)} .layer-name`);await rect(selector);
    const points=await evaluate(`(()=>{const a=document.querySelector('${row(extra)} .layer-name').getBoundingClientRect(),b=document.querySelector('${selector}').getBoundingClientRect();return{source:{x:a.x+a.width/2,y:a.y+a.height/2},target:{x:b.x+b.width/2,y:b.y+b.height/2}}})()`);
    await mouse('mousePressed',points.source);await mouse('mouseMoved',points.target);
    assert.equal(await evaluate(`document.querySelector('${selector}').classList.contains('${thumbnail?'layer-drop-attach':'layer-drop-into'}')`),true,thumbnail?'group thumbnail previews output attachment':'group body previews folder insertion');
    await mouse('mouseReleased',points.target);
    assert.equal(await evaluate(`(()=>{const l=layerApp.state().layers.find(l=>Number(l.id)===${extra});return ${thumbnail?`l.relationship?.kind==='effect'&&Number(l.relationship.target)===${group}`:'!l.relationship&&l.depth===1'}})()`),true);
    await invoke('undo');
  }
  await layer({op:'collapse',id:group});
  await layer({op:'delete',id:extra});
  await layer({op:'select',id:top,mask:false});
  const settings=await evaluate('layerApp.state().settings');
  for(const width of [1440,900]) {
    await call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await settle();
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});
      for(const accent of [null,'#12ab56','#808080']) {
        await send({type:'restore_settings',settings:{...settings,theme,accent}});
        await rect(`${row(top)} .layer-thumbnail`);await wait(`!!document.querySelector('.layer-connections [data-kind="clip"][data-to="${base}"]')`);
        await mouse('mouseMoved',{x:width/2,y:900});
        const geometry=await evaluate(`(()=>{const list=document.querySelector('.layer-list'),bounds=list.getBoundingClientRect(),svg=document.querySelector('.layer-connections'),rail=svg.querySelector('[data-kind="clip"][data-to="${base}"]'),glyphs=[...svg.querySelectorAll('[data-kind="effect"] svg')],fx=document.querySelector('${row(top)} .layer-thumbnail'),paper=[...layerApp.state().layers].find(l=>l.label==='Paper'),load=document.querySelector('${row(saved)} .selection-layer-load'),owner=document.querySelector('${row(owner)} .layer-thumbnail'),probe=document.createElement('span');probe.style.color=layerApp.state().palette.relationship;document.body.append(probe);const result={railColor:getComputedStyle(rail).color,paletteColor:getComputedStyle(probe).color,railD:rail.firstChild.getAttribute('d'),railWidth:getComputedStyle(rail).strokeWidth,fxColor:glyphs[0]&&getComputedStyle(glyphs[0]).color,neutral:getComputedStyle(document.querySelector('${row(owner)} .layer-link')).color,glyphWidths:glyphs.map(n=>parseFloat(getComputedStyle(n).width)),fxBackground:getComputedStyle(fx).backgroundColor,fxCanvas:!!fx.querySelector('canvas'),fxWidth:fx.getBoundingClientRect().width,ownerWidth:owner.getBoundingClientRect().width,gutter:document.querySelector('.layer-connection-gutter').getBoundingClientRect().width,loadBackground:getComputedStyle(load).backgroundColor,loadWidth:load.getBoundingClientRect().width,paper:paper.has_thumbnail&&!paper.adjustment_effect,overlayPointer:getComputedStyle(svg).pointerEvents,overlayWidth:svg.getBoundingClientRect().width,listWidth:bounds.width,edges:layerApp.state().layer_tools.connections.filter(e=>e.kind==='effect').every(e=>{const ids=layerApp.state().layers.map(l=>String(l.id));return ids.indexOf(String(e.to))===ids.indexOf(String(e.from))+1})};probe.remove();return result;})()`);
        assert.equal(geometry.railColor,geometry.paletteColor);
        assert.match(geometry.railD,/^M[\d.-]+ [\d.-]+V[\d.-]+$/);
        assert.equal(geometry.railWidth,'2px');assert.equal(geometry.fxColor,geometry.neutral);
        assert.ok(geometry.glyphWidths.length>0&&geometry.glyphWidths.every(n=>Math.abs(n-12)<.01),JSON.stringify(geometry.glyphWidths));
        assert.equal(geometry.fxBackground,'rgba(0, 0, 0, 0)');assert.equal(geometry.fxCanvas,false);
        assert.deepEqual([geometry.fxWidth,geometry.ownerWidth,geometry.gutter,geometry.loadWidth],[30,30,3,30]);
        assert.equal(geometry.loadBackground,'rgba(0, 0, 0, 0)');assert.equal(geometry.paper,true);assert.equal(geometry.overlayPointer,'none');
        assert.equal(geometry.overlayWidth,geometry.listWidth);assert.equal(geometry.edges,true);
      }
      await send({type:'restore_settings',settings:{...settings,theme}});
      const box=await rect('.layers-panel');
      await writeFile(`${directory}/relationships-${width}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip:{...box,scale:1}})).data,'base64'));
    }
  }
  await send({type:'restore_settings',settings});
  for(let i=0;i<32;i++)await layer({op:'new',group:false,clipped:false});
  await rect(`${row(top)} .layer-thumbnail`);
  const before=await evaluate(`document.querySelector('.layer-connections [data-kind="clip"][data-to="${base}"]')?.firstChild.getAttribute('d')`);
  await evaluate("document.querySelector('#layer-rows').scrollTop+=20");await settle();
  const after=await evaluate(`document.querySelector('.layer-connections [data-kind="clip"][data-to="${base}"]')?.firstChild.getAttribute('d')`);
  assert.ok(before&&after&&before!==after,'scroll moves retained relationship geometry');
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  console.log('PASS: contextual attachment, neutral adjacent FX links, darker accent rail, inherited eye, backgroundless FX, saved Selection normalized drops, collapsed owner, themes, custom/neutral accents, narrow/wide, scroll');
}

async function checkScaleRelief({call,evaluate,settle,theme,directory}) {
  const paper=(await evaluate("layerApp.state().layers.map(l=>({id:Number(l.id),label:l.label}))")).find(l=>l.label==='Paper').id;
  await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${paper},mask:false}})`);await settle();
  const scale=()=>evaluate("layerApp.state().layer_properties.controls.find(c=>c.key==='scale').value.value");
  const slider=await evaluate("(()=>{const c=[...document.querySelectorAll('.number-control')].find(n=>n.querySelector('.number-title')?.textContent==='Scale'&&n.offsetParent);c.scrollIntoView({block:'nearest'});const r=c.querySelector('.number-slider').getBoundingClientRect();return{x:r.x,y:r.y+r.height/2,width:r.width}})()");
  const spread=async()=>{
    const {data}=await call('Page.captureScreenshot',{format:'png'});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();
      const r=document.getElementById('canvas').getBoundingClientRect(),s=image.width/innerWidth,canvas=document.createElement('canvas');
      canvas.width=image.width;canvas.height=image.height;const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);
      const x=Math.round((r.x+r.width*.35)*s),y=Math.round((r.y+r.height*.35)*s),w=Math.round(r.width*.3*s),h=Math.round(r.height*.3*s);
      const d=ctx.getImageData(x,y,w,h).data;let sum=0,squares=0;for(let i=0;i<d.length;i+=4){const v=(d[i]+d[i+1]+d[i+2])/3;sum+=v;squares+=v*v;}
      const n=d.length/4;return Math.sqrt(Math.max(0,squares/n-(sum/n)**2));})()`);
  };
  await new Promise(resolve=>setTimeout(resolve,2500));
  const flat=await spread();
  const before=await scale();
  const mouse=(type,f)=>call('Input.dispatchMouseEvent',{type,x:slider.x+slider.width*f,y:slider.y,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  await mouse('mousePressed',.5);
  for(let i=1;i<=30;i++){await mouse('mouseMoved',.5+.4*i/30);await new Promise(resolve=>setTimeout(resolve,16));}
  const shown=Date.now()+8000;let dragging=flat;
  while(Date.now()<shown&&(dragging=await spread())<=flat+2)await new Promise(resolve=>setTimeout(resolve,100));
  const capture=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/scale-relief-${theme}.png`,Buffer.from(capture.data,'base64'));
  const dragged=await scale();
  await mouse('mouseReleased',.9);await settle();
  assert.ok(dragged>before*1.5,`Dragging Scale enlarges the paper: ${before} -> ${dragged}`);
  assert.ok(dragging>flat+2,`The relief shades the canvas while Scale is held: spread ${flat.toFixed(2)} -> ${dragging.toFixed(2)}`);
  const end=Date.now()+8000;let settled=dragging;
  while(Date.now()<end&&(settled=await spread())>flat+1)await new Promise(resolve=>setTimeout(resolve,200));
  assert.ok(settled<=flat+1,`The relief hides after release: spread ${settled.toFixed(2)}`);
  await evaluate("layerApp.dispatch({type:'invoke',command:'undo'})");await settle();
  assert.equal(await scale(),before,'A Scale drag is one Undo step');
}

async function checkPaperLighting({call,evaluate,settle,theme,directory}) {
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const wait=(condition,timeout=10000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20)}poll()})`);
  const field=key=>`document.querySelector('[data-property-key="${key}"]')`;
  const value=key=>evaluate(`layerApp.state().layer_properties.controls.find(c=>c.key===${JSON.stringify(key)}).value.value`);
  const point=(selector,fx=.5,fy=.5)=>evaluate(`(()=>{const n=${selector};n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width*${fx},y:r.y+r.height*${fy}}})()`);
  const mouse=(type,p,{clickCount=1,modifiers=0}={})=>call('Input.dispatchMouseEvent',{type,...p,modifiers,button:type==='mouseMoved'?'none':'left',buttons:type==='mousePressed'?1:0,clickCount});
  const drag=async(points,modifiers=[])=>{
    await mouse('mousePressed',points[0],{modifiers:modifiers[0]??0});await settle();
    for(const [index,p] of points.entries())if(index){await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,modifiers:modifiers[index]??0,button:'left',buttons:1});await settle();}
    await mouse('mouseReleased',points.at(-1),{modifiers:modifiers.at(-1)??0});await settle();
  };
  const key=async(name,code,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,code,modifiers,windowsVirtualKeyCode:{Enter:13,Shift:16,ArrowLeft:37,ArrowRight:39}[name]});await settle();};
  const undo=()=>send({type:'invoke',command:'undo'});
  const capture=async name=>{
    await evaluate('layerApp.app.wait_for_canvas()');await settle();
    await writeFile(`${directory}/${name}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    const clip=await evaluate(`(()=>{const r=document.querySelector('.effect-properties').getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,scale:3}})()`);
    await writeFile(`${directory}/${name}-properties-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const lighting=()=>evaluate(`(()=>{const light=${field('light')},relief=${field('relief')},through=${field('through_paint')},dial=light.querySelector('.number-dial-face'),style=getComputedStyle(dial);
    const box=n=>n.getBoundingClientRect(),d=box(dial),track=box(relief.querySelector('.number-track')),row=box(light);
    return{disabled:[light,through].map(n=>n.classList.contains('disabled')),opacity:Number(getComputedStyle(light).opacity),tab:dial.tabIndex,ariaDisabled:dial.getAttribute('aria-disabled'),role:dial.getAttribute('role'),valueText:dial.getAttribute('aria-valuetext'),
      size:[d.width,d.height],rowHeights:[row.height,box(relief).height],centered:Math.abs(d.top+d.height/2-(row.top+row.height/2)),end:d.right-track.right,radius:style.borderRadius,corner:style.cornerShape??'',
      dot:parseFloat(getComputedStyle(dial,'::after').width)/d.width,labelEnd:box(light.querySelector('.number-title')).right<=d.left}})()`);
  const paper=await evaluate('Number(layerApp.state().layers.find(l=>l.paper).id)');
  await send({type:'layer',action:{op:'select',id:paper,mask:false}});
  await wait(`!!${field('light')}?.querySelector('.number-dial-face')`);
  assert.equal(await value('relief'),0,'Paper starts flat');
  const flat=await lighting();
  assert.deepEqual([flat.disabled,flat.opacity,flat.tab,flat.ariaDisabled,flat.role],[[true,true],.4,-1,'true','slider'],'A flat paper dims Light and Shows through paint');
  const dialCenter=await point(`${field('light')}.querySelector('.number-dial-face')`);
  await drag([dialCenter,{x:dialCenter.x-14,y:dialCenter.y}]);
  assert.equal(await value('light'),225,'A disabled dial accepts no input');
  await capture('paper-lighting-flat');
  await mouse('mousePressed',await point(`${field('relief')}.querySelector('.number-value')`));await mouse('mouseReleased',await point(`${field('relief')}.querySelector('.number-value')`));await settle();
  await evaluate(`(()=>{const n=${field('relief')}.querySelector('.number-entry');n.value='150';n.dispatchEvent(new Event('input',{bubbles:true}))})()`);
  await key('Enter','Enter');
  await wait(`!${field('light')}.classList.contains('disabled')`);
  const lit=await lighting();
  assert.deepEqual([lit.disabled,lit.opacity,lit.tab,lit.ariaDisabled],[[false,false],1,0,'false'],'Texture visibility turns on the lighting controls');
  assert.deepEqual(lit.size,[28,28],'The dial is a slider row less the control spacing above and below');
  assert.equal(lit.rowHeights[0],lit.rowHeights[1],'The Light row is as tall as a slider row');
  assert.ok(lit.centered<.5,`The dial is centered in its row: ${lit.centered}`);
  assert.ok(Math.abs(lit.end)<.5,`The dial ends where the slider tracks end: ${lit.end}`);
  assert.equal(lit.radius,'50%');assert.ok(['','round','superellipse(1)'].includes(lit.corner),`The dial is a true circle: ${lit.corner}`);
  assert.ok(Math.abs(lit.dot-.22)<.01,`The dot's radius is 0.22 of the dial's: ${lit.dot}`);
  assert.equal(lit.labelEnd,true,'The label stops before the dial');
  assert.equal(lit.valueText,'225 °');
  await capture('paper-lighting-lit');
  const dial=`${field('light')}.querySelector('.number-dial-face')`;
  await drag([await point(dial,.08,.5)]);
  assert.equal(await value('light'),180,"A press on the dial's left edge lights from the left");
  const top=await point(dial,.5,.08),center=await point(dial);
  await drag([top,{x:center.x+30,y:center.y-20},{x:center.x+200,y:center.y}]);
  assert.equal(await value('light'),0,'Dragging keeps turning the dial after the pointer leaves it');
  await undo();assert.equal(await value('light'),180,'One drag is one step');
  await key('Shift','ShiftLeft');await evaluate(`${dial}.focus()`);
  const focus=await evaluate(`(()=>{const n=${dial};return{focused:document.activeElement===n,visible:n.matches(':focus-visible'),ring:getComputedStyle(n).boxShadow,active:document.activeElement?.outerHTML.slice(0,120)}})()`);
  assert.equal(focus.focused&&focus.visible,true,`The dial takes keyboard focus: ${JSON.stringify(focus)}`);assert.notEqual(focus.ring,'none','Keyboard focus shows a ring');
  await capture('paper-lighting-focus');
  await key('ArrowRight','ArrowRight');assert.equal(await value('light'),181);
  await key('ArrowLeft','ArrowLeft',8);assert.equal(await value('light'),166,'Shift steps by 15°');
  await undo();assert.equal(await value('light'),181,'Each key press is one step');await undo();assert.equal(await value('light'),180);
  for(const clickCount of [1,2]){await mouse('mousePressed',center,{clickCount});await mouse('mouseReleased',center,{clickCount});}
  await settle();await wait(`layerApp.state().layer_properties.controls.find(c=>c.key==='light').value.value===225`);
  await undo();assert.equal(await value('light'),180,'Double-clicking the dial restores the default in one step');
  const readout=await point(`${field('relief')}.querySelector('.number-value')`);
  await drag([readout,{x:readout.x,y:readout.y-10},{x:readout.x,y:readout.y-21}]);
  assert.equal(await value('relief'),155,'A scrub snaps to whole percent');
  await drag([readout,{x:readout.x,y:readout.y-10},{x:readout.x,y:readout.y-20}],[8,8,8]);
  assert.ok(Math.abs(await value('relief')-155.5)<1e-9,`Shift scrubs ten times slower and keeps fractions: ${await value('relief')}`);
  await drag([readout,{x:readout.x,y:readout.y-20},{x:readout.x,y:readout.y-60},{x:readout.x,y:readout.y-100}],[0,8,8,0]);
  assert.ok(Math.abs(await value('relief')-167)<1e-9,`Changing Shift mid-scrub continues from the current value: ${await value('relief')}`);
  for(let step=0;step<5;step++)await undo();
  assert.equal(await value('relief'),0);assert.equal(await value('light'),225);
  await wait(`${field('light')}.classList.contains('disabled')`);
}
async function checkTextureChoice({call,evaluate,settle,theme,directory,device}) {
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const wait=(condition,timeout=10000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20)}poll()})`);
  const box=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const press=async(find,button='left')=>{const p=await evaluate(`(()=>{const n=${find};n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mouseMoved','mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:type==='mouseMoved'?'none':button,buttons:type==='mousePressed'?(button==='right'?2:1):0,clickCount:1});await settle();};
  const click=selector=>press(`document.querySelector(${JSON.stringify(selector)})`);
  const key=async(name,code)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:name,code,windowsVirtualKeyCode:{Enter:13,Escape:27,End:35,Home:36,ArrowLeft:37,ArrowUp:38,ArrowRight:39,ArrowDown:40}[name],...(type==='keyDown'&&name==='Enter'?{text:'\r'}:{})});await settle();};
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const paper=await evaluate('Number(layerApp.state().layers.find(l=>l.paper).id)'),ink=await evaluate('Number(layerApp.state().layers.find(l=>!l.paper).id)');
  const row=`.layer-row[data-layer="${paper}"]`,thumbnail=`${row} .layer-texture`,dialog='#texture-dialog[open]',opened=`!!document.querySelector('${dialog}')`,closed=`!document.querySelector('${dialog}')`;
  const tile=index=>`${dialog} .texture-tile[data-texture-tile="${index}"]`;
  const texture=()=>evaluate(`(()=>{const s=layerApp.state(),control=s.layer_properties.controls.find(c=>c.paper);return{paper:s.layers.find(l=>Number(l.id)===${paper}).paper,layer:Number(s.layer_properties.layer),value:control?Number(control.value.value):null,labels:control?.paper.tiles.map(t=>t.label)??[],import:control?.paper.import?.label??null,revision:String(s.document_file.revision)}})()`);
  const pending=()=>evaluate(`[...document.querySelectorAll('${dialog} .texture-tile')].findIndex(n=>n.getAttribute('aria-checked')==='true')`);
  const editColor=async()=>{
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
    await wait(`document.querySelector('.color-dialog[open] .color-wheel')?.width>0`);await settle();
    const shell=await box('.color-dialog[open]');
    await evaluate(`document.querySelector('.color-dialog[open] .color-cancel').click()`);await wait(`!document.querySelector('.color-dialog[open]')`);
    return shell;
  };
  const layout=()=>evaluate(`(()=>{const d=document.querySelector('${dialog}'),r=d.getBoundingClientRect(),title=d.querySelector('.dialog-title'),t=title.getBoundingClientRect(),tiles=[...d.querySelectorAll('.texture-tile')],frame=n=>n.querySelector('.texture-frame'),top=tiles[0].getBoundingClientRect().top,preview=frame(tiles[0]).querySelector('canvas'),p=preview.getBoundingClientRect(),footer=d.querySelector('.dialog-footer'),page=d.querySelector('.dialog-page');
    return{shell:{x:r.x,y:r.y,width:r.width,height:r.height},title:title.textContent,titleCenter:t.left+t.width/2-(r.left+r.width/2),columns:tiles.filter(n=>n.getBoundingClientRect().top===top).length,frames:tiles.every(n=>frame(n).getBoundingClientRect().width===102&&frame(n).getBoundingClientRect().height===102),preview:[p.width,p.height,preview.width],
      rings:tiles.map(n=>getComputedStyle(frame(n)).outlineStyle==='solid'&&getComputedStyle(frame(n)).outlineWidth==='2px'),captions:tiles.map(n=>n.querySelector('.texture-caption').textContent),focused:tiles.indexOf(document.activeElement),
      buttons:[...footer.querySelectorAll('button')].filter(n=>!n.hidden).map(n=>[n.textContent,n.getBoundingClientRect().height]),divider:getComputedStyle(footer).borderTopWidth,scrolls:page.scrollHeight>page.clientHeight,
      visible:(()=>{const b=page.getBoundingClientRect();return tiles.filter(n=>{const r=n.getBoundingClientRect();return r.top>=b.top&&r.bottom<=b.bottom}).length})()}})()`);
  await send({type:'layer',action:{op:'select',id:paper,mask:false}});
  const shell=await editColor(),initial=await texture(),last=initial.labels.length-1,other=(initial.value+2)%initial.labels.length;
  assert.equal(initial.paper?.editable,true,'Paper rows publish their editable texture');
  await send({type:'layer',action:{op:'select',id:ink,mask:false}});
  const rowLayout=await evaluate(`(()=>{const row=document.querySelector('${row}'),t=row.querySelector('.layer-texture'),visible=[...row.querySelector('.layer-thumbnails').children].filter(n=>!n.hidden&&n.getBoundingClientRect().width>0),r=t.getBoundingClientRect(),p=t.querySelector('.paper-preview').getBoundingClientRect();
    return{id:t.id,last:visible.at(-1)===t,afterContent:row.querySelector('.layer-thumbnail').getBoundingClientRect().right<=r.left,beforeName:r.right<=row.querySelector('.layer-text').getBoundingClientRect().left,size:[r.width,r.height],preview:[p.width,p.height],canvas:!!t.querySelector('canvas'),border:getComputedStyle(t).borderTopWidth,editing:t.classList.contains('editing-target'),title:t.title,label:t.ariaLabel,description:t.getAttribute('aria-description'),others:[...document.querySelectorAll('.layer-row:not([data-layer="${paper}"]) .layer-texture')].every(n=>n.hidden)}})()`);
  assert.deepEqual(rowLayout,{id:`layer-texture-${paper}`,last:true,afterContent:true,beforeName:true,size:[30,30],preview:[28,28],canvas:true,border:'1px',editing:false,title:'Choose Texture',label:'Choose Texture',description:initial.paper.label,others:true},'Paper rows show their texture immediately left of the name');
  await click(thumbnail);await wait(opened);await settle();
  const selected=await texture();
  assert.equal(selected.layer,paper,'The texture thumbnail selects its Paper');
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${paper}).selected`),true);
  const shown=await layout();
  for(const side of ['x','width'])assert.ok(Math.abs(shown.shell[side]-shell[side])<=1,`Choose Texture has the Edit Color dialog's ${side}: ${JSON.stringify([shown.shell,shell])}`);
  assert.ok(shown.visible>=Math.min(shown.captions.length,shown.columns*3),`Choose Texture shows three full rows: ${shown.visible} of ${shown.captions.length} in ${shown.columns} columns`);
  assert.equal(shown.title,'Choose Texture');assert.ok(Math.abs(shown.titleCenter)<=1,'The title is centered');
  assert.deepEqual(shown.captions,initial.labels,'One tile per texture, in order');
  assert.equal(shown.columns,Math.min(6,initial.labels.length),'Six tiles per row at full width');
  assert.equal(shown.frames,true,'Tiles are 102 by 102');
  assert.deepEqual(shown.preview,[96,96,96],'Previews show at their own size');
  assert.deepEqual(shown.rings,initial.labels.map((_,i)=>i===initial.value),'Only the current texture carries the ring');
  assert.equal(shown.focused,initial.value,'Keyboard focus starts on the current texture');
  assert.deepEqual(shown.buttons,[...(initial.import?[[initial.import,34]]:[]),['Cancel',34],['Use Texture',34]]);assert.equal(shown.divider,'1px');
  assert.equal(shown.scrolls,shown.visible<shown.captions.length,'The grid scrolls inside the dialog only when papers are hidden');
  await click(tile(other));
  assert.equal(await pending(),other,'A tap moves the pending texture');
  assert.notEqual(await evaluate(`getComputedStyle(document.querySelector('${tile(other)} .texture-frame')).backgroundColor`),'rgba(0, 0, 0, 0)','Hover tints the tile frame');
  assert.deepEqual(await texture(),selected,'Choosing a tile does not edit the drawing');
  await capture(`texture-dialog-${theme}`);
  await click(`${dialog} #texture-cancel`);await wait(closed);
  assert.deepEqual(await texture(),selected,'Cancel changes nothing');
  const property=await evaluate(`(()=>{const n=document.querySelector('#property-texture'),r=n.getBoundingClientRect(),p=n.querySelector('.paper-preview').getBoundingClientRect();return{name:n.querySelector('.property-texture-name').textContent,title:n.title,description:n.getAttribute('aria-description'),preview:[p.width,p.height],canvas:!!n.querySelector('canvas'),inset:p.left-r.left,height:r.height,select:!!document.querySelector('[data-property-key="texture"] select')}})()`);
  assert.deepEqual(property,{name:initial.paper.label,title:'Texture',description:initial.paper.label,preview:[28,28],canvas:true,inset:6,height:28,select:false},'Properties shows the texture as one button');
  await capture(`texture-rows-${theme}`);
  await click('#property-texture');await wait(opened);
  assert.equal(await pending(),initial.value,'Reopening starts from the current texture');
  const columns=(await layout()).columns;
  let expected=initial.value;
  for(const [name,next] of [['ArrowRight',v=>v+1],['ArrowDown',v=>v+columns],['ArrowLeft',v=>v-1],['ArrowUp',v=>v-columns],['End',()=>last],['ArrowDown',v=>v+columns],['Home',()=>0],['ArrowUp',v=>v-columns]]) {
    const target=next(expected);if(target>=0&&target<=last)expected=target;await key(name,name);
    assert.equal(await pending(),expected,`${name} moves the pending texture`);
    assert.equal(await evaluate('document.activeElement.dataset.textureTile'),String(expected),`${name} moves keyboard focus`);
  }
  await key('Escape','Escape');await wait(closed);
  assert.deepEqual(await texture(),selected,'Escape changes nothing');
  await click('#property-texture');await wait(opened);await click(`${dialog} #texture-apply`);await wait(closed);
  assert.deepEqual(await texture(),selected,'Use Texture without a new choice edits nothing');
  await click('#property-texture');await wait(opened);await click(tile(other));await click(`${dialog} #texture-apply`);await wait(closed);
  const applied=await texture();
  assert.deepEqual([applied.value,applied.paper.label],[other,initial.labels[other]],'Use Texture applies the pending texture');
  assert.deepEqual(await evaluate(`[document.querySelector('#property-texture .property-texture-name').textContent,document.querySelector('${thumbnail}').getAttribute('aria-description')]`),[initial.labels[other],initial.labels[other]]);
  await invoke('undo');assert.equal((await texture()).value,initial.value,'One Undo restores the texture');
  await invoke('redo');assert.equal((await texture()).value,other);
  await invoke('undo');
  const [step,offset]=initial.value<last?['ArrowRight',1]:['ArrowLeft',-1];
  await click(thumbnail);await wait(opened);await key(step,step);await key('Enter','Enter');await wait(closed);
  assert.equal((await texture()).value,initial.value+offset,'Enter uses the pending texture');
  await invoke('undo');assert.equal((await texture()).value,initial.value);
  const german=await readFile(new URL('../../assets/locales/de/resources.ftl',import.meta.url),'utf8'),copy=name=>german.split('\n').find(line=>line.startsWith(`${name} = `)).slice(name.length+3);
  const settings=await evaluate('JSON.stringify(layerApp.state().settings)'),language=async(saved,tag)=>{await send({type:'restore_saved_settings',saved});await wait(`!layerApp.app.language_pending()&&document.documentElement.lang===${JSON.stringify(tag)}`,30000);await settle();};
  await click(thumbnail);await wait(opened);await click(tile(0));
  await evaluate(`window.textureDialogNode=document.querySelector('${dialog}')`);
  await language(await evaluate(`JSON.stringify({...layerApp.state().settings,language:{Explicit:'de'}})`),'de');
  const relabeled=await evaluate(`(()=>{const d=document.querySelector('${dialog}');return{same:d===textureDialogNode,title:d.querySelector('.dialog-title').textContent,label:d.ariaLabel,buttons:[...d.querySelectorAll('.dialog-footer button:not([hidden])')].map(n=>n.textContent),captions:[...d.querySelectorAll('.texture-caption')].map(n=>n.textContent),labels:layerApp.state().layer_properties.controls.find(c=>c.paper).paper.tiles.map(t=>t.label),cancel:layerApp.app.bootstrap_view().common.cancel,focused:document.activeElement?.dataset.textureTile}})()`);
  assert.deepEqual(relabeled,{same:true,title:copy('resources-paper-choose'),label:copy('resources-paper-choose'),buttons:[...(initial.import?[copy('resources-paper-import')]:[]),relabeled.cancel,copy('resources-paper-use')],captions:relabeled.labels,labels:relabeled.labels,cancel:relabeled.cancel,focused:'0'},'A language change relabels the open dialog');
  assert.notDeepEqual(relabeled.captions,initial.labels);assert.equal(await pending(),0,'Relabeling keeps the pending texture');
  await capture(`texture-dialog-de-${theme}`);
  await language(settings,'en');
  await click(`${dialog} #texture-cancel`);await wait(closed);
  assert.equal((await texture()).value,initial.value);
  await click(thumbnail);await wait(opened);await click(tile(other));
  await send({type:'layer',action:{op:'select',id:ink,mask:false}});
  await click(`${dialog} #texture-apply`);await wait(closed);
  assert.equal(await evaluate("document.querySelector('#status').textContent"),await evaluate('layerApp.app.bootstrap_view().action_failed'),'Use Texture reports a dialog whose Paper left Properties');
  await send({type:'layer',action:{op:'select',id:paper,mask:false}});
  assert.equal((await texture()).value,initial.value,'A stale dialog edits nothing');
  const unmasked=()=>evaluate(`(()=>{const s=layerApp.state(),l=s.layers.find(l=>Number(l.id)===${paper});return{mask:l.mask_id??null,has:l.has_mask,revision:String(s.document_file.revision)}})()`),before=await unmasked();
  const footerMask=`document.querySelector('.layer-footer [aria-label="Add mask"]')`;
  assert.equal(await evaluate(`${footerMask}.disabled`),true,'Paper cannot take a mask from the Layers footer');
  await press(footerMask);
  await press(`document.querySelector('${row} .layer-name')`,'right');
  const contextMenu=`document.querySelector('.panel-context-menu:popover-open')`,maskLabel=await evaluate(`layerApp.app.layer_menu(layerApp.state().layers.find(l=>Number(l.id)===${paper}).id,false).sections.flat().find(i=>i.sections?.flat().some(s=>s.action?.action?.op==='add_mask')).label`);
  await wait(`!!${contextMenu}`);
  await press(`[...${contextMenu}.querySelectorAll('button')].find(n=>n.querySelector('.menu-label')?.textContent===${JSON.stringify(maskLabel)})`);
  const maskItems=()=>evaluate(`[...${contextMenu}.querySelectorAll(':scope > button:not(.submenu-back)')].map(n=>[n.querySelector('.menu-label').textContent,n.disabled])`);
  const refused=[['Add mask',true],['Mask: reveal selection',true],['Mask: hide selection',true],['Paste mask',true]];
  assert.deepEqual(await maskItems(),refused,'The Paper Mask submenu offers nothing');
  await capture(`texture-row-mask-menu-${theme}`);
  await press(`[...${contextMenu}.querySelectorAll('button')].find(n=>n.querySelector('.menu-label')?.textContent==='Add mask')`);
  assert.deepEqual(await maskItems(),refused);
  await key('Escape','Escape');await wait(`!${contextMenu}`);
  assert.deepEqual(await unmasked(),{...before,mask:null,has:false},'Paper stays without a mask');
  await send({type:'layer',action:{op:'lock',id:paper,value:true}});await send({type:'layer',action:{op:'select',id:ink,mask:false}});
  assert.equal((await texture()).paper.editable,false,'Locking Paper makes its texture read-only');
  await click(thumbnail);await new Promise(resolve=>setTimeout(resolve,250));
  assert.equal(await evaluate(opened),false,'A locked Paper does not open Choose Texture');
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${paper}).selected`),true,'The locked texture thumbnail still selects its Paper');
  await send({type:'layer',action:{op:'lock',id:paper,value:false}});
  if(!device) {
    await call('Emulation.setDeviceMetricsOverride',{width:700,height:900,deviceScaleFactor:1,mobile:false});await settle();
    const narrowShell=await editColor();
    await evaluate(`document.querySelector('${thumbnail}').click()`);await wait(opened);await settle();
    const narrow=await layout();
    for(const side of ['x','y','width','height'])assert.ok(Math.abs(narrow.shell[side]-narrowShell[side])<=1,`Narrow Choose Texture matches Edit Color ${side}: ${JSON.stringify([narrow.shell,narrowShell])}`);
    assert.ok(narrow.columns<6&&narrow.frames,'Narrow dialogs wrap full-size tiles');
    await capture(`texture-dialog-narrow-${theme}`);
    await evaluate(`document.querySelector('${dialog} #texture-cancel').click()`);await wait(closed);
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});await settle();
  }
}

export async function checkPaperRows({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/paper-ports/web';
  await mkdir(directory,{recursive:true});
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const layer=action=>send({type:'layer',action});
  const invoke=command=>send({type:'invoke',command});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+5000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20)}poll()})`);
  const rows=()=>evaluate('layerApp.state().layers.map(l=>({id:Number(l.id),label:l.label,drawing:l.drawing,mask_selected:l.mask_selected}))');
  const missing=()=>evaluate("!!document.querySelector('#layer-add-paper')");
  const mouse=async(type,x,y)=>{await call('Input.dispatchMouseEvent',{type,x,y,button:type==='mouseMoved'?'none':'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const bounds=()=>evaluate("(()=>{const n=document.querySelector('#layer-add-paper');n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()");
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await checkPaperLighting({call,evaluate,settle,theme,directory});
    await checkTextureChoice({call,evaluate,settle,theme,directory,device});
    await checkScaleRelief({call,evaluate,settle,theme,directory});
    assert.equal(await missing(),false,'Paper suppresses Add paper');
    const initialCount=(await rows()).length;
    await evaluate(`document.querySelector('.layer-footer [aria-label="New layer"]').focus()`);
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13,...(type==='keyDown'?{text:'\r'}:{})});
    await settle();await wait(`layerApp.state().layers.length===${initialCount+1}`);assert.equal((await rows()).length,initialCount+1,'Existing footer button keeps native Enter activation');
    await invoke('undo');
    let all=await rows();
    let paper=all.find(l=>l.label==='Paper').id;
    assert.equal(String(all.at(-1).id),String(paper),'Paper is the bottom row');
    await layer({op:'delete',id:paper});
    assert.equal(await missing(),true,'Deleting Paper offers Add paper');
    const geometry=await evaluate("(()=>{const n=document.querySelector('#layer-add-paper'),s=getComputedStyle(n),r=n.getBoundingClientRect(),previous=n.previousElementSibling,p=previous.getBoundingClientRect();return{tag:n.tagName,last:n===n.parentElement.lastElementChild,parent:n.parentElement.id,height:r.height,rowHeight:p.height,top:r.top,bottom:p.bottom,weight:s.fontWeight,radius:s.borderRadius,align:s.justifyContent,label:n.ariaLabel}})()");
    assert.deepEqual({tag:geometry.tag,last:geometry.last,parent:geometry.parent,height:geometry.height,weight:geometry.weight,radius:geometry.radius,align:geometry.align,label:geometry.label},{tag:'BUTTON',last:true,parent:'layer-rows',height:40,weight:'400',radius:'0px',align:'center',label:'Add paper'});
    assert.equal(geometry.height,geometry.rowHeight,'Add paper matches ordinary row height');
    assert.equal(geometry.top,geometry.bottom,'Add paper immediately follows the last layer');
    const textSize=await evaluate("document.documentElement.style.getPropertyValue('--ui-text-size')");
    try {
      await evaluate("document.documentElement.style.setProperty('--ui-text-size','24px')");await settle();
      const enlarged=await evaluate("(()=>{const n=document.querySelector('#layer-add-paper'),r=n.getBoundingClientRect(),previous=n.previousElementSibling,p=previous.getBoundingClientRect(),label=n.querySelector('.layer-name'),l=label.getBoundingClientRect(),range=document.createRange();range.selectNodeContents(label);const text=range.getBoundingClientRect();return{font:getComputedStyle(label).fontSize,height:r.height,rowHeight:p.height,top:r.top,previousBottom:p.bottom,textTop:text.top,textBottom:text.bottom,textLeft:text.left,textRight:text.right,labelTop:l.top,labelBottom:l.bottom,labelLeft:l.left,labelRight:l.right,rowTop:r.top,rowBottom:r.bottom}})()");
      assert.equal(enlarged.font,'24px');assert.equal(enlarged.height,enlarged.rowHeight,'Enlarged Add paper matches the standard row height');
      assert.equal(enlarged.top,enlarged.previousBottom,'Enlarged rows keep their vertical spacing');
      assert.ok(enlarged.textTop>=enlarged.labelTop-.5&&enlarged.textBottom<=enlarged.labelBottom+.5,'Enlarged text fits its unclipped label line box');
      assert.ok(enlarged.textLeft>=enlarged.labelLeft-.5&&enlarged.textRight<=enlarged.labelRight+.5,'Enlarged text fits its label width');
      assert.ok(enlarged.textTop>=enlarged.rowTop+2&&enlarged.textBottom<=enlarged.rowBottom-2,'Enlarged text fits inside row padding');
    } finally {
      await evaluate(`document.documentElement.style.setProperty('--ui-text-size',${JSON.stringify(textSize)})`);await settle();
    }
    const point=await bounds();
    await mouse('mouseMoved',point.x,point.y);
    assert.equal(await evaluate("document.querySelector('#layer-add-paper').matches(':hover')"),true);
    await mouse('mousePressed',point.x,point.y);
    assert.equal(await evaluate("document.querySelector('#layer-add-paper').matches(':active')"),true);
    await mouse('mouseMoved',point.x-400,point.y);
    await mouse('mouseReleased',point.x-400,point.y);
    assert.equal(await missing(),true,'Releasing outside cancels Add paper');
    for(const [key,code,value,text] of [['Enter','Enter',13,'\r'],[' ','Space',32,' ']]) {
      await evaluate("document.querySelector('#layer-add-paper').focus()");
      assert.equal(await evaluate("document.activeElement.id"),'layer-add-paper','Add paper receives keyboard focus');
      await call('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:value,text});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:value});await settle();
      await wait("!document.querySelector('#layer-add-paper')");
      assert.equal(await missing(),false,`${code} activates Add paper`);
      await invoke('undo');assert.equal(await missing(),true);
    }
    for(const device of ['touch','pen']) {
      const contact=async(phase,p)=>{
        if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[phase],touchPoints:phase==='up'?[]:[{id:1,...p}]});
        else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[phase],...p,pointerType:'pen',button:'left',buttons:phase==='up'?0:1,clickCount:1,force:phase==='up'?0:.7});
        await settle();
      };
      const start=await bounds(),outside={x:start.x-400,y:start.y};
      const before=(await rows()).length;
      await contact('down',start);await contact('move',outside);await contact('up',outside);
      assert.equal(await missing(),true,`${device} release outside cancels Add paper`);
      assert.equal((await rows()).length,before);
      const tap=await bounds();await contact('down',tap);await contact('up',tap);
      await wait("!document.querySelector('#layer-add-paper')");
      assert.equal(await missing(),false,`${device} tap adds Paper`);
      assert.equal((await rows()).length,before+1,`${device} tap adds exactly one Paper`);
      await invoke('undo');assert.equal(await missing(),true,`${device} insertion is one Undo step`);
      assert.equal((await rows()).length,before);
    }
    if(device&&process.env.CAPY_ANDROID_SERIAL) {
      const shell=(...args)=>promisify(execFile)(process.env.ADB??'adb',['-s',process.env.CAPY_ANDROID_SERIAL,'shell',...args]);
      const screen=await evaluate('({ratio:devicePixelRatio,width:screen.width,height:screen.height})');
      const probe=[Math.round(screen.width*screen.ratio/2),Math.round(screen.height*screen.ratio/2)];
      await evaluate(`(()=>{const n=document.createElement('div');n.id='paper-calibration';n.style.cssText='position:fixed;inset:0;z-index:2147483647';n.addEventListener('pointerdown',e=>{window.paperCalibration={x:e.clientX,y:e.clientY};e.preventDefault();e.stopPropagation()});document.body.append(n)})()`);
      try {
        await shell('input','touchscreen','tap',...probe.map(String));await wait('window.paperCalibration');
        const calibration=await evaluate('paperCalibration');
        await evaluate("document.querySelector('#paper-calibration').remove()");
        const p=await bounds(),before=(await rows()).length;
        await evaluate("document.querySelector('#layer-add-paper').addEventListener('pointerdown',e=>{window.paperOsPointer=e.pointerType},{once:true})");
        const physical=[Math.round(probe[0]+(p.x-calibration.x)*screen.ratio),Math.round(probe[1]+(p.y-calibration.y)*screen.ratio)];
        await shell('input','touchscreen','tap',...physical.map(String));
        await wait("!document.querySelector('#layer-add-paper')");
        assert.equal(await evaluate('paperOsPointer'),'touch','OS touch reaches Add paper');
        assert.equal((await rows()).length,before+1,'OS touch adds exactly one Paper');
        await invoke('undo');assert.equal(await missing(),true,'OS insertion is one Undo step');
      } finally {
        await evaluate("document.querySelector('#paper-calibration')?.remove();delete window.paperCalibration;delete window.paperOsPointer");
      }
    }
    const activate=await bounds();await mouse('mousePressed',activate.x,activate.y);await mouse('mouseReleased',activate.x,activate.y);
    await wait("!document.querySelector('#layer-add-paper')");
    assert.equal(await missing(),false,'Pointer activates Add paper');
    all=await rows();paper=all.find(l=>l.label==='Paper').id;
    assert.equal(String(all.at(-1).id),String(paper));
    await invoke('undo');assert.equal(await missing(),true);
    await invoke('redo');assert.equal(await missing(),false);
    const ink=all.find(l=>l.id!==paper).id;
    await layer({op:'select',id:paper,mask:false});
    assert.deepEqual((await rows()).filter(l=>l.drawing).map(l=>l.id),[ink],'Paper draws into ink above');
    await layer({op:'select',id:ink,mask:false});await layer({op:'group_selected'});
    await layer({op:'select',id:paper,mask:false});
    assert.equal((await rows()).some(l=>l.drawing),false,'A group above Paper blocks fallback drawing');
    await invoke('undo');
    for(const program of ['solid_color','gradient_fill']) {
      await send({type:'effect',action:{op:'insert',effect:program}});
      const fill=(await rows()).find(l=>l.label===(program==='solid_color'?'Solid Color':'Gradient Fill')).id;
      await layer({op:'new',group:false,clipped:false});
      const above=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
      await layer({op:'select',id:fill,mask:false});
      assert.deepEqual((await rows()).filter(l=>l.drawing).map(l=>l.id),[above],`${program} draws into ink above`);
      await layer({op:'add_mask',id:fill,replace:false});await layer({op:'select',id:fill,mask:false});
      assert.equal((await rows()).find(l=>l.id===fill).mask_selected,true,`${program} selects its existing mask`);
      await layer({op:'delete',id:fill});await layer({op:'delete',id:above});
    }
    for(const row of await rows())await layer({op:'delete',id:row.id});
    assert.equal((await rows()).length,0);
    assert.equal(await missing(),true,'An empty drawing offers Add paper');
    let position=await bounds();await mouse('mousePressed',position.x,position.y);await mouse('mouseReleased',position.x,position.y);
    await wait("!document.querySelector('#layer-add-paper')");
    assert.equal((await rows()).length,1);assert.equal(await missing(),false);
    paper=(await rows())[0].id;
    for(let i=0;i<20;i++)await layer({op:'new',group:false,clipped:false});
    await layer({op:'delete',id:paper});
    position=await bounds();
    assert.equal(await evaluate("(()=>{const n=document.querySelector('#layer-rows'),b=document.querySelector('#layer-add-paper').getBoundingClientRect(),r=n.getBoundingClientRect();return n.scrollHeight>n.clientHeight&&n.scrollTop>0&&b.bottom<=r.bottom+1&&b.top>=r.top})()"),true,'Add paper scrolls after the bottom layer');
    const capture=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/add-paper-${theme}.png`,Buffer.from(capture.data,'base64'));
    await mouse('mousePressed',position.x,position.y);await mouse('mouseReleased',position.x,position.y);
    await wait("!document.querySelector('#layer-add-paper')");
    assert.equal(await missing(),false);assert.equal((await rows()).at(-1).label,'Paper');
    for(const row of (await rows()).filter(l=>l.label!=='Paper'))await layer({op:'delete',id:row.id});
    await layer({op:'new',group:false,clipped:false});
  }
  console.log('PASS: Paper lighting (disabled while flat, Light dial row geometry, press, drag past the dial, keys, focus ring and double-click reset as single Undo steps, snapped and Shift scrubs); Scale relief while dragging and hidden after release; Choose Texture from Paper rows and Properties (tiles, pending choice, keyboard, Cancel/Escape, Use Texture with one Undo step, live language, no Paper mask, locked Paper, Edit Color size); Add paper layout, mouse/touch/pen activation and cancellation, keyboard, Undo/Redo, empty and scrolling lists, bottom Paper and fill mask/group drawing policy in both themes');
}
