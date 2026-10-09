import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readPackage,packageObjects} from './package-fixture.test.mjs';
import {png} from './clone-journey.test.mjs';

const openMenu='.panel-context-menu:popover-open';

export async function checkImageRows({call,evaluate,settle,canvasPixels}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/image-rows/web';
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+' '+JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice?.text,status:document.querySelector('#status')?.textContent})));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    await send({type:'invoke',command});
  };
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?String(v):v))');
  const idle=()=>wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
  const rows=async()=>(await state()).layers.filter(l=>l.object);
  const selected=async()=>(await rows()).filter(o=>o.selected).map(o=>o.id);
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)return null;const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);assert.ok(r&&r.width&&r.height,`${selector} is shown`);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const mouse=(type,p,modifiers=0,button='left')=>call('Input.dispatchMouseEvent',{type,...p,modifiers,button,pointerType:'mouse',buttons:type==='mouseReleased'||type==='mouseMoved'&&button==='none'?0:button==='right'?2:1,clickCount:1});
  const click=async(p,modifiers=0,button='left')=>{await mouse('mousePressed',p,modifiers,button);await mouse('mouseReleased',p,modifiers,button);await settle();};
  const drag=async(from,to,steps=8)=>{
    await mouse('mousePressed',from);
    for(let i=1;i<=steps;i++){await mouse('mouseMoved',{x:from.x+(to.x-from.x)*i/steps,y:from.y+(to.y-from.y)*i/steps});await settle();}
    await mouse('mouseReleased',to);await settle();
  };
  const key=async(key,code,windowsVirtualKeyCode,modifiers=0)=>{
    const text=key==='Enter'&&!modifiers?'\r':undefined;
    for(const type of [text?'keyDown':'rawKeyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode,modifiers,...(type==='keyDown'?{text}:{})});
    await settle();
  };
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:c.translation,v:c.viewport,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const row=id=>`.layer-row[data-layer="${id}"]`;
  const pasteImage=async(width,height,color,offset=null)=>{
    await click(await evaluate('({x:innerWidth/2,y:3})'));await wait('document.hasFocus()');
    const bytes=png(width,height,()=>color,3).toString('base64');
    await evaluate(`(async()=>{const bytes=Uint8Array.from(atob('${bytes}'),c=>c.charCodeAt(0));
      await navigator.clipboard.write([new ClipboardItem({'image/png':new Blob([bytes],{type:'image/png'})})]);})()`);
    const before=(await rows()).length;
    await key('v','KeyV',86,2);
    await wait(`layerApp.state().canvas_bar?.context.kind==='placement'&&layerApp.state().commands.find(c=>c.id==='apply_transform')?.enabled`);
    if(offset){const [w,h]=await size();await drag(await screen(w/2-150,h/2-100),await screen(w/2-150+offset[0],h/2-100+offset[1]));}
    await key('Enter','Enter',13);await idle();
    await wait(`layerApp.state().layers.filter(l=>l.object).length===${before+1}&&layerApp.state().canvas_bar?.context.kind!=='placement'`);
  };
  const size=()=>evaluate('[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height]');
  const pixelAt=async p=>{
    const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:Math.round(p.x)-2,y:Math.round(p.y)-2,width:5,height:5,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot.data}';await image.decode();const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);return Array.from(x.getImageData(2,2,1,1).data)})()`);
  };
  const shows=async(p,rgb,label)=>{
    let last;
    for(const end=Date.now()+60000;Date.now()<end;await pause(200)){last=await pixelAt(p);if(last.slice(0,3).every((v,i)=>Math.abs(v-rgb[i])<24))return;}
    assert.fail(`${label}: ${JSON.stringify(last)} at ${JSON.stringify(p)}`);
  };
  const save=async()=>{
    await evaluate(`window.showSaveFilePicker=async o=>{const directory=await(await navigator.storage.getDirectory()).getDirectoryHandle('capy-image-rows',{create:true});return imageRows.handle=await directory.getFileHandle(crypto.randomUUID()+'.capy',{create:true});};window.imageRows??={};`);
    await invoke('save_document_as');await idle();
    await evaluate('(async()=>{imageRows.saved=new Uint8Array(await(await imageRows.handle.getFile()).arrayBuffer())})()');
    return readPackage(evaluate,'imageRows.saved');
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  try {
    await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    await idle();await invoke('new_document');
    await wait('document.querySelector("dialog[open] [data-document-field=width]")');
    await evaluate(`{for(const [field,value] of [['width',2000],['height',1500]]){const input=document.querySelector('dialog[open] [data-document-field='+field+']');input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));}document.querySelector('dialog[open] [data-document-action=create]').click();}`);
    await idle();await invoke('fit_canvas');
    await pasteImage(600,450,[0xdd,0x2a,0x1f]);
    await pasteImage(600,450,[0x1f,0x3a,0xde],[360,270]);
    {const [w,h]=await size();
      await shows(await screen(w/2-200,h/2-150),[0xdd,0x2a,0x1f],'the first pasted image is presented');
      await shows(await screen(w/2+450,h/2+400),[0x1f,0x3a,0xde],'the second pasted image is presented');}
    const affines=packageObjects(await save(),'capy.image-object/1').map(o=>o.data.affine);
    assert.deepEqual(affines.map(a=>a.slice(0,4)),[[1,0,0,1],[1,0,0,1]]);
    const shift=[Math.abs(affines[1][4]-affines[0][4]),Math.abs(affines[1][5]-affines[0][5])];
    assert.ok(Math.abs(shift[0]-360)<1&&Math.abs(shift[1]-270)<1,'dragging during placement moves the pasted image '+JSON.stringify(affines));
    const [front,back]=await rows();
    assert.equal((await rows()).length,2,'Each paste creates an Object layer');
    assert.equal(await evaluate("document.querySelectorAll('.layer-object-row,.layer-expand').length"),0,'Object layers have no child rows or expansion');
    const order=()=>rows().then(rows=>rows.map(row=>row.id));
    await click(await middle(`${row(back.id)} .layer-name`));
    assert.deepEqual(await selected(),[back.id]);
    await click(await middle(`${row(front.id)} .layer-name`),8);
    assert.equal((await selected()).length,2,'Shift-click selects sibling Object layers');
    await click(await middle(`${row(front.id)} .layer-thumbnail`));
    await click(await middle(`${row(front.id)} .layer-icon:first-child`));
    await wait(`!layerApp.state().layers.find(l=>l.id===${front.id}n).visible`);
    await invoke('undo');await wait(`layerApp.state().layers.find(l=>l.id===${front.id}n).visible`);
    const original=await order(),start=await middle(`${row(front.id)} .layer-name`),target=await rect(row(back.id));
    await drag(start,{x:start.x,y:target.y+target.height*.8},10);
    await wait(`layerApp.state().layers.filter(l=>l.object).map(l=>String(l.id)).join()==='${back.id},${front.id}'`);
    await invoke('undo');assert.deepEqual(await order(),original,'Layer reorder is one Undo');
    await invoke('move');
    const [width,height]=await size();
    await click(await screen(width/2-200,height/2-150));
    await wait(`layerApp.state().layers.find(l=>l.id===${back.id}n).editing`);
    await click(await screen(width/2+450,height/2+400));
    await wait(`layerApp.state().layers.find(l=>l.id===${front.id}n).editing`);
    for(const [w,h] of [[1440,1000],[900,700]]) {
      await call('Emulation.setDeviceMetricsOverride',{width:w,height:h,deviceScaleFactor:1,mobile:false});await pause(300);
      await wait(`document.querySelector('${row(front.id)} .layer-thumbnail canvas')?.dataset.previewRevision`);
      await capture(`object-layers-${theme??'default'}-${w}`);
    }
    console.log('Object layers: native row selection, visibility, reorder, canvas picking, individual previews and placement');
  } catch(error) {
    await capture('failure');
    console.error('Image rows state',JSON.stringify((await state()).layers.map(l=>({id:l.id,label:l.label,object:l.object})),null,0));
    throw error;
  } finally {
    if(theme)await send({type:'set_theme',theme});
  }
}
