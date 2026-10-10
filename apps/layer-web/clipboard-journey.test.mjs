import assert from 'node:assert/strict';
import {readPackage,packageObject,packageObjects,packageOccurrences} from './package-fixture.test.mjs';

const bar='.canvas-action-bar';
const visible=`(()=>{const b=document.querySelector('${bar}');return !!b&&!b.hidden&&!b.classList.contains('suppressed')})()`;
const openMenu='.panel-context-menu:popover-open';

export async function checkClipboard({call,evaluate,settle}) {
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+' '+JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,requests:layerApp.state().requests,menu:!!document.querySelector('${openMenu}'),status:document.querySelector('#status')?.textContent})));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);
    await send({type:'invoke',command});
  };
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const idle=()=>wait('!layerApp.state().document_file.busy&&!layerApp.state().requests.some(r=>r.kind.type==="document")');
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  let touchId=700;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(points,kind)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',points[0],kind);
    for(const p of points.slice(1)){await pointer('mouseMoved',p,kind);await settle();}
    await pointer('mouseReleased',points.at(-1),kind);await settle();
  };
  const key=async(key,{ctrl=true,shift=false,alt=false}={})=>{
    const modifiers=(alt?1:0)|(ctrl?2:0)|(shift?8:0),code=`Key${key.toUpperCase()}`,windowsVirtualKeyCode=key.toUpperCase().charCodeAt(0);
    await call('Input.dispatchKeyEvent',{type:'keyDown',modifiers,key:shift?key.toUpperCase():key,code,windowsVirtualKeyCode});
    await call('Input.dispatchKeyEvent',{type:'keyUp',modifiers,key:shift?key.toUpperCase():key,code,windowsVirtualKeyCode});
    await settle();
  };
  const row=label=>`[...document.querySelectorAll('${openMenu} button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const chooseRow=async(label,kind)=>{
    await wait(`!!${row(label)}&&!${row(label)}.disabled`);
    await tap(await evaluate(`(()=>{const r=${row(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
  };
  const shown=selector=>`(n=>!!n&&!n.closest('.canvas-action-bar-item').hidden)(document.querySelector(${JSON.stringify(selector)}))`;
  const barMenu=async(id,label,kind)=>{
    const selector=`${bar} [data-canvas-bar-menu="${id}"]`;
    await wait(`!!document.querySelector('${selector}')&&${visible}`);
    if(await evaluate(shown(selector))){
      await tap(await middle(selector),kind);
    } else {
      await tap(await middle(`${bar} .canvas-action-bar-more`),kind);
      await chooseRow(await evaluate(`document.querySelector('${selector}').getAttribute('aria-label')`),kind);
    }
    await chooseRow(label,kind);
  };
  const headerRow=label=>`[...document.querySelectorAll('.header-menu[open] .popover button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const editMenu=async(label,kind)=>{
    await tap(await middle('.header-menu[data-menu="edit"] > summary'),kind);
    await wait(`document.querySelector('.header-menu[data-menu="edit"]').open`);
    for(const row of ['Paste Special',label]) {
      await wait(`!!${headerRow(row)}&&!${headerRow(row)}.disabled`);
      await tap(await evaluate(`(()=>{const r=${headerRow(row)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
    }
  };
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:c.translation,v:c.viewport,r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async(x,y)=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const nonce=()=>evaluate('layerApp.app.clip_nonce()??null');
  const copied=async previous=>{await wait(`(layerApp.app.clip_nonce()??null)!==${JSON.stringify(previous)}`);await idle();return nonce();};
  const clipboardPng=()=>evaluate(`(async()=>{const items=await navigator.clipboard.read();const item=items.find(i=>i.types.includes('image/png'));
    const custom=items.find(i=>i.types.includes('web application/x-capycanvas-clip'));
    const bitmap=await createImageBitmap(await item.getType('image/png'));
    return{extent:[bitmap.width,bitmap.height],nonce:custom?await(await custom.getType('web application/x-capycanvas-clip')).text():null};})()`);
  const layers=async()=>(await state()).layers.length;
  const select=async(kind,[x0,y0,x1,y1])=>{
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('rectangle_select');
    const [a,b]=[await screen(x0,y0),await screen(x1,y1)];
    await drag([a,{x:(a.x+b.x)/2,y:(a.y+b.y)/2},b],kind==='touch'?'pen':kind);
    await wait(`layerApp.state().layer_tools.has_selection&&${visible}`);
  };
  await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  await invoke('fit_canvas');
  await send({type:'set_color',rgba:[.1,.3,.8,1]});
  const doc=(await state()).tabs[0],[width,height]=[doc.width,doc.height];
  const area=[width*.3,height*.3,width*.6,height*.55];
  await select('mouse',area);
  await invoke('fill_selection');await idle();
  for(const kind of ['pen','touch','mouse']) {
    await select(kind,area);
    const before=await nonce(),count=await layers();
    await barMenu('copy','Copy',kind);
    const clip=await copied(before);
    const png=await clipboardPng();
    const expected=[Math.round(area[2])-Math.round(area[0]),Math.round(area[3])-Math.round(area[1])];
    assert.ok(png.extent.every((v,i)=>Math.abs(v-expected[i])<=2),`${kind}: another app reads the selection's PNG ${JSON.stringify(png.extent)} ${JSON.stringify(expected)}`);
    assert.equal(png.nonce,clip,`${kind}: the clipboard carries this window's nonce`);
    await editMenu('Paste in Place',kind);
    await wait(`layerApp.state().layers.length===${count+1}`);await idle();
    assert.notEqual((await state()).canvas_bar?.context.kind,'placement',`${kind}: a copy from Capy pastes with no handles`);
    await invoke('undo');await wait(`layerApp.state().layers.length===${count}`);
    console.log(`${kind}: Copy ▾ › Copy, then Edit › Paste Special › Paste in Place`);
  }

  const count=await layers(),before=await nonce();
  await key('c');
  const keyed=await copied(before);
  await key('v');
  await wait(`layerApp.state().layers.length===${count+1}`);await idle();
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement','Ctrl+V pastes a visible copy in place');
  await invoke('undo');await wait(`layerApp.state().layers.length===${count}`);
  await key('c',{shift:true});
  assert.notEqual(await copied(keyed),keyed,'Ctrl+Shift+C copies merged');
  await invoke('paste_into');
  await wait(`layerApp.state().layers.length===${count+1}`);await idle();
  const into=(await state()).layers.find(l=>l.selected);
  assert.ok(into.has_mask,'Paste Into masks the new layer');
  assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false,'the selection became the mask');
  await invoke('undo');await wait(`layerApp.state().layers.length===${count}&&layerApp.state().layer_tools.has_selection`);

  const typed=await nonce();
  await evaluate(`(()=>{const input=document.createElement('input');input.id='clipboard-typing';input.value='Typed text';document.body.append(input);input.focus();input.select();})()`);
  await key('c');await key('v');await pause(300);
  assert.equal(await nonce(),typed,'Ctrl+C in a text field copies no pixels');
  assert.equal(await layers(),count,'Ctrl+V in a text field pastes no layer');
  assert.equal((await state()).requests.filter(r=>r.kind.type==='document').length,0);
  await evaluate(`document.getElementById('clipboard-typing').remove();layerApp.canvas.focus()`);
  assert.deepEqual(await evaluate(`(()=>{const editor=document.createElement('div'),text=document.createElement('span');
    editor.contentEditable='true';text.textContent='Nested text';editor.append(text);document.body.append(editor);
    const native=['c','x','v'].map(key=>{const options={key,ctrlKey:true,bubbles:true,cancelable:true};
      const unhandled=text.dispatchEvent(new KeyboardEvent('keydown',options));text.dispatchEvent(new KeyboardEvent('keyup',options));return unhandled;});
    editor.remove();return native;})()`),[true,true,true],'nested editable text keeps native Cut, Copy and Paste');

  const protectedNonce=await nonce();
  await evaluate(`(()=>{const dialog=document.createElement('dialog');document.body.append(dialog);dialog.showModal();
    for(const key of ['c','x','v']){const options={key,ctrlKey:true,bubbles:true,cancelable:true};layerApp.canvas.dispatchEvent(new KeyboardEvent('keydown',options));layerApp.canvas.dispatchEvent(new KeyboardEvent('keyup',options));}
    const data=new DataTransfer();data.items.add(new File(['pixels'],'late.png',{type:'image/png'}));
    layerApp.canvas.dispatchEvent(new ClipboardEvent('paste',{clipboardData:data,bubbles:true,cancelable:true}));dialog.close();dialog.remove();layerApp.canvas.focus();})()`);
  await pause(100);
  assert.equal(await nonce(),protectedNonce,'a new native dialog blocks artwork Cut and Copy');
  assert.equal(await layers(),count,'a late canvas-targeted paste cannot bypass a new dialog');
  assert.equal((await state()).requests.length,0);

  const external=await evaluate(`(async()=>{const c=new OffscreenCanvas(64,48),x=c.getContext('2d');x.fillStyle='#e04010';x.fillRect(0,0,64,48);
    await navigator.clipboard.write([new ClipboardItem({'web image/tiff':new Blob(['broken preferred TIFF'],{type:'image/tiff'}),'image/png':await c.convertToBlob()})]);return (await navigator.clipboard.read())[0].types.includes('web image/tiff');})()`);
  assert.ok(external,'the clipboard offers malformed preferred TIFF plus valid PNG');
  await call('Input.dispatchKeyEvent',{type:'keyDown',modifiers:8,key:'Insert',code:'Insert',windowsVirtualKeyCode:45});
  await call('Input.dispatchKeyEvent',{type:'keyUp',modifiers:8,key:'Insert',code:'Insert',windowsVirtualKeyCode:45});await settle();
  await wait(`layerApp.state().layers.length===${count+1}&&layerApp.state().canvas_bar?.context.kind==='placement'`);
  await invoke('cancel_transform');await wait(`layerApp.state().layers.length===${count}`);await idle();
  await key('v',{shift:true});
  await wait(`layerApp.state().layers.length===${count+1}`);await idle();
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement','Paste to Shown Position centres another app\'s image without handles');
  await invoke('undo');await wait(`layerApp.state().layers.length===${count}`);
  const drawings=()=>evaluate('layerApp.app.document_tabs(0).tabs.map(tab=>String(tab.id))');
  let previous=await drawings(),selected=await evaluate('String(layerApp.app.document_tabs(0).selected)');
  await editMenu('Paste as New Image','mouse');
  await wait(`layerApp.app.document_tabs(0).tabs.length===${previous.length+1}&&String(layerApp.app.document_tabs(0).selected)!==${JSON.stringify(selected)}&&!layerApp.documents.busy()`);
  await wait('layerApp.state().tabs[0].width===64&&layerApp.state().tabs[0].height===48');await idle();
  assert.equal((await state()).document_file.location??null,null);
  assert.equal((await state()).document_file.modified,true,'pasted image needs its own save');
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement','new image is ready to edit');
  const publicCopy=await nonce();await key('c');await copied(publicCopy);
  previous=await drawings();selected=await evaluate('String(layerApp.app.document_tabs(0).selected)');
  await editMenu('Paste as New Image','mouse');
  await wait(`layerApp.app.document_tabs(0).tabs.length===${previous.length+1}&&String(layerApp.app.document_tabs(0).selected)!==${JSON.stringify(selected)}&&!layerApp.documents.busy()`);await idle();
  await wait('layerApp.state().tabs[0].width===64&&layerApp.state().tabs[0].height===48');
  assert.equal((await state()).document_file.modified,true,'internal clipboard new image needs its own save');
  const active=() => state().then(s=>s.layers.find(l=>l.editing));
  const original=await active(),wholeCount=await layers();
  await evaluate(`layerApp.dispatch({type:'set_layer_opacity',id:${original.id}n,opacity:.37})`);await settle();
  assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false);
  const wholeBefore=await nonce();await key('c');await copied(wholeBefore);
  await key('v');await wait(`layerApp.state().layers.length===${wholeCount+1}`);await idle();
  const pasted=await active();
  assert.deepEqual([pasted.label,pasted.opacity,pasted.blend],[original.label,Math.fround(.37),original.blend],'whole-layer Paste retains authored layer properties');
  const cutBefore=await nonce();await key('x');await copied(cutBefore);
  await wait(`layerApp.state().layers.length===${wholeCount}`);await idle();
  assert.equal((await state()).layers.some(l=>l.id===pasted.id),false,'whole-layer Cut removes the copied layer');
  await invoke('undo');await wait(`layerApp.state().layers.length===${wholeCount+1}`);
  const authoredRow=({paint_revision,...row})=>row;
  assert.deepEqual(authoredRow((await state()).layers.find(l=>l.id===pasted.id)),authoredRow(pasted),'Undo restores the cut layer');
  await invoke('undo');await wait(`layerApp.state().layers.length===${wholeCount}`);

  await evaluate(`window.clipboardGeometry={save:window.showSaveFilePicker};window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(b){clipboardGeometry.saved=new Uint8Array(b instanceof Blob?await b.arrayBuffer():b)},async close(){},async abort(){}}}})`);
  const save=async()=>{await invoke('save_document_as');await idle();return readPackage(evaluate,'clipboardGeometry.saved');};
  try {
    await invoke('fit_canvas');
    const p=await middle('#canvas');
    await call('Input.dispatchMouseEvent',{type:'mouseWheel',...p,deltaX:80,deltaY:40});await settle();
    const expected=await evaluate(`(()=>{const c=layerApp.app.camera(),[x,y,w,h]=c.work_area;return [(x+w/2-c.translation[0])/c.zoom-32,(y+h/2-c.translation[1])/c.zoom-24].map(Math.round)})()`);
    assert.notDeepEqual(expected,[0,0],'the view has panned away from the copied position');
    await key('v',{shift:true});await wait(`layerApp.state().layers.length===${wholeCount+1}`);await idle();
    assert.notEqual((await state()).canvas_bar?.context.kind,'placement');
    const centered=packageOccurrences(await save()).filter(o=>o.data.content.paint);
    assert.deepEqual((centered[0].data.offset??[0,0]).map(Number),expected,'Ctrl+Shift+V centres retained layers on the panned view');
    await invoke('undo');await wait(`layerApp.state().layers.length===${wholeCount}`);

    await evaluate(`(async()=>{const c=new OffscreenCanvas(128,96),x=c.getContext('2d');x.fillStyle='#e04010';x.fillRect(0,0,128,96);await navigator.clipboard.write([new ClipboardItem({'image/png':await c.convertToBlob()})]);})()`);
    await key('v');await wait(`layerApp.state().canvas_bar?.context.kind==='placement'`);
    await invoke('apply_transform');await idle();
    const manifest=await save(),image=packageObjects(manifest,'capy.image-object/1')[0];
    assert.deepEqual(packageObject(manifest,image.data.image).data.extent,[128,96]);
    assert.deepEqual((image.data.affine??[1,0,0,1,0,0]).slice(0,4),[1,0,0,1],'an oversized external image retains full pixel size');
    await invoke('undo');await wait(`layerApp.state().layers.length===${wholeCount}`);
  } finally {
    await evaluate('window.showSaveFilePicker=clipboardGeometry.save;delete window.clipboardGeometry');
  }
  const retainedLayer=await active(),retainedNonce=await nonce();await key('c');await copied(retainedNonce);
  await invoke('new_document');
  await wait(`!!document.querySelector('dialog[open] [data-document-field="width"]')`);
  await evaluate(`(()=>{const dialog=document.querySelector('dialog[open]');for(const [field,value] of [['width','64'],['height','48']]){const input=dialog.querySelector('[data-document-field='+field+']');input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));}
    dialog.querySelector('[data-document-field="space"]').value='DisplayP3';dialog.querySelector('[data-document-field="depth"]').value='U16';dialog.querySelector('[data-document-action="create"]').click();})()`);
  await wait(`!layerApp.documents.busy()&&layerApp.app.brush_ready()&&layerApp.app.document_color().space==='DisplayP3'&&layerApp.app.document_color().depth==='U16'`);
  const targetCount=await layers();await key('v');await wait(`layerApp.state().layers.length===${targetCount+1}`);await idle();
  const converted=await active();
  assert.deepEqual([converted.label,converted.opacity,converted.blend],[retainedLayer.label,retainedLayer.opacity,retainedLayer.blend],'worker colour conversion preserves retained layer properties');
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement');
  assert.deepEqual(await evaluate('layerApp.app.document_color()'),{space:'DisplayP3',depth:'U16'});
  await invoke('undo');await wait(`layerApp.state().layers.length===${targetCount}`);
  const layer=async action=>send({type:'layer',action});
  await invoke('fit_canvas');await invoke('select_all');
  await send({type:'set_color',rgba:[.8,.1,.2,1]});await invoke('fill_selection');await idle();
  const first=(await active()).id;
  await invoke('add_layer');await send({type:'set_color',rgba:[.1,.7,.2,1]});await invoke('fill_selection');await idle();
  const second=(await active()).id;
  await select('mouse',[8,8,40,32]);await layer({op:'toggle_selection',id:first});
  assert.equal((await state()).layers.filter(l=>l.selected).length,2);
  const regionCount=await layers(),regionBefore=(await state()).layers.filter(l=>[first,second].includes(l.id));
  let prior=await nonce();await key('c');await copied(prior);
  assert.deepEqual((await clipboardPng()).extent,[32,24]);
  await key('v');await wait(`layerApp.state().layers.length===${regionCount+2}`);await idle();
  assert.equal((await state()).layers.filter(l=>l.selected).length,2,'region Paste retains separate selected layers');
  await invoke('undo');await wait(`layerApp.state().layers.length===${regionCount}`);
  prior=await nonce();await key('x');await copied(prior);await idle();
  await wait(`layerApp.state().layers.filter(l=>[${first}n,${second}n].includes(l.id)).every(l=>l.paint_revision!==${JSON.stringify(regionBefore.map(l=>l.paint_revision))}[Number(l.id)===${first}?0:1])`);
  assert.equal(await layers(),regionCount,'region Cut keeps both layers');
  await invoke('undo');await idle();
  await layer({op:'group_selected'});
  prior=await nonce();await key('c');await copied(prior);
  await key('v');await wait(`layerApp.state().layers.length===${regionCount+4}`);await idle();
  assert.equal((await state()).layers.filter(l=>l.group).length,2,'group region Paste keeps both group roots');
  await invoke('undo');await wait(`layerApp.state().layers.length===${regionCount+1}`);
  const groupChildren=(await state()).layers.filter(l=>[first,second].includes(l.id));
  prior=await nonce();await key('x');await copied(prior);await idle();
  await wait(`layerApp.state().layers.filter(l=>[${first}n,${second}n].includes(l.id)).every(l=>l.paint_revision!==${JSON.stringify(groupChildren.map(l=>l.paint_revision))}[Number(l.id)===${first}?0:1])`);
  assert.equal(await layers(),regionCount+1,'group region Cut retains the group and children');
  await invoke('undo');await idle();
  await invoke('undo');await wait(`layerApp.state().layers.length===${regionCount}`);
  await layer({op:'select',id:second,mask:false});await invoke('deselect');
  await layer({op:'add_mask',id:second,replace:false});await layer({op:'select',id:second,mask:true});
  const maskBefore=(await active()),maskRevision=()=>evaluate(`layerApp.state().layers.find(l=>Number(l.id)===${second}).mask_revision`);
  const unchangedPaint=async()=>assert.equal((await active()).paint_revision,maskBefore.paint_revision,'mask clipboard preserves artwork');
  prior=await nonce();await key('c');await copied(prior);
  assert.deepEqual((await clipboardPng()).extent,[64,48]);
  let revision=await maskRevision();prior=await nonce();await key('x');await copied(prior);
  await wait(`layerApp.state().layers.find(l=>Number(l.id)===${second}).mask_revision!==${JSON.stringify(revision)}`);await idle();await unchangedPaint();
  await invoke('undo');await idle();
  revision=await maskRevision();await key('v');
  await wait(`layerApp.state().layers.find(l=>Number(l.id)===${second}).mask_revision!==${JSON.stringify(revision)}`);await idle();await unchangedPaint();
  assert.equal(await layers(),regionCount,'retained mask Paste creates no layer');
  await invoke('undo');await idle();
  await evaluate(`(async()=>{const c=new OffscreenCanvas(32,24),x=c.getContext('2d');x.fillStyle='#808080';x.fillRect(0,0,32,24);await navigator.clipboard.write([new ClipboardItem({'image/png':await c.convertToBlob()})]);})()`);
  revision=await maskRevision();await key('v');
  await wait(`layerApp.state().layers.find(l=>Number(l.id)===${second}).mask_revision!==${JSON.stringify(revision)}`);await idle();await unchangedPaint();
  assert.equal(await layers(),regionCount,'external PNG Paste writes the focused mask');
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement');
  await invoke('undo');await idle();
  console.log('Selected-layer region Copy/Cut/Paste preserves separate layers; focused mask C/X/V and external PNG Paste preserve artwork');
  console.log('Keyboard copy, paste, Copy Merged, Paste Into, text focus, new images, whole-layer Cut/Undo, shown position and full-size external paste');
}
