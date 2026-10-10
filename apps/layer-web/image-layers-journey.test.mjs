import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readPackage,imageIdentity} from './package-fixture.test.mjs';
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
    await wait(`document.querySelector('.header-menu[data-menu="edit"]').open`);
    for(const caption of ['Paste Special',label]) {
      const row=`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(caption)})`;
      await wait(`!!${row}&&!${row}.disabled`);
      await click(await evaluate(`(()=>{const r=${row}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`));
    }
  };
  const select=async([x0,y0,x1,y1])=>{
    await invoke('rectangle_select');
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await drag(line(await screen(x0,y0),await screen(x1,y1),4));
    await wait('layerApp.state().layer_tools.has_selection');
  };
  const nonce=()=>evaluate('layerApp.app.clip_nonce()??null');
  const copied=async previous=>{await wait(`(layerApp.app.clip_nonce()??null)!==${JSON.stringify(previous)}`);await idle();};
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
    const objects=() => state().then(s=>s.layers.filter(l=>l.object));
    const [front,back]=await objects(),layer=back.id;
    assert.equal((await objects()).length,2,'Each external paste creates an Object layer');
    const points=[[230,180],[530,400],[110,430],[600,40]],original=[RED,BLUE,GREEN,WHITE];
    await shows(points,original,'The fixture shows independent red, blue, green and paper');
    const fixture=await save(),sources=imageIdentity(fixture);
    await send(`{type:'select_layer',id:${layer}n}`);await idle();
    await layerMenu(layer,'Rasterize Layer');await wait(`!layerApp.state().layers.find(l=>l.id===${layer}n).object`);await idle();
    assert.equal((await layerBy(layer)).object,false);
    await shows(points,original,'Rasterizing one layer preserves the composite');
    await invoke('undo');await idle();assert.equal((await layerBy(layer)).object,true);
    assert.deepEqual(imageIdentity(await save()),sources,'Undo restores exact original sources');
    await layerMenu(ink,'Convert to Object Layer');await wait(`layerApp.state().layers.find(l=>l.id===${ink}n).object`);await idle();
    assert.equal((await layerBy(ink)).object,true);
    await invoke('pen');await idle();
    await shows(points,original,'Converting raster to Object preserves the composite');
    await invoke('undo');await idle();assert.equal((await layerBy(ink)).object,false);
    await send(`{type:'select_layer',id:${front.id}n}`);await idle();
    const count=(await state()).layers.length;
    await chord('e');await wait(`layerApp.state().layers.length===${count-1}`);await idle();
    await shows(points,original,'Merge Down preserves sibling Object appearance');
    await invoke('undo');await idle();assert.equal((await objects()).length,2);
    await send(`{type:'select_layer',id:${layer}n}`);await idle();
    const previous=await nonce();await chord('c');await copied(previous);
    await chord('v');await wait(`layerApp.state().layers.length===${count+1}`);await idle();
    assert.equal((await state()).layers.find(l=>l.editing).object,true,'Internal Copy/Paste retains Object content');
    await invoke('undo');await idle();
    const cutNonce=await nonce();await chord('x');await copied(cutNonce);
    await wait(`layerApp.state().layers.length===${count-1}`);await idle();
    await invoke('undo');await idle();assert.equal((await layerBy(layer)).object,true,'One Undo restores a cut Object layer');
    await select([400,60,600,220]);await editMenu('Paste Into');
    await wait(`layerApp.state().layers.length===${count+1}`);await idle();
    const pasted=(await state()).layers.find(l=>l.editing);
    assert.equal(pasted.object,true);assert.equal(pasted.has_mask,true,'Paste Into attaches a layer mask');
    assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false);
    await invoke('undo');await idle();assert.equal((await state()).layers.length,count);
    await invoke('deselect');
    await send(`{type:'layer',action:{op:'add_mask',id:${layer}n,replace:false}}`);await idle();
    await send(`{type:'layer',action:{op:'select',id:${layer}n,mask:true}}`);await idle();
    const masked=await save();
    await invoke('eraser');await send(`{type:'set_brush_size',value:50}`);
    await drag(line(await screen(220,160),await screen(280,190)));await idle();
    assert.deepEqual(imageIdentity(await save()),imageIdentity(masked),'Mask painting leaves original image/profile samples unchanged');
    await invoke('undo');await idle();
    await send(`{type:'layer',action:{op:'select',id:${layer}n,mask:false}}`);await idle();
    await select([80,80,320,260]);
    const beforeCrop=await save();await invoke('crop_canvas_to_selection');await idle();
    assert.deepEqual(imageIdentity(await save()),imageIdentity(beforeCrop),'Cropping retains original image/profile bytes');
    await invoke('undo');await idle();await invoke('deselect');
    await evaluate(`window.showOpenFilePicker=async()=>[{async getFile(){return new File([imageLayers.saved],'objects.capy')}}]`);
    const beforeReopen=await save();await invoke('open_document');await idle();
    assert.deepEqual(imageIdentity(await save()),imageIdentity(beforeReopen),'Save/reopen preserves separate source/profile identities');
    await capture('object-layers');
    console.log('Object layers: separate imports, raster conversion, merge, LayerClip clipboard, mask, crop, Undo and exact source reopen');
  } catch(error) {
    await capture('failure');
    console.error('Image layers state',JSON.stringify((await state()).layers.map(l=>({id:l.id,label:l.label,object:l.object,has_mask:l.has_mask,editing:l.editing}))),await evaluate('JSON.stringify(layerApp.state().notice)'));
    throw error;
  } finally {
    await evaluate('if(window.imageLayers){window.showSaveFilePicker=imageLayers.save;delete window.imageLayers}');
  }
}
