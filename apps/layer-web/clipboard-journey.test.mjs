import assert from 'node:assert/strict';

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
    await wait(`document.querySelector('.header-menu[data-menu="edit"]').open&&!!${headerRow(label)}&&!${headerRow(label)}.disabled`);
    await tap(await evaluate(`(()=>{const r=${headerRow(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`),kind);
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
    console.log(`${kind}: Copy ▾ › Copy, then Edit › Paste in Place`);
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

  const external=await evaluate(`(async()=>{const c=new OffscreenCanvas(64,48),x=c.getContext('2d');x.fillStyle='#e04010';x.fillRect(0,0,64,48);
    await navigator.clipboard.write([new ClipboardItem({'image/png':await c.convertToBlob()})]);return true;})()`);
  assert.ok(external);
  await key('v');
  await wait(`layerApp.state().layers.length===${count+1}&&layerApp.state().canvas_bar?.context.kind==='placement'`);
  await invoke('cancel_transform');await wait(`layerApp.state().layers.length===${count}`);await idle();
  await key('v',{shift:true});
  await wait(`layerApp.state().layers.length===${count+1}`);await idle();
  assert.notEqual((await state()).canvas_bar?.context.kind,'placement','Paste in Place centres another app\'s image without handles');
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
  console.log('Keyboard copy, paste, Copy Merged, Paste Into, text focus and another app\'s image');
}
