import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readPackage,packageObject,packageObjects,packageOccurrences} from './package-fixture.test.mjs';
import {png} from './clone-journey.test.mjs';

const openMenu='.panel-context-menu:popover-open';
const map=([a,b,c,d,e,f],x,y)=>[a*x+c*y+e,b*x+d*y+f];
const RED=[0xdd,0x2a,0x1f],BLUE=[0x1f,0x3a,0xde],GREEN=[40,200,60],WHITE=[255,255,255];

export async function checkImageLayers({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/image-layers/web';
  await mkdir(directory,{recursive:true});
  const theme=await evaluate('layerApp.state().settings.theme ?? "default"');
  const wait=(expression,timeout=60000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+' '+JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice?.text,status:document.querySelector('#status')?.textContent})));else setTimeout(check,30);}check();})`,);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${action})`);await settle();};
  const invoke=async command=>{
    await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    const reply=await call('Runtime.evaluate',{expression:`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`,userGesture:true});
    assert.equal(reply.exceptionDetails,undefined);await settle();
  };
  const command=id=>evaluate(`(c=>c&&{enabled:c.enabled,label:c.label,reason:c.disabled_reason??null})(layerApp.state().commands.find(c=>c.id===${JSON.stringify(id)}))`);
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?String(v):v))');
  const idle=()=>wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&!layerApp.state().requests.some(r=>r.kind.type==="document")');
  const layerBy=async id=>(await state()).layers.find(l=>l.id===String(id));
  const children=async id=>(await layerBy(id))?.objects.map(o=>o.id)??[];
  const selected=async id=>(await layerBy(id))?.objects.filter(o=>o.selected).map(o=>o.id)??[];
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)return null;const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);assert.ok(r&&r.width&&r.height,`${selector} is shown`);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const mouse=(type,p,modifiers=0,button='left')=>call('Input.dispatchMouseEvent',{type,...p,modifiers,button,pointerType:'mouse',buttons:type==='mouseReleased'?0:button==='right'?2:1,clickCount:1});
  const click=async(p,modifiers=0,button='left')=>{await mouse('mousePressed',p,modifiers,button);await mouse('mouseReleased',p,modifiers,button);await settle();};
  const drag=async(points,pointerType='mouse')=>{
    for(const [i,p] of points.entries()){await call('Input.dispatchMouseEvent',{type:i===0?'mousePressed':'mouseMoved',...p,button:'left',buttons:1,clickCount:1,pointerType,force:.6});await settle();}
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...points.at(-1),button:'left',buttons:0,clickCount:1,pointerType,force:0});await settle();
  };
  const line=(a,b,steps=6)=>Array.from({length:steps+1},(_,i)=>({x:a.x+(b.x-a.x)*i/steps,y:a.y+(b.y-a.y)*i/steps}));
  const key=async(key,code,windowsVirtualKeyCode,modifiers=0)=>{
    const text=key==='Enter'&&!modifiers?'\r':undefined;
    for(const type of [text?'keyDown':'rawKeyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode,modifiers,...(type==='keyDown'&&text?{text}:{})});
    await settle();
  };
  const chord=letter=>key(letter,`Key${letter.toUpperCase()}`,letter.toUpperCase().charCodeAt(0),2);
  const capture=async name=>writeFile(`${directory}/${name}-${theme}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:c.translation,v:c.viewport,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const pixelAt=async(x,y)=>{
    const p=await screen(x,y);
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:Math.round(p.x)-2,y:Math.round(p.y)-2,width:5,height:5,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);return Array.from(x.getImageData(2,2,1,1).data)})()`);
  };
  const close=(a,b,tolerance=24)=>a.slice(0,3).every((v,i)=>Math.abs(v-b[i])<=tolerance);
  const shows=async(points,expected,label)=>{
    let last;
    for(const end=Date.now()+60000;Date.now()<end;await pause(150)){
      last=[];for(const [x,y] of points)last.push(await pixelAt(x,y));
      if(last.every((p,i)=>close(p,expected[i])))return last;
    }
    assert.fail(`${label}: ${JSON.stringify(last)} != ${JSON.stringify(expected)}`);
  };
  const save=async()=>{
    await invoke('save_document_as');await idle();
    await evaluate('(async()=>{imageLayers.saved=new Uint8Array(await(await imageLayers.handle.getFile()).arrayBuffer())})()');
    return readPackage(evaluate,'imageLayers.saved');
  };
  const pasteImage=async(width,height,color,offset=[0,0])=>{
    await click(await evaluate('({x:innerWidth/2,y:3})'));await wait('document.hasFocus()');
    const bytes=png(width,height,()=>color,3).toString('base64');
    await evaluate(`(async()=>{const bytes=Uint8Array.from(atob('${bytes}'),c=>c.charCodeAt(0));await navigator.clipboard.write([new ClipboardItem({'image/png':new Blob([bytes],{type:'image/png'})})]);})()`);
    await chord('v');
    await wait(`layerApp.state().canvas_bar?.context.kind==='placement'`);
    if(offset[0]||offset[1]){const [w,h]=await size(),a=await screen(w/2-width/4,h/2-height/4),b=await screen(w/2-width/4+offset[0],h/2-height/4+offset[1]);await drag(line(a,b));}
    await key('Enter','Enter',13);await idle();
    await wait(`layerApp.state().canvas_bar?.context.kind!=='placement'`);
  };
  const size=()=>evaluate('[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height]');
  const layerMenu=async(id,label,part='.layer-name')=>{
    await click(await middle(`.layer-row[data-layer="${id}"] ${part}`),0,'right');
    const item=`[...document.querySelectorAll('${openMenu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
    await wait(`!!${item}&&!${item}.disabled`);
    await click(await evaluate(`(()=>{const r=${item}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`));
  };
  const editMenu=async label=>{
    await click(await middle('.header-menu[data-menu="edit"] > summary'));
    const row=`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
    await wait(`document.querySelector('.header-menu[data-menu="edit"]').open&&!!${row}&&!${row}.disabled`);
    await click(await evaluate(`(()=>{const r=${row}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`));
  };
  const select=async([x0,y0,x1,y1])=>{
    await invoke('rectangle_select');
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await drag(line(await screen(x0,y0),await screen(x1,y1),4));
    await wait('layerApp.state().layer_tools.has_selection');
  };
  const nonce=()=>evaluate('layerApp.app.clip_nonce()??null');
  const copied=async previous=>{await wait(`(layerApp.app.clip_nonce()??null)!==${JSON.stringify(previous)}`);await idle();};
  const affine=(m,id)=>packageObject(m,id).data.affine??[1,0,0,1,0,0];
  const imageLayerIds=m=>packageOccurrences(m).filter(o=>o.data.content.objects).map(o=>o.id);
  try {
    await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
    await evaluate(`window.imageLayers={save:window.showSaveFilePicker};window.showSaveFilePicker=async o=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-image-layers',{create:true});return imageLayers.handle=await directory.getFileHandle(crypto.randomUUID()+'.capy',{create:true});};`);
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    await invoke('new_document');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label)?.click()`);
    await wait('document.querySelector("dialog[open] [data-document-field=width]")');
    await evaluate(`{for(const [id,value] of [['width',640],['height',480]]){const entry=document.querySelector('dialog[open] [data-document-field='+id+']');entry.value=value;entry.dispatchEvent(new Event('input',{bubbles:true}));}document.querySelector('dialog[open] [data-document-action=create]').click();}`);
    await idle();await invoke('fit_canvas');
    const ink=(await state()).layers.find(l=>l.editing).id;
    await invoke('pen');await send(`{type:'select_brush',id:1}`);await send(`{type:'set_brush_size',value:30}`);
    await send(`{type:'color',action:{op:'set_slot',slot:'foreground',color:{space:'Srgb',rgba:[${GREEN.map(v=>v/255)},1]}}}`);
    await drag(line(await screen(40,430),await screen(180,430)),'pen');await idle();
    await pasteImage(240,180,RED);
    await pasteImage(240,180,BLUE,[120,90]);
    const layer=(await state()).layers.find(l=>l.object_count===2).id;
    await send(`{type:'object',action:{op:'expand',layer:${layer}n,expanded:true}}`);
    const [blue,red]=await children(layer);
    const points=[[230,180],[530,400],[110,430],[600,40]],original=[RED,BLUE,GREEN,WHITE];
    await shows(points,original,'The fixture shows red, blue, green and paper');
    const fixture=await save(),imageCount=packageObjects(fixture,'capy.image-object/1').length;
    assert.equal(imageCount,2);

    await layerMenu(layer,'Rasterize Layer');
    await wait(`!layerApp.state().layers.find(l=>l.id==${layer}n).object_count`);await idle();
    await shows(points,original,'Rasterized images keep their appearance');
    const rasterized=await save();
    assert.equal(imageLayerIds(rasterized).length,0);assert.equal(packageObjects(rasterized,'capy.image-object/1').length,0,'The image records are consumed');
    await capture('image-rasterized');
    await invoke('undo');await idle();
    assert.deepEqual(await children(layer),[blue,red],'Undo restores the same images');

    await send(`{type:'layer',action:{op:'select',id:${ink},mask:false}}`);
    await layerMenu(ink,'Convert to Image Layer');
    await wait(`layerApp.state().layers.find(l=>l.id==${ink}n).object_count===1`);await idle();
    await shows(points,original,'Converted paint keeps its appearance');
    await capture('paint-converted');
    await invoke('undo');await idle();
    assert.equal((await layerBy(ink)).object_count,0,'Undo restores the paint layer');
    await shows(points,original,'Undo restores the paint');

    await send(`{type:'layer',action:{op:'select',id:${layer},mask:false}}`);
    const count=(await state()).layers.length;
    await click(await middle(`.layer-row[data-layer="${layer}"] .layer-name`));
    await chord('e');
    await wait(`layerApp.state().layers.length===${count-1}`);await idle();
    await shows(points,original,'Merging the image layer down keeps the composite');
    await invoke('undo');await idle();
    assert.equal((await state()).layers.length,count);assert.deepEqual(await children(layer),[blue,red]);

    await call('Input.dispatchMouseEvent',{type:'mousePressed',...await middle(`.layer-row[data-layer="${layer}"] .layer-thumbnail`),modifiers:2,button:'left',buttons:1,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...await middle(`.layer-row[data-layer="${layer}"] .layer-thumbnail`),modifiers:2,button:'left',buttons:0,clickCount:1});await settle();
    await wait('layerApp.state().layer_tools.has_selection');
    assert.equal((await layerBy(layer)).object_count,2,'Selecting opacity keeps the images');
    const bounds=await evaluate('layerApp.state().canvas_bar?.anchor??null');
    console.log('Alpha selection bounds',JSON.stringify(bounds));
    if(bounds)assert.ok(Math.abs(bounds[0]-200)<1.5&&Math.abs(bounds[1]-150)<1.5&&Math.abs(bounds[2]-560)<1.5&&Math.abs(bounds[3]-420)<1.5,'Alpha selection covers both images: '+bounds);
    assert.equal((await command('deselect')).enabled,false,'Deselect on image content clears images, and none are selected');
    await invoke('rectangle_select');await invoke('deselect');
    console.log('Image layers: Rasterize Layer, Convert to Image Layer, Merge Down and Select Layer Opacity');

    await send(`{type:'layer',action:{op:'select',id:${layer},mask:false}}`);
    await invoke('pen');
    const before=await save();
    await drag(line(await screen(260,200),await screen(320,220)),'pen');
    await wait('layerApp.state().notice?.actions?.length===3');
    assert.deepEqual((await state()).notice.actions.map(a=>a.id),['add_mask','new_paint_layer','rasterize_layer']);
    assert.equal(await evaluate(`document.querySelectorAll('.canvas-notice-actions .canvas-notice-action').length`),3,'Web shows every notice action');
    await capture('image-paint-refusal');
    assert.deepEqual(packageObjects(await save(),'capy.paint-source/2').map(p=>p.data.tiles),packageObjects(before,'capy.paint-source/2').map(p=>p.data.tiles),'The refused stroke paints nothing');
    await drag(line(await screen(260,200),await screen(320,220)),'pen');
    await wait('layerApp.state().notice?.actions?.length===3');
    await click(await evaluate(`(()=>{const r=document.querySelectorAll('.canvas-notice-actions .canvas-notice-action')[2].getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`));
    await wait(`!layerApp.state().layers.find(l=>l.id==${layer}n).object_count`);await idle();
    await drag(line(await screen(260,200),await screen(320,220)),'pen');await idle();
    const strokePixel=await pixelAt(290,210);
    assert.ok(close(strokePixel,GREEN),'The next stroke paints the rasterized layer: '+strokePixel);
    await invoke('undo');await invoke('undo');await idle();
    assert.deepEqual(await children(layer),[blue,red],'Undo restores the image layer after the notice rasterizes it');
    await drag(line(await screen(260,200),await screen(320,220)),'pen');
    await wait('layerApp.state().notice?.actions?.length===3');
    const again=(await state()).notice;
    assert.ok(again.actions.every(a=>a.enabled),'After Undo the refusal offers enabled actions again: '+JSON.stringify(again.actions));
    await click(await evaluate(`(()=>{const r=document.querySelectorAll('.canvas-notice-actions .canvas-notice-action')[2].getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`));
    await wait(`!layerApp.state().layers.find(l=>l.id==${layer}n).object_count`);await idle();
    await invoke('undo');await idle();
    assert.deepEqual(await children(layer),[blue,red],'Rasterize Layer works again after Undo');
    console.log('Image layers: paint refusal notice and its Rasterize Layer action');


    await send(`{type:'layer',action:{op:'select',id:${layer},mask:false}}`);
    await invoke('move');
    await click(await screen(230,180));
    await wait(`(o=>o.join()==='${red}')(layerApp.state().layers.find(l=>l.id==${layer}n).objects.filter(o=>o.selected).map(o=>String(o.id)))`);
    const redAffine=affine(await save(),packageObjects(fixture,'capy.image-object/1').find(o=>o.data.affine[4]<300).id);
    let previous=await nonce();await chord('c');await copied(previous);
    assert.equal(await evaluate(`(async()=>{const items=await navigator.clipboard.read();return items.some(i=>i.types.includes('image/png'))&&items.some(i=>i.types.includes('web application/x-capycanvas-clip'))})()`),true,'Copying images publishes a PNG and the Capy clip');
    assert.deepEqual(await children(layer),[blue,red],'Copy leaves the drawing unchanged');
    await chord('v');
    await wait(`layerApp.state().layers.find(l=>l.id==${layer}n).object_count===3`);await idle();
    const pastedId=(await selected(layer))[0];
    assert.ok(pastedId&&pastedId!==red,'The paste is a new image');
    const afterPaste=await save(),objects=packageObjects(afterPaste,'capy.image-object/1');
    assert.equal(objects.length,3);
    assert.equal(packageObjects(afterPaste,'capy.image/1').length,2,'The paste shares the immutable image');
    assert.equal(objects.filter(o=>JSON.stringify(o.data.affine??[1,0,0,1,0,0])===JSON.stringify(redAffine)).length,2,'Pasting keeps the copied document position');
    await invoke('undo');await idle();
    assert.deepEqual(await children(layer),[blue,red],'Pasting is one undo step');
    await click(await screen(230,180));
    await wait(`(o=>o.join()==='${red}')(layerApp.state().layers.find(l=>l.id==${layer}n).objects.filter(o=>o.selected).map(o=>String(o.id)))`);
    previous=await nonce();await chord('x');await copied(previous);
    await wait(`layerApp.state().layers.find(l=>l.id==${layer}n).object_count===1`);
    await chord('v');
    await wait(`layerApp.state().layers.find(l=>l.id==${layer}n).object_count===2`);await idle();
    assert.equal(packageObjects(await save(),'capy.image-object/1').filter(o=>JSON.stringify(o.data.affine??[1,0,0,1,0,0])===JSON.stringify(redAffine)).length,1,'Cut and paste restores the image in place');
    await capture('image-cut-paste');
    console.log('Image layers: image Copy, Paste, Cut and Paste');

    const order=(await state()).layers.length;
    await select([400,60,600,220]);
    await editMenu((await command('paste_into')).label);
    await wait(`layerApp.state().layers.length===${order+1}`);await idle();
    const frame=(await state()).layers.find(l=>l.editing);
    assert.equal(frame.object_count,1,'Paste Into makes an image layer');assert.equal(frame.has_mask,true,'with a mask from the selection');
    assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false,'The selection became the mask');
    await capture('image-paste-into');
    const frameMask=async()=>{const m=await save();return packageObject(m,imageLayerIds(m).find(id=>packageObject(m,id).data.mask)).data.mask;};
    const mask=await frameMask();
    await invoke('move');
    await send(`{type:'object',action:{op:'expand',layer:${frame.id}n,expanded:true}}`);
    const inner=(await layerBy(frame.id)).objects[0].id;
    await send(`{type:'object',action:{op:'select',id:${inner}n,extend:false}}`);
    const innerAffine=async()=>{const m=await save();return packageObject(m,packageObject(m,imageLayerIds(m).find(id=>packageObject(m,id).data.mask)).data.content.objects).data.children.map(ref=>affine(m,ref.ref??ref))[0];};
    const innerStart=await innerAffine();
    await drag(line(await screen(...map(innerStart,60,45)),await screen(...map(innerStart,90,65))));
    assert.notDeepEqual(await innerAffine(),innerStart,'The image moves behind its mask');
    assert.deepEqual(await frameMask(),mask,'Moving the image keeps the mask frame fixed');
    await send(`{type:'layer',action:{op:'link_mask',id:${frame.id},value:false}}`);
    const unlinked=await frameMask();
    assert.equal(unlinked.linked,false);
    await send(`{type:'layer',action:{op:'link_mask',id:${frame.id},value:true}}`);
    assert.deepEqual(await frameMask(),mask,'Relinking restores the linked mask exactly');
    for(let i=0;i<4;i++)await invoke('undo');
    await idle();
    assert.equal((await state()).layers.length,order,'Paste Into undoes in one step');
    console.log('Image layers: Paste Into, moving behind the mask, unlink and relink');

    await invoke('move');
    await click(await screen(230,180));
    await wait(`(o=>o.length===1)(layerApp.state().layers.find(l=>l.id==${layer}n).objects.filter(o=>o.selected))`);
    const target=(await selected(layer))[0];
    await select([150,120,330,280]);
    await key('Delete','Delete',46);
    await wait('layerApp.state().notice?.actions?.length===3');
    assert.deepEqual((await state()).notice.actions.map(a=>a.id),['add_mask','new_paint_layer','rasterize_layer'],'Clearing pixels on images refuses with the image actions');
    assert.equal((await children(layer)).length,2,'Delete with a pixel target never deletes images');
    await capture('image-pixel-target');
    await invoke('move');
    assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true,'Returning to Move keeps the pixel selection');
    await click(await screen(230,180));
    await wait(`(o=>o.length===1)(layerApp.state().layers.find(l=>l.id==${layer}n).objects.filter(o=>o.selected))`);
    await key('Delete','Delete',46);
    await wait(`layerApp.state().layers.find(l=>l.id==${layer}n).object_count===1`);
    assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true,'Deleting the image keeps the pixel selection');
    await invoke('undo');await idle();
    assert.equal((await children(layer)).length,2);assert.ok((await children(layer)).includes(target));

    const uncropped=await save(),placements=Object.fromEntries(packageObjects(uncropped,'capy.image-object/1').map(o=>[o.id,o.data.affine]));
    await invoke('crop_canvas_to_selection');
    await wait(`layerApp.state().tabs[0].width!==640`);await idle();
    const cropped=await save(),shift=[];
    for(const object of packageObjects(cropped,'capy.image-object/1')){
      const before=placements[object.id],after=object.data.affine,offset=packageObject(cropped,imageLayerIds(cropped)[0]).data.offset?.map(Number)??[0,0];
      assert.deepEqual(after.slice(0,4),before.slice(0,4),'Cropping never resamples an image');
      const moved=[after[4]+offset[0]-before[4],after[5]+offset[1]-before[5]];
      if(shift.length)assert.deepEqual(moved,shift,'Every image keeps its place relative to the artwork');else shift.push(...moved);
    }
    assert.equal(packageObjects(cropped,'capy.image-object/1').length,2,'Cropping keeps every image, inside the frame or not');
    assert.ok(shift[0]<0&&shift[1]<0,'Content moves by the crop origin: '+shift);
    await capture('image-crop-keeps-images');
    await invoke('undo');await idle();
    assert.deepEqual(await size(),[640,480],'One undo step restores the frame');

    await click(await screen(600,40));await key('c','KeyC',67);
    await wait(`layerApp.state().canvas_bar?.context.kind==='crop'&&layerApp.state().commands.find(c=>c.id==='crop_delete_cropped_pixels')?.enabled`);
    if(!(await evaluate(`layerApp.state().commands.find(c=>c.id==='crop_delete_cropped_pixels').selected`)))await invoke('crop_delete_cropped_pixels');
    await capture('crop-delete-paint-pixels');
    await drag(line(await screen(0,0),await screen(150,100),4));
    await invoke('apply_transform');
    await wait(`layerApp.state().tabs[0].width<640&&layerApp.state().canvas_bar?.context.kind!=='crop'`);await idle();
    const deleted=await save();
    assert.equal(packageObjects(deleted,'capy.image-object/1').length,2,'Deleting cropped paint pixels keeps every placed image');
    for(const object of packageObjects(deleted,'capy.image-object/1'))assert.deepEqual(object.data.affine.slice(0,4),placements[object.id].slice(0,4));
    await invoke('undo');await idle();
    assert.deepEqual(await size(),[640,480]);
    await shows(points,original,'Undo brings the deleted paint pixels back');
    await send(`{type:'layer',action:{op:'select',id:${layer},mask:false}}`);
    await invoke('rectangle_select');if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await send(`{type:'layer',action:{op:'add_mask',id:${layer},replace:false}}`);await send(`{type:'layer',action:{op:'select',id:${layer},mask:true}}`);
    await invoke('eraser');await send(`{type:'select_brush',id:3}`);await send(`{type:'set_brush_size',value:60}`);
    const stroke=line(await screen(210,160),await screen(260,200));
    await evaluate(`layerApp.restartGpu().then(()=>{window.readyAtDown=[];addEventListener('pointerdown',()=>readyAtDown.push(layerApp.app.brush_ready()),{capture:true,once:true})})`);
    await drag(stroke,'pen');await idle();
    assert.deepEqual(JSON.parse(await evaluate('JSON.stringify(readyAtDown)')),[false],'The stroke begins while the restarted canvas prepares its shaders');
    assert.ok(packageObjects(await save(),'capy.coverage-source/2').some(c=>c.data.tiles?.length),'Erasing the image layer mask stores coverage');
    const masked=await children(layer);
    await layerMenu(layer,'Rasterize and Apply Mask','.layer-thumbnail ~ .layer-thumbnail');
    await wait(`(l=>!l.object_count&&!l.has_mask)(layerApp.state().layers.find(l=>l.id==${layer}n))`);await idle();
    let applied;
    for(const end=Date.now()+30000;Date.now()<end;await pause(150)){applied=await pixelAt(230,180);if(!close(applied,RED))break;}
    assert.ok(!close(applied,RED),'The applied mask hides part of the red image: '+applied);
    await shows([[530,400]],[BLUE],'Rasterize and Apply Mask keeps unmasked images');
    await invoke('undo');await idle();
    assert.equal((await layerBy(layer)).has_mask,true,'Undo restores the mask');assert.deepEqual(await children(layer),masked,'Undo restores the same images');
    await send(`{type:'layer',action:{op:'delete_mask',id:${layer}}}`);await idle();
    console.log('Image layers: Rasterize and Apply Mask');
    console.log(`PASS image layers (web, ${theme}): conversions, merges, alpha selection, refusal actions, apply mask, clipboard, Paste Into, targets, crop and delete cropped paint pixels; screenshots in ${directory}`);
  } catch(error) {
    await capture('failure');
    console.error('Image layers state',JSON.stringify((await state()).layers.map(l=>({id:l.id,label:l.label,object_count:l.object_count,has_mask:l.has_mask,editing:l.editing}))),await evaluate('JSON.stringify(layerApp.state().notice)'));
    throw error;
  } finally {
    await evaluate('if(window.imageLayers){window.showSaveFilePicker=imageLayers.save;delete window.imageLayers}');
  }
}
