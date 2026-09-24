import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';

export async function checkToolbarComponents({ call, evaluate, settle }) {
  const dir = process.env.LAYER_TEST_ARTIFACTS || '/tmp/capy-toolbar-web'; await mkdir(dir, { recursive: true });
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const invoke = command => send({ type: 'invoke', command });
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30)}check()})`);
  async function workspace(id) {
    await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify({ type: 'switch', id: `builtin:workspace:${id}` }))});null`);
    await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:${id}'&&!JSON.parse(layerApp.app.workspace_view()).busy`); await settle();
  }
  const rect = selector => evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  async function click(selector, device = 'mouse', offset = { x: 0, y: 0 }) {
    const b = await rect(selector), x = b.x + b.width / 2 + offset.x, y = b.y + b.height / 2 + offset.y;
    assert.ok(b.width > 0 && b.height > 0, selector);
    if (device === 'touch') {
      await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ id: 1, x, y }] });
      await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    } else for (const type of ['mousePressed', 'mouseReleased']) await call('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1, pointerType: device, force: type === 'mouseReleased' ? 0 : .7 });
    await settle();
  }
  async function drag(selector, vertical, device = 'mouse') {
    const r = await rect(selector);
    const a = { x: r.x + r.width * (vertical ? .5 : .2), y: r.y + r.height * (vertical ? .8 : .5) };
    const b = { x: r.x + r.width * (vertical ? .5 : .8), y: r.y + r.height * (vertical ? .2 : .5) };
    await gesture(a, b, device);
    assert.equal(await evaluate(`!!document.querySelector('.toolbar-brush-preview:popover-open')`), false, `${device}: drag lift dismisses preview`);
  }
  async function gesture(a, b, device) {
    if (device === 'touch') {
      await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ id: 1, ...a }] });
      await call('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ id: 1, ...b }] });
      await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    } else {
      await call('Input.dispatchMouseEvent', { type: 'mousePressed', ...a, button: 'left', buttons: 1, clickCount: 1, pointerType: device, force: .7 });
      await call('Input.dispatchMouseEvent', { type: 'mouseMoved', ...b, button: 'left', buttons: 1, pointerType: device, force: .7 });
      await call('Input.dispatchMouseEvent', { type: 'mouseReleased', ...b, button: 'left', buttons: 0, clickCount: 1, pointerType: device });
    }
    await settle();
  }
  const capture = async name => { const shot = await call('Page.captureScreenshot', { format: 'png' }); await writeFile(`${dir}/${name}.png`, Buffer.from(shot.data, 'base64')); };
  await workspace('painter'); await invoke('brush');
  const slider = '[data-toolbar-component=brush_size_slider] input.number-slider';
  await wait(`!!document.querySelector(${JSON.stringify(slider)})`);
  for (const device of ['mouse', 'touch', 'pen']) {
    await send({ type: 'set_tool_setting', id: 'size', value: 5 });
    await drag(slider, true, device);
    const diameter = await evaluate('layerApp.state().brush.diameter');
    if (!(diameter > 5)) {
      await capture('slider-failure');
      console.log(await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(slider)}),r=n.getBoundingClientRect();return {diameter:layerApp.state().brush.diameter,rect:r.toJSON(),html:n.closest('[data-toolbar-component]').outerHTML}})()`));
    }
    assert.ok(diameter > 5, `${device} vertical size: ${diameter}`);
  }
  const centered = await evaluate(`(()=>{const r=document.querySelector('[data-toolbar-component=brush_size_slider]').getBoundingClientRect(),track=document.querySelector('[data-toolbar-component=brush_size_slider] .number-track').getBoundingClientRect();return {root:r.toJSON(),track:track.toJSON()}})()`);
  assert.ok(Math.abs(centered.root.x+centered.root.width/2-centered.track.x-centered.track.width/2)<2, JSON.stringify(centered));
  assert.ok(Math.abs(centered.track.y-centered.root.y-(centered.root.bottom-centered.track.bottom))<2, 'equal slider end padding');
  await capture('sketch-sliders');
  await checkSliderBookmarks({ call, evaluate, settle, click, gesture, rect, capture, send, slider });
  for (const device of ['mouse', 'touch', 'pen']) {
    const cap = await rect('[data-toolbar-component=brush_size_slider] .toolbar-slider-cap');
    const opacity = await rect('[data-toolbar-component=brush_opacity_slider]');
    const before = await evaluate('layerApp.state().brush.diameter');
    async function contact(type, p) {
      if (device === 'touch') await call('Input.dispatchTouchEvent', { type: { down: 'touchStart', move: 'touchMove', up: 'touchEnd' }[type], touchPoints: type === 'up' ? [] : [{ id: 1, ...p }] });
      else await call('Input.dispatchMouseEvent', { type: { down: 'mousePressed', move: 'mouseMoved', up: 'mouseReleased' }[type], ...p, button: 'left', buttons: type === 'up' ? 0 : 1, clickCount: 1, pointerType: device, force: type === 'up' ? 0 : .7 });
      await settle();
    }
    const destination = { x: opacity.x + opacity.width / 2, y: opacity.y + opacity.height - 12 };
    await contact('down', { x: cap.x + cap.width / 2, y: cap.y + cap.height / 2 });
    await evaluate('new Promise(r=>setTimeout(r,650))');
    assert.equal(await evaluate('document.querySelector(".panel-context-menu").matches(":popover-open")'), device !== 'mouse');
    await contact('move', destination);
    assert.equal(await evaluate('layerApp.state().brush.diameter'), before, 'a held cap reorders without changing its value');
    assert.equal(await evaluate('document.querySelector(".drop-indicator").hidden'), false, `${device}: held cap has a reorder target`);
    for (const type of ['keyDown', 'keyUp']) await call('Input.dispatchKeyEvent', { type, key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
    await contact('up', destination);
    await evaluate('new Promise(r=>setTimeout(r,350))');
  }

  // Real handle drags must expose the compact targets before committing them.
  const panel = await evaluate("document.querySelector('[data-toolbar-component=brush_size_slider]').closest('[data-panel]').dataset.panel");
  const [w, h, top] = await evaluate('[innerWidth,innerHeight,document.querySelector("#header").getBoundingClientRect().bottom]');
  for (const device of ['mouse', 'touch', 'pen']) for (const edge of ['left', 'right', 'top', 'bottom']) for (const alignment of ['start', 'center', 'end']) {
    await send({ type: 'move_panel', panel, target: { kind: 'float', position: [w / 2 - 120, h / 2 - 120] } });
    const handle = await rect(`.toolbar-controls[data-panel="${panel}"] > .panel-grip`);
    const a = { x: handle.x + handle.width / 2, y: handle.y + handle.height / 2 };
    const horizontal = ['top', 'bottom'].includes(edge);
    const along = alignment === 'start' ? 70 : alignment === 'end' ? (horizontal ? w : h) - 70 : (horizontal ? w / 2 : (top + h) / 2);
    const b = horizontal ? { x: along, y: edge === 'top' ? top + 3 : h - 3 } : { x: edge === 'left' ? 3 : w - 3, y: alignment === 'start' ? top + 70 : along };
    async function input(type, p) {
      if (device === 'touch') await call('Input.dispatchTouchEvent', { type: { down: 'touchStart', move: 'touchMove', up: 'touchEnd' }[type], touchPoints: type === 'up' ? [] : [{ id: 1, ...p }] });
      else await call('Input.dispatchMouseEvent', { type: { down: 'mousePressed', move: 'mouseMoved', up: 'mouseReleased' }[type], ...p, button: 'left', buttons: type === 'up' ? 0 : 1, clickCount: 1, pointerType: device, force: type === 'up' ? 0 : .7 });
      await settle();
    }
    await input('down', a); await input('move', { x: w / 2, y: h / 2 }); await input('move', b);
    const hint = await evaluate('layerApp.app.workspace_update().drag?.drop_hint?.target');
    assert.deepEqual(hint, { kind: 'compact_edge', edge, alignment }, `${device}/${edge}/${alignment}`);
    await input('up', b);
    assert.ok(await evaluate(`layerApp.state().workspace.layout.bands.some(b=>b.edge===${JSON.stringify(edge)}&&b.alignment===${JSON.stringify(alignment)})`));
    await invoke('undo_workspace');
    assert.ok(await evaluate('layerApp.state().workspace.layout.floating.length>0'));
    await invoke('redo_workspace');
    if (alignment === 'center') {
      await click(slider, device);
      const toolbar = await rect(`.toolbar-controls[data-panel="${panel}"]`);
      const preview = await rect('.toolbar-brush-preview:popover-open');
      const gap = edge === 'left' ? preview.x-toolbar.x-toolbar.width
        : edge === 'right' ? toolbar.x-preview.x-preview.width
        : edge === 'top' ? preview.y-toolbar.y-toolbar.height : toolbar.y-preview.y-preview.height;
      assert.ok(gap >= 7 && gap <= 16, `${device}/${edge}: preview clears toolbar, gap=${gap}`);
      for (const type of ['keyDown', 'keyUp']) await call('Input.dispatchKeyEvent', { type, key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 });
    }
  }

  await workspace('photographer'); await invoke('brush');
  await wait('!!document.querySelector("[data-toolbar-setting=size]")');
  const value = '[data-toolbar-setting=size] .number-value';
  const initial = await rect(value);
  for (const size of [.5, 31.9, 32, 2048]) {
    await send({ type: 'set_tool_setting', id: 'size', value: size });
    assert.deepEqual(await rect(value), initial, 'fixed numeric width');
  }
  await capture('photo-options');
  await invoke('rectangle_select');
  const segments = await rect('[data-toolbar-choice=selection-mode]'), dropdown = await rect('[data-toolbar-choice=variant] > button');
  assert.deepEqual([segments.y, segments.height], [dropdown.y, dropdown.height], 'segments match the dropdown height');
  for (const [i, device] of ['mouse', 'touch', 'pen'].entries()) {
    await click(`[data-toolbar-segment=selection-mode-${i + 1}]`, device);
    assert.ok(await evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(['selection_add', 'selection_subtract', 'selection_intersect'][i])}).selected`));
  }
  await invoke('auto_select');
  assert.ok(await evaluate('!!document.querySelector("[data-toolbar-choice=selection-source]:not(.toolbar-segments)")'));
  for (const theme of ['light', 'dark']) {
    await send({ type: 'set_theme', theme }); await capture(`selection-${theme}`);
  }
  for (const edge of ['left', 'right', 'top', 'bottom']) {
    for (const alignment of ['start', 'center', 'end']) {
      await send({ type: 'move_panel', panel: 'commands', target: { kind: 'compact_edge', edge, alignment } });
      assert.equal(await evaluate('layerApp.state().workspace.layout.bands.find(b=>b.alignment)?.alignment'), alignment);
    }
  }
  await send({ type: 'move_panel', panel: 'commands', target: { kind: 'edge', edge: 'left', outer: true } });
  await invoke('brush'); await settle();
  await click('[data-toolbar-setting=size] .toolbar-number-face');
  assert.ok(await evaluate('!!document.querySelector(".toolbar-editor-popover:popover-open input.number-slider")'));
  await invoke('eraser');
  assert.equal(await evaluate('!!document.querySelector(".toolbar-editor-popover:popover-open")'), false, 'context change closes editor');
  await capture('vertical-options');
  await invoke('brush'); await send({ type: 'set_tool_setting', id: 'size', value: 2048 });
  for (const style of ['small', 'medium', 'large', 'medium_labeled', 'labeled']) {
    await send({ type: 'customize', action: { type: 'set_tile_style', panel: 'commands', style } });
    await capture(`vertical-${style}`);
    const textFits = await evaluate(`(()=>{const n=document.querySelector('[data-toolbar-setting=size] .toolbar-face-value');return n.scrollWidth<=n.parentElement.clientWidth})()`);
    assert.ok(textFits, `${style}: four-digit value fits: ${JSON.stringify(await evaluate(`(()=>{const n=document.querySelector('[data-toolbar-setting=size] .toolbar-face-value');return {text:n.textContent,width:n.scrollWidth,available:n.parentElement.clientWidth,font:getComputedStyle(n).font}})()`))}`);
  }
  await send({ type: 'move_panel', panel: 'commands', target: { kind: 'edge', edge: 'top', outer: true } });
  await send({ type: 'customize', action: { type: 'set_tile_style', panel: 'commands', style: 'small' } });
  await click('[data-toolbar-setting=size] .number-value', 'touch');
  assert.ok(await evaluate('!document.querySelector("[data-toolbar-setting=size] .number-entry").hidden'));
  await click('#canvas', 'touch');
  assert.ok(await evaluate('document.querySelector("[data-toolbar-setting=size] .number-entry").hidden'));
  await click('[data-toolbar-component=tool_options] > .toolbar-more');
  await wait('!!document.querySelector(".content-drawer[data-drawer=tool]")');
  await evaluate('new Promise(r=>setTimeout(r,350))'); await settle();
  const drawerBounds = await rect('.content-drawer[data-drawer=tool]');
  const moreBounds = await rect('[data-toolbar-component=tool_options] > .toolbar-more');
  assert.ok(Math.abs(drawerBounds.x + drawerBounds.width - moreBounds.x - moreBounds.width) < 2, 'drawer aligns with More');
  await capture('options-drawer');
  await send({ type: 'customize', action: { type: 'close_expanded' } });

  console.log('Toolbar sliders/options: fixed values, segmented choices, contexts and compact placements passed');
}

async function checkSliderBookmarks({ call, evaluate, settle, click, gesture, rect, capture, send, slider }) {
  const preview = '.toolbar-brush-preview:popover-open';
  assert.equal(await evaluate(`document.querySelectorAll('[data-toolbar-component=brush_size_slider] .number-value').length`), 0);
  for (const device of ['mouse', 'touch', 'pen']) {
    await click(slider, device, { x: 0, y: -23 });
    assert.ok(await evaluate(`!!document.querySelector('${preview}')`), `${device}: tap retains preview`);
    const value = await evaluate('layerApp.state().brush.diameter');
    assert.ok(await evaluate(`(()=>{const c=document.querySelector('${preview} canvas');return c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((v,i)=>i%4===3&&v>0)})()`), 'preview paints the actual tip');
    await click(`${preview} .toolbar-preview-bookmark`, device);
    assert.equal(await evaluate(`document.querySelector('${preview} .toolbar-preview-bookmark').getAttribute('aria-label')`), 'Remove bookmark');
    await capture(`preview-${device}`);
    await send({ type: 'set_tool_setting', id: 'size', value: 3 });
    assert.equal(await evaluate(`document.querySelector('${preview} .toolbar-preview-bookmark').getAttribute('aria-label')`), 'Bookmark this value');
    const mark = '[data-toolbar-component=brush_size_slider] .toolbar-slider-mark';
    await click(mark, device, { x: 0, y: 24 });
    assert.notEqual(await evaluate('layerApp.state().brush.diameter'), value, `${device}: distant tap does not snap`);
    await click(mark, device, { x: 0, y: 16 });
    assert.equal(await evaluate('layerApp.state().brush.diameter'), value, `${device}: nearby tap recalls exact bookmark`);
    assert.ok(await evaluate(`(()=>{const m=document.querySelector('${mark}.selected').getBoundingClientRect(),h=document.querySelector('[data-toolbar-component=brush_size_slider] .toolbar-slider-thumb').getBoundingClientRect();return Math.abs(m.y+m.height/2-h.y-h.height/2)<.1&&Math.abs(m.x+m.width/2-h.x-h.width/2)<.1})()`), 'selected bookmark is centered inside handle');
    const m = await rect(mark), x = m.x + m.width / 2, y = m.y + m.height / 2;
    await gesture({x, y: y+24}, {x, y: y+16}, device);
    assert.notEqual(await evaluate('layerApp.state().brush.diameter'), value, `${device}: dragging near bookmark does not snap`);
    await click(mark, device, {x:0, y:16});
    assert.equal(await evaluate('layerApp.state().brush.diameter'), value);
    await click(`${preview} .toolbar-preview-bookmark`, device);
    assert.equal(await evaluate(`document.querySelectorAll('[data-toolbar-component=brush_size_slider] .toolbar-slider-mark').length`), 0);
    for (const type of ['mousePressed','mouseReleased']) await call('Input.dispatchMouseEvent',{type,x:700,y:150,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});
    await settle();
    assert.equal(await evaluate(`!!document.querySelector('${preview}')`), false, `${device}: outside dismiss`);
  }
  await send({type:'set_tool_setting',id:'size',value:2048});
  await click('[data-toolbar-component=brush_size_slider] .toolbar-slider-cap');
  assert.ok(await evaluate(`(()=>{const c=document.querySelector('${preview} canvas'),w=c.width,h=c.height,p=c.getContext('2d').getImageData(0,0,w,h).data;return [[1,h>>1],[w-2,h>>1],[w>>1,h-2]].every(([x,y])=>p[(y*w+x)*4+3]>0)})()`), 'large size stamp reaches every popup edge');
  for (const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    assert.ok(await evaluate(`(()=>{const p=document.querySelector('${preview}'),c=p.querySelector('canvas'),bg=getComputedStyle(p).backgroundColor.match(/[\\d.]+/g).slice(0,3).map(Number),ctx=c.getContext('2d'),pixel=y=>ctx.getImageData(c.width>>1,y,1,1).data,distance=y=>bg.reduce((n,v,i)=>n+Math.abs(v-pixel(y)[i]),0);return distance(4)<distance(20)&&distance(20)<distance(48)})()`), `${theme}: header fades gradually from the top using the current background`);
    await capture(`size-edge-fill-${theme}`);
  }
  await click('[data-toolbar-component=brush_opacity_slider] input.number-slider');
  const alpha = () => evaluate(`(()=>{const c=document.querySelector('${preview} canvas');return c.getContext('2d').getImageData(0,0,c.width,c.height).data.reduce((n,v,i)=>n+(i%4===3?v:0),0)})()`);
  await send({type:'set_tool_setting',id:'opacity',value:1}); const full = await alpha();
  await send({type:'set_tool_setting',id:'opacity',value:.5}); const half = await alpha();
  assert.ok(half/full > .48 && half/full < .52, 'preview opacity follows the value once');
  await capture('opacity-preview');
  await send({type:'invoke', command:'eraser'});
  assert.equal(await evaluate(`!!document.querySelector('${preview}')`), false, 'tool switch closes old preview');
  await send({type:'invoke', command:'brush'});
}
