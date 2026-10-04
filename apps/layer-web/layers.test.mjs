import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

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
  assert.equal(await evaluate("document.querySelector('#zen-button').title"), "Zen mode (Ctrl+Alt+J)");
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
  const send=async value=>{await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);await settle();};
  const layer=action=>send({type:'layer',action});
  const invoke=command=>send({type:'invoke',command});
  const active=()=>evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  const row=id=>`#layer-rows .layer-row[data-layer="${id}"]`;
  const attachment=()=>evaluate(`(()=>{const n=document.querySelector('.layer-attachment');return{label:n.ariaLabel,description:n.getAttribute('aria-description'),icon:n.firstChild.dataset.asset,checked:n.getAttribute('aria-pressed'),disabled:n.disabled}})()`);
  const rect=async selector=>{
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`);await settle();
    return evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  };
  const mouse=async(type,p)=>{await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});await settle();};
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const attach=async()=>{await evaluate("document.querySelector('.layer-attachment').click()");await settle();};
  const insert=async effect=>{await send({type:'effect',action:{op:'insert',effect}});return active();};
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy');
  await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');
  if(await evaluate("layerApp.state().commands.find(c=>c.id==='reset_layout').enabled")) {
    await invoke('reset_layout');
    await wait("!!document.querySelector('.workspace-form[open] .suggested-action')");
    await evaluate("document.querySelector('.workspace-form[open] .suggested-action').click()");
    await wait("!document.querySelector('.workspace-form[open]')&&!JSON.parse(layerApp.app.workspace_view()).busy");
  }
  await layer({op:'new',group:false,clipped:false});const base=await active();
  await layer({op:'rename',id:base,name:'Base {ink} 🎨'});
  const blur=await insert('gaussian_blur');await attach();
  await layer({op:'select',id:base,mask:false});
  await layer({op:'new',group:false,clipped:true});const owner=await active();
  await layer({op:'rename',id:owner,name:'Shadows'});
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
