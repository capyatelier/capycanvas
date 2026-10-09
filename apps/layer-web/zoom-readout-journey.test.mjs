import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const readout='#view-info',menu='.zoom-menu';
const opened=`!!document.querySelector('${menu}:popover-open')`;
const canvasFocused='document.activeElement===layerApp.canvas';
const WEBP_LIMIT='WebP export is limited to 16,384 pixels per side';

export async function checkWheelNavigation({call,evaluate,settle}) {
  const camera = () => evaluate('JSON.parse(JSON.stringify(layerApp.state().camera,(_,v)=>typeof v==="bigint"?Number(v):v))');
  const browser = () => evaluate('({dpi:devicePixelRatio,width:innerWidth,height:innerHeight,scale:visualViewport.scale})');
  const send = async action => {await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle();};
  const originalTheme = await evaluate('layerApp.state().settings.theme');
  await send({type:'invoke',command:'fit_canvas'});
  const point = await evaluate('(()=>{const [x,y,w,h]=layerApp.state().camera.work_area,c=layerApp.canvas.getBoundingClientRect(),s=c.width/layerApp.canvas.width;return{x:c.x+(x+w/2)*s,y:c.y+(y+h/2)*s}})()');
  const mouse = (type,button,buttons,p=point) => call('Input.dispatchMouseEvent',{type,...p,button,buttons,clickCount:1});
  const wheel = async(deltaY,modifiers=0,buttons=0,p=point,deltaX=0) => {
    await call('Input.dispatchMouseEvent',{type:'mouseWheel',...p,deltaX,deltaY,modifiers,buttons}); await settle();
  };
  await evaluate(`window.wheelEvents=[];window.addEventListener('wheel',e=>wheelEvents.push({prevented:e.defaultPrevented,target:e.target.id}),false)`);
  const initialBrowser = await browser();
  for (const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    for (const [button,buttons] of [['none',0],['middle',4],['right',2]]) {
      await mouse('mouseMoved','none',0);
      if (buttons) await mouse('mousePressed',button,buttons);
      const before = await camera();
      await wheel(20,0,buttons);
      const panned = await camera();
      assert.ok(panned.translation[1]<before.translation[1],`${theme} ${button}: wheel pans`);
      assert.equal(panned.zoom,before.zoom);
      await wheel(20,8,buttons);
      const horizontal = await camera();
      assert.ok(horizontal.translation[0]<panned.translation[0]);
      assert.equal(horizontal.translation[1],panned.translation[1]);
      await wheel(-40,2,buttons);
      const zoomed = await camera();
      assert.ok(zoomed.zoom>horizontal.zoom,`${theme} ${button}: Ctrl-wheel zooms`);
      assert.equal(zoomed.rotation,horizontal.rotation);
      const physical = await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect();return[(${point.x}-r.x)*layerApp.canvas.width/r.width,(${point.y}-r.y)*layerApp.canvas.height/r.height]})()`);
      for(let axis=0;axis<2;axis++)assert.ok(Math.abs((physical[axis]-horizontal.translation[axis])/horizontal.zoom-(physical[axis]-zoomed.translation[axis])/zoomed.zoom)<.001,'zoom stays anchored');
      await wheel(40,10,buttons);
      assert.ok(Math.abs((await camera()).zoom-horizontal.zoom)<.00001,'Ctrl takes precedence over Shift');
      assert.deepEqual(await browser(),initialBrowser,'canvas scroll never zooms the browser');
      if(buttons) {
        const beforeMove = await camera();
        await mouse('mouseMoved',button,buttons,{x:point.x+10,y:point.y+8}); await settle();
        const moved = await camera();
        assert.ok(moved.translation[0]>beforeMove.translation[0] && moved.translation[1]>beforeMove.translation[1],'held pointer resumes panning after the wheel');
        await mouse('mouseReleased',button,0,{x:point.x+10,y:point.y+8}); await settle();
        const released = await camera();
        await mouse('mouseMoved','none',0); await settle();
        assert.deepEqual(await camera(),released,'release ends the pan contact');
      }
    }
    await send({type:'set_zoom_locked',locked:true});
    const locked = await camera();
    await wheel(-40,2);
    assert.deepEqual(await camera(),locked,'wheel respects view lock');
    await send({type:'set_zoom_locked',locked:false});
    await mouse('mousePressed','left',1); await settle();
    const painting = await camera();
    await wheel(-40,2,1);
    assert.deepEqual(await camera(),painting,'wheel preserves an active paint contact');
    await mouse('mouseReleased','left',0); await settle();
    await send({type:'invoke',command:'undo'});
  }
  assert.ok(await evaluate('wheelEvents.length>20 && wheelEvents.every(e=>e.prevented)'),'native canvas wheel events are cancelled before browser defaults');
  await evaluate(`document.documentElement.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,clientX:${point.x},clientY:${point.y},deltaY:-20,ctrlKey:true}))`);
  assert.equal(await evaluate('wheelEvents.at(-1).prevented'),true,'ancestor-targeted canvas scroll is owned');
  const panel = await evaluate('(()=>{const n=document.querySelector(".dock-group:not(.floating-panel)");const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()');
  const beforePanel = await camera();
  await wheel(20,0,0,panel);
  assert.deepEqual(await camera(),beforePanel,'panel scrolling does not navigate the canvas');
  assert.deepEqual(await browser(),initialBrowser);
  await checkCanvasNavigation({call,evaluate,settle});
  await send({type:'set_theme',theme:originalTheme});
  console.log('PASS: canvas wheel, held middle/right button, modifiers, anchoring, release, view lock, paint exclusion and browser zoom ownership');
}

async function checkCanvasNavigation({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/navigation-controls/web';await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const camera=()=>evaluate('(()=>{const c=layerApp.state().camera;return{zoom:c.zoom,rotation:c.rotation,t:Array.from(c.translation),flipped:Array.from(c.flipped)}})()');
  const artwork=()=>evaluate('({paint:layerApp.state().layers.map(l=>[String(l.id),String(l.paint_revision)]),undo:layerApp.state().commands.find(c=>c.id==="undo").enabled})');
  const tool=()=>evaluate('layerApp.state().layer_tools.tool');
  const key=async(key,code,vk,down,modifiers=0)=>{await call('Input.dispatchKeyEvent',{type:down?'rawKeyDown':'keyUp',key,code,windowsVirtualKeyCode:vk,nativeVirtualKeyCode:vk,modifiers});await settle();};
  const press=async(name,code,vk,modifiers=0)=>{await key(name,code,vk,true,modifiers);await key(name,code,vk,false,modifiers);};
  const doubleTool=async()=>{
    const p=await evaluate('(()=>{const r=document.querySelector(".tile-button button[data-command=hand]").getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()');
    for(const clickCount of [1,2])for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount});
    await settle();
  };
  await evaluate(`window.navigationEvents=[];for(const type of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture'])document.addEventListener(type,e=>navigationEvents.push({type,id:e.pointerId,buttons:e.buttons,button:e.button,target:e.target.id||e.target.className,x:e.clientX,y:e.clientY}),true)`);
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});await invoke('reset_view');await invoke('pen');
    await evaluate('layerApp.canvas.focus()');
    const point=await evaluate('(()=>{const [x,y,w,h]=layerApp.state().camera.work_area,r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(x+w/2)*r.width/layerApp.canvas.width,y:r.y+(y+h/2)*r.height/layerApp.canvas.height}})()');
    const mouse=async(type,p=point,modifiers=0)=>{await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,modifiers});await settle();};
    const kept=await artwork();
    await press('z','KeyZ',90);assert.equal(await tool(),'zoom','Z selects Zoom');
    let before=await camera();await mouse('mousePressed');await mouse('mouseReleased');
    assert.ok((await camera()).zoom>before.zoom,'Zoom click increases magnification');
    const first=(await camera()).zoom;await mouse('mousePressed');await mouse('mouseReleased');
    assert.ok((await camera()).zoom>first,'rapid Zoom canvas clicks keep increasing magnification');
    before=await camera();await mouse('mousePressed');await mouse('mouseMoved',{x:point.x+80,y:point.y+64});await mouse('mouseReleased',{x:point.x+80,y:point.y+64});
    assert.notEqual((await camera()).zoom,before.zoom,`Zoom drag changes magnification: ${JSON.stringify(await evaluate('navigationEvents.slice(-12)'))}`);
    await invoke('reset_view');await invoke('pen');
    const painting=await tool();before=await camera();
    await key('Control','ControlLeft',17,true,2);await key(' ','Space',32,true,2);
    await mouse('mousePressed',point,2);await mouse('mouseMoved',{x:point.x+48,y:point.y},2);
    const zoomed=await camera();assert.ok(zoomed.zoom>before.zoom,'Ctrl+Space drag right zooms in');
    const physical=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect();return[(${point.x}-r.x)*layerApp.canvas.width/r.width,(${point.y}-r.y)*layerApp.canvas.height/r.height]})()`);
    for(let axis=0;axis<2;axis++)assert.ok(Math.abs((physical[axis]-before.t[axis])/before.zoom-(physical[axis]-zoomed.t[axis])/zoomed.zoom)<.01,'drag zoom retains its anchor');
    await key(' ','Space',32,false,2);await key('Control','ControlLeft',17,false);
    await mouse('mouseMoved',{x:point.x+80,y:point.y});assert.ok((await camera()).zoom>zoomed.zoom,'key release keeps captured zoom');
    await mouse('mouseReleased',{x:point.x+80,y:point.y});assert.equal(await tool(),painting,'temporary zoom restores painting');
    before=await camera();await key('Alt','AltLeft',18,true,1);await key(' ','Space',32,true,1);
    await mouse('mousePressed',point,1);await mouse('mouseReleased',point,1);
    await key(' ','Space',32,false,1);await key('Alt','AltLeft',18,false);
    assert.ok((await camera()).zoom<before.zoom,'Alt+Space click zooms out');
    const start={x:point.x+80,y:point.y};
    await key('Shift','ShiftLeft',16,true,8);await key(' ','Space',32,true,8);
    await mouse('mousePressed',start,8);await mouse('mouseMoved',{x:start.x,y:start.y+64},8);
    const rotated=await camera();assert.ok(Math.abs(rotated.rotation)>.01,'Shift+Space rotates');
    await key(' ','Space',32,false,8);await key('Shift','ShiftLeft',16,false);
    await mouse('mouseMoved',{x:point.x,y:point.y+80});await mouse('mouseReleased',{x:point.x,y:point.y+80});
    assert.ok(Math.abs((await camera()).rotation-rotated.rotation)>.01,'key release keeps captured rotation');
    assert.equal(await tool(),painting);
    await press('r','KeyR',82);assert.equal(await tool(),'rotate_view','R selects Rotate View');
    const rotation=(await camera()).rotation;await press('-','Minus',189);assert.notEqual((await camera()).rotation,rotation,'minus rotates left');
    await press('5','Digit5',53);assert.equal((await camera()).rotation,0,'5 resets rotation');
    for(const [name,code,vk,modifiers] of [[';','Semicolon',186,2],['=','Equal',187,2],['+','Equal',187,10],['+','NumpadAdd',107,2]]) {
      before=await camera();await press(name,code,vk,modifiers);assert.ok((await camera()).zoom>before.zoom,`${code} zoom shortcut applies`);
    }
    await send({type:'set_rotation',rotation:.4});await invoke('flip_horizontal');await invoke('fit_canvas');
    let fitted=await camera();assert.ok(Math.abs(fitted.rotation-.4)<.00001,'Fit preserves rotation');assert.equal(fitted.flipped[0],true,'Fit preserves reflection');
    await invoke('reset_rotation');fitted=await camera();assert.equal(fitted.rotation,0);assert.equal(fitted.flipped[0],true,'Reset Rotation preserves reflection');
    await invoke('reset_view');fitted=await camera();assert.equal(fitted.rotation,0);assert.deepEqual(fitted.flipped,[false,false]);
    await invoke('zoom');await send({type:'set_zoom',zoom:.37});await doubleTool();
    assert.equal((await camera()).zoom,1,'double-clicking the Zoom button selects Actual Pixels');assert.equal(await tool(),'zoom');
    await invoke('rotate_view');await send({type:'set_rotation',rotation:.4});await doubleTool();
    assert.equal((await camera()).rotation,0,'double-clicking Rotate View resets rotation');assert.equal(await tool(),'rotate_view');
    await invoke('hand');await send({type:'set_zoom',zoom:2});await doubleTool();
    assert.ok((await camera()).zoom<1,'double-clicking Hand fits');assert.equal(await tool(),'hand');
    assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,'double-click closes the tool drawer');
    assert.deepEqual(await artwork(),kept,'navigation preserves paint and Undo');
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/navigation-${theme}.png`,Buffer.from(shot.data,'base64'));
    await invoke('pen');await mouse('mousePressed');await mouse('mouseMoved',{x:point.x+32,y:point.y+12});await mouse('mouseReleased',{x:point.x+32,y:point.y+12});
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+10000;function check(){const paint=layerApp.state().layers.map(l=>[String(l.id),String(l.paint_revision)]);if(JSON.stringify(paint)!==${JSON.stringify(JSON.stringify(kept.paint))})resolve(true);else if(performance.now()>end)reject(Error('painting after navigation'));else setTimeout(check,30);}check();})`);
    assert.equal((await artwork()).undo,true,'painting resumes after navigation');await invoke('undo');
  }
  console.log('PASS: Zoom tool, temporary zoom/rotation, mid-contact key release, shortcuts, view reset and paint preservation in both themes');
}

export async function checkZoomReadout({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/zoom-readout/web-tablet':'artifacts/zoom-readout/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const waitLong=async(expression,rounds=8)=>{for(let i=1;;i++){try{return await wait(expression);}catch(error){if(i>=rounds)throw error;}}};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const camera=()=>evaluate('(()=>{const c=layerApp.state().camera;return{zoom:c.zoom,rotation:c.rotation,t:Array.from(c.translation),flipped:Array.from(c.flipped),revision:Number(c.revision)}})()');
  const whole=async label=>{const c=await camera();assert.ok(c.t.every(Number.isInteger),`${label}: whole device pixels ${JSON.stringify(c.t)}`);};
  const text=()=>evaluate(`document.querySelector('${readout}').textContent`);
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const row=label=>`[...document.querySelectorAll('${menu} .zoom-menu-items button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const rowMiddle=label=>evaluate(`(()=>{const r=${row(label)}.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  let touchId=90;
  const pointer=(type,p,kind)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}]})
    :call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const press=async(key,code,vk,modifiers=0)=>{for(const type of ['rawKeyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk,nativeVirtualKeyCode:vk,modifiers});};
  const key=async(...args)=>{await press(...args);await settle();};
  const focusCanvas=async()=>{await evaluate('layerApp.canvas.focus()');assert.ok(await evaluate(canvasFocused));};
  const openWith=async kind=>{await tap(await middle(readout),kind);await wait(opened);};
  const choose=async(label,kind)=>{await wait(`!!${row(label)}`);await tap(await rowMiddle(label),kind);};
  const screenshot=async name=>{
    const r=await rect(menu),b=await rect(readout),x=Math.max(0,Math.min(r.x,b.x)-24),y=Math.max(0,r.y-24);
    const clip={x,y,width:Math.max(r.x+r.width,b.x+b.width)+24-x,height:b.y+b.height+24-y,scale:1};
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null');
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  await evaluate(`window.zoomWorkspace=layerApp.state().workspace;if(!zoomWorkspace.layout.canvas_info.visible){const w=layerApp.state().workspace;w.layout.canvas_info={visible:true};layerApp.dispatch({type:'restore_workspace',workspace:w});}`);
  await settle();
  try {
    await wait(`!document.querySelector('#canvas-status').hidden&&document.querySelector('${readout}').getBoundingClientRect().width>0`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    assert.equal(await evaluate(`document.querySelector('${readout}').tagName`),'BUTTON');
    assert.equal(await evaluate(`document.querySelector('${readout}').tabIndex`),-1,'the readout stays out of the tab order');

    for(const [index,kind] of ['mouse','touch','pen'].entries()) {
      await send({type:'set_zoom',zoom:.37});
      await focusCanvas();
      await openWith(kind);
      assert.ok(await evaluate(canvasFocused),`${kind}: opening leaves focus on the canvas`);
      assert.equal(await evaluate(`Number(document.querySelector('${menu} input.number-slider').getAttribute('aria-valuetext')?.replace(/[^0-9.]/g,''))`),37,`${kind}: the field shows the camera zoom`);
      const labels=await evaluate(`[...document.querySelectorAll('${menu} .zoom-menu-items .menu-label')].map(n=>n.textContent)`);
      assert.deepEqual(labels,['Zoom in','Zoom out','Fit canvas','Actual Pixels','25%','50%','100%','200%','400%','Lock zoom','Reset rotation','Lock rotation'],'the shared zoom menu');
      assert.ok(await evaluate(`(()=>{const zoom=document.querySelector('${menu} .number-control'),rotation=document.querySelector('${menu} .rotation-field');return zoom.getBoundingClientRect().height===rotation.getBoundingClientRect().height&&!rotation.innerText.includes('Rotation')&&!rotation.querySelector('.number-step');})()`),'rotation matches the single-row zoom slider without a visible label or step buttons');
      assert.ok(await evaluate(`(()=>{const zoom=document.querySelector('${menu} .number-control').getBoundingClientRect(),rotation=document.querySelector('${menu} .rotation-field').getBoundingClientRect(),lockZoom=${row('Lock zoom')}.getBoundingClientRect(),reset=${row('Reset rotation')}.getBoundingClientRect(),lockRotation=${row('Lock rotation')}.getBoundingClientRect(),buttons=document.querySelector('${menu} .navigator-buttons').getBoundingClientRect();return zoom.bottom<=lockZoom.top&&lockZoom.bottom<=rotation.top&&rotation.bottom<=reset.top&&reset.bottom<=lockRotation.top&&lockRotation.bottom<=buttons.top;})()`),'zoom controls precede rotation controls, with navigation buttons at the bottom');
      if(index===0)for(const name of ['light','dark']){await send({type:'set_theme',theme:name});await pause(150);await screenshot(`zoom-menu-${name}`);}
      await choose('200%',kind);
      await wait(`layerApp.state().camera.zoom===2&&!${opened}`);
      await whole(`${kind} 200%`);
      await wait(`document.querySelector('${readout}').textContent==='200% · 0°'`);
      assert.ok(await evaluate(canvasFocused),`${kind}: choosing a level keeps focus on the canvas`);
    }
    for (const theme of ['light', 'dark']) {
      await send({type:'set_theme',theme});
      await openWith('mouse');
      await choose('Lock rotation','mouse');
      await wait('layerApp.state().camera.rotation_locked');
      await openWith('mouse');
      assert.equal(await evaluate(`${row('Lock rotation')}.getAttribute('aria-checked')`),'true');
      await choose('Lock zoom','mouse');
      await wait('layerApp.state().camera.zoom_locked');
      await openWith('mouse');
      assert.equal(await evaluate(`${row('Lock zoom')}.getAttribute('aria-checked')`),'true');
      const before = await camera();
      await evaluate('void layerApp.app.gesture(100,100,140,120,1.5,.4)');await settle();
      const locked = await camera();
      assert.equal(locked.zoom,before.zoom);assert.equal(locked.rotation,before.rotation);
      for(const id of ['zoom_in','zoom_out','rotate_right','rotate_left','flip_horizontal','flip_vertical']) {
        const before = await camera();
        await tap(await middle(`#zoom-${id}`),'mouse');
        await wait(`layerApp.state().camera.revision>${before.revision}`);
        const after = await camera();
        if(id.startsWith('zoom_'))assert.notEqual(after.zoom,before.zoom);
        else if(id.startsWith('rotate_'))assert.notEqual(after.rotation,before.rotation);
        else assert.notDeepEqual(after.flipped,before.flipped);
        assert.ok(await evaluate(opened));
      }
      await tap(await middle(`${menu} .rotation-field .number-value`),'mouse');
      await call('Input.insertText',{text:'45'});await key('Enter','Enter',13);
      await wait('Math.abs(layerApp.state().camera.rotation-Math.PI/4)<.00001');
      const slider = `${menu} .rotation-field input.number-slider`;
      const r = await rect(slider);
      await tap({x:r.x+r.width*.3,y:r.y+r.height/2},'mouse');
      await wait('Math.abs(layerApp.state().camera.rotation-Math.PI/4)>.01');
      await choose('Reset rotation','mouse');
      await wait(`Math.abs(layerApp.state().camera.rotation)<.00001&&!${opened}`);
      await openWith('mouse');await choose('Lock rotation','mouse');
      await openWith('mouse');await choose('Lock zoom','mouse');
      await invoke('flip_horizontal');await invoke('flip_vertical');
      await send({type:'set_zoom',zoom:.37});
    }
    if(theme)await send({type:'set_theme',theme});

    await send({type:'set_rotation',rotation:Math.PI/2});await send({type:'set_zoom',zoom:.37});
    await openWith('mouse');await choose('Actual Pixels','mouse');
    await wait(`layerApp.state().camera.zoom===1&&!${opened}`);
    await whole('Actual Pixels at a quarter turn');
    assert.equal(await text(),'100% · 90°');
    await send({type:'set_rotation',rotation:0});

    await focusCanvas();
    await openWith('pen');
    await tap(await middle(`${menu} .number-value`),'pen');
    await wait(`document.activeElement===document.querySelector('${menu} .number-entry')`);
    await call('Input.insertText',{text:'50'});
    await key('Enter','Enter',13);
    await wait('layerApp.state().camera.zoom===0.5');
    await whole('typed 50');
    assert.ok(await evaluate(opened),'typing keeps the menu open');
    assert.equal(await text(),'50% · 0°');
    await tap(await middle(`${menu} .number-value`),'mouse');
    await call('Input.insertText',{text:'5000'});
    await key('Enter','Enter',13);
    await wait(`layerApp.state().camera.zoom===16&&${row('Zoom in')}.disabled`);
    await key('Escape','Escape',27);
    await wait(`!${opened}`);
    await wait(canvasFocused);

    await openWith('mouse');
    await key('Escape','Escape',27);
    await wait(`!${opened}`);
    assert.ok(await evaluate(canvasFocused),'Escape hands focus back to the canvas');

    const pages=device?[]:(await call('Target.getTargets',{},null)).targetInfos.filter(t=>t.type==='page');
    const own=pages.find(t=>t.attached),other=pages.find(t=>!t.attached);
    const visible=()=>evaluate('document.visibilityState');
    const idle="layerApp.state().commands.find(c=>c.id==='actual_pixels').enabled";
    const front=async()=>{
      await call('Target.activateTarget',{targetId:own.targetId},null);
      await wait("document.visibilityState==='visible'&&document.hasFocus()");
      await evaluate('layerApp.wake()');await wait(idle);
    };
    if(own&&other) {
      await call('Emulation.setFocusEmulationEnabled',{enabled:false});
      await front();
    }
    await evaluate(`window.zoomKeys=[];window.addEventListener('keydown',e=>{const v={key:e.key};zoomKeys.push(v);setTimeout(()=>{v.prevented=e.defaultPrevented},0)})`);
    for(const [name,chord] of [['Ctrl+1',['1','Digit1',49,2]],['Ctrl+Alt+0',['0','Digit0',48,3]]]) {
      await wait(idle);await send({type:'set_zoom',zoom:.37});await focusCanvas();
      await key(...chord);
      await wait('layerApp.state().camera.zoom===1',20000).catch(async e=>{throw Error(`${name}: ${JSON.stringify(await evaluate('({keys:zoomKeys,focus:document.hasFocus(),vis:document.visibilityState,active:document.activeElement?.id||document.activeElement?.tagName,zoom:layerApp.state().camera.zoom})'))}`)});
      await whole(name);
      assert.equal((await evaluate('zoomKeys.at(-1)')).prevented,true,`${name} is not left to the browser`);
      assert.equal(await visible(),'visible',`${name} keeps this tab in front`);
    }
    if(own&&other) {
      await evaluate(`window.zoomBlock=e=>{if(e.key==='1')e.stopImmediatePropagation()};window.addEventListener('keydown',zoomBlock,true)`);
      await press('1','Digit1',49,2);
      await wait("document.visibilityState==='hidden'",10000);
      await evaluate(`window.removeEventListener('keydown',zoomBlock,true)`);
      await front();
      await call('Emulation.setFocusEmulationEnabled',{enabled:true});
    }

    if(device&&process.env.CAPY_ANDROID_SERIAL) {
      const shell=(...args)=>promisify(execFile)(process.env.ADB??'adb',['-s',process.env.CAPY_ANDROID_SERIAL,'shell',...args]);
      const screen=await evaluate('({ratio:devicePixelRatio,width:screen.width,height:screen.height})');
      const probe=[Math.round(screen.width*screen.ratio/2),Math.round(screen.height*screen.ratio/2)];
      await evaluate(`(()=>{const o=document.createElement('div');o.id='zoom-calibration';o.style.cssText='position:fixed;inset:0;z-index:2147483647';
        o.addEventListener('pointerdown',e=>{window.zoomCalibration={x:e.clientX,y:e.clientY};e.preventDefault();e.stopPropagation();});document.body.append(o);})()`);
      await shell('input','touchscreen','tap',...probe.map(String));
      await wait('window.zoomCalibration');
      const calibration=await evaluate('zoomCalibration');
      await evaluate(`document.querySelector('#zoom-calibration').remove();delete window.zoomCalibration`);
      const offset=[probe[0]-calibration.x*screen.ratio,probe[1]-calibration.y*screen.ratio];
      const physical=p=>[Math.round(offset[0]+p.x*screen.ratio),Math.round(offset[1]+p.y*screen.ratio)].map(String);
      const source={pen:'stylus',touch:'touchscreen',mouse:'mouse'};
      await evaluate(`window.zoomInput=[];document.addEventListener('pointerdown',e=>zoomInput.push({type:e.pointerType,readout:!!e.target.closest('${readout}'),menu:!!e.target.closest('${menu}'),canvas:e.target===layerApp.canvas}),true)`);
      for(const kind of ['touch','pen','mouse']) {
        await send({type:'set_zoom',zoom:.37});await focusCanvas();
        await evaluate('zoomInput.length=0');
        await shell('input',source[kind],'tap',...physical(await middle(readout)));
        await wait(opened);
        assert.ok(await evaluate(canvasFocused),`OS ${kind}: opening leaves focus on the canvas`);
        await shell('input',source[kind],'tap',...physical(await rowMiddle('200%')));
        await wait(`layerApp.state().camera.zoom===2&&!${opened}`);
        await whole(`OS ${kind} 200%`);
        const events=await evaluate('zoomInput');
        assert.ok(events.length>=2&&events.every(e=>e.type===kind&&!e.canvas&&(e.readout||e.menu)),`OS ${kind} taps reach the readout and menu only: ${JSON.stringify(events)}`);
      }
    }

    if (process.argv.includes('--zoom-controls')) {
      console.log('Footer zoom/rotation controls passed: locks, slider, reset, Navigator buttons, mouse/touch/pen, both themes and keyboard focus');
      return;
    }
    await invoke('fit_canvas');
    const files=`window.zoomFiles`;
    await evaluate(`window.zoomPicker=window.showSaveFilePicker;${files}=new Map();window.zoomPicks=0;
      window.showSaveFilePicker=async options=>{zoomPicks++;return{name:options.suggestedName,async createWritable(){let bytes;return{async write(value){bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},async close(){${files}.set(options.suggestedName,bytes)},async abort(){}}}}};`);
    const exportAs=async(format,{expectError=null,destination=null}={})=>{
      await invoke('export_document');
      await wait(`!!document.querySelector('dialog[open] select[aria-label="Format"]')`);
      if(destination!==null) {
        await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Destination"]');s.value=${JSON.stringify(String(destination))};s.dispatchEvent(new Event('change'));})()`);
        await wait(`document.querySelector('dialog[open] select[aria-label="Pixel size"]').value==='Fit'&&!document.querySelector('dialog[open] select[aria-label="Format"]').disabled`);
      }
      await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Format"]');s.value=${JSON.stringify(format)};s.dispatchEvent(new Event('change'));})()`);
      const depth=await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Bit depth"]');s.value='U8';s.dispatchEvent(new Event('change'));return [...s.options].filter(o=>!o.disabled).map(o=>o.value)})()`);
      if(format==='Webp')assert.deepEqual(depth,['U8'],'WebP is 8-bit only');
      await evaluate(`(()=>{const s=document.querySelector('dialog[open] select[aria-label="Dither"]');s.value='None';s.dispatchEvent(new Event('change'));})()`);
      const picks=await evaluate('zoomPicks');
      await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Choose File…').click()`);
      if(expectError) {
        await wait(`document.querySelector('dialog[open] .error-message')?.textContent.includes(${JSON.stringify(expectError)})`);
        assert.equal(await evaluate('zoomPicks'),picks,'refused before choosing a file or rendering');
        await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Cancel').click()`);
        await wait(`!document.querySelector('dialog[open]')&&!layerApp.state().document_file.busy`);
        return null;
      }
      const extension=format==='Webp'?'webp':'png';
      await waitLong(`[...${files}.keys()].some(k=>k.endsWith('.${extension}'))&&!layerApp.state().document_file.busy`);
      return [...(await evaluate(`[...${files}.entries()].filter(([k])=>k.endsWith('.${extension}')).map(([k])=>k)`))].at(-1);
    };
    await invoke('pen');await wait('layerApp.app.brush_ready()');
    const c=await middle('#canvas');
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...c,button:'left',buttons:1,clickCount:1,pointerType:'pen',force:.7});
    for(let i=1;i<=12;i++)await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:c.x+i*12,y:c.y+i*5,button:'left',buttons:1,pointerType:'pen',force:.7});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:c.x+144,y:c.y+60,button:'left',buttons:0,clickCount:1,pointerType:'pen'});
    await wait("layerApp.state().commands.find(c=>c.id==='undo').enabled");
    const painted=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/paint-before-export.png`,Buffer.from(painted.data,'base64'));
    const webp=await exportAs('Webp'),png=await exportAs('Png');
    const decoded=await evaluate(`(async()=>{
      const bytes=${files}.get(${JSON.stringify(webp)}),ascii=a=>String.fromCharCode(...a);
      const decode=async(data,type)=>{const bitmap=await createImageBitmap(new Blob([data],{type}));const c=new OffscreenCanvas(bitmap.width,bitmap.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(bitmap,0,0);return{width:bitmap.width,height:bitmap.height,pixels:x.getImageData(0,0,bitmap.width,bitmap.height).data};};
      const a=await decode(bytes,'image/webp'),b=await decode(${files}.get(${JSON.stringify(png)}),'image/png');
      let diff=0,ink=0;for(let i=0;i<a.pixels.length;i++){diff=Math.max(diff,Math.abs(a.pixels[i]-b.pixels[i]));}
      for(let i=0;i<a.pixels.length;i+=4)if([0,1,2,3].some(j=>Math.abs(a.pixels[i+j]-a.pixels[j])>32))ink++;
      return{riff:ascii(bytes.slice(0,4)),webp:ascii(bytes.slice(8,12)),lossless:ascii(bytes).includes('VP8L'),icc:ascii(bytes).includes('ICCP'),
        size:[a.width,a.height],png:[b.width,b.height],document:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height],diff,ink};
    })()`);
    await writeFile(`${directory}/export.webp`,Buffer.from(await evaluate(`Array.from(${files}.get(${JSON.stringify(webp)}))`)));
    await writeFile(`${directory}/export.png`,Buffer.from(await evaluate(`Array.from(${files}.get(${JSON.stringify(png)}))`)));
    assert.equal(webp.split('.').at(-1),'webp');
    assert.deepEqual([decoded.riff,decoded.webp,decoded.lossless,decoded.icc],['RIFF','WEBP',true,true],'a lossless WebP with its profile');
    assert.deepEqual(decoded.size,decoded.document,'the WebP decodes at the document size');
    assert.deepEqual(decoded.png,decoded.document);
    assert.ok(decoded.ink>20,`the stroke is in the export: ${decoded.ink}`);
    assert.ok(decoded.diff<=1,`the WebP matches the 8-bit PNG export: ${decoded.diff}`);

    const poster=await evaluate(`(async()=>{const base=(await layerApp.app.export_presets({type:'get',index:0})).recipe;
      const recipe=layerApp.app.export_draft({...base,size:{Fit:{bounds:[20000,20000],enlarge:true}}},{type:'format',value:'Webp'}).recipe;
      return Number((await layerApp.app.export_presets({type:'save',name:'Poster WebP',recipe})).index)})()`);
    try { await exportAs('Webp',{expectError:WEBP_LIMIT,destination:poster}); }
    finally { await evaluate(`layerApp.app.export_presets({type:'remove',index:${poster}}).then(()=>true)`); }
    console.log(`Zoom readout (mouse, touch, pen${device&&process.env.CAPY_ANDROID_SERIAL?', OS taps':''}), Actual Pixels, typed zoom, chords and WebP export passed`);
  } finally {
    await evaluate(`if(window.zoomPicker!==undefined)window.showSaveFilePicker=zoomPicker;document.querySelector('${menu}:popover-open')?.hidePopover();if(!zoomWorkspace.layout.canvas_info.visible)layerApp.dispatch({type:'restore_workspace',workspace:zoomWorkspace})`);
    if(theme)await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
  }
}
