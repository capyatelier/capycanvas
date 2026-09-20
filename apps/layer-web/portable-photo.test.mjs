import assert from 'node:assert/strict';
import {mkdir,readFile,writeFile} from 'node:fs/promises';

// Real UI, Wasm workers, GPU capture and OPFS output. Only OS picker handles
// are supplied by the harness. Native browser decoding is an independent SDR
// interoperability check, never part of the application's photo pipeline.
export async function checkPortablePhoto({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/portable-photo/web';
  await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>60000)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1600)));else setTimeout(poll,30)}catch(e){reject(e)}}poll()})`);
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);};
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing enabled '+${JSON.stringify(label)});b.click()})()`);
  const set=(label,value)=>evaluate(`(()=>{const n=document.querySelector('dialog[open] [aria-label="'+${JSON.stringify(label)}+'"]');if(!n||n.disabled)throw Error('Missing enabled '+${JSON.stringify(label)});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);
  const idle=()=>wait('!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
  const open=async name=>{
    const epoch=await evaluate('Number(layerApp.state().document_file.epoch)');
    await evaluate(`portablePhoto.openName=${JSON.stringify(name)}`);await invoke('open_document');
    await wait(`Number(layerApp.state().document_file.epoch)!==${epoch}&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()`);
  };
  const histogram=()=>evaluate(`(async()=>{const c=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify((await layerApp.app.histogram(c)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v))}finally{c.free()}})()`);
  const begin=async range=>{await invoke('export_document');await wait(`!!document.querySelector('dialog[open] [aria-label="Dynamic range"]')`);await set('Dynamic range',range);};
  const preview=async()=>{await click('Preview Output');await wait(`document.querySelectorAll('dialog[open] .color-comparison canvas').length===2&&!Array.from(document.querySelectorAll('dialog[open] button')).find(b=>b.textContent==='Preview Output').disabled`);};
  const outputPixels=()=>evaluate(`(()=>{const c=document.querySelector('canvas[aria-label="Output preview"]');return {extent:[c.width,c.height],pixels:Array.from(c.getContext('2d').getImageData(0,0,c.width,c.height).data)}})()`);
  const close=async()=>{await click('Cancel');await idle();};
  const save=async name=>{
    await evaluate('portablePhoto.last=null');await click('Choose File…');await idle();
    await wait('!!portablePhoto.last');
    const result=await evaluate(`({options:portablePhoto.saveOptions,bytes:Array.from(portablePhoto.last)})`);
    await evaluate(`portablePhoto.files.set(${JSON.stringify(name)},portablePhoto.last.slice())`);
    await writeFile(`${directory}/${name}`,new Uint8Array(result.bytes));
    return result;
  };
  const state=()=>evaluate(`JSON.parse(JSON.stringify({file:layerApp.state().document_file,layers:layerApp.state().layers,history:layerApp.state().history},(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const results={browser:await evaluate('navigator.userAgent'),imports:[],exports:[]};
  await evaluate(`window.portablePhoto={files:new Map(),open:window.showOpenFilePicker,save:window.showSaveFilePicker};
    portablePhoto.dismiss=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>['Keep for Later','Discard Changes'].includes(b.textContent))?.click(),50);
    window.showOpenFilePicker=async()=>[{name:portablePhoto.openName,async getFile(){return new File([portablePhoto.files.get(portablePhoto.openName)],portablePhoto.openName)}}];
    window.showSaveFilePicker=async options=>{portablePhoto.saveOptions=options;return{name:options.suggestedName,async createWritable(){let bytes;return{async write(b){bytes=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){portablePhoto.last=bytes},async abort(){}}}}};`);
  try {
    for(const [folder,name] of [['heif','p3-grid-8bit.heic'],['heif','p3-gray-10bit.heic'],['heif','flat-red-8bit.heic'],['avif','p3-12bit.avif'],['avif','hdr-rgb.avif']]) {
      const bytes=await readFile(new URL(`../../crates/layer-color/tests/fixtures/${folder}/${name}`,import.meta.url));
      await evaluate(`portablePhoto.files.set(${JSON.stringify(name)},Uint8Array.from(atob(${JSON.stringify(bytes.toString('base64'))}),c=>c.charCodeAt(0)))`);
      await open(name);
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      results.imports.push({name,color:await evaluate('layerApp.app.document_color()')});
    }
    assert.equal(results.imports.at(-1).color.depth,'F16');
    console.log('Portable browser Open: HEIC 8/10-bit, 12-bit AVIF and HDR gain maps use shared Rust.');

    // Preserve one fixed master; output and preview must not alter its history,
    // source layers, epoch or saved SDR recipe.
    for(const range of ['jpeg','avif']) {
      await open('hdr-rgb.avif');
      await evaluate(`layerApp.dispatch({type:'effect',action:{op:'insert',effect:'exposure'}});
        layerApp.dispatch({type:'effect',action:{op:'set',layer:Number(layerApp.state().layer_properties.layer),key:'exposure',value:{kind:'number',value:2}}});`);
      await settle();
      const before=await state(),master=await histogram();
      assert.ok(master.channels.some(c=>c.above>0),'Edited HDR samples exceed SDR white');
      await begin(range);
      assert.ok(await evaluate(`Array.from(document.querySelector('[aria-label="Dynamic range"]').options).filter(o=>['jpeg','avif'].includes(o.value)).every(o=>!o.disabled)`));
      if(range==='jpeg') {
        await click('Preview Output');
        await wait(`!!document.querySelector('dialog[open] .error-message').textContent`);
        assert.match(await evaluate(`document.querySelector('dialog[open] .error-message').textContent`),/transparen|background|flatten/i);
        await set('Transparency','White');
      }
      await set('Quality',90);
      await set('Pixel size','Fit');await set('Maximum width',32);await set('Maximum height',24);
      await set('Resolution metadata','Ppi');await set('Pixels per inch',144);
      await preview();
      const hdr=await outputPixels();await set('Preview rendition','sdr');const sdr=await outputPixels();
      await set('Preview rendition','hdr');assert.deepEqual(await outputPixels(),hdr,'Switching views reuses the completed encoding');
      if(await evaluate(`Array.from(document.querySelectorAll('dialog[open] button')).find(b=>b.textContent==='Choose File…').disabled`)) {
        await evaluate(`const n=document.querySelector('[aria-label="Clip out-of-range HDR colors"]');n.checked=true;n.dispatchEvent(new Event('change',{bubbles:true}))`);
        await preview();await set('Preview rendition','sdr');
      } else await set('Preview rendition','sdr');
      const encodedSdr=await outputPixels();
      await evaluate(`document.querySelector('canvas[aria-label="Output preview"]').scrollIntoView({block:'center'})`);await settle();
      await writeFile(`${directory}/${range}-preview.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false})).data,'base64'));
      const name=`web-hdr.${range==='jpeg'?'jpg':'avif'}`,file=await save(name);
      const mime=`image/${range}`;
      assert.deepEqual(file.options.types[0].accept,{[mime]:[range==='jpeg'?'.jpg':'.avif']});
      assert.ok(file.options.suggestedName.endsWith(range==='jpeg'?'.jpg':'.avif'));
      assert.deepEqual(await histogram(),master);
      assert.deepEqual(await state(),before,'Delivery leaves master/document history unchanged');
      // Avoid quantizing premultiplied wide-gamut pixels before color conversion.
      // Chrome's default Image path loses visible precision around transparent,
      // saturated colors; libavif's straight RGBA16 agrees with the Rust preview.
      const decoded=await evaluate(`(async()=>{const blob=new Blob([portablePhoto.files.get(${JSON.stringify(name)})],{type:${JSON.stringify(mime)}});const image=await createImageBitmap(blob,{premultiplyAlpha:'none'});try{const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const x=c.getContext('2d',{willReadFrequently:true,colorSpace:'srgb'});x.drawImage(image,0,0);return {extent:[c.width,c.height],pixels:Array.from(x.getImageData(0,0,c.width,c.height).data)}}finally{image.close()}})()`);
      assert.deepEqual(decoded.extent,encodedSdr.extent);
      let maximum=0;
      for(let i=0;i<decoded.pixels.length;i++) {
        // Unpremultiplication at nearly transparent pixels magnifies rounding;
        // compare visible color in premultiplied bytes, and alpha independently.
        const a=decoded.pixels[(i&~3)+3]/255,b=encodedSdr.pixels[(i&~3)+3]/255;
        const difference=i%4===3?Math.abs(decoded.pixels[i]-encodedSdr.pixels[i]):Math.abs(decoded.pixels[i]*a-encodedSdr.pixels[i]*b);
        maximum=Math.max(maximum,difference);
      }
      assert.ok(maximum<=4,`${range}: browser SDR decode differs from encoded Rust preview by ${maximum}; first pixels ${decoded.pixels.slice(0,16)} vs ${encodedSdr.pixels.slice(0,16)}`);
      await open(name);assert.equal(await evaluate('layerApp.app.document_color().depth'),'F16');
      const reopened=await histogram();assert.ok(reopened.channels.some(c=>c.above>0),'Saved file reopens as HDR');
      if(range==='jpeg')assert.equal(reopened.transparent,0);
      else assert.ok(decoded.pixels.some((v,i)=>i%4===3&&v<250),'AVIF retains coverage');
      results.exports.push({name,bytes:file.bytes.length,extent:decoded.extent,maximumSdrDifference:maximum,hdrPreview:hdr.extent,sdrPreview:sdr.extent});
      console.log(`${name}: encoded previews, correct picker type, save, independent SDR decode and HDR reopen pass.`);
    }

    // A saved cross-platform gain-map preset remains editable and selectable.
    await begin('avif');
    await set('Preset name','Portable HDR');await click('Save Preset');
    await wait(`Array.from(document.querySelector('[aria-label="Destination"]').options).some(o=>o.textContent==='Portable HDR')`);
    await close();await begin('jpeg');
    const index=await evaluate(`Array.from(document.querySelector('[aria-label="Destination"]').options).find(o=>o.textContent==='Portable HDR').value`);
    await set('Destination',index);await wait(`document.querySelector('[aria-label="Dynamic range"]').value==='avif'&&!document.querySelector('[aria-label="Destination"]').disabled`);
    await close();

    // Cancel a live file worker, check that its temporary OPFS job disappears,
    // and retry without touching the document or publishing a partial file.
    await begin('avif');
    const cancellation=await evaluate(`(async()=>{const id=layerApp.state().requests.find(r=>r.kind.type==='document'&&r.kind.request.type==='export').id;
      const recipe=layerApp.app.export_draft(layerApp.app.export_form().recipes[0][1],{type:'format',value:'AvifHdrMapped'}).recipe;
      recipe.size={Fit:{bounds:[1024,1024],enlarge:true}};const c=layerApp.app.capture_control();
      const post=Worker.prototype.postMessage;let timer,encoding=false,started;
      Worker.prototype.postMessage=function(message,...args){const result=post.call(this,message,...args);if(message.request?.operation==='output-encode'){encoding=true;started=performance.now();timer=setTimeout(()=>c.cancel(),25)}return result};
      try{await layerApp.app.export_image(id,recipe,c,true);return {error:null,encoding}}catch(e){return {error:String(e),encoding,milliseconds:performance.now()-started}}finally{Worker.prototype.postMessage=post;clearTimeout(timer);c.free()}})()`);
    assert.equal(cancellation.encoding,true,'Cancellation starts after dispatching the codec worker');
    assert.match(cancellation.error,/cancel/i);
    assert.ok(cancellation.milliseconds<3000,`Worker cancellation took ${cancellation.milliseconds}ms`);
    results.cancellation=cancellation;
    await preview();await close();
    const jobs=await evaluate(`(async()=>{const root=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-output');const names=[];for await(const n of root.keys())names.push(n);return names})()`);
    assert.deepEqual(jobs,[],'Successful, preview and cancelled outputs release their temporary files');
    assert.equal(await evaluate('layerApp.state().host_error??null'),null);
    await writeFile(`${directory}/report.json`,JSON.stringify(results,null,2)+'\n');
    console.log('Gain-map saved presets, cancellation/retry and OPFS cleanup pass.');
  } finally {
    await evaluate(`clearInterval(portablePhoto.dismiss);window.showOpenFilePicker=portablePhoto.open;window.showSaveFilePicker=portablePhoto.save;`);
  }
}
