import assert from 'node:assert/strict';

// Run on the real WebGPU host. More than sixteen source tiles forces a cold
// neighborhood decode while G-Pen is initializing its destination paint tiles.
// An optional URL exercises the same workflow on the full 61 MP tablet fixture.
export async function checkPhotoPaint({call, evaluate, settle}, photoUrl = null) {
  const wait = condition => evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve();else if(performance.now()-start>120000)reject(Error(${JSON.stringify(condition)}+': '+document.body.innerText.slice(-1200)));else setTimeout(poll,30)}catch(e){reject(e)}}poll()})`);
  const invoke = async command => {
    await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    await evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  };
  const histogram = () => evaluate(`(async()=>{const c=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify((await layerApp.app.histogram(c)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v))}finally{c.free()}})()`);
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await evaluate(`window.photoPaint={open:window.showOpenFilePicker,save:window.showSaveFilePicker,files:new Map()};`);
  try {
    await evaluate(`(async()=>{
      layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
      let blob;
      if(${JSON.stringify(photoUrl)}){const response=await fetch(${JSON.stringify(photoUrl)});if(!response.ok)throw Error('Photo fixture unavailable');blob=await response.blob();}
      else{const c=new OffscreenCanvas(4353,769),x=c.getContext('2d',{willReadFrequently:true});x.fillStyle='rgb(70,140,210)';x.fillRect(0,0,c.width,c.height);blob=await c.convertToBlob({type:'image/png'});}
      window.showOpenFilePicker=async()=>[{name:'photo-paint.png',async getFile(){return new File([blob],'photo-paint.png')}}];
      window.showSaveFilePicker=async options=>({name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){photoPaint.files.set(options.suggestedName,bytes)},async abort(){}}}});
    })()`);
    await invoke('open_document');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    await invoke('fit_canvas');
    const extent = await evaluate('[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height]');
    assert.deepEqual(extent, photoUrl ? [9504, 6336] : [4353, 769]);
    const opaque = value => {
      assert.equal(value.transparent, 0, 'G-Pen must preserve the opaque photograph in touched tiles');
      assert.equal(value.pixels, extent[0] * extent[1]);
      return value;
    };
    const original = opaque(await histogram());
    console.log(`Photo ${extent.join('×')}: imported with no transparent pixels`);
    await evaluate(`layerApp.dispatch({type:'select_brush',id:1});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[1,0,.7,1/3]}}})`);
    await settle();
    const point = await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    for (const [type, dx, buttons] of [['mousePressed',-80,1],['mouseMoved',0,1],['mouseMoved',80,1],['mouseReleased',80,0]]) {
      await call('Input.dispatchMouseEvent', {type,x:point.x+dx,y:point.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.65:0});
      await settle();
    }
    await wait('layerApp.state().document_file.modified');
    const painted = opaque(await histogram());
    assert.notDeepEqual(painted, original, 'The stroke must change actual photo pixels');
    await invoke('undo'); await settle(); assert.deepEqual(opaque(await histogram()), original);
    await invoke('redo'); await settle(); assert.deepEqual(opaque(await histogram()), painted);
    console.log('G-Pen opacity and exact undo/redo passed');
    const save = async () => {
      await invoke('save_document_as');
      await wait('!layerApp.state().document_file.busy && !layerApp.state().document_file.modified');
      return evaluate(`(()=>{const bytes=[...photoPaint.files.values()].at(-1);return JSON.parse(new TextDecoder().decode(bytes.slice(52,52+Number(new DataView(bytes.buffer,bytes.byteOffset).getBigUint64(12,true)))));})()`);
    };
    const saved = await save();
    assert.ok(saved.blobs.length > 0);
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'photo-paint.capy',async getFile(){return new File([[...photoPaint.files.values()].at(-1)],'photo-paint.capy')}}];`);
    await invoke('open_document'); await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.app.brush_ready()');
    assert.deepEqual(opaque(await histogram()), painted);
    const reopened = await save();
    assert.deepEqual(reopened.blobs, saved.blobs, 'Native paint backing survives reopening exactly');
    console.log('Native save/reopen passed; replacing GPU');
    await evaluate('layerApp.restartGpu()');
    await wait('layerApp.app.brush_ready() && layerApp.startupTimes.complete!==null');
    assert.deepEqual(opaque(await histogram()), painted);
    assert.equal(await evaluate('layerApp.state().host_error??null'), null);
    console.log(`Photo ${extent.join('×')}: G-Pen preserves opacity; exact undo/redo, native save/reopen and GPU recovery passed`);
  } finally {
    await evaluate('window.showOpenFilePicker=photoPaint.open;window.showSaveFilePicker=photoPaint.save;delete window.photoPaint;');
  }
}
