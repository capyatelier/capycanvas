import assert from 'node:assert/strict';

import {createDrawing} from './new-drawing-fixture.mjs';
import {vectorHarness} from './vector-journey-fixture.mjs';

export async function checkEraserBubbles({call, evaluate, settle}) {
  const dir = process.env.LAYER_TEST_ARTIFACTS ?? 'artifacts/eraser-bubbles/web';
  const {pause, send, invoke, capture} = await vectorHarness({call, evaluate, settle}, {directory: dir});
  const saved = await evaluate('layerApp.app.workspace_persistence()'),
        savedId = await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  let original;
  let device = 'mouse', lastRawUp = 0;
  const wait = async (e, ms = 30000) => {
    const end = Date.now() + ms;
    while (!await evaluate(e)) {
      if (Date.now() > end) throw Error(e);
      await pause(30);
    }
  };
  const pointer = async (type, p, kind = device) => {
    if (kind === 'touch')
      await call('Input.dispatchTouchEvent', {
        type: {down: 'touchStart', move: 'touchMove', up: 'touchEnd'}[type],
        touchPoints: type === 'up' ? [] : [{id: 1, x: p[0], y: p[1]}]
      });
    else
      await call('Input.dispatchMouseEvent', {
        type: {down: 'mousePressed', move: 'mouseMoved', up: 'mouseReleased'}[type],
        x: p[0],
        y: p[1],
        button: 'left',
        buttons: type === 'up' ? 0 : 1,
        clickCount: 1,
        pointerType: kind,
        force: type === 'up' ? 0 : .7
      });
    if (type === 'up') lastRawUp = Date.now();
  };
  const stroke = async (points, kind = device) => {
    await pointer('down', points[0], kind);
    for (const p of points.slice(1)) {
      await pointer('move', p, kind);
      await settle();
    }
    await pointer('up', points.at(-1), kind);
    await settle();
    await pause(250);
  };
  const at = selector => evaluate(`(()=>{const n=document.querySelector(${
      JSON.stringify(selector)}),r=n?.getBoundingClientRect();if(!r?.width||!r.height)throw Error('Missing '+${
      JSON.stringify(selector)});return[r.x+r.width/2,r.y+r.height/2]})()`);
  const tap = async p => {
    const nativeButton = device === 'touch' &&
        await evaluate(`!!document.elementFromPoint(${p[0]},${p[1]})?.closest('button,summary,input,select')`);
    if (nativeButton) await pause(Math.max(0, 400 - (Date.now() - lastRawUp)));
    await pointer('down', p);
    await pause(50);
    await pointer('up', p);
    lastRawUp = Date.now();
    await settle();
    await pause(nativeButton ? 400 : 180);
  };
  const ready = async () => {
    await wait(
        '!layerApp.documents.busy()&&!layerApp.documents.interactionBusy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&!layerApp.app.canvas_work_pending()&&layerApp.app.startup_progress()[2]');
    await evaluate('layerApp.wake()');
    await settle();
    await evaluate('layerApp.app.wait_for_canvas()');
  };
  const persisted = async () => {
    await evaluate('layerApp.app.workspace_capture();layerApp.app.workspace_observe()');
    await wait('(v=>v.ready&&!v.busy&&!v.switcher_busy&&!v.dirty)(JSON.parse(layerApp.app.workspace_view()))');
  };
  const shot = capture;
  const bubbleView = id => evaluate(`JSON.parse(JSON.stringify(layerApp.app.bubbles().find(b=>b.view.id===${
      id}).view,(_,v)=>typeof v==='bigint'?String(v):v))`);
  const pixels = async p => {
    const s = await call(
        'Page.captureScreenshot',
        {format: 'png', clip: {x: p[0] - 50, y: p[1] - 35, width: 100, height: 70, scale: 1}});
    return evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${
        s.data}';await i.decode();const c=document.createElement('canvas');c.width=100;c.height=70;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(i,0,0);return [...g.getImageData(0,0,100,70).data]})()`);
  };
  try {
    await send({type: 'workspace_manager', command: {type: 'switch', id: 'builtin:workspace:painter'}});
    await wait(
        'JSON.parse(layerApp.app.workspace_view()).id===\'builtin:workspace:painter\'&&!JSON.parse(layerApp.app.workspace_view()).busy');
    original = await evaluate('layerApp.app.workspace_persistence()');
    for (const theme of ['light', 'dark'])
      for (device of ['mouse', 'touch', 'pen'])
        for (const selected of [false, true]) {
          const name = `${theme}-${device}-${selected ? 'selected' : 'unselected'}`;
          await call('Emulation.setTouchEmulationEnabled', {enabled: false, maxTouchPoints: 5});
          const fixture = structuredClone(original);
          fixture.layout.bubbles = {bubbles: [], rings: [], kept: [], show_sliders: true};
          await send({type: 'restore_workspace', workspace: fixture});
          await send({type: 'set_theme', theme});
          const previousEpoch = await evaluate('String(layerApp.state().document_file.epoch)');
          await invoke('new_document');
          await wait('!!document.querySelector(\'#new-drawing-dialog[open] #new-drawing-title\')');
          const title = await at('#new-drawing-title');
          await pointer('down', title, 'mouse');
          await pointer('up', title, 'mouse');
          await evaluate(createDrawing({width: 1600, height: 1000, paper: 'white', relief: 0}));
          await wait(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(previousEpoch)}`);
          await ready();
          await send({type: 'select_brush', id: 1});
          assert.deepEqual(
              await evaluate('[layerApp.state().brush.preset,layerApp.state().brush.tool]'), [1, 'pen'],
              'Direct SelectBrush activates G Pen');
          await send({type: 'set_brush_size', value: 80});
          await send({type: 'set_color', rgba: [0, 0, 0, 1]});
          const settings = await evaluate('layerApp.state().settings');
          await send({type: 'restore_settings', settings: {...settings, theme, cursor: 'none', feedback: false}});
          await ready();
          await call('Emulation.setTouchEmulationEnabled', {enabled: device === 'touch', maxTouchPoints: 5});
          const p = await evaluate(
              '(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return[r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]]})()');
          await stroke([[p[0] - 100, p[1]], [p[0], p[1]], [p[0] + 100, p[1]]], 'pen');
          await ready();
          await call(
              'Input.dispatchMouseEvent',
              {type: 'mouseMoved', x: 2, y: 2, button: 'none', buttons: 0, pointerType: 'pen'});
          await settle();
          const painted = await pixels(p);
          const header = await evaluate(
              'layerApp.state().workspace.layout.header.zones.flat().find(e=>e.item.control?.command===\'eraser\').id');
          const from = await at(`[data-header-item="${header}"] .header-tool`);
          const spot = await evaluate('(()=>{const a=layerApp.app.camera().work_area;return[a[0]+a[2]*.8,a[1]+a[3]*.3]})()');
          if (selected) {
            await send({type: 'select_brush', id: 3});
            await ready();
          }
          assert.equal(await evaluate("layerApp.state().brush.tool==='eraser'"), selected);
          assert.ok(await evaluate(`layerApp.app.can_take_bubble({kind:'header',id:${header}},${spot[0]},${spot[1]},innerWidth,innerHeight)`), 'The leaf Eraser is a bubble source');
          if (selected)
            assert.deepEqual(await evaluate('[layerApp.state().tool_set.groups.length,layerApp.state().tool_set.subtools.length]'), [0, 0]);
          await stroke([from, [from[0], from[1] + 8], [from[0], from[1] + 75], [spot[0], spot[1] - 30], spot]);
          const bubbles = await evaluate('layerApp.app.bubbles().map(b=>b.view.id)');
          assert.ok(bubbles.length, 'Dragging the Sketch Eraser creates its bubble');
          await shot(name + '-created');
          const id = bubbles.at(-1), selector = `.tool-bubble[data-bubble="${id}"] .bubble-face`;
          await invoke('pen');
          await tap(await at(selector));
          await ready();
          assert.deepEqual(await evaluate('[layerApp.state().brush.tool,layerApp.state().brush.preset]'), ['eraser', 3]);
          const selectedView = await bubbleView(id);
          assert.ok(selectedView.rings.length, 'Eraser exposes its settings halo');
          const body = await evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
          assert.deepEqual([body.width, body.height], selectedView.face);
          await tap(await at(selector));
          const fan = await evaluate(`layerApp.app.bubbles().find(b=>b.view.id===${id}).view.chooser`);
          assert.equal(fan.length, 1, 'The singleton fan keeps Eraser');
          assert.deepEqual(
              [fan[0].kind, fan[0].label, fan[0].icon, fan[0].selected, fan[0].enabled],
              ['tool', 'Eraser', 'eraser', true, true]);
          const fanCenter = await at(selector);
          await tap([fanCenter[0] + fan[0].center[0], fanCenter[1] + fan[0].center[1]]);
          assert.ok(
              await evaluate(`layerApp.app.bubbles().find(b=>b.view.id===${id}).view.rings.length>0`),
              'Choosing Eraser restores its settings');
          const ring = (await bubbleView(id)).rings[0];
          assert.ok(ring);
          const oldValue =
              await evaluate(`layerApp.state().tool_settings.find(s=>s.id===${JSON.stringify(ring.setting)}).value`),
                ringCenter = await at(selector), ringPoint = f => {
                  const angle = (ring.arc.start + ring.arc.sweep * f) * Math.PI / 180;
                  return [
                    ringCenter[0] + ring.arc.radius * Math.cos(angle),
                    ringCenter[1] + ring.arc.radius * Math.sin(angle)
                  ];
                };
          const target = ring.fill < .5 ? .8 : .2;
          await stroke([ringPoint(ring.fill), ringPoint(target)]);
          assert.notEqual(
              await evaluate(`layerApp.state().tool_settings.find(s=>s.id===${JSON.stringify(ring.setting)}).value`),
              oldValue, 'Native arc input changes the Eraser setting');
          const c = await at(selector);
          if (device === 'mouse') {
            for (const type of ['mousePressed', 'mouseReleased'])
              await call('Input.dispatchMouseEvent', {
                type,
                x: c[0],
                y: c[1],
                button: 'right',
                buttons: type === 'mousePressed' ? 2 : 0,
                clickCount: 1,
                pointerType: 'mouse'
              });
          } else {
            await pointer('down', c);
            await pause(1100);
            await pointer('up', c);
          }
          await settle();
          const options = await evaluate(
              '[...document.querySelectorAll(\'.panel-context-menu:popover-open .menu-label\')].map(n=>n.textContent)');
          assert.ok(options.length);
          for (const type of ['keyDown', 'keyUp'])
            await call('Input.dispatchKeyEvent', {type, key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27});
          await settle();
          await send({type: 'set_brush_size', value: 120});
          await stroke([[p[0], p[1] - 80], [p[0], p[1]], [p[0], p[1] + 80]], 'pen');
          await ready();
          await call(
              'Input.dispatchMouseEvent',
              {type: 'mouseMoved', x: 2, y: 2, button: 'none', buttons: 0, pointerType: 'pen'});
          await settle();
          const erased = await pixels(p), center = (35 * 100 + 50) * 4;
          assert.ok(painted[center] < 100, 'The fixture has ink at the crossing');
          assert.ok(erased[center] > 230, 'The selected Eraser removes the ink');
          await invoke('undo');
          await ready();
          assert.deepEqual(await pixels(p), painted, 'Eraser Undo restores exact pixels');
          await invoke('redo');
          await ready();
          assert.deepEqual(await pixels(p), erased, 'Eraser Redo restores exact pixels');
          await shot(name + '-erased');
          const home = await at(selector);
          await stroke([home, [home[0], home[1] - 25], [home[0], 12]]);
          await wait(`!layerApp.app.bubbles().some(b=>b.view.id===${id})`);
          assert.ok(await evaluate(
              `document.querySelector('[data-header-item=\"${header}\"] .header-tool').checkVisibility()`));
          console.log('PASS Eraser bubble', name);
        }
  } finally {
    await call('Emulation.setTouchEmulationEnabled', {enabled: false});
    if (original) {
      await send({type: 'restore_workspace', workspace: original});
      assert.deepEqual(await evaluate('layerApp.app.workspace_persistence()'), original, 'Sketch is restored before leaving');
      await persisted();
    }
    await send({type: 'workspace_manager', command: {type: 'switch', id: savedId}});
    await wait(`JSON.parse(layerApp.app.workspace_view()).id===${JSON.stringify(savedId)}&&!JSON.parse(layerApp.app.workspace_view()).busy`);
    if (savedId !== 'builtin:workspace:painter') await send({type: 'restore_workspace', workspace: saved});
    await persisted();
    assert.equal(await evaluate('JSON.parse(layerApp.app.workspace_view()).id'), savedId);
    assert.deepEqual(await evaluate('layerApp.app.workspace_persistence()'), saved, 'The original workspace is restored');
  }
}
