import assert from 'node:assert/strict';
import {packageObject,packageComposition,packageOccurrences,rasterIdentity,sourceContent,imageIdentity,imageContent} from './package-fixture.test.mjs';
import {png} from './clone-journey.test.mjs';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {measurePlacedPhotos,placementSave} from './image-placement-motion.test.mjs';

const map=([a,b,c,d,e,f],x,y)=>[a*x+c*y+e,b*x+d*y+f];
const unmap=([a,b,c,d,e,f],[x,y])=>{const det=a*d-b*c;return[(d*(x-e)-c*(y-f))/det,(-b*(x-e)+a*(y-f))/det];};
const near=(a,b,tolerance=.01)=>a.length===b.length&&a.every((v,i)=>Math.abs(v-b[i])<tolerance);

// Real browser file-input/drop transport, shared image placement and native archives.
// LAYER_PHOTO_FILES optionally selects camera originals (JSON array of paths).
export async function checkImagePlacement({call,evaluate,settle}) {
  const root=await mkdtemp(join(tmpdir(),'capy-image-placement-'));
  const captures=process.env.LAYER_IMAGE_CAPTURE_DIR;
  if(captures)await mkdir(captures,{recursive:true});
  const capture=async name=>{
    if(!captures)return;
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(join(captures,`${name}.png`),Buffer.from(shot.data,'base64'));
  };
  const wait=async condition=>{
    const start=Date.now();while(!await evaluate(`!!(${condition})`)){
      if(Date.now()-start>180000)throw Error(condition+': '+await evaluate('JSON.stringify({file:layerApp.state().document_file,requests:layerApp.state().requests,error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector("#status")?.textContent},(_,v)=>typeof v==="bigint"?String(v):v)'));
      await new Promise(resolve=>setTimeout(resolve,100));
    }
  };
  const invoke=async command=>{
    await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`,userGesture:true});
    assert.equal(reply.exceptionDetails,undefined);await settle();
  };
  const command=id=>evaluate(`(c=>c&&{enabled:c.enabled,reason:c.disabled_reason??null,selected:c.selected})(layerApp.state().commands.find(c=>c.id===${JSON.stringify(id)}))`);
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?String(v):v))');
  const idle=()=>wait('!layerApp.state().document_file.busy');
  const placed=()=>wait("layerApp.state().canvas_bar?.context.kind==='placement'&&layerApp.state().commands.find(c=>c.id==='apply_transform').enabled");
  const click=async selector=>{
    await wait(`(n=>n && !n.disabled && n.getAttribute('aria-disabled')!=='true' && !n.closest('.canvas-action-bar.suppressed'))(document.querySelector(${JSON.stringify(selector)}))`);
    const p=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
  };
  const choose=async files=>{
    await wait('document.querySelector("input[type=file]")');
    const {root}=await call('DOM.getDocument');
    const {nodeId}=await call('DOM.querySelector',{nodeId:root.nodeId,selector:'input[type=file]'});
    await evaluate(`document.querySelector('input[type=file]').addEventListener('change',e=>{placementTest.inputFiles=[...e.target.files]},{once:true})`);
    await call('DOM.setFileInputFiles',{nodeId,files});
  };
  const importFiles=async files=>{await invoke('import_image');await choose(files);await idle();await placed();};
  const drop=async(p,files)=>{
    await wait('layerApp.state().commands.find(c=>c.id==="import_image").enabled');
    for(const type of ['dragEnter','dragOver','drop'])await call('Input.dispatchDragEvent',{type,...p,data:{items:[],files,dragOperationsMask:1}});
  };
  const save=placementSave({evaluate,invoke,idle});
  const imageLayers=m=>packageOccurrences(m).filter(o=>o.data.content.objects);
  const objectsIn=m=>imageLayers(m).map(o=>packageObject(m,o.data.content.objects));
  const extent=(m,object)=>packageObject(m,object.data.image).data.extent;
  const affine=object=>object.data.affine??[1,0,0,1,0,0];
  const activeLayer=async()=>(await state()).layers.find(l=>l.editing);
  const selectedImages=async()=>(await state()).layers.filter(o=>o.object&&o.selected).map(o=>o.id);
  const screen=async(x,y)=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
  const pointer=async(type,p,pointerType='pen')=>{await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType,force:type==='mouseReleased'?0:.65});await settle();};
  const drag=async(p,dx,dy)=>{await pointer('mousePressed',p);for(let i=1;i<=4;i++)await pointer('mouseMoved',{x:p.x+dx*i/4,y:p.y+dy*i/4});await pointer('mouseReleased',{x:p.x+dx,y:p.y+dy});await wait('layerApp.app.brush_ready()');};
  const stroke=async(x=1000,y=750)=>{
    await wait('layerApp.app.brush_ready()');
    const p=await screen(x,y);
    for(const [type,dx] of [['mousePressed',-30],['mouseMoved',0],['mouseMoved',30],['mouseReleased',30]])await pointer(type,{x:p.x+dx,y:p.y});
    await wait('layerApp.app.brush_ready()');
  };
  const pixelAt=async(x,y)=>{
    const p=await screen(x,y);
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:p.x,y:p.y,width:1,height:1,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const canvas=document.createElement('canvas');canvas.width=canvas.height=1;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return Array.from(context.getImageData(0,0,1,1).data);})()`);
  };
  const selectImage=async layer=>{
    await wait('!layerApp.documents.busy()');
    const row=(await state()).layers.find(o=>o.id===String(layer));
    await evaluate(`layerApp.dispatch({type:'select_layer',id:${row.id}n})`);await settle();
    assert.deepEqual(await selectedImages(),[row.id]);
  };
  let files;
  try {
    await call('Page.setInterceptFileChooserDialog',{enabled:true});
    await evaluate(`window.placementTest={open:window.showOpenFilePicker,save:window.showSaveFilePicker};window.showOpenFilePicker=undefined;
      window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(b){placementTest.saved=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){},async abort(){}}}});
      layerApp.dispatch({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});`);
    if(process.env.LAYER_PHOTO_FILES)files=JSON.parse(process.env.LAYER_PHOTO_FILES);
    else {
      files=[];
      for(const [i,w,h] of [[0,3000,2400],[1,800,600]]){
        const bytes=png(w,h,(x,y)=>{const t=(x/w+y/h)/2;return [Math.round(255*(1-t)),0,Math.round(255*t),x===0||y===0||x===w-1||y===h-1?0:255];},4);
        assert.equal(bytes[25],6,'Photo fixture retains transparent RGBA edges');
        const path=join(root,`photo-${i}.png`);await writeFile(path,new Uint8Array(bytes));files.push(path);
      }
    }
    const bad=join(root,'malformed.png');await writeFile(bad,'not an image');
    await invoke('new_document');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label)?.click()`);
    await wait('document.querySelector("dialog[open] [data-document-field=width]")');
    await evaluate(`{for(const [id,value] of [['width',2000],['height',1500]]){const entry=document.querySelector('dialog[open] [data-document-field='+id+']');entry.value=value;entry.dispatchEvent(new Event('input',{bubbles:true}));}document.querySelector('dialog[open] [data-document-action=create]').click();}`);
    await idle();await wait('layerApp.app.brush_ready()');
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await invoke('select_all');await invoke('fill_selection');
      const linkedPaint=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${linkedPaint},replace:false}});layerApp.dispatch({type:'layer',action:{op:'select',id:${linkedPaint},mask:false}})`);await settle();
      await invoke('scale_rotate');await wait("layerApp.state().canvas_bar?.context.kind==='transform'");
      await invoke('transform_warp');await invoke('cancel_transform');
      await invoke('undo');await invoke('undo');await invoke('deselect');
    }
    console.log('Image placement: paint Warp preview and Cancel passed');
    const base=await state(),baseCount=base.layers.length;
    assert.deepEqual(await evaluate('layerApp.app.photo_formats().map(f=>f.name)'),['OpenEXR','TIFF','PNG','WebP','BMP','JPEG','GIF','HEIF','AVIF']);

    await importFiles(files);
    assert.equal((await state()).layers.length,baseCount+files.length,'Each file arrives as a named Object layer');
    assert.equal((await state()).layers.filter(l=>l.object).length,files.length);
    assert.equal((await selectedImages()).length,files.length,'Placement selects every inserted image');
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,baseCount,'Cancel leaves no records');
    assert.equal((await state()).document_file.modified,base.document_file.modified);
    assert.equal((await command('undo')).enabled,base.commands.find(c=>c.id==='undo').enabled,'Cancel leaves no history');

    await importFiles(files);
    await click('.canvas-action-bar [data-command=apply_transform]');
    const fitted=await save(),sources=imageIdentity(fitted),originalContents=imageContent(fitted);
    assert.equal(imageLayers(fitted).length,files.length);
    assert.equal(sources.length,files.length,'Each placed photo is one immutable image');
    const placedObjects=objectsIn(fitted);
    assert.equal(placedObjects.length,files.length);
    for(const object of placedObjects){
      const [w,h]=extent(fitted,object),scale=Math.min(1,2000/w,1500/h);
      assert.ok(near(affine(object),[scale,0,0,scale,(2000-w*scale)/2,(1500-h*scale)/2],1e-6),'Interactive placement fits and centres each image');
      assert.equal(object.data.interpolation??'linear','linear');
    }
    await invoke('undo');assert.equal((await state()).layers.length,baseCount,'Placement is one undo step');
    await invoke('redo');assert.equal((await state()).layers.length,baseCount+files.length);
    await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.saved],'placed.capy')}}]`);
    await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');
    await evaluate('window.showOpenFilePicker=undefined');
    assert.deepEqual(imageIdentity(await save()),sources,'Reopen keeps image identity and samples');
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    assert.deepEqual(imageIdentity(await save()),sources,'GPU replacement retains placed images');
    if(!process.env.LAYER_PHOTO_FILES) {
      let photoPixel;
      for(const end=Date.now()+60000;Date.now()<end;await new Promise(r=>setTimeout(r,200))){photoPixel=await pixelAt(400,300);if(photoPixel[0]>20&&photoPixel[1]<60&&photoPixel[2]>10)break;}
      assert.ok(photoPixel[0]>20&&photoPixel[1]<60&&photoPixel[2]>10,'Placed photo is visible after GPU restart: '+photoPixel);
    }
    console.log('Image placement: batch, Cancel, fit, one-step history, reopen and GPU replacement passed');

    await evaluate('placementTest.placedMaster=placementTest.saved.slice()');
    const reopened=await save(),large=objectsIn(reopened).find(o=>extent(reopened,o)[0]===Math.max(...objectsIn(reopened).map(o=>extent(reopened,o)[0]))),largeExtent=extent(reopened,large);
    const objectAffine=async()=>affine(packageObject(await save(),large.id));
    const start=affine(large),largeCentre=map(start,largeExtent[0]/2,largeExtent[1]/2);
    const name=packageOccurrences(reopened).find(o=>o.data.content.objects?.ref===large.id).data.name;
    const layer=(await state()).layers.find(l=>l.object&&l.label===name).id;
    await invoke('move');await selectImage(layer);
    for(const id of ['transform_distort','transform_warp']) {
      const shown=await command(id);
      if(shown){assert.equal(shown.enabled,false,`${id} is unavailable for images`);assert.ok(shown.reason,`${id} says why`);}
    }
    await click('.canvas-action-bar [data-command=placement_original_size]');
    const original=await objectAffine();
    assert.ok(near(original.slice(0,4),[1,0,0,1],1e-9),'Original Size restores unit source scale: '+original);
    assert.ok(near(map(original,largeExtent[0]/2,largeExtent[1]/2),largeCentre,1e-6),'Original Size keeps the image centre');
    await invoke('undo');assert.ok(near(await objectAffine(),start,1e-9),'Original Size is one undo step');

    await drag(await screen(...map(start,largeExtent[0]*.3,largeExtent[1]*.3)),40,25);
    const moved=await objectAffine(),delta=[moved[4]-start[4],moved[5]-start[5]];
    assert.ok(Math.hypot(...delta)>10&&near(moved.slice(0,4),start.slice(0,4),1e-9),'Dragging the image moves it: '+moved);
    await invoke('transform_again');
    const again=await objectAffine();
    assert.ok(near([again[4]-moved[4],again[5]-moved[5]],delta,1e-6),'Transform Again repeats the last image move');
    await invoke('undo');assert.ok(near(await objectAffine(),moved,1e-9),'Transform Again is one undo step');
    await invoke('redo');assert.ok(near(await objectAffine(),again,1e-9));
    await invoke('undo');await invoke('undo');assert.ok(near(await objectAffine(),start,1e-9),'The drag is one undo step');

    const zoom=(await state()).camera.zoom;
    await drag(await screen(...largeCentre),21,13);
    const pivot=[largeCentre[0]+21/zoom,largeCentre[1]+13/zoom];
    assert.ok(near(await objectAffine(),start,1e-9),'Moving the pivot leaves the image in place');
    await invoke('transform_rotate_right');
    assert.ok(near(map(await objectAffine(),...unmap(start,pivot)),pivot,.05),'A quarter turn keeps the custom pivot fixed');
    await invoke('undo');assert.ok(near(await objectAffine(),start,1e-9),'The quarter turn is one undo step');

    if(!(await command('transform_snapping'))?.selected)await invoke('transform_snapping');
    assert.equal((await command('transform_snapping')).selected,true,'Snapping publishes its shared state');
    const snapZoom=(await state()).camera.zoom;
    assert.ok(Math.abs(largeCentre[0]-2000)>100,'The snap fixture starts away from its target');
    await drag(await screen(largeCentre[0]+90,largeCentre[1]+80),(2000-largeCentre[0])*snapZoom-3,0);
    const snapped=map(await objectAffine(),largeExtent[0]/2,largeExtent[1]/2);
    assert.ok(Math.abs(snapped[0]-2000)<.05,'A body drag within three logical pixels snaps the image centre to the canvas edge: '+snapped);
    await invoke('undo');assert.ok(near(await objectAffine(),start,1e-9),'The snapped drag is one undo step');
    await invoke('transform_snapping');
    console.log('Image placement: Original Size, drag, Transform Again, custom pivot turn, snapping and unavailable Distort/Warp passed');

    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      const before=await save(),label=(await state()).layers.find(l=>l.id===String(layer)).label;
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${layer},mask:false}})`);await settle();
      await invoke('brush');
      await evaluate(`layerApp.dispatch({type:'select_brush',id:21});layerApp.dispatch({type:'set_brush_size',value:80});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[.15,.25,.9,1]}}});`);await settle();
      await stroke();
      await wait(`layerApp.state().notice?.actions?.length===3`);
      const offered=(await state()).notice;
      assert.deepEqual(offered.actions.map(a=>a.id),['add_mask','new_paint_layer','rasterize_layer'],'Painting on images offers mask, paint layer and rasterize');
      assert.deepEqual(rasterIdentity(await save()),rasterIdentity(before),'The refused stroke changes nothing');
      assert.deepEqual(imageIdentity(await save()),sources);
      await capture(`${theme}-image-refusal`);
      await stroke();
      await wait(`layerApp.state().notice?.actions?.length===3&&layerApp.state().notice.id!==${offered.id}n`);
      const actionButton=await evaluate(`(()=>{const b=document.querySelectorAll('.canvas-notice-actions .canvas-notice-action')[2];const r=b.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2,label:b.textContent,disabled:b.disabled}})()`);
      assert.equal(actionButton.disabled,false,'Rasterize Layer is offered enabled');
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...actionButton,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
      await wait(`!layerApp.state().layers.find(l=>l.id==${layer}n)?.object&&layerApp.app.brush_ready()&&!layerApp.state().document_file.busy`);
      const rasterized=await save();
      assert.equal(imageLayers(rasterized).length,files.length-1,'Rasterize Layer converts only the selected Object layer');
      const paint=packageOccurrences(rasterized).find(o=>o.data.name===label&&o.data.content.paint);
      assert.ok(paint,'The occurrence keeps its name');
      await stroke();
      const painted=await save();
      assert.notDeepEqual(rasterIdentity(painted),rasterIdentity(rasterized),'The rasterized layer paints immediately');
      assert.ok(new Set(packageObject(painted,paint.data.content.paint).data.tiles.map(t=>t.plane)).has('watercolor_wetness'),'Wet watercolor writes its scalar plane on the rasterized layer');
      await invoke('liquify');await evaluate(`layerApp.dispatch({type:'select_brush',id:13});layerApp.dispatch({type:'set_brush_size',value:80});`);await settle();
      await stroke();assert.notDeepEqual(rasterIdentity(await save()),rasterIdentity(painted),'Liquify edits the rasterized layer');
      await invoke('undo');await invoke('undo');assert.deepEqual(rasterIdentity(await save()),rasterIdentity(rasterized));
      await invoke('undo');
      const restored=await save();
      assert.equal(imageLayers(restored).length,files.length,'One Undo restores the selected Object beside its unchanged siblings');
      assert.deepEqual(imageIdentity(restored),sources);
      assert.deepEqual(objectsIn(restored).map(o=>o.id).sort(),objectsIn(before).map(o=>o.id).sort(),'Undo restores the image identities');
      console.log(`Image placement ${theme}: paint refusal, Rasterize Layer, painting, Liquify and one-step Undo passed`);
    }


    const before=await state();await invoke('import_image');await choose([files[0],bad]);await idle();
    assert.equal((await state()).layers.length,before.layers.length,'A malformed second file discards the batch');assert.ok((await state()).host_error);
    const editing=(await activeLayer()).id;
    await invoke('import_image');await wait('document.querySelector("input[type=file]")');
    const last=before.layers.at(-1).id;
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${last},mask:false}})`);await settle();
    assert.equal((await activeLayer()).id,editing,'The selected layer cannot change while the file chooser is open');
    await choose([files[0]]);await idle();await placed();
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);
    const p=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect(),c=layerApp.app.camera(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.6)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.6)*r.height/c.viewport[1]}})()`);
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${layer},mask:false}})`);await settle();
    await drop(p,[files[0]]);
    await idle();await placed();
    assert.equal((await state()).layers.length,before.layers.length+1,'A canvas drop creates a sibling Object layer');
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);
    const paint=before.layers.find(l=>!l.group&&!l.object&&l.label!=='Paper');
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${paint.id},mask:false}})`);await settle();
    await drop(p,[files[0]]);
    await idle();await placed();
    assert.equal((await state()).layers.length,before.layers.length+1,'A canvas drop above paint makes an image layer');
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);
    const childRow=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${layer}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await drop(childRow,[files[1]]);
    await idle();await placed();
    assert.equal((await state()).layers.length,before.layers.length+1,'A drop on an Object row creates a named sibling');
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);


    await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
    const copied=await call('Runtime.evaluate',{expression:`navigator.clipboard.write([new ClipboardItem({['web '+placementTest.inputFiles[0].type]:placementTest.inputFiles[0]})])`,userGesture:true,awaitPromise:true});
    assert.equal(copied.exceptionDetails,undefined);
    const countBefore=(await state()).layers.length;
    await invoke('paste_image');await idle();await placed();
    await click('.canvas-action-bar [data-command=apply_transform]');
    const pasted=imageContent(await save());
    assert.ok(pasted.some(image=>originalContents.some(original=>JSON.stringify(image)===JSON.stringify(original))),'Clipboard retains original source samples');
    await invoke('undo');assert.equal((await state()).layers.length,countBefore);

    await evaluate(`placementTest.read=File.prototype.arrayBuffer;File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    const pendingLayers=(await state()).layers.length;
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await click('.file-progress button');await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,pendingLayers,'A cancelled read publishes nothing');
    await evaluate(`File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,pendingLayers,'A read from the retired GPU cannot publish layers');

    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'new',group:true,clipped:false}})`);await settle();
    const group=(await state()).layers.find(l=>l.group),groupCount=(await state()).layers.length;
    const rowPoint=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${group.id}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await drop(rowPoint,[files[0]]);
    await idle();await placed();await click('.canvas-action-bar [data-command=apply_transform]');
    const grouped=await save(),groupOccurrence=packageOccurrences(grouped).find(o=>o.data.name===group.label&&o.data.content.stack);
    assert.ok(packageObject(grouped,groupOccurrence.data.content.stack).data.entries.some(ref=>packageObject(grouped,ref).data.content.objects),'A drop on a group row inserts an image layer into the group');
    await invoke('undo');assert.equal((await state()).layers.length,groupCount);
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'lock',id:${group.id},value:true}})`);await settle();
    await drop(rowPoint,[files[0]]);
    await settle();assert.equal((await state()).layers.length,groupCount,'A locked group refuses the drop');assert.equal((await state()).document_file.busy,false);

    for(let i=0;i<files.length;i++) {
      await invoke('open_document');await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label)?.click()`);
      await choose([files[i]]);await idle();await wait('layerApp.app.brush_ready()');
      const opened=await save(),photo=sourceContent(opened)[0];
      assert.equal(imageLayers(opened).length,0,'Open Image makes a paint layer');
      assert.ok(originalContents.some(original=>JSON.stringify(photo)===JSON.stringify(original)),'Open and placement decode the same exact source samples');
      assert.deepEqual(packageComposition(opened).data.size,photo.extent,'Open uses oriented source dimensions');
    }
    const ready=()=>wait('!layerApp.documents.busy()');
    await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.placedMaster],'placed.capy')}}]`);
    await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');await evaluate('window.showOpenFilePicker=undefined');
    for(const theme of ['light','dark']) {
      await ready();await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      const baseline=await save(),ownerOccurrence=imageLayers(baseline)[0],masked=(await state()).layers.find(l=>l.object).id;
      await ready();await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${masked},replace:false}});layerApp.dispatch({type:'layer',action:{op:'link_mask',id:${masked},value:false}});layerApp.dispatch({type:'layer',action:{op:'select',id:${masked},mask:true}})`);await settle();
      await invoke('eraser');await ready();await evaluate(`layerApp.dispatch({type:'select_brush',id:3});layerApp.dispatch({type:'set_brush_size',value:120});`);await settle();
      await stroke();
      const erased=await save(),owner=m=>packageObject(m,ownerOccurrence.id).data;
      assert.ok(rasterIdentity(erased).some(r=>r.type==='capy.coverage-source/2'&&r.tiles.length),'Erasing the image layer mask writes stored coverage');
      assert.equal(owner(erased).mask.linked,false);
      await ready();await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${masked},mask:false}})`);await settle();
      await invoke('move');await selectImage(masked,large.data.name);
      await drag(await screen(...map(start,largeExtent[0]*.4,largeExtent[1]*.4)),30,20);
      const movedImage=await save();
      assert.notDeepEqual(affine(packageObject(movedImage,large.id)),start,'The image moves inside its layer');
      assert.deepEqual(owner(movedImage).mask,owner(erased).mask,'Moving an image leaves the layer mask in place');
      assert.deepEqual(owner(movedImage).offset,owner(erased).offset,'Moving an image leaves the layer offset unchanged');
      await capture(`${theme}-masked-image-moved`);
      await invoke('undo');await invoke('undo');await invoke('undo');await invoke('undo');
      assert.deepEqual(owner(await save()).mask,owner(baseline).mask);
    }
    console.log('Image placement: unlinked layer mask stays fixed while an image moves behind it');
    await importFiles([files[0]]);await invoke('zen_mode');
    await call('Emulation.setDeviceMetricsOverride',{width:360,height:640,deviceScaleFactor:1,mobile:false});await settle();
    await wait(`!document.querySelector('.canvas-action-bar').classList.contains('suppressed')`);
    assert.equal(await evaluate(`(()=>{const r=document.querySelector('.canvas-action-bar').getBoundingClientRect();return r.x>=0&&r.right<=innerWidth&&r.y>=0&&r.bottom<=innerHeight})()`),true,'Placement controls stay reachable in a narrow viewport');
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await capture(`${theme}-narrow-placement`);
    }
    await click('.canvas-action-bar [data-command=cancel_transform]');
    await call('Emulation.clearDeviceMetricsOverride');await invoke('zen_mode');
    if(process.env.LAYER_IMAGE_MOTION==='1') {
      await invoke('new_document');await wait('document.querySelector("dialog[open] [data-document-field=width]")');
      await evaluate(`{for(const [id,value] of [['width',2000],['height',1500]]){const entry=document.querySelector('dialog[open] [data-document-field='+id+']');entry.value=value;entry.dispatchEvent(new Event('input',{bubbles:true}));}document.querySelector('dialog[open] [data-document-action=create]').click();}`);
      await idle();await wait('layerApp.app.brush_ready()');
      const started=Date.now();await importFiles(files);await click('.canvas-action-bar [data-command=apply_transform]');
      const loadingMs=Date.now()-started,baseline=await save();
      await measurePlacedPhotos({call,evaluate,settle,invoke,save,baseline,loadingMs});
    }
    if(captures){
      await call('Emulation.setDeviceMetricsOverride',{width:640,height:480,deviceScaleFactor:1,mobile:false});
      await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','tool_settings'].includes(id)}})`);
      await settle();await invoke('fit_canvas');await invoke('zoom_out');
      for(const theme of ['light','dark']){
        await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
        await invoke('scale_rotate');await wait("layerApp.state().canvas_bar?.context.kind==='transform'");
        await wait(`document.querySelector('[data-tool-choice-bar="transform-reference"]')`);
        assert.equal(await evaluate(`document.querySelectorAll('[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone]').length`),9,'Position presents every shared reference anchor');
        for(const anchor of [0,8,4]) {
          await click(`[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone="${anchor}"]`);
          assert.equal(await evaluate(`document.querySelector('[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone="${anchor}"]').getAttribute('aria-pressed')`),'true','Native anchor activation publishes selected shared state');
        }
        assert.equal((await command('transform_again')).enabled,false,'Again is unavailable during an active transform');
        await invoke('transform_warp');
        await wait(`(()=>{const n=document.querySelector('.canvas-action-bar:not(.suppressed)'),r=n?.getBoundingClientRect();return r&&r.width>0&&r.x>=0&&r.right<=innerWidth})()`);
        assert.equal(await evaluate(`document.getElementById('workspace').classList.contains('zen-hidden')`),false,'Narrow editing keeps ordinary controls visible');
        await capture(`${theme}-narrow-transform`);
        await click('.canvas-action-bar-more');await wait(`!!document.querySelector('[popover]:popover-open')`);
        await capture(`${theme}-narrow-more`);
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await settle();
        assert.equal((await state()).canvas_bar?.context.kind,'transform','Escape closes native More while preserving the active transform');
        assert.equal(await evaluate(`!!document.querySelector('.panel-context-menu:popover-open')`),false);
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
        assert.equal((await state()).canvas_bar?.context.kind,'transform','Releasing the consumed menu Escape preserves the active transform');
        await click('.canvas-action-bar [data-command=cancel_transform]');
        await wait(`layerApp.state().canvas_bar?.context.kind!=='transform'`);
        await capture(`${theme}-narrow-editing`);
      }
      await call('Emulation.clearDeviceMetricsOverride');
    }
    console.log('Image placement: Open, batches, image layers, transforms, rasterize, mask, malformed/stale/cancelled requests, canvas/row/group/locked drops, clipboard and compact controls passed');
  } finally {
    await call('Page.setInterceptFileChooserDialog',{enabled:false});
    await evaluate('if(placementTest.read)File.prototype.arrayBuffer=placementTest.read');
    await evaluate('window.showOpenFilePicker=placementTest.open;window.showSaveFilePicker=placementTest.save;delete window.placementTest');
    await rm(root,{recursive:true,force:true});
  }
}
