import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Dedicated origin only. Uses the actual WebGPU masks and native browser input.
export async function checkTonalSelections({call,evaluate,settle}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/tonal-parity/web';await mkdir(dir,{recursive:true});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(poll,30)}poll()})`);
  const idle=async()=>{await settle();await evaluate('new Promise(r=>setTimeout(r,300))');await wait('!layerApp.state().document_file.busy');assert.equal(await evaluate('layerApp.state().host_error??null'),null);};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)});null`);await idle();};
  const invoke=command=>send({type:'invoke',command});
  const capture=async name=>{const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${name}.png`,Buffer.from(shot.data,'base64'));return shot;};
  const pixelsAt=(shot,points)=>evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${shot.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d');g.drawImage(i,0,0);return ${JSON.stringify(points)}.map(p=>Array.from(g.getImageData(p.x*c.width/innerWidth,p.y*c.height/innerHeight,1,1).data))})()`);
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});const r=n.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  async function gesture(a,b,device='mouse',cancel=false) {
    if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...a}]});
      await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:1,...b}]});
      await call('Input.dispatchTouchEvent',{type:cancel?'touchCancel':'touchEnd',touchPoints:[]});
    } else {
      await call('Input.dispatchMouseEvent',{type:'mousePressed',...a,button:'left',buttons:1,clickCount:1,pointerType:device,force:.7});
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...b,button:'left',buttons:1,pointerType:device,force:.7});
      if(cancel)for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...b,button:'left',buttons:0,clickCount:1,pointerType:device});
    }
    await idle();
  }
  const click=async(selector,device='mouse')=>{const r=await rect(selector);assert.ok(r.width&&r.height,selector);const p={x:r.x+r.width/2,y:r.y+r.height/2};await gesture(p,p,device);};
  const values=()=>evaluate('["tonal_lower","tonal_upper"].map(id=>layerApp.state().tool_settings.find(f=>f.id===id).value)');
  async function type(selector,text) {
    await click(`${selector} .number-value`);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
    await call('Input.insertText',{text});
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await idle();
  }
  async function workspace(id) {
    await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify({type:'switch',id:`builtin:workspace:${id}`}))});null`);
    await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:${id}'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await idle();
  }
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  await wait('layerApp.state().commands.find(c=>c.id==="open_document")?.enabled');
  await evaluate(`window.tonalOpen=window.showOpenFilePicker;window.showOpenFilePicker=async()=>{const c=document.createElement('canvas');c.width=500;c.height=200;const g=c.getContext('2d');[0,64,128,190,255].forEach((v,i)=>{g.fillStyle='rgb('+[v,v,v].join(',')+')';g.fillRect(i*100,0,100,200)});const blob=await new Promise(r=>c.toBlob(r));return[{name:'tonal-patches.png',getFile:async()=>new File([blob],'tonal-patches.png',{type:'image/png'})}]}`);
  try {
    await invoke('open_document');await wait('layerApp.state().document_file.unsaved_name==="tonal-patches"&&!layerApp.state().document_file.busy');
    await workspace('painter');await invoke('fit_canvas');await invoke('tonal_select');
    const header=await evaluate('String(layerApp.state().workspace.layout.header.zones.flat().find(e=>e.item.control?.command==="select").id)');
    const opener=`[data-header-item="${header}"] .header-tool`;
    await click(opener);if(!await evaluate('!!layerApp.state().customization.drawer'))await click(opener);
    const form='.content-drawer .tonal-settings';
    assert.equal(await evaluate(`document.querySelectorAll('${form} [data-tool-choice-tone]').length`),6);
    assert.equal(await evaluate(`!!document.querySelector('${form} .selection-menu-button')`),false);
    await click(`${form} [data-tool-choice-tone="4"]`);await wait('layerApp.state().layer_tools.has_selection');
    assert.equal(await evaluate('layerApp.state().layer_tools.quick_mask'),false,'ordinary selection first');
    await capture('sdr-presets');
    await invoke('quick_mask');
    await click(opener);await invoke('fit_canvas');
    // Sample actual shaded pixels at the ends of the gray chart.
    const samples=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return [50,450].map(x=>({x:r.x+(x*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(100*c.zoom+c.translation[1])*r.height/c.viewport[1]}))})()`);
    const shot=await capture('quick-mask');
    const pixels=await pixelsAt(shot,samples);
    assert.ok(pixels[1][0]>pixels[1][1]+30,'highlight mask shades the white patch');
    assert.ok(Math.abs(pixels[0][0]-pixels[0][1])<15,'black remains outside highlights');
    await click(opener);await click(`${form} [data-tool-choice-tone="5"]`);
    await type(`${form} [data-tool-setting=tonal_lower]`,'-6.0');await type(`${form} [data-tool-setting=tonal_upper]`,'1.0');
    await type(`${form} [data-tool-setting=tonal_lower]`,'-6.0');
    const bounds=await rect(`${form} .range-control`),track=await rect(`${form} .interval-track`);
    assert.equal(bounds.height,28);assert.ok(track.width>bounds.width*.6);
    for(const id of ['tonal_lower','tonal_upper'])assert.ok((await rect(`${form} [data-tool-setting=${id}]`)).width<=48);
    for(const device of ['mouse','touch','pen'])for(const index of [0,1]) {
      const thumb=await rect(`${form} .interval-thumb.${index?'upper':'lower'}`),before=await values();
      const a={x:thumb.x+3,y:thumb.y+7};await gesture(a,{x:a.x+(index?8:-8),y:a.y},device);
      const after=await values();assert.ok(index?after[1]>before[1]:after[0]<before[0],`${device} endpoint`);assert.equal(after[1-index],before[1-index]);
    }
    await type(`${form} [data-tool-setting=tonal_lower]`,'-20.0');await type(`${form} [data-tool-setting=tonal_upper]`,'12.0');
    assert.deepEqual(await values(),[-20,12]);
    await type(`${form} [data-tool-setting=tonal_lower]`,'13');assert.deepEqual(await values(),[12,12]);
    await type(`${form} [data-tool-setting=tonal_lower]`,'-7.2');await type(`${form} [data-tool-setting=tonal_upper]`,'2.3');
    for(const theme of ['light','dark']){await send({type:'set_theme',theme});await capture(`panel-${theme}`);}
    await click(opener);await invoke('save_selection_layer');await send({type:'layer',action:{op:'cancel_rename'}});
    assert.ok(await evaluate('layerApp.state().layer_tools.mask_editing.layer!==null'));
    await click(opener);await click(`${form} [data-tool-choice-tone="2"]`);await click(opener);
    assert.ok(await evaluate('layerApp.state().layers.some(l=>l.selection_layer)'));
    await invoke('return_to_artwork');await invoke('quick_mask');
    // Canvas sampling still reads artwork, not the red mask overlay.
    const a=samples[0];await gesture(a,a,'pen');
    await wait('layerApp.state().tool_settings.some(f=>f.id==="tonal_lower")');
    assert.ok((await values())[1]<-5,'black sample remains dark through Quick Mask');
    await workspace('photographer');await invoke('tonal_select');
    await click('[data-toolbar-segment="tonal-tones-5"]');
    const inline='.toolbar-range';
    if(!(await rect(inline)).width) {
      // On the 1200px tablet, free the leading command tiles for the complete form.
      await evaluate(`(()=>{const w=structuredClone(layerApp.state().workspace);for(const p of w.layout.panels)if(p.content.tiles?.some(t=>t.control.kind==='tool_options'))p.content.tiles=p.content.tiles.filter(t=>t.control.kind==='tool_options');layerApp.dispatch({type:'restore_workspace',workspace:w})})()`);await idle();
    }
    assert.ok((await rect(inline)).width>=280);assert.equal((await rect('[data-toolbar-choice="tonal-tones"]')).height,24);
    await type(`${inline} [data-toolbar-setting=tonal_lower]`,'-6.0');await type(`${inline} [data-toolbar-setting=tonal_upper]`,'1.0');
    for(const device of ['mouse','touch','pen'])for(const index of [0,1]) {
      const thumb=await rect(`${inline} .interval-thumb.${index?'upper':'lower'}`),before=await values();
      const a={x:thumb.x+3,y:thumb.y+7};await gesture(a,{x:a.x+(index?6:-6),y:a.y},device);
      const after=await values();assert.ok(index?after[1]>before[1]:after[0]<before[0]);
    }
    await capture('toolbar-custom');
    const before=await values(),thumb=await rect(`${inline} .interval-thumb.lower`),p={x:thumb.x+3,y:thumb.y+7};
    await gesture(p,{x:p.x-10,y:p.y},'mouse',true);assert.deepEqual(await values(),before,'Escape restores the endpoint');
    // A float document adds Bright HDR and still keeps Custom last.
    await evaluate('layerApp.dispatch({type:"invoke",command:"new_document"});null');await wait(`!!document.querySelector('.document-dialog [aria-label="Bit depth"]')`);
    await evaluate(`(()=>{const d=document.querySelector('.document-dialog');d.querySelector('[aria-label="Bit depth"]').value='F16';d.querySelectorAll('input[type=number]').forEach((n,i)=>n.value=i?200:500);[...d.querySelectorAll('button')].find(b=>b.textContent==='Create').click()})()`);
    await wait('layerApp.app.document_color().depth==="F16"&&!layerApp.state().document_file.busy');
    await invoke('select_all');await send({type:'set_color',rgba:[1,1,1,1]});await send({type:'color',action:{op:'hdr_intensity',stops:2}});await invoke('fill_selection');await invoke('deselect');
    await invoke('tonal_select');
    assert.deepEqual(await evaluate('layerApp.state().tool_extra[0].Choice.items.slice(-2).map(i=>i.icon)'),['tonal-bright-hdr','tonal-custom']);
    await click('[data-toolbar-segment="tonal-tones-5"]');await wait('layerApp.state().layer_tools.has_selection');
    await invoke('quick_mask');await invoke('fit_canvas');
    const hdrCenter=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return {x:r.x+(250*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(100*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
    const [hdrPixel]=await pixelsAt(await capture('hdr-bright'),[hdrCenter]);
    assert.ok(hdrPixel[0]>hdrPixel[1]+30,'Bright HDR covers artwork at +2 stops');
    await click('[data-toolbar-segment="tonal-tones-0"]');
    const [excluded]=await pixelsAt(await capture('hdr-shadows-excluded'),[hdrCenter]);
    assert.ok(excluded[0]>200&&excluded[1]>200,'Shadows excludes HDR artwork');
    console.log('PASS Huion Web tonal presets, compact interval, mouse/touch/pen, numeric bounds, Quick Mask pixels, saved masks, sampling, toolbar, cancellation and HDR');
  } finally {await evaluate('window.showOpenFilePicker=window.tonalOpen;delete window.tonalOpen');}
}
