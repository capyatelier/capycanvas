import assert from 'node:assert/strict';

// Run with the real Wasm app and WebGPU, including on scaled tablet Chrome.
export async function checkSdrColor({call,evaluate,settle}, photoUrl='/pkg/prophoto16.png') {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>60000)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status').textContent));else setTimeout(poll,30);}catch(e){reject(e)}}poll();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing button '+${JSON.stringify(label)});b.click();})()`);
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await evaluate(`window.sdrFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value)},async close(){sdrFiles.set(options.suggestedName,bytes)},async abort(){}}}});
    window.sdrManifest=bytes=>JSON.parse(new TextDecoder().decode(bytes.slice(52,52+Number(new DataView(bytes.buffer,bytes.byteOffset).getBigUint64(12,true)))));`);
  await invoke('new_document');await wait('!!document.querySelector(\"dialog[open]\")');
  if(await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Discard Changes")'))await click('Discard Changes');
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');for(const [label,value] of [['Width','513'],['Height','257']]){const input=[...d.querySelectorAll('input[type=number]')].find(i=>i.getAttribute('aria-label').startsWith(label));input.value=value;}
    d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U16';})()`);
  await click('Create');await wait('!layerApp.state().document_file.busy && layerApp.app.document_color().space==="DisplayP3" && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'DisplayP3',depth:'U16'});
  await evaluate(`layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'ProPhoto',rgba:[.85,.021,.6,.33333334]}}});window.sdrColor=JSON.stringify(layerApp.state().colors.foreground);`);
  await evaluate(`[...document.querySelectorAll('button')].find(b=>b.textContent==='Edit Color…'&&!b.closest('dialog')).click()`);
  for(const model of ['srgb_hex','oklch','hsv','hls','document_rgb'])await evaluate(`(()=>{const s=document.querySelector('.color-dialog select');s.value=${JSON.stringify(model)};s.dispatchEvent(new Event('change'));})()`);
  await click('Use Color');assert.equal(await evaluate('JSON.stringify(layerApp.state().colors.foreground)'),await evaluate('sdrColor'));
  await evaluate(`[...document.querySelectorAll('button')].find(b=>b.textContent==='Palettes…').click()`);
  await evaluate(`document.querySelector('.color-library input[aria-label="New palette or swatch name"]').value='Tablet SDR '+Date.now()`);
  await click('Save Current Color');
  assert.ok(await evaluate(`layerApp.state().colors.library.palettes.some(p=>p.swatches.some(s=>JSON.stringify(s.color)===sdrColor))`));await click('Close');
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  for(const [type,dx,buttons] of [['mousePressed',0,1],['mouseMoved',45,1],['mouseReleased',45,0]]) {await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
  await wait('layerApp.state().document_file.modified');await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
  const paint=await evaluate('sdrManifest([...sdrFiles.values()].at(-1))');assert.equal(paint.document.color.depth,'U16');assert.equal(paint.document.color.space,'DisplayP3');assert.ok(paint.blobs.length>0);assert.ok(paint.blobs.every(b=>b.descriptor.bits_per_channel===16));
  await evaluate(`(async()=>{window.sdrPhotoBytes=new Uint8Array(await (await fetch(${JSON.stringify(photoUrl)})).arrayBuffer());window.showOpenFilePicker=async()=>[{name:'prophoto16.png',async getFile(){return new File([sdrPhotoBytes],'prophoto16.png')}}];})()`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.document_color().space==="ProPhoto" && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'ProPhoto',depth:'U16'});assert.equal(await evaluate('layerApp.state().document_file.location??null'),null);
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
  const source=await evaluate('sdrManifest([...sdrFiles.values()].at(-1))');assert.equal(source.tiled_sources.images.length,1);assert.equal(source.tiled_sources.images[0].depth,'U16');assert.ok(source.tiled_sources.images[0].tiles.length>0);assert.equal(source.tiled_sources.profiles.length,1);
  await evaluate(`window.sdrPhotoMaster=[...sdrFiles.values()].at(-1).slice();window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.state().document_file.location?.name==="photo-master.capy" && layerApp.app.brush_ready()');
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
  const restored=await evaluate('sdrManifest(sdrFiles.get("photo-master.capy"))');assert.deepEqual(restored.tiled_sources,source.tiled_sources);assert.deepEqual(restored.blobs,source.blobs);assert.deepEqual(restored.document.color,source.document.color);
  console.log('P3 U16 creation/painting, exact numeric color/palette retention, ProPhoto16 open and native source save/reopen passed');
}
