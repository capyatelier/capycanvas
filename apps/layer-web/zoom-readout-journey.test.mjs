import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const readout='#view-info',menu='.zoom-menu';
const opened=`!!document.querySelector('${menu}:popover-open')`;
const canvasFocused='document.activeElement===layerApp.canvas';
const WEBP_LIMIT='WebP export is limited to 16,384 pixels per side';

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

    await invoke('rotate_right');await send({type:'set_zoom',zoom:.37});
    await openWith('mouse');await choose('Actual Pixels','mouse');
    await wait(`layerApp.state().camera.zoom===1&&!${opened}`);
    await whole('Actual Pixels at a quarter turn');
    assert.equal(await text(),'100% · 90°');
    await invoke('rotate_left');

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
    const webp=await exportAs('Webp'),png=await exportAs('Png');
    const decoded=await evaluate(`(async()=>{
      const bytes=${files}.get(${JSON.stringify(webp)}),ascii=a=>String.fromCharCode(...a);
      const decode=async(data,type)=>{const bitmap=await createImageBitmap(new Blob([data],{type}));const c=new OffscreenCanvas(bitmap.width,bitmap.height),x=c.getContext('2d');x.drawImage(bitmap,0,0);return{width:bitmap.width,height:bitmap.height,pixels:x.getImageData(0,0,bitmap.width,bitmap.height).data};};
      const a=await decode(bytes,'image/webp'),b=await decode(${files}.get(${JSON.stringify(png)}),'image/png');
      let diff=0,ink=0;for(let i=0;i<a.pixels.length;i++){diff=Math.max(diff,Math.abs(a.pixels[i]-b.pixels[i]));}
      for(let i=0;i<a.pixels.length;i+=4)if([0,1,2,3].some(j=>Math.abs(a.pixels[i+j]-a.pixels[j])>32))ink++;
      return{riff:ascii(bytes.slice(0,4)),webp:ascii(bytes.slice(8,12)),lossless:ascii(bytes).includes('VP8L'),icc:ascii(bytes).includes('ICCP'),
        size:[a.width,a.height],png:[b.width,b.height],document:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height],diff,ink};
    })()`);
    assert.equal(webp.split('.').at(-1),'webp');
    assert.deepEqual([decoded.riff,decoded.webp,decoded.lossless,decoded.icc],['RIFF','WEBP',true,true],'a lossless WebP with its profile');
    assert.deepEqual(decoded.size,decoded.document,'the WebP decodes at the document size');
    assert.deepEqual(decoded.png,decoded.document);
    assert.ok(decoded.ink>20,`the stroke is in the export: ${decoded.ink}`);
    assert.ok(decoded.diff<=1,`the WebP matches the 8-bit PNG export: ${decoded.diff}`);
    await writeFile(`${directory}/export.webp`,Buffer.from(await evaluate(`Array.from(${files}.get(${JSON.stringify(webp)}))`)));

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
