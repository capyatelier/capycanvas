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
          await contact(device,target);
          assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),false,'one Capy tap exits Zen');
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
    console.log('PASS: Zen defaults, settings controls, all four options in both themes, touch/mouse/pen edge contacts, one-tap Capy exit, keyboard recovery, customized title bar, unchanged layout/camera');
  } catch (error) {
    console.error('Zen failure state:', await evaluate(`JSON.stringify({zen:layerApp.state().workspace.zen_mode,settings:layerApp.state().settings,settingsOpen:layerApp.state().settings_open,commands:layerApp.state().commands.filter(c=>c.id==='zen_mode'),status:document.querySelector('#status').textContent,hidden:document.querySelector('#workspace').classList.contains('zen-hidden'),focus:document.activeElement?.outerHTML.slice(0,160),popups:[...document.querySelectorAll('details[open],:popover-open:not(.hover-tooltip),dialog[open]')].map(n=>n.outerHTML.slice(0,160))})`));
    await capture('failure');
    throw error;
  } finally {
    await send({type:'close_settings'});
    await send({type:'restore_workspace',workspace:saved.workspace});
    await send({type:'restore_settings',settings:saved.settings});
  }
}
