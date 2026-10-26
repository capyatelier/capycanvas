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
  async function assertActionIconsCentred(context, expected) {
    const tiles = await evaluate(`[...document.querySelectorAll('[data-toolbar-component=tool_options] .toolbar-action > button')].filter(b=>b.checkVisibility()&&b.querySelector(':scope > svg')).map(b=>{const t=b.getBoundingClientRect(),svg=b.querySelector(':scope > svg'),i=svg.getBoundingClientRect();return {icon:svg.dataset.asset,offset:[i.x+i.width/2-t.x-t.width/2,i.y+i.height/2-t.y-t.height/2]}})`);
    assert.equal(tiles.length, expected, `${context}: visible icon-only Tool Options actions`);
    for (const tile of tiles) assert.deepEqual(tile.offset, [0, 0], `${context}: ${tile.icon} icon is centred in its tile like GTK`);
  }
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
  await checkToolbarSliderDisplay({call, evaluate, settle});
  const slider = '[data-toolbar-component=brush_size_slider] input.number-slider';
  await wait(`!!document.querySelector(${JSON.stringify(slider)})`);
  for (const device of ['mouse', 'touch', 'pen']) {
    await send({ type: 'set_tool_setting', id: 'size', value: 5 });
    await wait('layerApp.state().brush.diameter===5');
    await drag(slider, true, device);
    await wait('layerApp.state().brush.diameter>5');
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
  await checkToolbarStrengthPreviews({call,evaluate,settle});
  for (const device of ['mouse', 'touch', 'pen']) {
    const cap = await rect('[data-toolbar-component=brush_size_slider] .toolbar-slider-cap');
    const strength = await rect('[data-toolbar-component=brush_strength_slider]');
    const before = await evaluate('layerApp.state().brush.diameter');
    async function contact(type, p) {
      if (device === 'touch') await call('Input.dispatchTouchEvent', { type: { down: 'touchStart', move: 'touchMove', up: 'touchEnd' }[type], touchPoints: type === 'up' ? [] : [{ id: 1, ...p }] });
      else await call('Input.dispatchMouseEvent', { type: { down: 'mousePressed', move: 'mouseMoved', up: 'mouseReleased' }[type], ...p, button: 'left', buttons: type === 'up' ? 0 : 1, clickCount: 1, pointerType: device, force: type === 'up' ? 0 : .7 });
      await settle();
    }
    const destination = { x: strength.x + strength.width / 2, y: strength.y + strength.height - 12 };
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
  const originalTitle=await evaluate(`layerApp.app.panel_view(${JSON.stringify(panel)}).title`);
  await evaluate(`window.retainedToolbarGrip=document.querySelector('.toolbar-controls[data-panel="${panel}"] > .panel-grip')`);
  const renamedTitle='名称 {ink} toolbar';
  await send({type:'customize',action:{type:'rename_toolbar',panel}});
  await send({type:'customize',action:{type:'toolbar_name',name:renamedTitle}});
  await send({type:'customize',action:{type:'confirm_toolbar'}});
  assert.deepEqual(await evaluate(`(()=>{const grip=document.querySelector('.toolbar-controls[data-panel="${panel}"] > .panel-grip');return [grip===retainedToolbarGrip,grip.getAttribute('aria-label')];})()`),[true,'Move '+renamedTitle],'renaming preserves the toolbar grip and refreshes its literal-name caption');
  await send({type:'customize',action:{type:'rename_toolbar',panel}});
  await send({type:'customize',action:{type:'toolbar_name',name:originalTitle}});
  await send({type:'customize',action:{type:'confirm_toolbar'}});
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
    const field = await evaluate(`(()=>{const row=document.querySelector('[data-toolbar-setting=size]'),r=n=>n.getBoundingClientRect(),text=document.createRange(),value=row.querySelector('.number-value');text.selectNodeContents(value);
      const label=r(row.querySelector('.toolbar-option-label')),track=r(row.querySelector('.number-track')),slider=r(row.querySelector('input.number-slider')),box=r(row.querySelector('.number-value-box')),glyphs=text.getBoundingClientRect();
      return {label:label.width%1,reserve:box.width%1,insets:[slider.left-track.left,track.right-slider.right],gap:box.left-track.right,offset:Math.abs(Math.round(glyphs.left+glyphs.right-box.left-box.right))/2};})()`);
    assert.deepEqual(field, { label: 0, reserve: 0, insets: [2, 2], gap: 2, offset: 0 }, `${size}: GTK whole-pixel caption and reserve, slider insets and centered value`);
  }
  await capture('photo-options');
  await send({ type: 'set_tool_setting', id: 'size', value: 12 });
  const canvas = await rect('#canvas');
  await gesture({ x: canvas.x + canvas.width * .4, y: canvas.y + canvas.height * .5 },
    { x: canvas.x + canvas.width * .6, y: canvas.y + canvas.height * .5 }, 'mouse');
  await wait("layerApp.state().commands.find(command=>command.id==='scale_rotate').enabled");
  for (const theme of ['light', 'dark']) {
    await send({ type: 'set_theme', theme });
    for (const command of ['cancel_transform', 'apply_transform']) {
      await invoke('scale_rotate');
      await wait("layerApp.state().commands.find(command=>command.id==='apply_transform').enabled");
      const label = await evaluate(`layerApp.state().commands.find(command=>command.id===${JSON.stringify(command)}).label`);
      await click(`[data-toolbar-component=tool_options] .toolbar-action button[aria-label=${JSON.stringify(label)}]`);
      await wait("!layerApp.state().commands.find(command=>command.id==='apply_transform').enabled");
      await capture(`transform-${command}-${theme}`);
    }
  }
  await invoke('rectangle_select');
  await assertActionIconsCentred('horizontal Rectangle select', 4);
  const segments = await rect('[data-toolbar-choice=selection-mode]'), dropdown = await rect('[data-toolbar-choice=variant] > button');
  assert.deepEqual([segments.y, segments.height], [dropdown.y, dropdown.height], 'segments match the dropdown height');
  for (const theme of ['light', 'dark']) {
    await send({ type: 'set_theme', theme });
    const faces = await evaluate(`(() => {
      const color = layerApp.state().palette.control_fill;
      const probe = document.body.appendChild(document.createElement('i'));
      probe.style.backgroundColor = 'rgb(' + color.slice(0, 3).map(v => v * 255).join(' ') + ' / ' + color[3] + ')';
      const expected = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return { expected, actual: [
        document.querySelector('[data-toolbar-choice=variant] > button'),
        document.querySelector('[data-toolbar-choice=selection-mode] > button:not([aria-pressed=true])')
      ].map(node => getComputedStyle(node).backgroundColor) };
    })()`);
    assert.deepEqual(faces.actual, [faces.expected, faces.expected], `${theme} dropdown and unselected segment use the shared face fill`);
  }
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
  await invoke('ruler'); await settle();
  await assertActionIconsCentred('vertical Ruler', 3);
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
  for (const preedit of [false, true]) {
    const before = await evaluate('layerApp.state().brush.diameter');
    const raw = preedit ? '１２＋３' : '１２＋';
    await evaluate(`(()=>{const root=document.querySelector('[data-toolbar-setting=size] .number-control');
      root.valueButton.click();root.entry.value=${JSON.stringify(raw)};root.entry.dispatchEvent(new Event('input',{bubbles:true}));
      if(${preedit})root.entry.dispatchEvent(new CompositionEvent('compositionstart',{data:root.entry.value,bubbles:true}));
      root.valueButton.dispatchEvent(new WheelEvent('wheel',{deltaY:-1,bubbles:true,cancelable:true}));
      root.valueButton.dispatchEvent(new PointerEvent('pointerdown',{pointerId:989,pointerType:'pen',button:0,clientY:100,bubbles:true,cancelable:true}));
      root.valueButton.dispatchEvent(new PointerEvent('pointermove',{pointerId:989,pointerType:'pen',clientY:50,bubbles:true,cancelable:true}));
    })()`);
    assert.deepEqual(await evaluate(`(()=>{const root=document.querySelector('[data-toolbar-setting=size] .number-control');return [layerApp.state().brush.diameter,root.entry.value,root.entry.hidden];})()`),[before,raw,false],'toolbar wheel and scrub preserve rejected or composing drafts');
    await evaluate(`(()=>{const root=document.querySelector('[data-toolbar-setting=size] .number-control');if(${preedit})root.entry.dispatchEvent(new CompositionEvent('compositionend',{data:root.entry.value,bubbles:true}));root.cancelEditing();})()`);
  }
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

async function checkToolbarSliderDisplay({call, evaluate, settle}) {
  const dir = process.env.LAYER_TEST_ARTIFACTS || '/tmp/capy-toolbar-web';
  await evaluate('window.sliderDisplayWorkspace=structuredClone(layerApp.state().workspace)');
  for (const theme of ['light', 'dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
    for (const axis of ['vertical', 'horizontal']) for (const style of ['small', 'medium', 'large', 'medium_labeled', 'labeled']) {
      await evaluate(`(()=>{
        const workspace=structuredClone(sliderDisplayWorkspace),layout=workspace.layout;
        const panel=layout.panels.find(p=>p.content.kind==='toolbar'&&p.content.tiles.some(t=>t.control.kind==='brush_size_slider'));
        panel.tile_style=${JSON.stringify(style)}; panel.content.tiles=panel.content.tiles.filter(t=>['brush_size_slider','brush_strength_slider'].includes(t.control.kind));
        for(const tile of panel.content.tiles)tile.slider_style={show_value:true,expand:true};
        const metrics=layerApp.app.toolbar_ui({type:'style',style:panel.tile_style});
        layout.bands=[];layout.collapsed=[];layout.column_stacks=[];layout.column_scroll=[];layout.fit_height_groups=[];layout.fit_tab_groups=[];
        layout.floating=[{root:{kind:'tabs',id:2,panels:[panel.id],active:panel.id,tab_style:'automatic'},position:[30,90],
          width:${axis === 'vertical' ? 'metrics.size[0]*2+metrics.gap' : '1160'},height:${axis === 'vertical' ? '550' : 'metrics.size[1]'},default_width:null,
          toolbar_layout:${JSON.stringify(axis === 'vertical' ? 'compact' : 'horizontal')}}];
        layerApp.dispatch({type:'restore_workspace',workspace});
      })()`);
      await settle();
      for (const [size, strength] of [[24,.4],[99,.99],[100,1],[430,.95],[2048,1]]) {
        await evaluate(`layerApp.dispatch({type:'set_brush_size',value:${size}});layerApp.dispatch({type:'set_brush_strength',value:${strength}})`);
        await settle();
        const values = await evaluate(`Array.from(document.querySelectorAll('.toolbar-slider')).filter(row=>row.checkVisibility()).map(row=>{
          const cap=row.querySelector('.toolbar-slider-cap'),number=row.querySelector('.toolbar-slider-value'),unit=row.querySelector('.toolbar-slider-unit'),group=row.querySelector('.toolbar-slider-value-row');
          return {cap:cap.getBoundingClientRect().toJSON(),host:row.closest('.toolbar-controls').getBoundingClientRect().toJSON(),number:number.getBoundingClientRect().toJSON(),unit:unit.getBoundingClientRect().toJSON(),
            text:number.textContent,units:unit.textContent,flow:getComputedStyle(group).flexDirection,font:parseFloat(getComputedStyle(group).fontSize),standard:parseFloat(getComputedStyle(row).fontSize)};
        })`);
        assert.equal(values.length, 2, `${axis}/${style}: paired sliders`);
        for (const value of values) {
          const stacked = axis === 'vertical' && (style === 'small' || style === 'medium' && value.text.replace(/\D/g,'').length >= 3);
          assert.equal(value.flow, stacked ? 'column' : 'row', `${axis}/${style}/${value.text}: unit placement`);
          assert.ok(['px','%'].includes(value.units));
          assert.ok(value.cap.x >= value.host.x-.5 && value.cap.y >= value.host.y-.5
            && value.cap.right <= value.host.right+.5 && value.cap.bottom <= value.host.bottom+.5, `${axis}/${style}: both caps fit the toolbar`);
          for (const text of [value.number,value.unit]) {
            assert.ok(text.x >= value.cap.x+5.5 && text.y >= value.cap.y+5.5
              && text.right <= value.cap.right-5.5 && text.bottom <= value.cap.bottom-5.5, `${axis}/${style}/${value.text}: margins ${JSON.stringify(value)}`);
          }
          if (axis === 'horizontal' || style === 'large' || style.includes('labeled') || style === 'medium' && stacked)
            assert.ok(Math.abs(value.font-value.standard)<.05, `${axis}/${style}/${value.text}: standard font`);
        }
      }
      const shot = await call('Page.captureScreenshot',{format:'png'});
      await writeFile(`${dir}/slider-display-${theme}-${axis}-${style}.png`,Buffer.from(shot.data,'base64'));
    }
  }
  await evaluate('layerApp.dispatch({type:"restore_workspace",workspace:sliderDisplayWorkspace});delete window.sliderDisplayWorkspace');
  await settle();
}

async function checkSliderBookmarks({ call, evaluate, settle, click, gesture, rect, capture, send, slider }) {
  const preview = '.toolbar-brush-preview:popover-open';
  const stampReady=async old=>evaluate('new Promise((resolve,reject)=>{const end=performance.now()+20000;function poll(){const id=document.querySelector(".toolbar-brush-preview:popover-open canvas")?.dataset.previewRequest;if(id'+(old?'&&id!=='+JSON.stringify(old):'')+')return resolve();if(performance.now()>end)return reject(Error("GPU popup refresh"));setTimeout(poll,20)}poll()})');
  assert.equal(await evaluate(`document.querySelectorAll('[data-toolbar-component=brush_size_slider] .number-value').length`), 0);
  for (const device of ['mouse', 'touch', 'pen']) {
    await click(slider, device, { x: 0, y: -23 });
    assert.ok(await evaluate(`!!document.querySelector('${preview}')`), `${device}: tap retains preview`);
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function poll(){if(document.querySelector('${preview} canvas')?.dataset.previewRequest)return resolve();if(performance.now()>end)return reject(Error('GPU tip preview delivery'));setTimeout(poll,20)}poll()})`);
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
  await click('[data-toolbar-component=brush_size_slider] .toolbar-slider-cap');await stampReady();
  assert.ok(await evaluate(`(()=>{const c=document.querySelector('${preview} canvas'),w=c.width,h=c.height,p=c.getContext('2d').getImageData(0,0,w,h).data;return [[1,h>>1],[w-2,h>>1],[w>>1,h-2]].every(([x,y])=>p[(y*w+x)*4+3]>0)})()`), 'large size stamp reaches every popup edge');
  for (const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    assert.ok(await evaluate(`(()=>{const p=document.querySelector('${preview}'),c=p.querySelector('canvas'),bg=getComputedStyle(p).backgroundColor.match(/[\\d.]+/g).slice(0,3).map(Number),ctx=c.getContext('2d'),pixel=y=>ctx.getImageData(c.width>>1,y,1,1).data,distance=y=>bg.reduce((n,v,i)=>n+Math.abs(v-pixel(y)[i]),0);return distance(4)<distance(20)&&distance(20)<distance(48)})()`), `${theme}: header fades gradually from the top using the current background`);
    await capture(`size-edge-fill-${theme}`);
  }
  const largeRequest=await evaluate(`document.querySelector('${preview} canvas').dataset.previewRequest`);
  await send({type:'set_tool_setting',id:'size',value:3});await stampReady(largeRequest);
  const fade = await evaluate(`(()=>{const p=document.querySelector('${preview}'),style=p.closest('[data-toolbar-component]').dataset.tileStyle,g=layerApp.app.toolbar_ui({type:'slider_preview',control:{kind:'brush_size_slider'},style,value:3,length:0,extent:1}),c=p.querySelector('canvas'),r=c.width/c.getBoundingClientRect().width,ctx=c.getContext('2d'),alpha=y=>ctx.getImageData(c.width>>1,Math.round(y*r),1,1).data[3];return {style,presented:p.closest('.toolbar-controls').dataset.tileStyle,tile:layerApp.app.toolbar_ui({type:'style',style}).size[1],length:g.header_fade,opacity:g.header_fade_opacity,alpha:[0,g.header_fade/2,g.header_fade+4].map(alpha)}})()`);
  assert.equal(fade.style, fade.presented); assert.ok(Math.abs(fade.length - 65 * fade.tile / 54) < .01, `65px fade on medium tiles: ${JSON.stringify(fade)}`);
  assert.ok(Math.abs(fade.alpha[0]-255*fade.opacity)<8&&Math.abs(fade.alpha[1]-255*fade.opacity/2)<8&&fade.alpha[2]===0, `header fade follows the shared layout: ${JSON.stringify(fade)}`);
  await click('[data-toolbar-component=brush_size_slider] input.number-slider');
  await stampReady();
  await send({type:'invoke', command:'eraser'});
  assert.equal(await evaluate(`!!document.querySelector('${preview}')`), false, 'tool switch closes old preview');
  await send({type:'invoke', command:'brush'});
}


export async function checkToolbarStrengthPreviews({call,evaluate,settle}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||'/tmp/capy-toolbar-web';await mkdir(dir,{recursive:true});const reports=[],defaults=[];
  const wait=condition=>evaluate('new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if('+condition+')return resolve();if(performance.now()>end)return reject(Error('+JSON.stringify(condition)+'));setTimeout(poll,20)}poll()})');
  const send=async action=>{await evaluate('layerApp.dispatch('+JSON.stringify(action)+')');await settle();};
  const click=async selector=>{const point=await evaluate('(()=>{const r=document.querySelector('+JSON.stringify(selector)+').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()');for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const preview='.toolbar-brush-preview:popover-open',canvas=preview+' canvas';
  const alpha=()=>evaluate('(()=>{const c=document.querySelector('+JSON.stringify(canvas)+');return Array.from(c.getContext("2d").getImageData(0,0,c.width,c.height).data).filter((_,i)=>i%4===3)})()');
  const ready=async previous=>{await wait('!!document.querySelector('+JSON.stringify(canvas)+')?.dataset.previewRequest'+(previous?'&&document.querySelector('+JSON.stringify(canvas)+').dataset.previewRequest!=='+JSON.stringify(previous):''));await settle();};
  const request=()=>evaluate('document.querySelector('+JSON.stringify(canvas)+').dataset.previewRequest');
  const capture=async name=>{const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(dir+'/'+name+'.png',Buffer.from(shot.data,'base64'));};
  const history=()=>evaluate('(()=>{const s=layerApp.state();return{revision:String(layerApp.app.gpu_diagnostics().session.revision),undo:s.commands.find(c=>c.id==="undo").enabled,redo:s.commands.find(c=>c.id==="redo").enabled}})()');
  await evaluate('(()=>{window.__stampCopies=[];const copy=GPUCommandEncoder.prototype.copyBufferToBuffer;window.__originalStampCopy=copy;GPUCommandEncoder.prototype.copyBufferToBuffer=function(...a){if(a[0]?.label==="brush stamp output"&&a[2]?.label==="brush stamp readback")__stampCopies.push({source:a[0].label,target:a[2].label,bytes:Number(a[4])});return copy.apply(this,a)}})()');
  await evaluate('layerApp.app.workspace_input(JSON.stringify({type:"switch",id:"builtin:workspace:painter"}));null');
  await wait('JSON.parse(layerApp.app.workspace_view()).id==="builtin:workspace:painter"&&!JSON.parse(layerApp.app.workspace_view()).busy');
  await send({type:'invoke',command:'brush'});await wait('!!document.querySelector("[data-toolbar-component=brush_strength_slider] .toolbar-slider-cap")');
  await wait('layerApp.app.document_park_ready()');const before=await history();const initial=await evaluate('(()=>{const s=layerApp.state();return{preset:s.brush.preset,fields:s.tool_settings.map(f=>({id:f.id,value:f.value}))}})()');
  try {
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});
      for(const spec of [{id:2,name:'Pencil',loading:.7},{id:25,name:'Pointy',loading:.75},{id:26,name:'ShadingPencil',loading:.65},{id:27,name:'Charcoal',loading:.55},{id:6,name:'Chalk',loading:.15},{id:17,name:'Pastel',loading:.5},{id:43,name:'Crayon',loading:.9}]) {
        await send({type:'select_brush',id:spec.id});const fields=await evaluate('layerApp.state().tool_settings');assert.deepEqual(fields.map(f=>f.id),['size','flow','grain_depth','material_hardness','smoothing_strength','angle','opacity']);const smoothing=fields.find(f=>f.id==='smoothing_strength');assert.equal(smoothing.group_id,'');assert.equal(smoothing.label,'Smoothing');assert.deepEqual([smoothing.numeric.min,smoothing.numeric.max,smoothing.numeric.scale],[0,1,100]);assert.equal(fields.find(f=>f.id==='flow').value,1,spec.name+' defaultStrength100');const material=fields.find(f=>f.id==='material_hardness');assert.equal(material.label,'Material hardness');assert.equal(material.numeric.max,1);assert.equal(material.numeric.scale,100);assert.ok(Math.abs(material.value-spec.loading)<1e-6,spec.name+' authoredMaterial hardness');defaults.push({theme,...spec,fields});await capture('default-'+spec.name+'-'+theme);
      }
      await writeFile(dir+'/dry-defaults.json',JSON.stringify(defaults,null,2));
    }
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});
      for(const spec of [{id:2,name:'Pencil'},{id:25,name:'Pointy'},{id:43,name:'Crayon'},{id:3,name:'Eraser'},{id:10,name:'Smudge'}]) {
        await send({type:'select_brush',id:spec.id});for(const [id,value] of [['size',64],['opacity',1],['flow',.25]])await send({type:'set_tool_setting',id,value});
        await send({type:'customize',action:{type:'close_expanded'}});await click('[data-toolbar-component=brush_strength_slider] .toolbar-slider-cap');await ready();
        await evaluate('window.__strengthPopup=document.querySelector(".toolbar-brush-preview:popover-open");window.__strengthCanvas=__strengthPopup.querySelector("canvas");null');
        const low=await alpha(),lowSum=low.reduce((n,v)=>n+v,0);assert.ok(lowSum>0,theme+' '+spec.name+': contact visible');const lowRequest=await request();await capture('strength-'+spec.name+'-25-'+theme);
        await send({type:'set_tool_setting',id:'flow',value:1});await ready(lowRequest);const full=await alpha(),fullSum=full.reduce((n,v)=>n+v,0);const fullRequest=await request();
        assert.ok(fullSum>lowSum,theme+' '+spec.name+': Strength changes actual popup alpha');assert.equal(await evaluate('document.querySelector(".toolbar-brush-preview:popover-open")===__strengthPopup&&__strengthPopup.querySelector("canvas")===__strengthCanvas'),true,'value updates retain native popup/canvas');
        assert.ok(full.some(v=>v>0&&v<255),theme+' '+spec.name+': textured or soft edge');const idleCopies=await evaluate('__stampCopies.length');await evaluate('new Promise(resolve=>{let frames=0;function frame(){if(++frames===8)resolve();else requestAnimationFrame(frame)}requestAnimationFrame(frame)})');assert.equal(await evaluate('__stampCopies.length'),idleCopies,'unchanged open popup polls metadata without extra GPU image jobs');await capture('strength-'+spec.name+'-100-'+theme);
        if(spec.id===25){const width=await evaluate('document.querySelector('+JSON.stringify(canvas)+').width'),points=full.flatMap((v,i)=>v>8?[[i%width,Math.floor(i/width)]]:[]),span=axis=>Math.max(...points.map(p=>p[axis]))-Math.min(...points.map(p=>p[axis]))+1,x=span(0),y=span(1);assert.ok(Math.max(x,y)/Math.min(x,y)>1.1,'default Pointy45 elongated contact '+x+'x'+y);}
        if(spec.id===2||spec.id===43){const depth=await evaluate('layerApp.state().tool_settings.find(f=>f.id==="grain_depth").value');await send({type:'set_tool_setting',id:'grain_depth',value:0});await ready(fullRequest);const flat=await alpha();assert.ok(flat.reduce((n,v,i)=>n+(v!==full[i]),0)>50,'actual paper texture changes GPU popup contact');const flatRequest=await request();await send({type:'set_tool_setting',id:'grain_depth',value:depth});await ready(flatRequest);assert.deepEqual(await alpha(),full,'restored paper parameter reproduces exact popup mask');}
        const oldRequest=await request();await send({type:'set_tool_setting',id:'flow',value:0});await ready(oldRequest);assert.ok((await alpha()).every(v=>v===0),theme+' '+spec.name+': Strength zero clears contact');assert.deepEqual(await history(),before,'preview leaves artwork and Undo/Redo unchanged');await capture('strength-'+spec.name+'-0-'+theme);
        reports.push({theme,...spec,lowSum,fullSum,lowRequest,fullRequest,zeroEmpty:true,historyUnchanged:true});await writeFile(dir+'/strength-preview-results.json',JSON.stringify(reports,null,2));
        for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();assert.equal(await evaluate('!!document.querySelector(".toolbar-brush-preview:popover-open")'),false,'Escape retires popup');
      }
    }
    const copies=await evaluate('__stampCopies');assert.ok(copies.length>=30,'actual asynchronous GPU stamp copies');assert.ok(copies.every(c=>c.bytes===192*192),'bounded192² GPU alpha readback');await writeFile(dir+'/strength-preview-gpu-copies.json',JSON.stringify(copies,null,2));
  } finally {await evaluate('GPUCommandEncoder.prototype.copyBufferToBuffer=__originalStampCopy');}
  await send({type:'select_brush',id:initial.preset});for(const field of initial.fields)await send({type:'set_tool_setting',...field});await send({type:'invoke',command:'brush'});console.log('PASS: both-theme GPU Strength previews refresh, texture restores, Pointy45 and shape-only Eraser/Smudge, zero and unchanged artwork/history');
}
