import {histogramJourney} from './histogram-journey.mjs';
import assert from 'node:assert/strict';
import {paintBaseImage,authoredIdentity,readPackage,packageObject,packageComposition,packageOccurrences,packageResources,packageResourceIdentity,resourceIdentity,sourceIdentity,rasterIdentity,sourceContent,sourceSamples,packageObjects,imageIdentity,imageContent} from './package-fixture.test.mjs';
const rasterResources=m=>packageResources(m,'capy.raster-tile/1').map(resource=>resourceIdentity(m,{ref:resource.id}));
const compositionColor=m=>({space:'srgb',depth:'u8',...packageComposition(m).data.color});
const wireColor=c=>({space:{Srgb:'srgb',DisplayP3:'display_p3',AdobeRgb:'adobe_rgb',ProPhoto:'pro_photo'}[c.space],depth:c.depth.toLowerCase()});
const activeOccurrence=async(evaluate,m)=>packageOccurrences(m)[await evaluate('layerApp.state().layers.findIndex(l=>l.editing)')];
const runtimeId=async(evaluate,m,portable)=>evaluate(`Number(layerApp.state().layers[${packageOccurrences(m).findIndex(o=>o.id===portable)}].id)`);


const helpers=(evaluate,{timeout=60000,enabled=false}={})=>{
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>${timeout})reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1400)));else setTimeout(poll,30);}catch(e){reject(e)}}poll();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||${enabled}&&b.disabled)throw Error('Missing button '+${JSON.stringify(label)});b.click()})()`);
  const invoke=async command=>{await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);return evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const idle=()=>wait('!layerApp.state().document_file.busy && !layerApp.documents.busy() && layerApp.app.brush_ready()');
  return {wait,click,invoke,idle};
};

// Run with the real Wasm app and WebGPU, including on scaled tablet Chrome.
export async function checkSdrColor({call,evaluate,settle}, photoUrl='/pkg/prophoto16.png') {
  const {wait,click,invoke}=helpers(evaluate);
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  await evaluate(`window.sdrFiles=new Map();window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){sdrFiles.set(options.suggestedName,bytes)},async abort(){}}}});`);
  await invoke('new_document');await wait('!!document.querySelector(\"dialog[open]\")');
  if(await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Discard Changes")'))await click('Discard Changes');
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');for(const [field,value] of [['width','513'],['height','257']]){const input=d.querySelector('[data-document-field='+field+']');input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));}
    d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U16';})()`);
  await click('Create');await wait('!layerApp.state().document_file.busy && layerApp.app.document_color().space==="DisplayP3" && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'DisplayP3',depth:'U16'});
  await evaluate(`layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'ProPhoto',rgba:[.85,.021,.6,.33333334]}}});window.sdrColor=JSON.stringify(layerApp.state().colors.foreground);`);
  await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
  await wait(`!!document.querySelector('.color-dialog[open] .color-format')`);
  await evaluate(`(()=>{const d=document.querySelector('.color-dialog[open]');for(const format of d.querySelectorAll('.color-format')){const items=()=>[...format.parentNode.querySelectorAll('.color-format-item')];format.click();for(const form of [...items().map(i=>i.dataset.form),items()[0].dataset.form]){format.click();items().find(i=>i.dataset.form===form).click();}}})()`);
  await click('Use Color');assert.equal(await evaluate('JSON.stringify(layerApp.state().colors.foreground)'),await evaluate('sdrColor'));
  assert.equal(await evaluate(`!![...document.querySelectorAll('.color-wheel-control button')].find(b=>/Palettes/.test(b.textContent))`),false);
  await evaluate(`layerApp.dispatch({type:'color',action:{op:'library',action:{op:'store',palette:layerApp.state().color_library.palettes[0].id,name:'SDR precision regression',color:layerApp.state().colors.foreground}}})`);
  assert.ok(await evaluate(`layerApp.state().color_library.palettes.some(p=>p.swatches.some(s=>JSON.stringify(s.color)===sdrColor))`));
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  for(const [type,dx,buttons] of [['mousePressed',0,1],['mouseMoved',45,1],['mouseReleased',45,0]]) {await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
  await wait('layerApp.state().document_file.modified');await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
  const paint=await readPackage(evaluate,'[...sdrFiles.values()].at(-1)');assert.equal(compositionColor(paint).depth,'u16');assert.equal(compositionColor(paint).space,'display_p3');assert.ok(packageResources(paint,'capy.raster-tile/1').length>0);assert.ok(packageResources(paint,'capy.raster-tile/1').every(b=>b.data.depth==='u16'));
  await evaluate(`(async()=>{window.sdrPhotoBytes=new Uint8Array(await (await fetch(${JSON.stringify(photoUrl)})).arrayBuffer());window.showOpenFilePicker=async()=>[{name:'prophoto16.png',async getFile(){return new File([sdrPhotoBytes],'prophoto16.png')}}];})()`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.document_color().space==="ProPhoto" && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'ProPhoto',depth:'U16'});assert.equal(await evaluate('layerApp.state().document_file.location??null'),null);
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
  const source=await readPackage(evaluate,'[...sdrFiles.values()].at(-1)');assert.equal(sourceIdentity(source).length,1);assert.equal(sourceIdentity(source)[0].interpretation.depth,'u16');assert.ok(sourceIdentity(source)[0].tiles.length>0);assert.equal(packageResources(source,'capy.icc/1').length,1);
  await evaluate(`window.sdrPhotoMaster=[...sdrFiles.values()].at(-1).slice();window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.state().document_file.location?.name==="photo-master.capy" && layerApp.app.brush_ready()');
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
  const restored=await readPackage(evaluate,'sdrFiles.get("photo-master.capy")');assert.deepEqual(sourceIdentity(restored),sourceIdentity(source));assert.deepEqual(packageResourceIdentity(restored),packageResourceIdentity(source));assert.deepEqual(compositionColor(restored),compositionColor(source));
  const exported = async(format,profile,depth='U16',resize=false)=>{
    await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
    await evaluate(`(()=>{const d=document.querySelector('dialog[open]');const set=(label,value)=>{const s=d.querySelector('select[aria-label="'+label+'"]');s.value=value;s.dispatchEvent(new Event('change'));};
      set('Format',${JSON.stringify(format)});set('Output profile',${JSON.stringify(profile)});set('Bit depth',${JSON.stringify(depth)});set('Pixel size','Original');set('Dither','None');
      if(${resize}){const number=(label,value)=>{const entry=d.querySelector('input.number-entry[aria-label="'+label+'"]');entry.closest('.number-control').querySelector('.number-value').click();entry.value=value;entry.dispatchEvent(new Event('input',{bubbles:true}));};set('Pixel size','Fit');number('Maximum width (px)','257');number('Maximum height (px)','257');set('Resolution metadata','Ppi');number('Pixels per inch','300');}})()`);
    if(resize){await click('Preview Output');await wait(`!!document.querySelector('canvas[aria-label="Output preview"]')`);
      assert.deepEqual(await evaluate(`(()=>{const canvas=document.querySelector('canvas[aria-label="Output preview"]');return[canvas.width,canvas.height]})()`),[257,129]);
      assert.ok(await evaluate(`document.querySelector('dialog[open]').textContent.includes('excludes JPEG compression artifacts')`));}
    await click('Choose File…');await wait('!layerApp.state().document_file.busy');
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  };
  const originalProfile=await evaluate('String(layerApp.app.export_form().profiles.length-1)');
  for(const [format,extension] of [['Png','png'],['Tiff','tif']]){
    await exported(format,originalProfile);
    await evaluate(`window.sdrOutput=sdrFiles.get('photo-master.${extension}').slice();window.showOpenFilePicker=async()=>[{name:'identity.${extension}',async getFile(){return new File([sdrOutput],'identity.${extension}')}}];`);
    await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
    const output=await readPackage(evaluate,'sdrFiles.get("identity.capy")');
    assert.deepEqual(sourceSamples(output)[0],sourceSamples(source)[0],format+' retains every original sample');
    assert.deepEqual(sourceContent(output).map(o=>o.interpretation.profile),sourceContent(source).map(o=>o.interpretation.profile),format+' retains original ICC content digests and lengths independently of archive placement');
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}];`);
    await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.state().document_file.location?.name==="photo-master.capy" && layerApp.app.brush_ready()');
  }
  await exported('Png','1','U8',true);
  assert.ok(await evaluate('sdrFiles.get("photo-master.png").length>100'));
  await evaluate(`window.sdrReduced=sdrFiles.get('photo-master.png');window.showOpenFilePicker=async()=>[{name:'resized.png',async getFile(){return new File([sdrReduced],'resized.png')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'DisplayP3',depth:'U8'});
  await invoke('save_document_as');await wait('!layerApp.state().document_file.busy');
  const reduced=await readPackage(evaluate,'sdrFiles.get("resized.capy")');
  assert.deepEqual(packageComposition(reduced).data.size,[257,129]);assert.ok(packageComposition(reduced).data.resolution);
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
  const assumed=await readPackage(evaluate,'sdrFiles.get("untagged.capy")');
  assert.deepEqual(sourceSamples(assumed)[0],sourceSamples(source)[0]);
  await evaluate(`layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}})`);
  const scope=histogramJourney({evaluate,settle});
  const inspection={histogram:await scope.exact()};
  assert.deepEqual(inspection.histogram.color,{space:'AdobeRgb',depth:'U16'});
  assert.equal(Number(inspection.histogram.pixels)+Number(inspection.histogram.transparent),513*257);
  assert.equal(Number(inspection.histogram.transparent),31*257);
  for(const channel of inspection.histogram.channels)assert.equal(channel.bins.reduce((n,v)=>n+Number(v),0),Number(inspection.histogram.pixels));
  assert.ok((await scope.cancel()).retired);
  assert.deepEqual(await scope.exact(),inspection.histogram,'Revealing a retired subscription recaptures the same source');
  await scope.hide();
  await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
  await click('Preview Output');await click('Cancel');await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);
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
  const {wait,click,invoke}=helpers(evaluate,{timeout:120000,enabled:true});
  const save=async()=>{await invoke('save_document_as');await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');return readPackage(evaluate,'sdrFiles.get("untagged.capy")');};
  const backing=value=>({resources:packageResourceIdentity(value),rasters:rasterIdentity(value),sources:sourceIdentity(value),color:compositionColor(value)});
  await invoke('fit_canvas');
  await evaluate("layerApp.dispatch({type:'select_brush',id:1})");
  await settle();
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  await evaluate(`layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'DisplayP3',rgba:[.8,.2,.1,1]}}})`);
  await settle();
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
  const paintedHistogram={histogram:await histogramJourney({evaluate,settle}).exact()};
  assert.equal(paintedHistogram.histogram.pixels+paintedHistogram.histogram.transparent,513*257);
  await change('assign_profile','Color space','ProPhoto',false);assert.deepEqual(backing(await save()),backing(original));
  await change('assign_profile','Color space','ProPhoto');const assigned=await save();
  assert.equal(compositionColor(assigned).space,'pro_photo');assert.deepEqual(packageResourceIdentity(assigned),packageResourceIdentity(original));assert.deepEqual(sourceIdentity(assigned),sourceIdentity(original));
  async function history(command,expected){await invoke(command);await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');assert.deepEqual(backing(await save()),backing(expected));}
  await history('undo',original);await history('redo',assigned);
  await change('convert_color_space','Color space','DisplayP3');const converted=await save();
  assert.equal(compositionColor(converted).space,'display_p3');assert.notDeepEqual(packageResourceIdentity(converted),packageResourceIdentity(assigned));assert.deepEqual(sourceIdentity(converted),sourceIdentity(original));
  await change('change_bit_depth','Bit depth','U8');const reduced=await save();
  assert.equal(compositionColor(reduced).depth,'u8');assert.ok(packageResources(reduced,'capy.raster-tile/1').some(b=>b.data.depth==='u8'));assert.deepEqual(sourceIdentity(reduced),sourceIdentity(original));
  await history('undo',converted);await history('redo',reduced);
  await evaluate(`window.sdrColorMaster=sdrFiles.get('untagged.capy').slice();window.showOpenFilePicker=async()=>[{name:'untagged.capy',async getFile(){return new File([sdrColorMaster],'untagged.capy')}}];`);
  await invoke('open_document');await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');assert.deepEqual(backing(await save()),backing(reduced));
  await invoke('export_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);await evaluate(`(()=>{const f=document.querySelector('select[aria-label="Format"]');f.value='Png';f.dispatchEvent(new Event('change'))})()`);await click('Choose File…');await wait('!layerApp.state().document_file.busy');
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);assert.ok(await evaluate('sdrFiles.get("untagged.png")?.length>100'));
  console.log('Full-image comparison, canceled/committed assignment, conversion, depth change, exact undo/redo and native reopen passed; retained source samples/profile unchanged');
}

export async function checkSourceImports({call,evaluate}) {
  const {wait,invoke,click,idle}=helpers(evaluate);
  const save=async()=>{await invoke('save_document_as');await idle();return readPackage(evaluate,'sdrFiles.get(layerApp.state().document_file.location.name)');};
  const original=await readPackage(evaluate,'sdrPhotoMaster');
  await invoke('new_document');await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');for(const [field,value] of [['width','513'],['height','257']]){const input=d.querySelector('[data-document-field='+field+']');input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));}d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U8';})()`);
  await click('Create');await idle();const before=await save();
  await evaluate(`window.sdrPlaceFile=JSON.stringify(layerApp.state().document_file,(_,v)=>typeof v==='bigint'?Number(v):v);window.showOpenFilePicker=async()=>[{name:'retained-prophoto16.png',async getFile(){return new File([sdrPhotoBytes],'retained-prophoto16.png')}}];`);
  // The layer-panel button must use the retained-source path as well as the menu.
  await evaluate(`[...document.querySelectorAll('.layer-footer button')].find(button=>button.ariaLabel===layerApp.app.catalog().native_copy.layers.import_image).click()`);await idle();
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  assert.equal(await evaluate('Number(layerApp.state().document_file.epoch)'),await evaluate('JSON.parse(sdrPlaceFile).epoch'));
  assert.deepEqual(await evaluate('layerApp.state().document_file.location'),await evaluate('JSON.parse(sdrPlaceFile).location'));
  await wait('layerApp.state().commands.find(c=>c.id==="apply_transform")?.enabled');await invoke('apply_transform');await idle();
  const placed=await save();assert.deepEqual(compositionColor(placed),{space:'display_p3',depth:'u8'});
  assert.equal(sourceContent(original).length,1,'Open Image makes paint with a referenced base');
  assert.deepEqual(sourceContent(placed),sourceContent(before),'Placing an image adds no paint base');
  assert.equal(packageObjects(placed,'capy.image-object/1').length,1,'Placing an image makes an image object');
  assert.deepEqual(imageContent(placed),sourceContent(original),'The placed image keeps every source sample and its ICC interpretation');
  await invoke('undo');await idle();assert.deepEqual(imageIdentity(await save()),imageIdentity(before));
  await invoke('redo');await idle();assert.deepEqual(imageIdentity(await save()),imageIdentity(placed));
  await invoke('document_properties');await wait(`document.querySelector('dialog[open]')?.textContent.includes('Placed image')`);
  const details=await evaluate('document.querySelector("dialog[open]").textContent');assert.match(details,/Display P3/);assert.match(details,/16-bit RGB/);assert.match(details,/Source profile: ProPhoto RGB/);await click('Done');await idle();
  await evaluate(`window.sdrPlacedMaster=sdrFiles.get(layerApp.state().document_file.location.name).slice();window.showOpenFilePicker=async()=>[{name:'placed.capy',async getFile(){return new File([sdrPlacedMaster],'placed.capy')}}];`);
  await invoke('open_document');await idle();assert.deepEqual(imageIdentity(await save()),imageIdentity(placed));
  // Chromium custom image formats preserve the original bytes. Ordinary image/png
  // may have been sanitized by the clipboard producer/browser before we read it.
  const focus=await evaluate('({x:innerWidth/2,y:3})');
  for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...focus,button:'left',clickCount:1});
  await wait('document.hasFocus()');
  await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
  const pasted=await call('Runtime.evaluate',{expression:`(async()=>{await navigator.clipboard.write([new ClipboardItem({'web image/png':new Blob([sdrPhotoBytes],{type:'image/png'})})]);return true})()`,awaitPromise:true,returnByValue:true,userGesture:true});
  if(pasted.exceptionDetails)throw Error(JSON.stringify(pasted.exceptionDetails));
  const paste=await call('Runtime.evaluate',{expression:"layerApp.dispatch({type:'invoke',command:'paste_image'})",userGesture:true});assert.equal(paste.exceptionDetails,undefined);
  await idle();assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  await wait('layerApp.state().commands.find(c=>c.id==="apply_transform")?.enabled||!!layerApp.state().host_error');assert.equal(await evaluate('layerApp.state().host_error??null'),null);await invoke('apply_transform');await idle();
  const copy=await save();assert.equal(packageObjects(copy,'capy.image-object/1').length,packageObjects(placed,'capy.image-object/1').length+1,'External paste makes an image object');
  assert.deepEqual(sourceContent(copy),sourceContent(placed),'External paste adds no paint base');
  assert.deepEqual(compositionColor(copy),compositionColor(placed));assert.ok(imageContent(copy).length>=1);
  assert.deepEqual(imageContent(copy).map(o=>o.interpretation.profile),Array(imageContent(copy).length).fill(sourceContent(original)[0].interpretation.profile));
  for(const image of imageContent(copy)){assert.equal(image.interpretation.depth,'u16');assert.deepEqual(image.tiles,sourceContent(original)[0].tiles);}
  console.log('Layer-panel import and real custom-format clipboard paste make image objects that preserve every ProPhoto16 sample/ICC in a P3 U8 master; undo/redo, reopen and document details passed');
}

export async function checkSourceEdits({call,evaluate,settle}) {
  const {wait,invoke,click,idle}=helpers(evaluate);
  const save=async()=>{await invoke('save_document_as');await idle();return readPackage(evaluate,'sdrFiles.get(layerApp.state().document_file.location.name)');};
  const backing=m=>({resources:packageResourceIdentity(m),rasters:rasterIdentity(m),sources:sourceIdentity(m)});
  const source=async m=>paintBaseImage(m,(await activeOccurrence(evaluate,m)).data.content.paint);
  async function change(command,profile,apply=true){
    await invoke(command);await wait('!!document.querySelector("dialog[open]")');
    if(profile!==null)await evaluate(`(()=>{const select=document.querySelector('select[aria-label="Correct source profile"]');select.value=${JSON.stringify(profile)};select.dispatchEvent(new Event('change'));})()`);
    await click('Preview Complete Result');await wait(`!!document.querySelector('canvas[aria-label="Prepared composition"]')`);
    const adds=await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Add Corrected Source")');
    await click(!apply?'Cancel':command==='rasterize_source'?'Rasterize':adds?'Add Corrected Source':'Apply Profile');await idle();
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);return adds;
  }
  await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}]`);await invoke('open_document');await idle();
  const before=await save();assert.ok(await source(before),'Open Image makes paint with a referenced base');await change('repair_source_profile','2',false);assert.deepEqual(backing(await save()),backing(before));
  assert.equal(await change('repair_source_profile','2'),false);const repaired=await save();
  assert.deepEqual(rasterResources(repaired),rasterResources(before));assert.deepEqual((await source(repaired)).interpretation.profile,{builtin:'adobe_rgb'});
  await invoke('undo');await idle();assert.deepEqual(backing(await save()),backing(before));
  await invoke('redo');await idle();assert.deepEqual(backing(await save()),backing(repaired));
  await change('rasterize_source',null);const rasterized=await save();
  assert.equal(packageObject(rasterized,(await activeOccurrence(evaluate,rasterized)).data.content.paint).data.base.policy,'working_pixels');assert.equal((await source(rasterized)).interpretation.depth,'u16');assert.deepEqual((await source(rasterized)).interpretation.profile,{builtin:'pro_photo'});assert.deepEqual((await source(rasterized)).extent,(await source(repaired)).extent);
  await invoke('undo');await idle();assert.deepEqual(backing(await save()),backing(repaired));
  await invoke('fit_canvas');await invoke('pen');await settle();
  const point=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
  for(const [type,dx,buttons]of[['mousePressed',0,1],['mouseMoved',40,1],['mouseReleased',40,0]]){await call('Input.dispatchMouseEvent',{type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});await settle();}
  const painted=await save(),old=await activeOccurrence(evaluate,painted),oldPaint=packageObject(painted,old.data.content.paint);
  assert.equal(await change('repair_source_profile','3'),true);const added=await save();
  assert.equal(packageOccurrences(added).length,packageOccurrences(painted).length+1);assert.deepEqual(packageObject(added,old.id),old);assert.deepEqual(packageObject(added,old.data.content.paint),oldPaint);assert.deepEqual(rasterResources(added),rasterResources(painted));
  await evaluate(`window.sdrSourceMaster=sdrFiles.get(layerApp.state().document_file.location.name).slice();window.showOpenFilePicker=async()=>[{name:'source-edited.capy',async getFile(){return new File([sdrSourceMaster],'source-edited.capy')}}];`);
  await invoke('open_document');await idle();assert.deepEqual(backing(await save()),backing(added));
  await invoke('rasterize_source');await click('Preview Complete Result');await click('Cancel');await idle();assert.deepEqual(backing(await save()),backing(added));
  console.log('Source repair/rasterize full-composition comparisons, exact undo/redo, worker/preview cancellation, baked-paint preservation and save/reopen passed');
}

export async function checkExportPresets({evaluate}) {
  const {wait,click}=helpers(evaluate);
  const invoke=async()=>{await wait('!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id==="export_document")?.enabled');await evaluate(`layerApp.dispatch({type:'invoke',command:'export_document'})`);await wait(`!!document.querySelector('select[aria-label="Destination"]')`);};
  const preset=index=>evaluate(`(async()=>JSON.parse(JSON.stringify(await layerApp.app.export_presets({type:'get',index:${index}}),(_,v)=>typeof v==='bigint'?Number(v):v)))()`);
  const name='Tablet delivery '+Date.now();
  await invoke();await evaluate(`(()=>{const d=document.querySelector('dialog[open]');const select=(label,value)=>{const node=d.querySelector('select[aria-label="'+label+'"]');node.value=value;node.dispatchEvent(new Event('change'));};
    select('Format','Png');select('Output profile','1');select('Bit depth','U16');select('Pixel size','Fit');select('Resolution metadata','Ppi');
    for(const[label,value]of[['Maximum width (px)','321'],['Maximum height (px)','123'],['Pixels per inch','287']]){const entry=d.querySelector('input.number-entry[aria-label="'+label+'"]');entry.closest('.number-control').querySelector('.number-value').click();entry.value=value;entry.dispatchEvent(new Event('input',{bubbles:true}));}d.querySelector('input[aria-label="Preset name"]').value=${JSON.stringify(name)};})()`);
  await click('Save Preset');await wait(`document.querySelector('select[aria-label="Destination"]')?.selectedOptions[0]?.textContent===${JSON.stringify(name)}`);
  const index=await evaluate('Number(document.querySelector(\'select[aria-label="Destination"]\').value)');
  const saved=await preset(index);
  assert.equal(saved.recipe.profile.profile.Builtin,'DisplayP3');assert.equal(saved.recipe.depth,'U16');assert.deepEqual(saved.recipe.size,{Fit:{bounds:[321,123],enlarge:false}});assert.deepEqual(saved.recipe.resolution,{Ppi:287});
  await click('Cancel');await wait('!layerApp.state().document_file.busy');await invoke();
  await evaluate(`(()=>{const node=document.querySelector('select[aria-label="Destination"]');node.value=${JSON.stringify(String(index))};node.dispatchEvent(new Event('change'));})()`);
  await wait(`document.querySelector('select[aria-label="Bit depth"]').value==='U16' && document.querySelector('input[aria-label="Maximum width (px)"]').value==='321'`);
  assert.equal(await evaluate('document.querySelector(\'input[aria-label="Pixels per inch"]\').value'),'287');
  await evaluate(`const node=document.querySelector('select[aria-label="Transparency"]');node.value='White';node.dispatchEvent(new Event('change'));`);await click('Update Preset');
  await wait(`!document.querySelector('select[aria-label="Destination"]').disabled`);
  assert.equal((await preset(index)).recipe.background,'White');
  await click('Choose File…');await wait('!layerApp.state().document_file.busy');
  const remembered=await preset(3);assert.equal(remembered.recipe.background,'White');assert.deepEqual(remembered.recipe.resolution,{Ppi:287});
  await invoke();await evaluate(`(()=>{const node=document.querySelector('select[aria-label="Destination"]');node.value=${JSON.stringify(String(index))};node.dispatchEvent(new Event('change'));})()`);
  await wait(`!document.querySelector('select[aria-label="Destination"]').disabled`);await click('Delete Preset');
  await wait(`![...document.querySelector('select[aria-label="Destination"]').options].some(o=>o.textContent===${JSON.stringify(name)})`);
  await click('Reset Destination');await wait(`!document.querySelector('select[aria-label="Destination"]').disabled`);
  const reset=await preset(0);assert.equal(reset.recipe.format,'Png');assert.equal(reset.recipe.profile.profile.Builtin,'Srgb');assert.equal(reset.recipe.size,'Original');
  await click('Cancel');await wait('!layerApp.state().document_file.busy');
  console.log('Named export presets save/update/delete, restore all size/profile/depth/DPI choices across dialog reopen, remember successful delivery and reset destinations passed');
}

export async function checkProfileLibrary({evaluate}) {
  const {wait,invoke,click,idle}=helpers(evaluate);
  await evaluate(`window.showOpenFilePicker=async()=>[{name:'profile-library.png',async getFile(){return new File([sdrPhotoBytes],'profile-library.png')}}];`);
  await invoke('open_document');await idle();
  const report=await evaluate(`(async()=>{
    window.libraryBytes=new Uint8Array(layerApp.app.export_form().profiles.find(p=>p.profile.Icc).profile.Icc);
    window.libraryId=[...new Uint8Array(await crypto.subtle.digest('SHA-256',libraryBytes))].map(b=>b.toString(16).padStart(2,'0')).join('');
    const before=await layerApp.app.profile_library('list');
    const profile=await layerApp.app.profile_library('import',undefined,libraryBytes);await layerApp.app.profile_library('import',undefined,libraryBytes);
    const entries=await layerApp.app.profile_library('list');
    const exact=JSON.stringify((await layerApp.app.profile_library('get',libraryId)).profile.Icc)===JSON.stringify(profile.profile.Icc);
    await new Promise((resolve,reject)=>{const open=indexedDB.open('capy-color-preferences',2);open.onerror=()=>reject(open.error);open.onsuccess=()=>{const db=open.result,tx=db.transaction('profiles','readwrite');tx.objectStore('profiles').put(new Uint8Array([1,2,3]),libraryId);tx.oncomplete=()=>{db.close();resolve()};tx.onabort=()=>reject(tx.error)}});
    const corrupt=(await layerApp.app.profile_library('list')).find(p=>p.id===libraryId).issue;
    let rejected=false;try{await layerApp.app.profile_library('get',libraryId)}catch(e){rejected=e?.color_feature_error==='ProfileChanged'&&layerApp.app.color_feature_error_copy(e,true).includes('changed')}
    await layerApp.app.profile_library('import',undefined,libraryBytes);
    const recipe={...(await layerApp.app.export_presets({type:'get',index:0})).recipe,profile};const saved=await layerApp.app.export_presets({type:'save',name:'Library ownership '+Date.now(),recipe});
    await layerApp.app.profile_library('remove',libraryId);
    const retained=JSON.stringify((await layerApp.app.export_presets({type:'get',index:Number(saved.index)})).recipe.profile.profile.Icc)===JSON.stringify(profile.profile.Icc);
    await layerApp.app.export_presets({type:'remove',index:Number(saved.index)});await layerApp.app.profile_library('import',undefined,libraryBytes);
    return{exact,corrupt:!!corrupt,rejected,retained,count:entries.filter(e=>e.id===libraryId).length};
  })()`);
  assert.deepEqual(report,{exact:true,corrupt:true,rejected:true,retained:true,count:1});
  await invoke('export_document');await wait(`!!document.querySelector('select[aria-label="Output profile"]')`);await click('Saved Profiles…');
  await wait(`!![...document.querySelectorAll('.profile-library .profile-entry')].find(e=>e.textContent.includes(libraryId.slice(0,12)))`);
  await evaluate(`[...document.querySelectorAll('.profile-library .profile-entry')].find(e=>e.textContent.includes(libraryId.slice(0,12))).querySelector('button').click()`);
  // close() clears `open` before the queued close event resolves the picker.
  // Wait for that event's removal before inspecting its selected result.
  await wait(`!document.querySelector('.profile-library')`);
  const chosen=await evaluate(`document.querySelector('select[aria-label="Output profile"]').selectedOptions[0].textContent`);assert.match(chosen,/ProPhoto/i);
  await click('Cancel');await idle();
  await evaluate(`layerApp.dispatch({type:'open_settings',page:'color'})`);
  await wait(`!![...document.querySelectorAll('#settings button')].find(b=>b.textContent==='Manage Profiles…')`);
  await evaluate(`[...document.querySelectorAll('#settings button')].find(b=>b.textContent==='Manage Profiles…').click()`);
  await wait(`!![...document.querySelectorAll('.profile-library .profile-entry')].find(e=>e.textContent.includes(libraryId.slice(0,12)))`);
  await evaluate(`[...[...document.querySelectorAll('.profile-library .profile-entry')].find(e=>e.textContent.includes(libraryId.slice(0,12))).querySelectorAll('button')].find(b=>b.textContent==='Remove').click()`);
  await wait(`![...document.querySelectorAll('.profile-library .profile-entry')].some(e=>e.textContent.includes(libraryId.slice(0,12)))`);
  await evaluate(`[...document.querySelectorAll('.profile-library button')].find(b=>b.textContent==='Done').click();layerApp.dispatch({type:'close_settings'});`);
  await invoke('save_document_as');await idle();
  console.log('ICC library exact bytes/dedup, corruption rejection/repair, independent embedded preset ownership, saved-profile export picker and Preferences management passed');
}


export async function checkFlattenedCopy({evaluate}) {
  const {wait,invoke,click,idle}=helpers(evaluate,{enabled:true});
  const saved=await evaluate('({file:JSON.parse(JSON.stringify(layerApp.state().document_file,(_,v)=>typeof v==="bigint"?Number(v):v)),color:layerApp.app.document_color(),count:sdrFiles.size})');
  const prepare=async()=>{
    await invoke('convert_color_space');await wait(`!!document.querySelector('dialog[open] select[aria-label="Result"]')`);
    await evaluate(`(()=>{for(const [label,value]of [['Result','copy'],['Color space','Srgb']]){const s=document.querySelector('dialog[open] select[aria-label="'+label+'"]');s.value=value;s.dispatchEvent(new Event('change'));}})()`);
    await click('Preview Complete Result');await wait(`!!document.querySelector('canvas[aria-label="Prepared composition"]')`);
    assert.ok(await evaluate(`!![...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Save Copy…'&&!b.disabled)`));
  };
  await prepare();await click('Cancel');await idle();assert.equal(await evaluate('sdrFiles.size'),saved.count);
  // Cancel the actual destination after a complete candidate; publish no bytes.
  await evaluate(`window.sdrCopyPicker=showSaveFilePicker;window.showSaveFilePicker=async()=>{throw new DOMException('Cancelled','AbortError')}`);
  await prepare();await click('Save Copy…');await idle();assert.equal(await evaluate('sdrFiles.size'),saved.count);
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  await evaluate('window.showSaveFilePicker=sdrCopyPicker');
  await prepare();await click('Save Copy…');await idle();
  const copyName=saved.file.location.name.replace(/\.[^.]+$/,'')+' converted.capy';
  const copied=await readPackage(evaluate,`sdrFiles.get(${JSON.stringify(copyName)})`);
  assert.deepEqual(compositionColor(copied),{space:'srgb',depth:saved.color.depth.toLowerCase()});
  assert.equal(packageOccurrences(copied).length,1);assert.equal(packageObject(copied,packageOccurrences(copied)[0].data.content.paint).data.base.policy,'working_pixels');
  const current=await evaluate('JSON.parse(JSON.stringify(layerApp.state().document_file,(_,v)=>typeof v==="bigint"?Number(v):v))');
  for(const key of ['epoch','revision','modified','location'])assert.deepEqual(current[key],saved.file[key]);
  await invoke('save_document_as');await idle();
  const master=await readPackage(evaluate,`sdrFiles.get(${JSON.stringify(saved.file.location.name)})`);
  assert.deepEqual(compositionColor(master),wireColor(saved.color));
  assert.deepEqual(packageComposition(copied).data.size,packageComposition(master).data.size);
  assert.deepEqual(packageComposition(copied).data.resolution,packageComposition(master).data.resolution);
  await evaluate(`window.sdrCopy=sdrFiles.get(${JSON.stringify(copyName)});window.showOpenFilePicker=async()=>[{name:'converted.capy',async getFile(){return new File([sdrCopy],'converted.capy')}}]`);
  await invoke('open_document');await idle();await invoke('save_document_as');await idle();
  const reopened=await readPackage(evaluate,'sdrFiles.get("converted.capy")');assert.deepEqual(sourceIdentity(reopened),sourceIdentity(copied));assert.deepEqual(packageResourceIdentity(reopened),packageResourceIdentity(copied));
  console.log('Flattened full-composition conversion, preview/copy-picker cancellation, native copy reopen, preserved extent/precision and unchanged master checkpoints passed');
}


export async function checkPhotoCorrections({evaluate,settle}) {
  const {wait,invoke,idle}=helpers(evaluate);
  const save=async()=>{await invoke('save_document_as');await idle();return readPackage(evaluate,'sdrFiles.get(layerApp.state().document_file.location.name)');};
  await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-master.capy',async getFile(){return new File([sdrPhotoMaster],'photo-master.capy')}}]`);await invoke('open_document');await idle();
  const source=sourceIdentity(await save());
  const controls=[['exposure','exposure',.75],['white_balance','temperature',25],['levels','gamma',.9],['curves','rgb',[[0,0],[.213,.13],[.79,.9],[1,1]]],['hue_saturation','hue',10],['color_balance','midtones_red',12]];
  const ids=[];
  for(const [name,key,value] of controls){
    const id=await evaluate(`(()=>{layerApp.dispatch({type:'effect',action:{op:'insert',effect:${JSON.stringify(name)}}});const layer=Number(layerApp.state().layer_properties.layer);layerApp.dispatch({type:'effect',action:{op:'set',layer,key:${JSON.stringify(key)},value:{kind:${JSON.stringify(Array.isArray(value)?'curve':'number')},value:${JSON.stringify(value)}}}});layerApp.dispatch({type:'layer',action:{op:'add_mask',id:layer,replace:false}});return layer})()`);ids.push(id);await settle();
  }
  const edited=await save(),portableIds=await Promise.all(ids.map(async id=>packageOccurrences(edited)[await evaluate(`layerApp.state().layers.findIndex(l=>Number(l.id)===${id})`)].id));assert.deepEqual(sourceIdentity(edited),source);assert.equal(packageOccurrences(edited).filter(l=>l.data.content.effect&&l.data.mask).length,6);
  const histogram=histogramJourney({evaluate,settle}).exact;
  const before=await histogram();
  await evaluate(`window.sdrAdjusted=sdrFiles.get('photo-master.capy');window.showOpenFilePicker=async()=>[{name:'adjusted.capy',async getFile(){return new File([sdrAdjusted],'adjusted.capy')}}]`);await invoke('open_document');await idle();
  const reopened=await save();
  assert.deepEqual(authoredIdentity(reopened),authoredIdentity(edited));assert.deepEqual(sourceIdentity(reopened),source);assert.deepEqual(await histogram(),before);
  const reopenedIds=await Promise.all(portableIds.map(id=>runtimeId(evaluate,reopened,id)));
  for(const [i,[name,key,value]]of controls.entries()) {
    const alternate=Array.isArray(value)?[[0,0],[1,1]]:name==='levels'?1.2:-value;
    await evaluate(`layerApp.dispatch({type:'effect',action:{op:'set',layer:${reopenedIds[i]},key:${JSON.stringify(key)},value:{kind:${JSON.stringify(Array.isArray(value)?'curve':'number')},value:${JSON.stringify(alternate)}}}})`);
    await settle();assert.notDeepEqual(await histogram(),before,name+' changes the full composition after reopening');
    await evaluate(`layerApp.dispatch({type:'effect',action:{op:'set',layer:${reopenedIds[i]},key:${JSON.stringify(key)},value:{kind:${JSON.stringify(Array.isArray(value)?'curve':'number')},value:${JSON.stringify(value)}}}})`);
    await settle();assert.deepEqual(await histogram(),before,name+' reevaluates retained input exactly');
  }
  await evaluate(`layerApp.dispatch({type:'layer',action:{op:'invert_mask',id:${reopenedIds[0]}}})`);await settle();assert.notDeepEqual(await histogram(),before);
  await invoke('undo');await idle();assert.deepEqual(await histogram(),before);
  const final=await save();assert.deepEqual(sourceIdentity(final),source);
  assert.equal(await evaluate('layerApp.state().host_error??null'),null);
  console.log('All six ProPhoto U16 correction layers and masks remain revisable after reopen; full-resolution histograms change and restore exactly; original source bytes/profile retained');
}
