import assert from 'node:assert/strict';
import {packageObject,packageComposition,packageOccurrences,rasterIdentity,sourceContent} from './package-fixture.test.mjs';
const outer=p=>{const [x,y]=p?.translation??[0,0],m=p?.projective??[1,0,0,0,1,0,0,0,1];return[m[0]+x*m[6],m[1]+x*m[7],m[2]+x*m[8],m[3]+y*m[6],m[4]+y*m[7],m[5]+y*m[8],...m.slice(6)];};

import {png} from './clone-journey.test.mjs';
import {mkdtemp,mkdir,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {measurePlacedPhotos,placementSave,sourceIdentity} from './image-placement-motion.test.mjs';

// Real browser file-input/drop transport, shared placement and native archives.
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
      if(Date.now()-start>180000)throw Error(condition+': '+await evaluate('document.body.innerText.slice(-1500)'));
      await new Promise(resolve=>setTimeout(resolve,100));
    }
  };
  const invoke=async command=>{
    await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`,userGesture:true});
    assert.equal(reply.exceptionDetails,undefined);await settle();
  };
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const idle=()=>wait('!layerApp.state().document_file.busy');
  const activeOccurrence=async m=>packageOccurrences(m)[(await state()).layers.findIndex(l=>l.editing)];
  const runtimeId=async(m,portable)=>(await state()).layers[packageOccurrences(m).findIndex(o=>o.id===portable)].id;
  const placed=()=>wait('layerApp.state().commands.find(c=>c.id==="placement_original_size").enabled');
  const transforming=()=>wait("['transform','placement'].includes(layerApp.state().canvas_bar?.context.kind)&&layerApp.state().commands.find(c=>c.id==='apply_transform').enabled");
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
  const affine=p=>{assert.equal(p?.mesh??null,null);const m=outer(p);assert.deepEqual(m.slice(6),[0,0,1]);return[m[0],m[3],m[1],m[4],m[2],m[5]];};
  let files;
  try {
    await evaluate(`window.placementTestBounds=[];window.placementTestOriginalWorker=Worker;const OriginalWorker=Worker;window.Worker=class extends OriginalWorker{constructor(...args){super(...args);this.boundsIds=new Set();this.addEventListener('message',({data})=>{if(this.boundsIds.has(data.id))placementTestBounds.push(data);});}postMessage(message,...args){if(message.request?.operation==='snapshot'&&JSON.parse(message.request.metadata)[1].Bounds)this.boundsIds.add(message.id);return super.postMessage(message,...args);}};`);
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
    console.log('Image placement: document ready, checking linked paint Transform');
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await invoke('select_all');await invoke('fill_selection');
      const linkedPaint=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${linkedPaint},replace:false}});layerApp.dispatch({type:'layer',action:{op:'select',id:${linkedPaint},mask:false}})`);await settle();
      await invoke('scale_rotate');await wait("['transform','placement'].includes(layerApp.state().canvas_bar?.context.kind)");
      await invoke('transform_warp');await invoke('cancel_transform');
      await invoke('undo');await invoke('undo');await invoke('deselect');
    }
    console.log('Image placement: linked paint Transform Cancel passed');
    const base=await state(),baseCount=base.layers.length;
    assert.deepEqual(await evaluate('layerApp.app.photo_formats().map(f=>f.name)'),['OpenEXR','TIFF','PNG','WebP','BMP','JPEG','GIF','HEIF','AVIF']);
    await importFiles(files);
    assert.equal((await state()).layers.length,baseCount+files.length);
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,baseCount);
    assert.equal((await state()).document_file.modified,base.document_file.modified);
    await importFiles(files);
    const labels=(await state()).layers.slice(0,files.length).map(l=>l.label);
    await click('.canvas-action-bar [data-command=apply_transform]');
    const fitted=await save(),sources=sourceIdentity(fitted),originals=packageOccurrences(fitted).filter(o=>o.data.content.paint&&packageObject(fitted,o.data.content.paint).data.original).map(o=>packageObject(fitted,o.data.content.paint).data.original),originalContents=sourceContent(fitted);
    assert.equal(sources.length,files.length);
    for(let i=0;i<files.length;i++){
      const [w,h]=originals[i].extent,scale=Math.min(1,2000/w,1500/h),pose=affine(packageOccurrences(fitted)[i].data.placement);
      assert.ok(Math.abs(pose[0]-scale)<1e-6);assert.ok(Math.abs(pose[3]-scale)<1e-6);
      assert.ok(Math.abs(pose[4]-(2000-w*scale)/2)<.01);assert.ok(Math.abs(pose[5]-(1500-h*scale)/2)<.01);
    }
    await invoke('undo');assert.equal((await state()).layers.length,baseCount);
    await invoke('redo');assert.deepEqual((await state()).layers.slice(0,files.length).map(l=>l.label),labels);
    await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.saved],'placed.capy')}}]`);
    await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');
    await evaluate('window.showOpenFilePicker=undefined');
    assert.deepEqual(sourceIdentity(await save()),sources);
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    assert.deepEqual(sourceIdentity(await save()),sources,'GPU replacement retains placed source samples');
    if(!process.env.LAYER_PHOTO_FILES) {
    const photoPoint=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(400*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(300*c.zoom+c.translation[1])*r.height/c.viewport[1]};})()`);
    const photoShot=await call('Page.captureScreenshot',{format:'png',clip:{...photoPoint,width:1,height:1,scale:1}});
    const photoPixel=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${photoShot.data}';await image.decode();const canvas=new OffscreenCanvas(1,1),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);return Array.from(context.getImageData(0,0,1,1).data);})()`);
    assert.ok(photoPixel[0]>20&&photoPixel[1]<60&&photoPixel[2]>10,'Retained photo is visible after GPU restart: '+photoPixel);
    }
    const cancelledBounds=await call('Runtime.evaluate',{expression:`(async()=>{layerApp.dispatch({type:'invoke',command:'scale_rotate'});await new Promise(requestAnimationFrame);layerApp.dispatch({type:'invoke',command:'cancel_transform'});await new Promise(resolve=>setTimeout(resolve,300));if(layerApp.state().commands.find(c=>c.id==='placement_original_size').enabled)throw Error('Cancelled bounds opened a late placement');})()`,userGesture:true,awaitPromise:true});
    assert.equal(cancelledBounds.exceptionDetails,undefined);
    const timedTransform=async()=>{
      const reply=await call('Runtime.evaluate',{expression:`(async()=>{const started=performance.now();let callbacks=0;layerApp.dispatch({type:'invoke',command:'scale_rotate'});while(!layerApp.state().commands.find(c=>c.id==='placement_original_size').enabled){if(performance.now()-started>180000)throw Error('Transform did not finish');await new Promise(requestAnimationFrame);callbacks++;}return {ms:performance.now()-started,callbacks};})()`,userGesture:true,awaitPromise:true,returnByValue:true});
      assert.equal(reply.exceptionDetails,undefined);return reply.result.value;
    };
    console.log('Retained-photo cold Transform:',JSON.stringify(await timedTransform()));
    if(!process.env.LAYER_PHOTO_FILES) {
    const measured=await evaluate('placementTestBounds.at(-1)');
    assert.ok(measured?.result?.max.x>measured?.result?.min.x&&measured.result.max.y>measured.result.min.y,'RGBA retained photo returns nonempty worker bounds after GPU restart');
    }
    await invoke('cancel_transform');
    console.log('Retained-photo cached Transform:',JSON.stringify(await timedTransform()));
    await click('.canvas-action-bar [data-command=placement_original_size]');
    await click('.canvas-action-bar [data-command=apply_transform]');
    const native=await save();assert.equal(affine(packageOccurrences(native)[0].data.placement)[0],1);assert.deepEqual(sourceIdentity(native),sources);
    await evaluate('placementTest.retainedMaster=placementTest.saved.slice()');
    const stroke=async()=>{
      await wait('layerApp.app.brush_ready()');
      const p=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(1000*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(750*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
      for(const [type,dx] of [['mousePressed',-30],['mouseMoved',0],['mouseMoved',30],['mouseReleased',30]]){
        await call('Input.dispatchMouseEvent',{type,x:p.x+dx,y:p.y,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:'pen',force:type==='mouseReleased'?0:.65});await settle();
      }
      await wait('layerApp.app.brush_ready()');
    };
    const screen=async(x,y)=>evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(${x}*c.zoom+c.translation[0])*r.width/c.viewport[0],y:r.y+(${y}*c.zoom+c.translation[1])*r.height/c.viewport[1]}})()`);
    const pointer=async(type,p)=>{await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:'pen',force:type==='mouseReleased'?0:.65});await settle();};
    const tap=async p=>{await pointer('mousePressed',p);await pointer('mouseReleased',p);};
    const drag=async(p,dx,dy)=>{await pointer('mousePressed',p);await pointer('mouseMoved',{x:p.x+dx/2,y:p.y+dy/2});await pointer('mouseMoved',{x:p.x+dx,y:p.y+dy});await pointer('mouseReleased',{x:p.x+dx,y:p.y+dy});};
    const map=(m,x,y)=>{const w=m[6]*x+m[7]*y+m[8];return[(m[0]*x+m[1]*y+m[2])/w,(m[3]*x+m[4]*y+m[5])/w];};
    const inverse=m=>{const [a,b,c,d,e,f,g,h,i]=m,co=[e*i-f*h,c*h-b*i,b*f-c*e,f*g-d*i,a*i-c*g,c*d-a*f,d*h-e*g,b*g-a*h,a*e-b*d],det=a*co[0]+b*co[3]+c*co[6];return co.map(v=>v/det);};
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await invoke('brush');
      await evaluate(`layerApp.dispatch({type:'select_brush',id:21});layerApp.dispatch({type:'set_brush_size',value:80});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[.15,.25,.9,1]}}});`);await settle();
      await stroke();
      const rawBefore=await save(),retainedOccurrence=await activeOccurrence(rawBefore),retainedPortable=retainedOccurrence.id,paintId=retainedOccurrence.data.content.paint.ref;let retainedId=await runtimeId(rawBefore,retainedPortable);
      const ownerRaster=m=>rasterIdentity(m).filter(r=>r.id===paintId);
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${retainedId},replace:false}});layerApp.dispatch({type:'layer',action:{op:'select',id:${retainedId},mask:false}})`);await settle();
      await invoke('scale_rotate');await transforming();
      await evaluate(`layerApp.dispatch({type:'set_tool_setting',id:'transform_width',value:.5})`);await settle();await invoke('apply_transform');
      const repeatBefore=await save();
      await invoke('transform_again');const repeated=await save();
      const repeatPlacement=m=>packageObject(m,retainedPortable).data.placement;
      assert.notDeepEqual(repeatPlacement(repeated),repeatPlacement(repeatBefore),'Again applies the accepted retained outer delta');
      assert.deepEqual(sourceIdentity(repeated),sourceIdentity(repeatBefore),'Again preserves immutable retained sources');
      assert.deepEqual(ownerRaster(repeated),ownerRaster(repeatBefore),'Again preserves native material planes');
      await invoke('undo');assert.deepEqual(repeatPlacement(await save()),repeatPlacement(repeatBefore),'Again makes one undo step');
      await invoke('redo');assert.deepEqual(repeatPlacement(await save()),repeatPlacement(repeated),'Redo reapplies Again once');
      await invoke('undo');
      const pivotBase=await save(),pivotOuter=outer(repeatPlacement(pivotBase));
      const centerLocal=packageObject(rawBefore,retainedOccurrence.data.content.paint).data.original.extent.map(v=>v/2),centerDocument=map(pivotOuter,...centerLocal);
      await invoke('scale_rotate');await transforming();
      await drag(await screen(...centerDocument),21,13);
      const customPivot=[centerDocument[0]+21/(await state()).camera.zoom,centerDocument[1]+13/(await state()).camera.zoom];
      const customLocal=map(inverse(pivotOuter),...customPivot);
      await invoke('transform_rotate_right');await invoke('apply_transform');
      const pivotApplied=await save(),fixed=map(outer(repeatPlacement(pivotApplied)),...customLocal);
      assert.ok(Math.hypot(fixed[0]-customPivot[0],fixed[1]-customPivot[1])<.05,'Native custom pivot remains fixed under quarter-turn');
      await invoke('undo');assert.deepEqual(repeatPlacement(await save()),repeatPlacement(pivotBase),'Custom-pivot transform is one undo step');
      await invoke('scale_rotate');await transforming();await invoke('transform_snapping');
      assert.equal((await state()).commands.find(c=>c.id==='transform_snapping').selected,true,'Snapping toggle publishes shared selected state');
      const snapCenter=map(pivotOuter,...centerLocal),snapZoom=(await state()).camera.zoom;
      const canvasCenter=2000;
      assert.ok(Math.abs(snapCenter[0]-canvasCenter)>100,'The snap fixture starts away from its target');
      await drag(await screen(snapCenter[0]+90,snapCenter[1]+80),(canvasCenter-snapCenter[0])*snapZoom-3,0);
      await invoke('apply_transform');
      const snapped=map(outer(repeatPlacement(await save())),...centerLocal);
      assert.ok(Math.abs(snapped[0]-canvasCenter)<.05,'A native body drag within three logical pixels snaps the layer center exactly to the canvas right edge');
      await invoke('undo');assert.deepEqual(repeatPlacement(await save()),repeatPlacement(pivotBase),'Snapped drag creates one undo step');
      await invoke('scale_rotate');await transforming();await invoke('transform_snapping');
      await invoke('transform_distort');
      const anchor=(await state()).canvas_bar.anchor;
      await drag(await screen(anchor[0],anchor[1]),-25,12);await invoke('apply_transform');
      const distorted=await save(),outer=outer(repeatPlacement(distorted));
      assert.ok(outer[6]!==0||outer[7]!==0,'A real corner drag retains projective geometry');
      const [sourceWidth,sourceHeight]=packageObject(rawBefore,retainedOccurrence.data.content.paint).data.original.extent;
      const node=async(u,v)=>screen(...map(outer,u*sourceWidth,v*sourceHeight));
      await invoke('scale_rotate');await transforming();await invoke('transform_warp');await invoke('warp_split_cross');
      await tap(await node(.37,.61));await invoke('warp_select_points');
      await tap(await node(.37,.61));await tap(await node(2/3,.61));await invoke('warp_select_points');
      await drag(await node(.37,.61),18,12);await capture(`${theme}-retained-warp`);await invoke('apply_transform');
      const warped=await save(),mesh=repeatPlacement(warped).mesh;
      assert.ok(mesh&&mesh.breakpoints.every(points=>points.length===5),'Cross Split persists both exact mesh axes');
      for(const [u,v] of [[.37,.61],[2/3,.61]]) {
        const i=mesh.breakpoints[0].findIndex(p=>Math.abs(p-u)<1e-4),j=mesh.breakpoints[1].findIndex(p=>Math.abs(p-v)<1e-4);
        assert.ok(i>=0&&j>=0,'The selected split nodes remain in the nonuniform grid');
        const point=mesh.net[j*3*(3*(mesh.breakpoints[0].length-1)+1)+i*3];
        const [a,b,c,d,e,f]=mesh.frame,x=a*u+c*v+e,y=b*u+d*v+f;
        assert.ok(Math.hypot(point[0]-x,point[1]-y)>1,'Each selected Warp node actually moves');
      }
      assert.deepEqual(ownerRaster(warped),ownerRaster(rawBefore),'Retained Distort/Warp leaves all original raw planes unchanged');
      assert.deepEqual(sourceIdentity(warped),sources,'Retained Distort/Warp keeps the immutable photograph');
      await evaluate(`placementTest.warpedMaster=placementTest.saved.slice();window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.warpedMaster],'warped.capy')}}]`);
      await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');await evaluate('window.showOpenFilePicker=undefined');
      const reopened=await save();assert.deepEqual(repeatPlacement(reopened),repeatPlacement(warped));
      assert.deepEqual(ownerRaster(reopened),ownerRaster(rawBefore));retainedId=await runtimeId(reopened,retainedPortable);
      await wait(`(()=>{const l=layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(String(retainedId))}),c=document.querySelector('.layer-row[data-layer="${retainedId}"] .layer-thumbnail canvas');return l&&c?.dataset.previewRevision?.endsWith(':'+String(l.paint_revision))})()`);
      const retainedColors=await evaluate(`(()=>{const c=document.querySelector('.layer-row[data-layer="${retainedId}"] .layer-thumbnail canvas'),p=c.getContext('2d').getImageData(0,0,c.width,c.height).data;let n=0;for(let i=0;i<p.length;i+=4)if(p[i+3]&&Math.max(p[i],p[i+1],p[i+2])-Math.min(p[i],p[i+1],p[i+2])>20)n++;return n})()`);
      assert.ok(retainedColors>0,'Completed retained outer-mesh photo thumbnail contains original photo colors');
      const seeded=await save(),seededOccurrence=await activeOccurrence(seeded),seededId=await runtimeId(seeded,seededOccurrence.id);
      const rawMaterial=m=>packageObject(m,seededOccurrence.data.content.paint).data;
      const materialPlanes=m=>[...new Set(rawMaterial(m).tiles.map(t=>t.plane))].sort();
      assert.deepEqual(materialPlanes(seeded),['color','watercolor_wetness'],'Wet watercolor owns its dedicated scalar plane without generic wetness');
      assert.ok(rawMaterial(seeded).material?.watercolor,'The real wet stroke publishes its material style');
      const cancelledBake=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:'apply_transform_pixels'});if(!layerApp.state().commands.find(c=>c.id==='cancel_transform').enabled)throw Error('Pending bake must offer Cancel');layerApp.dispatch({type:'invoke',command:'cancel_transform'});`,userGesture:true});
      assert.equal(cancelledBake.exceptionDetails,undefined);await settle();
      assert.deepEqual(sourceIdentity(await save()),sources,'Cancelled bake retains original source samples');
      if(captures){
        await wait(`layerApp.state().commands.find(c=>c.id==='apply_transform_pixels').enabled`);
        const pending=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:'apply_transform_pixels'})`,userGesture:true});
        assert.equal(pending.exceptionDetails,undefined);
        await capture(`${theme}-applying`);await settle();
      }else await invoke('apply_transform_pixels');
      await wait(`!layerApp.state().commands.find(c=>c.id==='cancel_transform').enabled&&layerApp.app.brush_ready()`);
      const baked=await save(),bakedOccurrence=await activeOccurrence(baked);
      assert.ok(!packageObject(baked,bakedOccurrence.data.content.paint).data.original,'Bake removes the retained source association');
      assert.deepEqual(affine(bakedOccurrence.data.placement),[1,0,0,1,0,0]);
      assert.ok(rasterIdentity(baked).some(r=>r.tiles.length),'Bake publishes editable native tiles');
      assert.deepEqual(materialPlanes(baked),materialPlanes(seeded),'Bake preserves the real watercolor pigment and scalar planes');
      assert.deepEqual(rawMaterial(baked).material?.watercolor,rawMaterial(seeded).material?.watercolor,'Bake preserves the wet stroke material style');
      await invoke('pen');await evaluate(`layerApp.dispatch({type:'select_brush',id:1});layerApp.dispatch({type:'set_brush_size',value:24});layerApp.dispatch({type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[1,0,.7,1]}}});`);await settle();
      await stroke();const painted=await save();assert.notDeepEqual(rasterIdentity(painted),rasterIdentity(baked),'Painting edits the baked layer');
      if(captures){
        await wait(`(()=>{const l=layerApp.state().layers.find(l=>String(l.id)===${JSON.stringify(String(seededId))}),c=document.querySelector('.layer-row[data-layer="${seededId}"] .layer-thumbnail canvas');return l&&c?.dataset.previewRevision?.endsWith(':'+String(l.paint_revision))})()`);
        const colored=await evaluate(`(()=>{const c=document.querySelector('.layer-row[data-layer="${seededId}"] .layer-thumbnail canvas'),p=c.getContext('2d').getImageData(0,0,c.width,c.height).data;let n=0;for(let i=0;i<p.length;i+=4)if(p[i+3]&&Math.max(p[i],p[i+1],p[i+2])-Math.min(p[i],p[i+1],p[i+2])>20)n++;return n})()`);
        assert.ok(colored>0,'Completed baked-photo thumbnail contains the photo colors');
      }
      await capture(`${theme}-editing`);
      await invoke('liquify');await evaluate(`layerApp.dispatch({type:'select_brush',id:13});layerApp.dispatch({type:'set_brush_size',value:80});`);await settle();
      await stroke();const liquified=await save();assert.notDeepEqual(rasterIdentity(liquified),rasterIdentity(painted),'Liquify edits baked paint');
      await invoke('undo');assert.deepEqual(rasterIdentity(await save()),rasterIdentity(painted));
      await invoke('undo');assert.deepEqual(rasterIdentity(await save()),rasterIdentity(baked));
      await invoke('undo');assert.deepEqual(sourceIdentity(await save()),sources,'One Undo restores the retained photo after bake');
      assert.deepEqual(rasterIdentity(await save()),rasterIdentity(seeded),'Bake Undo keeps the pre-bake wet stroke');
      await invoke('redo');assert.deepEqual(rasterIdentity(await save()),rasterIdentity(baked));
      await evaluate(`placementTest.bakedMaster=placementTest.saved.slice();window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.bakedMaster],'baked.capy')}}]`);
      await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');
      assert.deepEqual(rasterIdentity(await save()),rasterIdentity(baked),'Baked native tiles survive reopen');
      assert.deepEqual(rawMaterial(await save()).material?.watercolor,rawMaterial(seeded).material?.watercolor,'The baked watercolor style survives reopen');
      await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.retainedMaster],'retained.capy')}}]`);
      await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');
      await evaluate('window.showOpenFilePicker=undefined');
      assert.deepEqual(sourceIdentity(await save()),sources);
      console.log(`Photo bake ${theme}: pending Cancel, native pixels, paint, Liquify, one-step Undo and reopen passed`);
    }
    for(const theme of ['light','dark']) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      const baseline=await save(),ownerOccurrence=await activeOccurrence(baseline),ownerPortable=ownerOccurrence.id,ownerPaint=ownerOccurrence.data.content.paint.ref,ownerId=await runtimeId(baseline,ownerPortable);
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'add_mask',id:${ownerId},replace:false}});layerApp.dispatch({type:'layer',action:{op:'link_mask',id:${ownerId},value:false}});layerApp.dispatch({type:'layer',action:{op:'select',id:${ownerId},mask:true}})`);await settle();
      await invoke('eraser');await evaluate(`layerApp.dispatch({type:'select_brush',id:3});layerApp.dispatch({type:'set_brush_size',value:120});`);await settle();
      await stroke();
      assert.ok(rasterIdentity(await save()).some(r=>r.type==='capy.coverage-source/1'&&r.tiles.some(t=>t.plane==='mask')),'Mask eraser stroke creates real scalar native pixels');
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${ownerId},mask:false}});layerApp.dispatch({type:'layer',action:{op:'new',group:true,clipped:false}})`);await settle();
      const groupId=Number((await state()).layer_tools.editing_layer.id);
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'reparent',id:${ownerId},parent:${groupId},index:0}});layerApp.dispatch({type:'layer',action:{op:'select',id:${groupId},mask:false}})`);await settle();
      await invoke('fit_canvas');await invoke('zoom_out');await invoke('zoom_out');
      await invoke('scale_rotate');await transforming();await invoke('transform_distort');
      const anchor=(await state()).canvas_bar.anchor;
      const corners=await Promise.all([[anchor[0],anchor[1]],[anchor[2],anchor[1]],
        [anchor[0],anchor[3]],[anchor[2],anchor[3]]].map(([x,y])=>screen(x,y)));
      let corner;
      for(const p of corners)if(await evaluate(`document.elementFromPoint(${p.x},${p.y})===layerApp.canvas`)){corner=p;break;}
      assert.ok(corner,`A group Distort corner is reachable through the visible native canvas: ${JSON.stringify({anchor,corners})}`);
      await drag(corner,20,12);await invoke('apply_transform');
      await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${ownerId},mask:true}})`);await settle();
      const beforeMaskBake=await save(),owner=m=>packageObject(m,ownerPortable).data;
      const withoutMask=m=>{const {mask,...layer}=owner(m);return layer;};
      assert.ok(outer(owner(beforeMaskBake).mask.placement)[6]!==0||outer(owner(beforeMaskBake).mask.placement)[7]!==0,'Independent mask retains projective geometry');
      await invoke('apply_transform_pixels');await wait(`!layerApp.state().commands.find(c=>c.id==='cancel_transform').enabled&&layerApp.app.brush_ready()`);
      const bakedMask=await save();
      assert.deepEqual(withoutMask(bakedMask),withoutMask(beforeMaskBake),'Scalar-only worker bake preserves the complete owner');
      assert.deepEqual(rasterIdentity(bakedMask).filter(r=>r.id===ownerPaint),rasterIdentity(beforeMaskBake).filter(r=>r.id===ownerPaint),'Scalar-only worker bake preserves owner raw Color/material');
      assert.deepEqual(sourceIdentity(bakedMask),sources,'Scalar-only worker bake preserves immutable owner photo');
      assert.deepEqual(outer(owner(bakedMask).mask.placement),[1,0,0,0,1,0,0,0,1]);
      assert.equal(owner(bakedMask).mask.linked,false);
      await capture(`${theme}-independent-mask-baked`);
      await invoke('undo');assert.deepEqual(owner(await save()).mask,owner(beforeMaskBake).mask);
      await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([placementTest.retainedMaster],'retained.capy')}}]`);
      await invoke('open_document');await idle();await wait('layerApp.app.brush_ready()');await evaluate('window.showOpenFilePicker=undefined');
    }
    // A malformed second file must discard all prepared sources.
    const before=await state();await invoke('import_image');await choose([files[0],bad]);await idle();
    assert.equal((await state()).layers.length,before.layers.length);assert.ok((await state()).host_error);
    // Selection changes while the file input is open must never retarget it.
    await invoke('import_image');await wait('document.querySelector("input[type=file]")');
    const last=before.layers.at(-1).id;
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'select',id:${last},mask:false}})`);
    await choose([files[0]]);await idle();assert.match((await state()).host_error,/changed/);
    assert.equal((await state()).layers.length,before.layers.length);
    // Browser external canvas file drops use the coordinates captured at drop.
    const p=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect(),c=layerApp.app.camera(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.6)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.6)*r.height/c.viewport[1]}})()`);
    await drop(p,[files[0]]);
    await idle();await placed();assert.equal((await state()).layers.length,before.layers.length+1);
    await click('.canvas-action-bar [data-command=cancel_transform]');
    assert.equal((await state()).layers.length,before.layers.length);
    // Placement controls remain reachable in a narrow viewport with panels hidden.
    await importFiles([files[0]]);await invoke('zen_mode');
    await call('Emulation.setDeviceMetricsOverride',{width:360,height:640,deviceScaleFactor:1,mobile:false});await settle();
    await wait(`!document.querySelector('.canvas-action-bar').classList.contains('suppressed')`);
    assert.equal(await evaluate(`(()=>{const r=document.querySelector('.canvas-action-bar').getBoundingClientRect();return r.x>=0&&r.right<=innerWidth&&r.y>=0&&r.bottom<=innerHeight})()`),true);
    for(const theme of ['light','dark']){
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);await settle();
      await capture(`${theme}-narrow-transform`);
    }
    await click('.canvas-action-bar [data-command=cancel_transform]');
    await call('Emulation.clearDeviceMetricsOverride');await invoke('zen_mode');
    await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
    const copied=await call('Runtime.evaluate',{expression:`navigator.clipboard.write([new ClipboardItem({['web '+placementTest.inputFiles[0].type]:placementTest.inputFiles[0]})])`,userGesture:true,awaitPromise:true});
    assert.equal(copied.exceptionDetails,undefined);
    await invoke('paste_image');await idle();await placed();
    assert.equal((await state()).layers.length,before.layers.length+1);
    await click('.canvas-action-bar [data-command=apply_transform]');
    const pasted=sourceContent(await save());
    assert.ok(pasted.some(image=>originalContents.some(original=>JSON.stringify(image)===JSON.stringify(original))),'Clipboard retains original source samples');
    await invoke('undo');assert.equal((await state()).layers.length,before.layers.length);
    // Cancel while an asynchronous file read is pending; release it afterwards
    // to prove that a late decoder completion cannot publish a partial batch.
    await evaluate(`placementTest.read=File.prototype.arrayBuffer;File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await click('.file-progress button');await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,before.layers.length);
    // An otherwise valid read that crosses GPU replacement must also retire.
    await evaluate(`File.prototype.arrayBuffer=function(){const file=this;return new Promise(resolve=>{placementTest.release=()=>placementTest.read.call(file).then(resolve)})}`);
    await invoke('import_image');await choose([files[0]]);await wait('placementTest.release');
    await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()');
    await evaluate('File.prototype.arrayBuffer=placementTest.read;placementTest.release();delete placementTest.release');await idle();
    assert.equal((await state()).layers.length,before.layers.length,'A read from the retired GPU cannot publish layers');
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'new',group:true,clipped:false}})`);await settle();
    const group=(await state()).layers.find(l=>l.group),groupCount=(await state()).layers.length;
    const rowPoint=await evaluate(`(()=>{const r=document.querySelector('.layer-row[data-layer="${group.id}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await drop(rowPoint,[files[0]]);
    await idle();await placed();await click('.canvas-action-bar [data-command=apply_transform]');
    const grouped=await save(),groupOccurrence=packageOccurrences(grouped)[(await state()).layers.findIndex(l=>l.id===group.id)];assert.ok(packageObject(grouped,groupOccurrence.data.content.stack).data.entries.some(ref=>packageObject(grouped,ref).data.name!==group.label));
    await invoke('undo');assert.equal((await state()).layers.length,groupCount);
    await evaluate(`layerApp.dispatch({type:'layer',action:{op:'lock',id:${group.id},value:true}})`);await settle();
    await drop(rowPoint,[files[0]]);
    await settle();assert.equal((await state()).layers.length,groupCount);assert.equal((await state()).document_file.busy,false);
    for(let i=0;i<files.length;i++) {
      await invoke('open_document');await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label)?.click()`);
      await choose([files[i]]);await idle();await wait('layerApp.app.brush_ready()');
      const opened=await save();
      assert.deepEqual(packageComposition(opened).data.frame.size,originals[i].extent,'Open uses oriented source dimensions');
      assert.ok(originalContents.some(original=>JSON.stringify(sourceContent(opened)[0])===JSON.stringify(original)),'Open and placement decode the same exact source samples');
    }
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
        await invoke('scale_rotate');await transforming();
        await wait(`document.querySelector('[data-tool-choice-bar="transform-reference"]')`);
        assert.equal(await evaluate(`document.querySelectorAll('[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone]').length`),9,'Position presents every shared reference anchor');
        for(const anchor of [0,8,4]) {
          await click(`[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone="${anchor}"]`);
          assert.equal(await evaluate(`document.querySelector('[data-tool-choice-bar="transform-reference"] [data-tool-choice-tone="${anchor}"]').getAttribute('aria-pressed')`),'true','Native anchor activation publishes selected shared state');
        }
        assert.equal((await state()).commands.find(c=>c.id==='transform_again').enabled,false,'Again is unavailable during an active transform');
        await invoke('transform_warp');
        await wait(`(()=>{const n=document.querySelector('.canvas-action-bar:not(.suppressed)'),r=n?.getBoundingClientRect();return r&&r.width>0&&r.x>=0&&r.right<=innerWidth})()`);
        assert.equal(await evaluate(`document.getElementById('workspace').classList.contains('zen-hidden')`),false,'Narrow editing keeps ordinary controls visible');
        await capture(`${theme}-narrow-transform`);
        await click('.canvas-action-bar-more');await wait(`!!document.querySelector('[popover]:popover-open')`);
        await capture(`${theme}-narrow-more`);
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await settle();
        assert.ok(['transform','placement'].includes((await state()).canvas_bar?.context.kind),'Escape closes native More while preserving the active transform');
        assert.equal(await evaluate(`!!document.querySelector('.panel-context-menu:popover-open')`),false);
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
        assert.ok(['transform','placement'].includes((await state()).canvas_bar?.context.kind),'Releasing the consumed menu Escape preserves the active transform');
        await click('.canvas-action-bar [data-command=cancel_transform]');
        await wait(`!['transform','placement'].includes(layerApp.state().canvas_bar?.context.kind)`);
        await capture(`${theme}-narrow-editing`);
      }
      await call('Emulation.clearDeviceMetricsOverride');
    }
    console.log('Image placement: Open, batches, clipboard, fit, Apply/Cancel, one-step history, exact sources after reopen, Original Size, malformed/stale/cancelled requests, canvas/group/locked drops and compact controls passed');
  } finally {
    await call('Page.setInterceptFileChooserDialog',{enabled:false});
    await evaluate('if(placementTest.read)File.prototype.arrayBuffer=placementTest.read');
    await evaluate('window.Worker=placementTestOriginalWorker;delete window.placementTestBounds;delete window.placementTestOriginalWorker;window.showOpenFilePicker=placementTest.open;window.showSaveFilePicker=placementTest.save;delete window.placementTest');
    await rm(root,{recursive:true,force:true});
  }
}
