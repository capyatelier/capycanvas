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
  const imageLayer=async()=>(await state()).layers.find(l=>l.object_count>0);
  const rows=async()=>(await imageLayer()).objects;
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
  let touchId=900;
  const touch=async(points)=>{
    touchId++;
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:touchId,...points[0]}]});
    for(const p of points.slice(1)){await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:touchId,...p}]});await settle();}
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();await pause(80);
  };
  const key=async(key,code,windowsVirtualKeyCode,modifiers=0)=>{
    const text=key==='Enter'&&!modifiers?'\r':undefined;
    for(const type of [text?'keyDown':'rawKeyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode,modifiers,...(type==='keyDown'?{text}:{})});
    await settle();
  };
  const capture=async name=>writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:c.translation,v:c.viewport,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const row=id=>`.layer-object-row[data-object="${id}"]`;
  const layerRow=id=>`.layer-row[data-layer="${id}"]`;
  const thumbnailsLoaded=`[...document.querySelectorAll('.layer-object-row canvas')].length>=2&&[...document.querySelectorAll('.layer-object-row canvas')].every(c=>c.dataset.previewRevision&&c.getContext('2d',{willReadFrequently:true}).getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0))`;
  const pasteImage=async(width,height,color,offset=null)=>{
    await click(await evaluate('({x:innerWidth/2,y:3})'));await wait('document.hasFocus()');
    const bytes=png(width,height,()=>color,3).toString('base64');
    await evaluate(`(async()=>{const bytes=Uint8Array.from(atob('${bytes}'),c=>c.charCodeAt(0));
      await navigator.clipboard.write([new ClipboardItem({'image/png':new Blob([bytes],{type:'image/png'})})]);})()`);
    const before=(await imageLayer())?.object_count??0;
    await key('v','KeyV',86,2);
    await wait(`layerApp.state().canvas_bar?.context.kind==='placement'&&layerApp.state().commands.find(c=>c.id==='apply_transform')?.enabled`);
    if(offset){const [w,h]=await size();await drag(await screen(w/2-150,h/2-100),await screen(w/2-150+offset[0],h/2-100+offset[1]));}
    await key('Enter','Enter',13);await idle();
    await wait(`(l=>l&&l.object_count===${before+1})(layerApp.state().layers.find(l=>l.object_count>0))&&layerApp.state().canvas_bar?.context.kind!=='placement'`);
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
    const layer=await imageLayer();
    assert.equal(layer.object_count,2,'the second paste joins the active image layer');
    assert.equal(layer.expanded,false);assert.equal(layer.objects.length,0);
    assert.equal(await evaluate(`document.querySelector('${layerRow(layer.id)} .layer-expand').hidden`),false,'image layers show the expand control');
    assert.equal(await evaluate(`document.querySelector('${layerRow(layer.id)} .layer-expand').getAttribute('aria-expanded')`),'false');
    await click(await middle(`${layerRow(layer.id)} .layer-expand`));
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).expanded&&document.querySelectorAll('.layer-object-row').length===2`);
    assert.equal(await evaluate(`document.querySelector('${layerRow(layer.id)} .layer-expand').getAttribute('aria-expanded')`),'true');
    const [front,back]=await rows();
    assert.equal(await evaluate(`[...document.querySelectorAll('#layer-rows .layer-row')].map(n=>n.dataset.layer??'o'+n.dataset.object).slice(0,3).join()`),
      [layer.id,'o'+front.id,'o'+back.id].join(),'image rows follow their layer front to back');
    const indent=await evaluate(`[document.querySelector('${layerRow(layer.id)} .layer-thumbnails').getBoundingClientRect().left,document.querySelector('${row(front.id)} .layer-thumbnails').getBoundingClientRect().left]`);
    assert.ok(indent[1]>indent[0],'image rows are indented under their layer');
    await wait(`[...document.querySelectorAll('.layer-object-row .layer-thumbnail')].every(b=>b.classList.contains('loaded')&&b.querySelector('canvas').dataset.previewRevision)`,60000);
    assert.equal(await evaluate(`[...document.querySelectorAll('.layer-object-row .layer-object-placeholder')].every(n=>getComputedStyle(n).display==='none')`),true,'loaded image previews replace the placeholder');
    await wait(`!!document.querySelector('${layerRow(layer.id)} .layer-thumbnail canvas')?.dataset.previewRevision`,60000);
    await wait(thumbnailsLoaded,60000);
    for(const object of [front,back]){
      assert.equal(await evaluate(`document.querySelector('${row(object.id)} .layer-name').textContent`),object.label);
      assert.equal(await evaluate(`document.querySelector('${row(object.id)} .layer-thumbnail').getAttribute('aria-label')`),object.label);
      assert.equal(await evaluate(`document.querySelector('${row(object.id)} .layer-object-spacer').getAttribute('aria-hidden')`),'true');
      assert.equal(await evaluate(`!!document.querySelector('${row(object.id)} .layer-link, ${row(object.id)} .layer-swipe-delete, ${row(object.id)} .layer-lock')`),false,'image rows have no mask, swipe or lock controls');
    }

    await click(await middle(`${row(back.id)} .layer-name`));
    await wait(`(o=>o.filter(o=>o.selected).length===1&&o.find(o=>o.selected).id==${back.id})(layerApp.state().layers.find(l=>l.object_count>0).objects)`);
    assert.equal(await evaluate(`document.querySelector('${row(back.id)}').classList.contains('selected')`),true);
    await click(await middle(`${row(front.id)} .layer-name`),8);
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.every(o=>o.selected)`);
    console.log('Image rows: expand, order, previews, click and Shift-click selection');

    await evaluate(`(()=>{const take=layerApp.app.take_layer_thumbnail.bind(layerApp.app);window.rowsProbe={seen:[],take};layerApp.app.take_layer_thumbnail=()=>{const r=take();if(r)rowsProbe.seen.push(Number(r[0]));return r;};})()`);
    await wait(`(()=>{if(!layerApp.app.request_layer_thumbnail(777001n,${front.id}n))return false;layerApp.restartGpu();return true;})()`);
    await wait('window.layerApp&&layerApp.app.brush_ready()',60000);
    await wait('rowsProbe.seen.includes(777001)',60000);
    await evaluate('layerApp.app.take_layer_thumbnail=rowsProbe.take;delete window.rowsProbe');
    console.log('Image rows: a preview accepted before a GPU restart still arrives');

    await evaluate(`document.querySelector('${row(front.id)} .layer-thumbnail').focus()`);
    await key('Enter','Enter',13);
    await wait(`(o=>o.filter(o=>o.selected).map(o=>o.id).join()==='${front.id}')(layerApp.state().layers.find(l=>l.object_count>0).objects)`);
    assert.equal(await evaluate(`document.querySelector('${row(front.id)} .layer-thumbnail').getAttribute('aria-pressed')`),'true');
    console.log('Image rows: keyboard activation selects one image');

    await click(await middle(`${row(front.id)} .layer-icon:first-child`));
    await wait(`!layerApp.state().layers.find(l=>l.object_count>0).objects.find(o=>o.id==${front.id}).visible&&document.querySelector('${row(front.id)}').classList.contains('layer-object-hidden')`);
    assert.equal(await evaluate(`document.querySelector('${row(front.id)} .layer-icon:first-child').getAttribute('aria-label')`),(await evaluate('layerApp.app.catalog().native_copy.layers.show_image')));
    await invoke('undo');
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.find(o=>o.id==${front.id}).visible`);
    console.log('Image rows: visibility toggle and undo');

    await click(await middle(`${row(back.id)} .layer-name`),0,'right');
    await wait(`!!document.querySelector('${openMenu}')&&document.querySelector('${openMenu}').querySelectorAll('button').length>2`);
    const menuLabels=await evaluate(`[...document.querySelectorAll('${openMenu} button .menu-label')].map(n=>n.textContent)`);
    assert.deepEqual(menuLabels,(await evaluate(`layerApp.app.object_menu(${back.id}n).sections.flat().map(i=>i.label)`)),'the row menu is the shared image menu');
    for(const name of ['light','dark']){await send({type:'set_theme',theme:name});await pause(200);await capture(`image-row-menu-${name}-1440`);}
    await key('Escape','Escape',27);
    await wait(`!document.querySelector('${openMenu}')`);

    const order=async()=>(await rows()).map(o=>o.id).join();
    const initial=await order();
    const start=await middle(`${row(front.id)} .layer-name`),target=await rect(row(back.id));
    await drag(start,{x:start.x,y:target.y+target.height*.8},10);
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.map(o=>o.id).join()==='${back.id},${front.id}'`);
    assert.equal(await evaluate(`document.querySelectorAll('.layer-drag-preview').length`),0);
    await invoke('undo');
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.map(o=>o.id).join()==='${initial}'`);
    console.log('Image rows: drag reorder within the image layer and undo');

    await invoke('move');
    const [width,height]=await size();
    await click(await screen(width/2-200,height/2-150));
    await wait(`(o=>o.filter(o=>o.selected).map(o=>o.id).join()==='${back.id}')(layerApp.state().layers.find(l=>l.object_count>0).objects)`);
    await click(await screen(width/2+450,height/2+400));
    await wait(`(o=>o.filter(o=>o.selected).map(o=>o.id).join()==='${front.id}')(layerApp.state().layers.find(l=>l.object_count>0).objects)`);
    await click(await screen(width*.05,height*.05));
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.every(o=>!o.selected)`);
    console.log('Move: canvas picking selects the frontmost image and an empty click clears');

    await touch([await screen(width/2-200,height/2-150)]);
    await wait(`(o=>o.filter(o=>o.selected).map(o=>o.id).join()==='${back.id}')(layerApp.state().layers.find(l=>l.object_count>0).objects)`);
    await click(await screen(width*.05,height*.05));
    await wait(`layerApp.state().layers.find(l=>l.object_count>0).objects.every(o=>!o.selected)`);
    const before=await camera(),a=await screen(width*.08,height*.9);
    assert.equal(await evaluate(`document.elementFromPoint(${a.x},${a.y})?.id`),'canvas');
    const ids=[touchId+1,touchId+2];touchId+=2;
    const fingers=i=>[{id:ids[0],x:a.x+i*12,y:a.y-i*5},{id:ids[1],x:a.x+80+i*12,y:a.y-i*5}];
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:fingers(0)});
    for(let i=1;i<9;i++){await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:fingers(i)});await settle();}
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();await pause(200);
    const after=await camera();
    assert.notDeepEqual(after.t,before.t,'an empty two-finger touch drag still navigates');
    assert.ok((await rows()).every(o=>!o.selected),'navigation selects no image');
    console.log('Move: touch selects an unselected image and empty touch navigates');

    for(const [w,h] of [[1440,1000],[900,700]]) {
      await call('Emulation.setDeviceMetricsOverride',{width:w,height:h,deviceScaleFactor:1,mobile:false});await pause(300);
      await click(await middle(`${row(front.id)} .layer-name`));await settle();
      await shows(await screen(width/2-200,height/2-150),[0xdd,0x2a,0x1f],`${w}x${h}: images are presented after a viewport change`);
      for(const name of ['light','dark']){await send({type:'set_theme',theme:name});await pause(250);await capture(`image-rows-${name}-${w}`);}
    }
    console.log(`PASS image rows (web): screenshots in ${directory}`);
  } catch(error) {
    await capture('failure');
    console.error('Image rows state',JSON.stringify((await state()).layers.map(l=>({id:l.id,label:l.label,object_count:l.object_count,expanded:l.expanded,objects:l.objects})),null,0));
    throw error;
  } finally {
    if(theme)await send({type:'set_theme',theme});
  }
}
