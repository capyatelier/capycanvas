import assert from 'node:assert/strict';

// Focused regression for the sole Zen mode, independent of a workspace's
// optional panels or its user-customized layout.
export async function checkZen({call, evaluate, settle}) {
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const invoke = command => send({type:'invoke', command});
  const hidden = () => evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')");
  const motion = async (x,y) => { await call('Input.dispatchMouseEvent', {type:'mouseMoved',x,y}); await settle(); };
  const tap = async () => {
    await call('Input.dispatchTouchEvent', {type:'touchStart',touchPoints:[{x:24,y:24}]});
    await call('Input.dispatchTouchEvent', {type:'touchEnd',touchPoints:[]}); await settle();
  };
  for (const theme of ['dark','light']) {
    await send({type:'set_theme',theme});
    await send({type:'restore_settings',settings:{...await evaluate('layerApp.state().settings'),total_zen:false}});
    assert.equal(await evaluate("'total_zen' in layerApp.state().settings"), false);
    await send({type:'open_settings',page:'appearance'});
    assert.equal(await evaluate("document.querySelector('#setting-total-zen')"), null);
    await send({type:'close_settings'});
    const saved = await evaluate('layerApp.state().workspace.layout');
    await invoke('zen_mode'); await motion(600,450);
    assert.ok(await hidden());
    assert.equal(await evaluate("getComputedStyle(document.querySelector('#zen-button')).pointerEvents"), 'none');
    assert.equal(await evaluate("document.querySelector('.zen-toolbar')"), null);
    await motion(6,6); assert.equal(await hidden(), false);
    await invoke('zen_mode');
    await motion(600,450);
    for (const enabled of [true,true,false]) {
      await tap();
      assert.equal(await evaluate('layerApp.state().workspace.zen_mode'),enabled,'hidden controls reveal before activation');
    }
    assert.deepEqual(await evaluate('layerApp.state().workspace.layout'), saved);
  }
  console.log('PASS: total Zen, removed legacy preference, edge reveal, touch recovery and unchanged workspace layout in both themes');
}
