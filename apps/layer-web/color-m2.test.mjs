import assert from 'node:assert/strict';

// Run with the real Wasm app and WebGPU, including on scaled tablet Chrome.
export async function checkSdrColor({call,evaluate,settle}, photoUrl='/pkg/prophoto16.png') {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>60000)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status').textContent));else setTimeout(poll,30);}catch(e){reject(e)}}poll();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing button '+${JSON.stringify(label)});b.click();})()`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);return evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  await evaluate(`window.sdrFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){sdrFiles.set(options.suggestedName,bytes)},async abort(){}}}});
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
  const exported = async(format,profile,depth='U16',resize=false)=>{
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
    await evaluate(`(()=>{const d=document.querySelector('dialog[open]');const set=(label,value)=>{const s=d.querySelector('select[aria-label="'+label+'"]');s.value=value;s.dispatchEvent(new Event('change'));};
      set('Format',${JSON.stringify(format)});set('Output profile',${JSON.stringify(profile)});set('Bit depth',${JSON.stringify(depth)});
      if(${resize}){set('Pixel size','Fit');d.querySelector('input[aria-label="Maximum width"]').value='257';d.querySelector('input[aria-label="Maximum height"]').value='257';set('Resolution metadata','Ppi');d.querySelector('input[aria-label="Pixels per inch"]').value='300';}})()`);
    await click('Choose File…');await wait('!layerApp.state().document_file.busy');
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  };
  const originalProfile=await evaluate('String(layerApp.app.export_form().profiles.length-1)');
  for(const [format,extension] of [['Png','png'],['Tiff','tif']]){
    await exported(format,originalProfile);
    await evaluate(`window.sdrOutput=sdrFiles.get('photo-master.${extension}').slice();window.showOpenFilePicker=async()=>[{name:'identity.${extension}',async getFile(){return new File([sdrOutput],'identity.${extension}')}}];`);
    await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
    const output=await evaluate('sdrManifest(sdrFiles.get("identity.capy"))');
    assert.deepEqual(output.tiled_sources.images[0].tiles,source.tiled_sources.images[0].tiles,format+' retains every original sample');
    assert.deepEqual(output.tiled_sources.profiles,source.tiled_sources.profiles,format+' retains the original ICC profile');
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}];`);
    await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.state().document_file.location?.name==="photo-master.capy" && layerApp.app.brush_ready()');
  }
  await exported('Png','1','U8',true);
  assert.ok(await evaluate('sdrFiles.get("photo-master.png").length>100'));
  await evaluate(`window.sdrReduced=sdrFiles.get('photo-master.png');window.showOpenFilePicker=async()=>[{name:'resized.png',async getFile(){return new File([sdrReduced],'resized.png')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'DisplayP3',depth:'U8'});
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
  const reduced=await evaluate('sdrManifest(sdrFiles.get("resized.capy"))');
  assert.deepEqual([reduced.document.width,reduced.document.height],[257,129]);assert.ok(reduced.document.resolution);
  await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  await exported('Jpeg','0','U8');
  assert.deepEqual(await evaluate('Array.from(sdrFiles.get("photo-master.jpg").slice(0,2))'),[255,216]);
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'ProPhoto',depth:'U16'});
  await evaluate(`layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:1}});
    window.sdrUntagged=(()=>{const parts=[sdrPhotoBytes.slice(0,8)];for(let offset=8;offset<sdrPhotoBytes.length;){const size=new DataView(sdrPhotoBytes.buffer,sdrPhotoBytes.byteOffset+offset,4).getUint32(0),tag=new TextDecoder().decode(sdrPhotoBytes.slice(offset+4,offset+8));if(!['iCCP','sRGB','gAMA','cHRM','cICP'].includes(tag))parts.push(sdrPhotoBytes.slice(offset,offset+size+12));offset+=size+12;}return new Blob(parts);})();
    window.showOpenFilePicker=async()=>[{name:'untagged.png',async getFile(){return new File([sdrUntagged],'untagged.png')}}];`);
  const beforeAssumption=await evaluate('Number(layerApp.state().document_file.epoch)');
  await invoke('open_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Interpret as"]')`);
  assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),beforeAssumption);await click('Cancel');await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),beforeAssumption);
  await invoke('open_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Interpret as"]')`);
  await evaluate(`document.querySelector('select[aria-label="Interpret as"]').value='2'`);await click('Use Profile');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'AdobeRgb',depth:'U16'});
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
  const assumed=await evaluate('sdrManifest(sdrFiles.get("untagged.capy"))');
  assert.deepEqual(assumed.tiled_sources.images[0].tiles,source.tiled_sources.images[0].tiles);
  await evaluate(`layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}})`);
  const inspection=await evaluate(`(async()=>{const control=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify(await layerApp.app.histogram(control),(_,v)=>typeof v==="bigint"?Number(v):v))}finally{control.free()}})()`);
  assert.deepEqual(inspection.histogram.color,{space:'AdobeRgb',depth:'U16'});
  assert.equal(Number(inspection.histogram.pixels)+Number(inspection.histogram.transparent),513*257);
  assert.equal(Number(inspection.histogram.transparent),31*257);
  for(const channel of inspection.histogram.channels)assert.equal(channel.bins.reduce((n,v)=>n+Number(v),0),Number(inspection.histogram.pixels));
  await invoke('histogram');await wait(`!!document.querySelector('.histogram-dialog[open]')`);
  await wait(`document.querySelector('.histogram-dialog').textContent.includes('Current committed drawing')`);
  await evaluate(`[...document.querySelectorAll('.histogram-dialog button')].find(b=>b.textContent==='Close').click()`);
  const cancelled=await evaluate(`(async()=>{const c=layerApp.app.capture_control();c.cancel();try{await layerApp.app.histogram(c);return false}catch(e){return String(e).toLowerCase().includes('cancel')}finally{c.free()}})()`);
  assert.ok(cancelled);
  const fileCount=await evaluate('sdrFiles.size');
  await evaluate(`window.sdrCancelObserver=new MutationObserver(()=>{const b=document.querySelector('.file-progress button');if(b){b.click();sdrCancelObserver.disconnect();}});sdrCancelObserver.observe(document.body,{childList:true,subtree:true});`);
  await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);await click('Choose File…');await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);assert.equal(await evaluate('sdrFiles.size'),fileCount);
  console.log('Cancelled export publishes no image and leaves the master usable');
  console.log('Full-resolution histogram excludes transparent pixels; nonmodal UI and cancellation passed');
  console.log('Untagged-photo cancellation retains the master; explicit Adobe RGB assumption preserves all original samples');
  console.log('Profiled PNG/TIFF retain every original U16 sample/profile; resized P3 PNG and sRGB JPEG export copies passed');
  console.log('P3 U16 creation/painting, exact numeric color/palette retention, ProPhoto16 open and native source save/reopen passed');
}

export async function checkColorEdits({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>120000)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1400)));else setTimeout(poll,30);}catch(e){reject(e)}}poll();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing enabled button '+${JSON.stringify(label)});b.click()})()`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);return evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const save=async()=>{await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');return evaluate('sdrManifest(sdrFiles.get("untagged.capy"))');};
  const backing=value=>({blobs:value.blobs,sources:value.tiled_sources,color:value.document.color});
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  await evaluate(`layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'DisplayP3',rgba:[.8,.2,.1,1]}}})`);
  for(const [type,dx,buttons] of [['mousePressed',0,1],['mouseMoved',45,1],['mouseReleased',45,0]]){await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
  await wait('layerApp.state().document_file.modified');const original=await save();
  async function change(command,label,value,apply=true){
    await invoke(command);await wait(`!!document.querySelector('dialog[open] select[aria-label=${JSON.stringify(label)}]')`);
    await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label=${JSON.stringify(label)}]');s.value=${JSON.stringify(value)};s.dispatchEvent(new Event('change'));})()`);
    await click('Preview Complete Result');await wait(`document.querySelectorAll('.color-comparison canvas').length===2`);
    assert.ok(await evaluate(`[...document.querySelectorAll('.color-comparison canvas')].every(c=>c.width<=512&&c.height<=384)`));
    await click(apply?'Apply':'Cancel');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  }
  await invoke('assign_profile');await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await click('Preview Complete Result');await click('Cancel');await wait('!layerApp.state().document_file.busy');assert.deepEqual(backing(await save()),backing(original));
  const paintedHistogram=await evaluate(`(async()=>{const c=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify(await layerApp.app.histogram(c),(_,v)=>typeof v==="bigint"?Number(v):v))}finally{c.free()}})()`);
  assert.equal(paintedHistogram.histogram.pixels+paintedHistogram.histogram.transparent,513*257);
  await change('assign_profile','Color space','ProPhoto',false);assert.deepEqual(backing(await save()),backing(original));
  await change('assign_profile','Color space','ProPhoto');const assigned=await save();
  assert.equal(assigned.document.color.space,'ProPhoto');assert.deepEqual(assigned.blobs,original.blobs);assert.deepEqual(assigned.tiled_sources,original.tiled_sources);
  async function history(command,expected){await invoke(command);await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');assert.deepEqual(backing(await save()),backing(expected));}
  await history('undo',original);await history('redo',assigned);
  await change('convert_color_space','Color space','DisplayP3');const converted=await save();
  assert.equal(converted.document.color.space,'DisplayP3');assert.notDeepEqual(converted.blobs,assigned.blobs);assert.deepEqual(converted.tiled_sources,original.tiled_sources);
  await change('change_bit_depth','Bit depth','U8');const reduced=await save();
  assert.equal(reduced.document.color.depth,'U8');assert.ok(reduced.blobs.some(b=>b.descriptor.bits_per_channel===8));assert.deepEqual(reduced.tiled_sources.images,original.tiled_sources.images);
  await history('undo',converted);await history('redo',reduced);
  await evaluate(`window.sdrColorMaster=sdrFiles.get('untagged.capy').slice();window.showOpenFilePicker=async()=>[{name:'untagged.capy',async getFile(){return new File([sdrColorMaster],'untagged.capy')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');assert.deepEqual(backing(await save()),backing(reduced));
  await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);await click('Choose File…');await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);assert.ok(await evaluate('sdrFiles.get("untagged.png")?.length>100'));
  console.log('Full-image comparison, canceled/committed assignment, conversion, depth change, exact undo/redo and native reopen passed; retained source samples/profile unchanged');
}

export async function checkSourceImports({call,evaluate}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>60000)reject(Error(${JSON.stringify(condition)}+': '+document.querySelector('#status').textContent));else setTimeout(poll,30);}catch(e){reject(e)}}poll();})`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const click=label=>evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)}).click()`);
  const idle=()=>wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  const save=async()=>{await invoke('save_document_as');await idle();return evaluate('sdrManifest(sdrFiles.get(layerApp.state().document_file.location.name))');};
  const original=await evaluate('sdrManifest(sdrPhotoMaster).tiled_sources');
  await invoke('new_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');for(const i of d.querySelectorAll('input[type=number]'))i.value=i.getAttribute('aria-label').startsWith('Width')?'513':'257';d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U8';})()`);
  await click('Create');await idle();const before=await save();
  await evaluate(`window.sdrPlaceFile=JSON.stringify(layerApp.state().document_file,(_,v)=>typeof v==='bigint'?Number(v):v);window.showOpenFilePicker=async()=>[{name:'retained-prophoto16.png',async getFile(){return new File([sdrPhotoBytes],'retained-prophoto16.png')}}];`);
  // The layer-panel button must use the retained-source path as well as the menu.
  await evaluate(`document.querySelector('button[aria-label="Import image as layer"]').click()`);await idle();
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),await evaluate('JSON.parse(sdrPlaceFile).epoch'));
  assert.deepEqual(await evaluate('layerApp.state().document_file.location'),await evaluate('JSON.parse(sdrPlaceFile).location'));
  const placed=await save();assert.deepEqual(placed.document.color,{space:'DisplayP3',depth:'U8'});assert.deepEqual(placed.tiled_sources.images,original.images);assert.deepEqual(placed.tiled_sources.profiles,original.profiles);
  await invoke('undo');await idle();assert.deepEqual((await save()).tiled_sources,before.tiled_sources);
  await invoke('redo');await idle();assert.deepEqual((await save()).tiled_sources,placed.tiled_sources);
  await invoke('document_properties');await wait(`document.querySelector('dialog[open]')?.textContent.includes('embedded ICC retained')`);
  const details=await evaluate('document.querySelector("dialog[open]").textContent');assert.match(details,/Display P3/);assert.match(details,/16-bit RGB/);await click('Done');await idle();
  await evaluate(`window.sdrPlacedMaster=sdrFiles.get(layerApp.state().document_file.location.name).slice();window.showOpenFilePicker=async()=>[{name:'placed.capy',async getFile(){return new File([sdrPlacedMaster],'placed.capy')}}];`);
  await invoke('open_document');await idle();assert.deepEqual((await save()).tiled_sources,placed.tiled_sources);
  // Chromium custom image formats preserve the original bytes. Ordinary image/png
  // may have been sanitized by the clipboard producer/browser before we read it.
  const focus=await evaluate('({x:innerWidth/2,y:3})');
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...focus,button:'left',clickCount:1});
  await wait('document.hasFocus()');
  const pasted=await call('Runtime.evaluate',{expression:`(async()=>{await navigator.clipboard.write([new ClipboardItem({'web image/png':new Blob([sdrPhotoBytes],{type:'image/png'})})]);return true})()`,awaitPromise:true,returnByValue:true,userGesture:true});
  if(pasted.exceptionDetails)throw Error(JSON.stringify(pasted.exceptionDetails));
  await call('Runtime.evaluate',{expression:"layerApp.dispatch({type:'invoke',command:'paste_image'})",userGesture:true});
  await idle();assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  const copy=await save();assert.equal(copy.document.layers.length,placed.document.layers.length+1);
  assert.deepEqual(copy.document.color,placed.document.color);assert.deepEqual(copy.tiled_sources.profiles,original.profiles);
  for(const image of copy.tiled_sources.images){assert.equal(image.depth,'U16');assert.deepEqual(image.tiles,original.images[0].tiles);}
  console.log('Layer-panel import and real custom-format clipboard paste preserve every ProPhoto16 sample/ICC in a P3 U8 master; undo/redo, reopen and document details passed');
}
