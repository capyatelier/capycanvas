import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Run inside the isolated selection-tools fixture on desktop or Android Chrome.
export async function checkPaintableSelections({call,evaluate,settle,send,invoke,point,at}) {
  const view=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_tools,(_,v)=>typeof v==="bigint"?String(v):v))');
  const layersOpener=await evaluate('String(layerApp.state().workspace.layout.header.zones.flat().find(e=>e.item.control?.panel==="layers").id)');
  const toggleLayers=async()=>{await evaluate(`document.querySelector('[data-header-item="${layersOpener}"] .header-tool').click()`);await settle();};
  const colors=await evaluate('JSON.stringify(layerApp.state().colors,(_,v)=>typeof v==="bigint"?String(v):v)');
  await send({type:'select_brush',id:1});
  await send({type:'set_brush_size',value:70});
  await invoke('quick_mask');
  assert.equal((await view()).quick_mask,true);
  await invoke('quick_mask');
  assert.equal((await view()).has_selection,false,'Leaving an untouched Quick Mask retains no selection');
  await invoke('quick_mask');
  assert.equal(await evaluate('!!document.querySelector("#selection-mask-actions")'),false);
  assert.equal(await evaluate('layerApp.state().layer_properties.controls.length'),3);
  assert.equal(await evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="mask_mode").value.value'),0);
  assert.equal(await evaluate('layerApp.state().layers[0].quick_mask'),true);
  assert.equal(await evaluate('layerApp.state().layers[0].selection_icon'),'layer-brush-symbolic');
  await toggleLayers();
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;const poll=()=>{if(document.querySelector('.content-drawer .layer-row[data-layer="0"] canvas')?.dataset.previewRevision)resolve(true);else if(performance.now()>end)reject(Error('Quick Mask thumbnail'));else setTimeout(poll,50);};poll();})`);

  await point(at(-45,0));
  assert.equal(await evaluate('!!layerApp.state().customization.drawer'),false,'Canvas contact dismisses the Layers drawer');
  // The dismissing contact is consumed; begin painting with a fresh contact.
  await point(at(-45,0),'pen','mouseReleased');
  await point(at(-45,0));
  const maskPixels=async()=>{
    const shot=await call('Page.captureScreenshot',{format:'png'}), center=at(0,0);
    return evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${shot.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(i,0,0);const scale=c.width/innerWidth;const p=g.getImageData((${center.x}-120)*scale,(${center.y}-70)*scale,240*scale,140*scale).data;let red=0;for(let n=0;n<p.length;n+=4)if(p[n]>p[n+1]+40&&p[n]>p[n+2]+40)red++;return red;})()`);
  };
  let previousPixels=await maskPixels();
  for(const x of [-40,-35]) {
    await point(at(x,0),'pen','mouseMoved');
    const next=await maskPixels();
    assert.ok(next>previousPixels+5,`G-Pen updates sub-spacing movement during contact: ${previousPixels} -> ${next}`);
    previousPixels=next;
  }
  for(let x=-30;x<=45;x+=15)await point(at(x,0),'pen','mouseMoved');
  await point(at(45,0),'pen','mouseReleased');
  assert.equal(await evaluate('JSON.stringify(layerApp.state().colors,(_,v)=>typeof v==="bigint"?String(v):v)'),colors,'Mask colors leave artwork colors intact');
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/selection-web';
  await mkdir(directory,{recursive:true});
  await toggleLayers();
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${directory}/quick-mask-${theme}.png`,Buffer.from(shot.data,'base64'));
    const center=at(0,0);
    const mask=shot;
    const red=await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${mask.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(i,0,0);const scale=c.width/innerWidth;const p=g.getImageData((${center.x}-90)*scale,(${center.y}-35)*scale,180*scale,70*scale).data;let red=0;for(let n=0;n<p.length;n+=4)if(p[n]>p[n+1]+40&&p[n]>p[n+2]+40)red++;return red;})()`);
    assert.ok(red>100,`${theme}: painted mask reaches presented pixels (${red})`);
  }
  await toggleLayers();
  await send({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:true}});
  await evaluate(`if(!document.querySelector('.effect-properties')?.getBoundingClientRect().height)document.querySelector('.dock-tab[data-panel="properties"],.column-tab[data-panel="properties"]')?.click()`);await settle();
  await evaluate(`(()=>{const n=[...document.querySelectorAll('.effect-properties select')].find(n=>n.options[0]?.text==='Paint selection');n.value='1';n.dispatchEvent(new Event('change',{bubbles:true}));})()`);await settle();
  assert.equal(await evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="mask_mode").value.value'),1);
  await send({type:'set_color',rgba:[.1,.6,.9,1]});
  assert.ok(await evaluate('layerApp.state().layer_tools.mask_editing.colors.foreground.rgba.every((v,i)=>Math.abs(v-[.1,.6,.9,1][i])<1e-6)'), 'Grayscale mask preserves full-color picking');
  await evaluate(`(()=>{const n=[...document.querySelectorAll('.effect-properties select')].find(n=>n.options[0]?.text==='Paint selection');n.value='0';n.dispatchEvent(new Event('change',{bubbles:true}));})()`);await settle();
  const propertiesShot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/quick-mask-properties.png`,Buffer.from(propertiesShot.data,'base64'));
  const propertyMaskPixels=await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${propertiesShot.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(i,0,0);const p=g.getImageData(0,100,c.width*.75,c.height-100).data;let red=0;for(let n=0;n<p.length;n+=4)if(p[n]>p[n+1]+40&&p[n]>p[n+2]+40)red++;return red;})()`);
  assert.ok(propertyMaskPixels>100,'Changing painting convention preserves the visible mask');
  await send({type:'set_color',rgba:[0,.5,1,1]});
  await evaluate(`document.querySelector('.effect-properties [data-action="paper-color-bucket"]').click()`);await settle();
  assert.deepEqual(await evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="mask_color").value.value.rgba'),[0,.5,1,1],'Bucket copies the mask painting color');
  await send({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:false}});
  await toggleLayers();
  await evaluate(`document.querySelector('.content-drawer .layer-row[data-layer="0"] .selection-layer-load').click()`);await settle();
  assert.equal((await view()).quick_mask,false);
  assert.equal((await view()).has_selection,true);
  await invoke('quick_mask');
  await invoke('save_selection_layer');
  assert.equal((await view()).quick_mask,false,'Saving exits Quick Mask');
  await send({type:'layer',action:{op:'cancel_rename'}});
  const id=await evaluate('String(layerApp.state().layers.find(l=>l.selection_layer).id)');
  assert.equal(String((await view()).mask_editing.layer),id,'Saving activates the saved mask');
  await invoke('return_to_artwork');
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>String(l.id)==='${id}').visible`),false,'Leaving a selection layer hides its overlay');
  const row=`.content-drawer .layer-row[data-layer="${id}"]`;
  assert.equal(await evaluate(`!!document.querySelector(${JSON.stringify(row+' .layer-name-entry')})`),false,'Model cancellation removes the rename editor');
  const sizes=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(row)});const a=r.querySelector('.layer-thumbnail').getBoundingClientRect(),b=r.querySelector('.selection-layer-load').getBoundingClientRect();return {aw:a.width,bw:b.width,gap:b.left-a.right};})()`);
  assert.ok(Math.abs(sizes.aw-sizes.bw)<=2 && sizes.gap>=0 && sizes.gap<10,JSON.stringify(sizes));
  const name=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(row+' .layer-name')});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  for(const clickCount of [1,2]) {
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...name,button:'left',buttons:1,clickCount});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...name,button:'left',buttons:0,clickCount});
  }
  await settle();
  assert.ok(await evaluate(`!!document.querySelector(${JSON.stringify(row+' .layer-name-entry')})`),'Double-click name edits inline');
  await call('Input.insertText',{text:'Saved test mask'});await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
  assert.equal(await evaluate(`layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(id)}).label`),'Saved test mask');
  // Actual tablet touch contacts must reach the same inline editor.
  const touchName=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(row+' .layer-name')}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  for(let tap=0;tap<2;tap++) {
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...touchName}]});
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  }
  await settle();
  assert.ok(await evaluate(`!!document.querySelector(${JSON.stringify(row+' .layer-name-entry')})`),'Double-tap name edits inline');
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
  // BigInt IDs follow the same native serialization used by real layer buttons.
  await evaluate(`layerApp.dispatch({type:'selection',action:{op:'edit_layer',id:BigInt(${JSON.stringify(id)})}})`);await settle();
  assert.equal(String((await evaluate('String(layerApp.state().layer_tools.mask_editing.layer)'))),id);
  await send({type:'selection',action:{op:'begin_resize',grow:true,layer:Number(id)}});
  assert.ok(await evaluate('document.querySelector("#selection-resize-dialog").open'));
  await evaluate('[...document.querySelectorAll("#selection-resize-dialog button")].find(b=>b.textContent==="Apply").click()');await settle();
  assert.equal(await evaluate('!!document.querySelector("#selection-resize-dialog")'),false);
  await invoke('undo');await invoke('redo');
  await send({type:'selection',action:{op:'begin_resize',grow:false,layer:Number(id)}});
  assert.equal(await evaluate('document.querySelector("#selection-resize-dialog h2").textContent'),'Shrink Selection');
  await evaluate('[...document.querySelectorAll("#selection-resize-dialog button")].find(b=>b.textContent==="Cancel").click()');await settle();
  assert.equal(await evaluate('!!document.querySelector("#selection-resize-dialog")'),false);
  await invoke('clear_selection_mask');
  await invoke('return_to_artwork');
  await evaluate(`layerApp.dispatch({type:'selection',action:{op:'load_layer',id:BigInt(${JSON.stringify(id)}),mode:'new',inverted:false}})`);await settle();
  assert.equal((await view()).has_selection,true,'Loading an empty stored mask retains an explicitly empty selection');
  await invoke('deselect');
  await invoke('reselect');
  assert.equal((await view()).has_selection,true);
  await invoke('deselect');
  await invoke('rectangle_select');await invoke('selection_new');
  for(const [key,code,windowsVirtualKeyCode,modifiers,mode] of [['Shift','ShiftLeft',16,8,'selection_add'],['Alt','AltLeft',18,1,'selection_subtract'],['Shift','ShiftLeft',16,9,'selection_intersect']]) {
    await call('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode,modifiers});await settle();
    assert.ok(await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(mode)}).selected`),mode);
    await call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode,modifiers:0});await settle();
    assert.ok(await evaluate('layerApp.state().commands.find(c=>c.id==="selection_new").selected'));
  }
  console.log('PASS Quick Mask normal row and properties, pen coverage, light/dark overlay, compact load icon, mouse/touch rename, Grow/Shrink, saved selection edit/load, reselect');
}
