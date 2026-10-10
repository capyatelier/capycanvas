import assert from 'node:assert/strict';

// Runs against desktop Chrome or an explicitly selected tablet test tab.
export async function checkZen({call, evaluate, settle, capture = async () => {}}) {
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const invoke = command => send({type:'invoke', command});
  const hidden = () => evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')");
  const capy = () => evaluate("!document.querySelector('#zen-capy').hidden");
  const point = selector => evaluate(`(() => {const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'center'});const r=n.getBoundingClientRect();return [r.x+r.width/2,r.y+r.height/2]})()`);
  const contact = async (device, [x,y]) => {
    if (device === 'touch') {
      await call('Input.dispatchTouchEvent', {type:'touchStart',touchPoints:[{x,y}]});
      await call('Input.dispatchTouchEvent', {type:'touchEnd',touchPoints:[]});
    } else {
      for (const type of ['mousePressed','mouseReleased']) await call('Input.dispatchMouseEvent', {
        type,x,y,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1,pointerType:device,
      });
    }
    // Android Chrome may delay the synthesized click to arbitrate double taps.
    await evaluate('new Promise(r=>setTimeout(r,350))'); await settle();
  };
  const motion = async position => { await call('Input.dispatchMouseEvent', {type:'mouseMoved',x:position[0],y:position[1]}); await settle(); };
  // Sample the rendered ancestor opacity while exiting, before contact()'s
  // click-settlement delay can hide a brief fade-to-invisible regression.
  const watchCapyExit = () => evaluate(`(() => {
    window.zenExitFrames = new Promise(resolve => {
      const frames = [], deadline = performance.now() + 2000;
      let exitTime;
      const opacity = node => {
        if (!node?.getClientRects().length) return 0;
        let value = 1;
        for (; node; node = node.parentElement) value *= Number(getComputedStyle(node).opacity);
        return value;
      };
      const sample = () => {
        const now = performance.now();
        if (!layerApp.state().workspace.zen_mode) {
          exitTime ??= now;
          frames.push({capy:opacity(document.querySelector('#header #zen-button')),
            controls:opacity(document.querySelector('#header [data-header-item]:not([data-kind="capy"])'))});
        }
        if (now >= deadline || (exitTime != null && now - exitTime >= 220)) resolve(frames);
        else requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
    return true;
  })()`);
  const checkCapyExit = async label => {
    const frames = await evaluate('window.zenExitFrames');
    assert.ok(frames.length > 1, `${label}: sampled the Zen exit transition`);
    assert.ok(frames.every(frame => frame.capy >= .999),
      `${label}: Capy stays opaque throughout exit: ${JSON.stringify(frames)}`);
    assert.ok(frames.some(frame => frame.controls > 0 && frame.controls < .99),
      `${label}: other header controls still fade in`);
  };
  const rectangles = selector => evaluate(`(() => {const n=document.querySelector(${JSON.stringify(selector)}),b=n.getBoundingClientRect(),g=n.querySelector('svg').getBoundingClientRect();return [[b.x,b.y,b.width,b.height],[g.x,g.y,g.width,g.height]]})()`);
  const watchRectangles = () => evaluate(`(() => {
    window.zenGeometryFrames = new Promise(resolve => {
      const frames=[], deadline=performance.now()+280;
      const sample=()=>{
        const zen=layerApp.state().workspace.zen_mode;
        const n=document.querySelector(zen?'#zen-capy':'#header #zen-button');
        if(n&&!n.hidden&&n.getClientRects().length){
          const b=n.getBoundingClientRect(),g=n.querySelector('svg').getBoundingClientRect();
          frames.push({zen,rects:[[b.x,b.y,b.width,b.height],[g.x,g.y,g.width,g.height]]});
        }
        if(performance.now()>=deadline)resolve(frames);else requestAnimationFrame(sample);
      };requestAnimationFrame(sample);
    });return true;
  })()`);
  const checkRectangles = async (expected,label,zen) => {
    const frames=await evaluate('window.zenGeometryFrames');
    assert.ok(frames.some(frame=>frame.zen===zen),`${label}: transition produced frames`);
    for(const frame of frames)assert.deepEqual(frame.rects,expected,`${label}: frame zen=${frame.zen}`);
  };
  const switchWorkspace = async name => {
    const id='builtin:workspace:'+name;
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:${JSON.stringify(id)}}));null`);
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function check(){const w=JSON.parse(layerApp.app.workspace_view());if(w.id===${JSON.stringify(id)}&&w.ready&&!w.busy)resolve();else if(performance.now()>end)reject(Error('workspace switch timeout'));else setTimeout(check,30)}check()})`);
    await settle();
  };
  const saved = await evaluate('({settings:layerApp.state().settings,workspace:layerApp.state().workspace,id:JSON.parse(layerApp.app.workspace_view()).id,viewport:[innerWidth,innerHeight,devicePixelRatio],direction:document.documentElement.dir})');
  const exit = async () => { if(await evaluate('layerApp.state().workspace.zen_mode')) await invoke('zen_mode'); };
  try {
    await exit();
    await send({type:'restore_settings',settings:{}});
    assert.equal(await evaluate('layerApp.state().settings.zen_show_capy'),true);
    assert.equal(await evaluate("'zen_reveal_at_edges' in layerApp.state().settings"),false);
    for (const preset of ['painter','illustrator','photographer']) {
      await switchWorkspace(preset);
      const initial=await evaluate('layerApp.state().workspace');
      for (const theme of ['dark','light']) for (const size of ['small','medium','large']) for (const show of [true,false]) {
      await send({type:'restore_workspace',workspace:initial});
      await send({type:'set_theme',theme});
      await send({type:'open_settings',page:'appearance'});
      await send({type:'customize',action:{type:'header',action:{type:'set_size',size}}});
      assert.equal(await evaluate("document.querySelector('#setting-zen-reveal-at-edges')"),null);
      if (await evaluate('layerApp.state().settings.zen_show_capy') !== show)
        await contact('touch', await point('#setting-zen-show-capy'));
      assert.equal(await evaluate('layerApp.state().settings.zen_show_capy'),show);
      await capture(`settings-${theme}-${show}-${size}`);
      await send({type:'close_settings'});
      const normal = await rectangles('#zen-button');
      const layout = await evaluate('layerApp.state().workspace.layout');
      const camera = await evaluate('JSON.stringify(layerApp.state().camera,(_,v)=>typeof v==="bigint"?String(v):v)');
      const [width,height] = await evaluate('[innerWidth,innerHeight]');
      const center = [width/2,height/2], edge = [width/2,6];
      for (const device of ['touch','mouse','pen']) {
        if(show)await watchRectangles();
        await invoke('zen_mode'); await motion(center);
        if(show)await checkRectangles(normal,`${preset}/${theme}/${size}/${device}: entry`,true);
        await evaluate('new Promise(r=>setTimeout(r,220))');
        assert.ok(await hidden()); assert.equal(await capy(),show);
        await capture(`zen-${theme}-${show}-${size}-${device}`);
        await motion(edge);
        await contact(device,edge);
        assert.ok(await hidden(),`${theme}/${show}/${device}: edges keep chrome hidden`);
        if (show) assert.deepEqual(await rectangles('#zen-capy'),normal,`${preset}/${theme}/${size}: button and glyph full bounds match`);
        if (show) {
          const target = await point('#zen-capy');
          if (device === 'mouse') await motion(target);
          await watchCapyExit();
          await watchRectangles();
          await contact(device,target);
          assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),false,'one Capy tap exits Zen');
          await checkCapyExit(`${preset}/${theme}/${size}/${device}`);
          await checkRectangles(normal,`${preset}/${theme}/${size}/${device}: exit`,false);
        } else {
          await evaluate('document.activeElement?.blur()');
          await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
          await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
          await settle();
          assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),false);
        }
        assert.equal(await hidden(),false); assert.equal(await capy(),false);
        assert.deepEqual(await evaluate('layerApp.state().workspace.layout'),layout);
        assert.deepEqual(await evaluate('JSON.stringify(layerApp.state().camera,(_,v)=>typeof v==="bigint"?String(v):v)'),camera);
      }
    }
    }
    await switchWorkspace('painter');
    await invoke('new_document');
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function check(){const dialog=document.querySelector('dialog[open]'),button=dialog&&[...dialog.querySelectorAll('button')].find(n=>n.textContent==='Create');if(button){for(const input of dialog.querySelectorAll('input[type=number]'))input.value=96;button.click();resolve();}else if(performance.now()>end)reject(Error('new document dialog'));else setTimeout(check,30)}check()})`);
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(layerApp.app.document_tabs(0).tabs.length>1&&!layerApp.documents.busy()&&layerApp.app.brush_ready())resolve();else if(performance.now()>end)reject(Error('second document ready'));else setTimeout(check,30)}check()})`);
    const customBase=await evaluate('layerApp.state().workspace');
    const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:false});await settle();};
    for(const direction of ['ltr','rtl'])for(const theme of ['dark','light'])for(const size of ['small','medium','large'])for(const kind of ['left','center','right','removed','recovery','overflow']){
      await evaluate(`document.documentElement.dir=${JSON.stringify(direction)}`);
      const workspace=structuredClone(customBase),header=workspace.layout.header;header.size=size;
      const entry=header.zones.flat().find(e=>e.item.kind==='capy');
      for(const zone of header.zones){const index=zone.indexOf(entry);if(index>=0)zone.splice(index,1);}
      if(['left','center','right'].includes(kind))header.zones[['left','center','right'].indexOf(kind)].push(entry);
      if(kind==='recovery')for(const zone of header.zones)for(let i=zone.length-1;i>=0;i--)if(['menu','menu_labels','workspaces'].includes(zone[i].item.kind))zone.splice(i,1);
      if(kind==='overflow'){for(let i=0;i<30;i++)header.zones[0].push({id:header.next_id++,item:{kind:'tool',control:{kind:'command',command:'pencil'}}});header.zones[0].push(entry);}
      await resize(kind==='overflow'?480:1200);
      await send({type:'restore_workspace',workspace});await send({type:'set_theme',theme});
      const visible=await evaluate("(() => {const n=document.querySelector('#header #zen-button');return !!n?.getClientRects().length})()");
      const normal=visible?await rectangles('#zen-button'):null;
      if(kind==='overflow')assert.equal(visible,false,`${direction}/${size}: Capy overflows`);
      if(kind==='recovery')assert.ok(await evaluate("document.querySelector('#header-recovery')?.getClientRects().length"));
      await send({type:'restore_settings',settings:{...saved.settings,zen_show_capy:true}});
      if(normal)await watchRectangles();
      await invoke('zen_mode');
      const actual=await rectangles('#zen-capy');
      if(normal){assert.deepEqual(actual,normal,`${direction}/${theme}/${size}/${kind}: full bounds`);await checkRectangles(normal,`${direction}/${theme}/${size}/${kind}: entry`,true);}
      else{const spec=await evaluate('layerApp.app.header_view().sizes.find(s=>s.id===layerApp.state().workspace.layout.header.size)');assert.deepEqual(actual[0],[spec.item_gap,spec.item_gap,spec.tile,spec.tile],`${direction}/${theme}/${size}/${kind}: fallback`);}
      if(normal)await watchRectangles();
      await contact('touch',await point('#zen-capy'));
      if(normal)await checkRectangles(normal,`${direction}/${theme}/${size}/${kind}: exit`,false);
      assert.equal(await hidden(),false);
    }
    await evaluate("document.documentElement.dir='ltr'");
    const restored=structuredClone(customBase);restored.zen_mode=true;restored.layout.header.size='medium';
    await resize(1200);
    await send({type:'restore_workspace',workspace:restored});
    for(const [width,size] of [[1200,'medium'],[480,'large']]){
      await resize(width);
      await send({type:'customize',action:{type:'header',action:{type:'set_size',size}}});
      assert.ok(await hidden());assert.ok(await capy());
      assert.deepEqual(await rectangles('#zen-capy'),await rectangles('#zen-button'),`restored Zen/hidden resize ${width}/${size}: full bounds`);
    }
    const retained=await rectangles('#zen-capy');
    await contact('touch',await point('#zen-capy'));
    assert.equal(await hidden(),false);
    assert.deepEqual(await rectangles('#zen-button'),retained,'resized restored Zen exits without moving');
    await evaluate(`document.documentElement.dir=${JSON.stringify(saved.direction)}`);
    await resize(saved.viewport[0]);
    await send({type:'restore_workspace',workspace:saved.workspace});
    // The fallback is independent of title-bar customization, including no Capy.
    const workspace = await evaluate('layerApp.state().workspace');
    for (const zone of workspace.layout.header.zones) for (let i=zone.length-1;i>=0;i--)
      if (zone[i].item.kind === 'capy') zone.splice(i,1);
    await send({type:'restore_workspace',workspace});
    await send({type:'restore_settings',settings:{}});
    await invoke('zen_mode'); assert.ok(await capy());
    await contact('touch',await point('#zen-capy')); assert.equal(await hidden(),false);
    assert.equal(await evaluate("document.querySelector('#header #zen-button')"),null);
    try {
      await call('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
      await invoke('zen_mode');
      assert.equal(await evaluate(`getComputedStyle(document.querySelector('#header [data-header-item]:not([data-kind="capy"])')).opacity`),'0');
      await invoke('zen_mode');
      assert.equal(await evaluate(`getComputedStyle(document.querySelector('#header [data-header-item]:not([data-kind="capy"])')).opacity`),'1',
        'reduced motion reveals the other header controls immediately');
    } finally {
      await call('Emulation.setEmulatedMedia',{features:[]});
    }
    console.log('PASS: Zen defaults, settings controls, Sketch/Paint/Photo full button and glyph bounds through entry/exit, all sizes/themes, RTL, custom zones, removal/recovery, narrow overflow, restored Zen/hidden resize, touch/mouse/pen exits, edges stay hidden, one-tap Capy exit without an opacity dip, other header controls still fade, keyboard recovery, customized title bar, unchanged layout/camera');
  } catch (error) {
    console.error('Zen failure state:', await evaluate(`JSON.stringify({zen:layerApp.state().workspace.zen_mode,settings:layerApp.state().settings,settingsOpen:layerApp.state().settings_open,commands:layerApp.state().commands.filter(c=>c.id==='zen_mode'),status:document.querySelector('#status').textContent,hidden:document.querySelector('#workspace').classList.contains('zen-hidden'),focus:document.activeElement?.outerHTML.slice(0,160),popups:[...document.querySelectorAll('details[open],:popover-open:not(.hover-tooltip),dialog[open]')].map(n=>n.outerHTML.slice(0,160))})`));
    await capture('failure');
    throw error;
  } finally {
    await evaluate(`delete window.zenExitFrames;delete window.zenGeometryFrames;document.documentElement.dir=${JSON.stringify(saved.direction)}`);
    await call('Emulation.setDeviceMetricsOverride',{width:saved.viewport[0],height:saved.viewport[1],deviceScaleFactor:saved.viewport[2],mobile:false});
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:${JSON.stringify(saved.id)}}));null`);
    await send({type:'close_settings'});
    await send({type:'restore_workspace',workspace:saved.workspace});
    await send({type:'restore_settings',settings:saved.settings});
  }
}
