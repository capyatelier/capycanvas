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
            controls:opacity(document.querySelector('#header [data-kind="settings"]'))});
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
  const saved = await evaluate('({settings:layerApp.state().settings,workspace:layerApp.state().workspace})');
  const exit = async () => { if(await evaluate('layerApp.state().workspace.zen_mode')) await invoke('zen_mode'); };
  try {
    await exit();
    await send({type:'restore_settings',settings:{}});
    assert.equal(await evaluate('layerApp.state().settings.zen_show_capy'),true);
    assert.equal(await evaluate('layerApp.state().settings.zen_reveal_at_edges'),false);
    for (const theme of ['dark','light']) for (const show of [true,false]) for (const edges of [false,true]) {
      await send({type:'set_theme',theme});
      await send({type:'open_settings',page:'appearance'});
      for (const [id,value] of [['zen_show_capy',show],['zen_reveal_at_edges',edges]]) {
        if (await evaluate(`layerApp.state().settings.${id}`) !== value)
          await contact('touch', await point(`#setting-${id.replaceAll('_','-')}`));
        assert.equal(await evaluate(`layerApp.state().settings.${id}`),value);
      }
      await capture(`settings-${theme}-${show}-${edges}`);
      await send({type:'close_settings'});
      const layout = await evaluate('layerApp.state().workspace.layout');
      const camera = await evaluate('JSON.stringify(layerApp.state().camera,(_,v)=>typeof v==="bigint"?String(v):v)');
      const [width,height] = await evaluate('[innerWidth,innerHeight]');
      const center = [width/2,height/2], edge = [width/2,6];
      for (const device of ['touch','mouse','pen']) {
        await invoke('zen_mode'); await motion(center);
        await evaluate('new Promise(r=>setTimeout(r,220))');
        assert.ok(await hidden()); assert.equal(await capy(),show);
        await capture(`zen-${theme}-${show}-${edges}-${device}`);
        await contact(device,edge);
        assert.equal(await hidden(),!edges,`${theme}/${show}/${device}: edge reveal ${edges}`);
        if (edges) {
          assert.ok(await evaluate('layerApp.state().workspace.zen_mode'),'revealing does not exit Zen');
          await motion(center); assert.ok(await hidden());
        }
        if (show) {
          const target = await point('#zen-capy');
          if (device === 'mouse') await motion(target);
          await watchCapyExit();
          await contact(device,target);
          assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),false,'one Capy tap exits Zen');
          await checkCapyExit(`${theme}/${edges}/${device}`);
        } else {
          // Keyboard recovery remains available even with both switches off.
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
      assert.equal(await evaluate(`getComputedStyle(document.querySelector('#header [data-kind="settings"]')).opacity`),'0');
      await invoke('zen_mode');
      assert.equal(await evaluate(`getComputedStyle(document.querySelector('#header [data-kind="settings"]')).opacity`),'1',
        'reduced motion reveals the other header controls immediately');
    } finally {
      await call('Emulation.setEmulatedMedia',{features:[]});
    }
    console.log('PASS: Zen defaults, settings controls, all four options in both themes, touch/mouse/pen edge contacts, one-tap Capy exit without an opacity dip, other header controls still fade, keyboard recovery, customized title bar, unchanged layout/camera');
  } catch (error) {
    console.error('Zen failure state:', await evaluate(`JSON.stringify({zen:layerApp.state().workspace.zen_mode,settings:layerApp.state().settings,settingsOpen:layerApp.state().settings_open,commands:layerApp.state().commands.filter(c=>c.id==='zen_mode'),status:document.querySelector('#status').textContent,hidden:document.querySelector('#workspace').classList.contains('zen-hidden'),focus:document.activeElement?.outerHTML.slice(0,160),popups:[...document.querySelectorAll('details[open],:popover-open:not(.hover-tooltip),dialog[open]')].map(n=>n.outerHTML.slice(0,160))})`));
    await capture('failure');
    throw error;
  } finally {
    await evaluate('delete window.zenExitFrames');
    await send({type:'close_settings'});
    await send({type:'restore_workspace',workspace:saved.workspace});
    await send({type:'restore_settings',settings:saved.settings});
  }
}
