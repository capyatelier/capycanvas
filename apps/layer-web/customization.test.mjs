import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkCustomization({ call, evaluate, settle, canvasPixels }) {
  const dir = "artifacts/ui/customization/web"; await mkdir(dir, { recursive: true });
  const wait = () => evaluate("new Promise(resolve => setTimeout(resolve, 260))");
  const send = (action) => evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const customize = (action) => send({ type: "customize", action });
  const rect = (selector) => evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
  const clickAt = async ({ x, y }) => {
    await call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await call("Input.dispatchMouseEvent", { type: "mousePressed", button: "left", clickCount: 1, x, y });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", button: "left", clickCount: 1, x, y });
    await wait();
  };
  const click = async (selector) => clickAt(await rect(selector));
  const context = async (selector) => {
    const { x, y } = await rect(selector);
    await call("Input.dispatchMouseEvent", { type: "mousePressed", button: "right", clickCount: 1, x, y });
    await call("Input.dispatchMouseEvent", { type: "mouseReleased", button: "right", clickCount: 1, x, y });
    await settle();
    assert.equal(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"), true,
      await evaluate(`JSON.stringify({hit:document.elementFromPoint(${x},${y})?.outerHTML,menu:document.querySelector('.panel-context-menu').outerHTML})`));
    return evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].map(n=>n.textContent)`);
  };
  const shot = async (name) => { await wait(); const image = await call("Page.captureScreenshot", { format: "png" }); await writeFile(`${dir}/${name}.png`, Buffer.from(image.data, "base64")); };
  const tab = (panel) => `.dock-tab[data-panel="${panel}"]`;
  const expanded = () => evaluate("layerApp.state().customization.expanded ?? null");
  await call("Emulation.setDeviceMetricsOverride", { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
  await settle(); await wait();
  const initial = await evaluate("layerApp.state().workspace");
  await shot("startup");
  const pixels = await canvasPixels();
  assert.ok(pixels.white > pixels.total * .1, `The WebGPU canvas must be visible in the composed application: ${JSON.stringify(pixels)}`);
  for (const theme of ["dark", "light"]) {
    await send({ type: "restore_workspace", workspace: initial });
    await send({ type: "set_theme", theme }); await wait();
    await shot(`initial-${theme}`);
    assert.deepEqual(await context(tab("sizes")), ["Tab Name", "Tab Icon", "Configure Panel…"]);
    await shot(`panel-menu-${theme}`);
    await click('.panel-context-menu button:nth-child(2)');
    assert.equal(await evaluate(`!!document.querySelector('${tab("sizes")} svg')`), true);
    await click(tab("sizes"));
    assert.equal(await expanded(), "sizes");
    assert.equal(await evaluate("document.querySelectorAll('.expanded-panel').length"), 1);
    assert.deepEqual(await evaluate(`(()=>{const root=document.querySelector('.expanded-panel');return [getComputedStyle(root).filter,...['.panel-preview','.panel-configuration'].map(s=>{const c=getComputedStyle(root.querySelector(s));return [c.boxShadow,c.filter]})];})()`),
      ["drop-shadow(rgba(0, 0, 0, 0.4) 0px 8px 12px)", ["none", "none"], ["none", "none"]],
      "one stronger shadow must wrap both columns, never their seam");
    await evaluate("window.originalPanel=document.querySelector('.sizes-panel');window.originalParent=originalPanel.parentElement;");
    await shot(`expanded-sizes-${theme}`);
    await click('.panel-configuration [data-visible="brush_opacity"]');
    assert.equal(await evaluate("document.querySelector('.sizes-panel [data-control=brush_opacity]').hidden"), false);
    await evaluate(`{const input=document.querySelector('.panel-configuration [data-control=brush_opacity] input');input.value=.42;input.dispatchEvent(new Event('input',{bubbles:true}));}`);
    assert.ok(Math.abs(await evaluate("layerApp.state().brush.opacity") - .42) < .001);
    assert.equal(await evaluate("document.querySelector('.sizes-panel [data-control=brush_opacity] input').value"), "0.42");
    await click(tab("sizes")); assert.equal(await expanded(), null);
    assert.equal(await evaluate("originalPanel.parentElement===originalParent"), true);
    await send({ type: "move_panel", panel: "sizes", target: { kind: "tab", group: 8, index: null } }); await wait();
    await click(tab("sizes")); assert.equal(await expanded(), "sizes");
    await shot(`expanded-left-second-${theme}`);
    await click(tab("layers")); assert.equal(await expanded(), "layers");
    await shot(`expanded-left-first-${theme}`);
    assert.equal(await evaluate("document.querySelector('.expanded-panel .panel-column-join').hidden"), false);
    await click(tab("layers")); assert.equal(await expanded(), null);
    // Both side and top/bottom bounds use the same Rust allocator.
    for (const edge of ["left", "top", "bottom"]) {
      await send({ type: "move_panel", panel: "sizes", target: { kind: "edge", edge, outer: false } }); await wait();
      await click(tab("sizes")); assert.equal(await expanded(), "sizes");
      const bounds = await evaluate(`(()=>{const root=document.querySelector('.expanded-panel'),p=root.querySelector('.panel-preview').getBoundingClientRect(),c=root.querySelector('.panel-configuration').getBoundingClientRect();return{p:{top:p.top,bottom:p.bottom},c:{top:c.top,bottom:c.bottom},count:root.querySelectorAll('.panel-preview').length};})()`);
      assert.equal(bounds.c.top - bounds.p.top, 36); assert.equal(bounds.c.bottom, bounds.p.bottom); assert.equal(bounds.count, 1);
      await shot(`expanded-${edge}-${theme}`); await click(tab("sizes"));
    }
    await send({ type: "restore_workspace", workspace: initial }); await wait();
    assert.deepEqual(await context('[data-panel="layers"] .dock-tabs > .panel-grip'), ["Tab Names", "Tab Icons", "New Toolbar…"]);
    await shot(`group-menu-${theme}`); await click('.panel-context-menu button:last-child');
    assert.equal(await evaluate("document.querySelector('#tool-picker').open"), true);
    await evaluate("{const name=document.querySelector('#toolbar-name');name.value='Layers';name.dispatchEvent(new Event('input',{bubbles:true}));}");
    assert.equal(await evaluate("document.querySelector('#confirm-tools').disabled"), true);
    await evaluate(`{const name=document.querySelector('#toolbar-name');name.value='Illustration ${theme}';name.dispatchEvent(new Event('input',{bubbles:true}));const search=document.querySelector('#tool-search');search.value='pencil';search.dispatchEvent(new Event('input',{bubbles:true}));}`);
    await click('.tool-choice:first-child'); await shot(`picker-${theme}`);
    await customize({ type: "picker_search", query: "opacity" });
    assert.equal(await evaluate("Number(layerApp.app.tool_picker().selected_count)"), 1, "search preserves selection");
    await customize({ type: "picker_search", query: "pencil" });
    assert.equal(await evaluate("document.querySelector('.tool-choice input').checked"), true);
    await click('#confirm-tools');
    assert.equal(await evaluate("document.querySelector('#tool-picker').open"), false);
    const custom = await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.content.name==='Illustration ${theme}').id`);
    assert.deepEqual(await context(`[data-panel="${custom}"] [data-tile]`), ["Remove Tool", "Insert Tools…"]);
    await shot(`tile-menu-${theme}`);
    await click('.panel-context-menu button:first-child');
    assert.equal(await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='${custom}').content.tiles.length`), 0);
    assert.deepEqual(await context(`[data-panel="${custom}"] .toolbar-controls`), ["Add Tools…"]);
    await click('.panel-context-menu button:first-child');
    await click('.tool-choice:first-child'); await click('.tool-choice:nth-child(2)'); await click('#confirm-tools');
    assert.equal(await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='${custom}').content.tiles.length`), 2);
    const beforeCancel = await evaluate("layerApp.state().workspace");
    await customize({ type: "new_toolbar", group: 8 });
    await customize({ type: "picker_name", name: `illustration ${theme}` });
    assert.equal(await evaluate("document.querySelector('#confirm-tools').disabled"), true, "custom names are case-insensitively unique");
    await customize({ type: "cancel_tools" });
    assert.deepEqual(await evaluate("layerApp.state().workspace"), beforeCancel, "cancel is transactional");
    const saved = await evaluate("layerApp.state().workspace");
    await send({ type: "restore_workspace", workspace: initial }); await send({ type: "restore_workspace", workspace: saved });
    assert.deepEqual(await evaluate("layerApp.state().workspace"), saved);
    await send({ type: "invoke", command: "reset_layout" });
    assert.ok(await evaluate(`layerApp.state().workspace.layout.panels.some(p=>p.id==='${custom}')`));
    await send({ type: "move_panel", panel: custom, target: { kind: "edge", edge: "bottom", outer: false } }); await wait();
    // Native HTML DND and touch movement use exactly the same core tile target.
    const before = await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='${custom}').content.tiles.map(t=>t.id)`);
    const dragged = await evaluate(`(()=>{window.dragSource=document.querySelector('[data-panel=toolbar] [data-tile]');window.dragData=new DataTransfer();dragSource.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:dragData}));const r=document.querySelector('[data-panel="${custom}"] [data-tile]').getBoundingClientRect();window.dropPoint={clientX:r.x+1,clientY:r.y+r.height/2};document.querySelector('#workspace').dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:dragData,...dropPoint}));return{tile:Number(dragSource.dataset.tile),kind:document.querySelector('.drop-indicator').dataset.kind,hidden:document.querySelector('.drop-indicator').hidden};})()`);
    assert.equal(dragged.kind, "tile"); assert.equal(dragged.hidden, false);
    await shot(`tile-drop-${theme}`);
    await evaluate(`document.querySelector('#workspace').dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:dragData,...dropPoint}));dragSource.dispatchEvent(new DragEvent('dragend',{bubbles:true,dataTransfer:dragData}));`);
    await wait();
    assert.deepEqual(await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='${custom}').content.tiles.map(t=>t.id)`), [dragged.tile, ...before]);
    await call("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 1 });
    const touch = (type, point) => call("Input.dispatchTouchEvent", { type, touchPoints: point ? [{ ...point, id: 1, radiusX: 1, radiusY: 1, force: 1 }] : [] });
    const first = await rect(`[data-panel="${custom}"] [data-tile="${dragged.tile}"]`);
    const last = await rect(`[data-panel="${custom}"] [data-tile="${before.at(-1)}"]`); last.x += 17;
    await touch("touchStart", first); await touch("touchMove", last); await settle();
    assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"), false);
    await touch("touchEnd"); await wait();
    assert.deepEqual(await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='${custom}').content.tiles.map(t=>t.id)`), [...before, dragged.tile]);
    const holdPoint = await rect(tab("brushes"));
    await touch("touchStart", holdPoint);
    await evaluate("new Promise(resolve=>setTimeout(resolve,550))");
    await touch("touchEnd"); await settle();
    assert.equal(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"), true);
    assert.equal(await expanded(), null, "a long press must not also activate the selected tab");
    await shot(`touch-context-${theme}`);
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await call("Emulation.setTouchEmulationEnabled", { enabled: false });
    await shot(`customized-${theme}`);
  }
  await send({ type: "restore_workspace", workspace: initial });
  const sizesGroup = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.active==='sizes').id");
  await customize({ type: "new_toolbar", group: sizesGroup });
  await evaluate(`{for(const c of layerApp.app.tool_picker().choices) layerApp.dispatch({type:'customize',action:{type:'picker_select',control:c.control,selected:true}});}`);
  await click('#confirm-tools');
  const many = await evaluate("layerApp.state().workspace.layout.panels.at(-1).id");
  // Every tile remains configured, even when a tabbed ribbon clips later rows.
  const clipped = await evaluate(`(()=>{const p=document.querySelector('[data-panel="${many}"] .tile-panel'),r=p.getBoundingClientRect(),tiles=[...p.querySelectorAll('[data-tile]')];return{overflow:getComputedStyle(p).overflowY,count:tiles.length,hidden:tiles.filter(t=>t.getBoundingClientRect().top>=r.bottom).length};})()`);
  assert.equal(clipped.overflow, "clip"); assert.ok(clipped.count > 25 && clipped.hidden > 0);
  await shot("clipped-ribbon");
  await send({ type: "move_panel", panel: many, target: { kind: "edge", edge: "left", outer: false } }); await wait();
  assert.ok(await evaluate(`document.querySelector('[data-panel="${many}"] .tile-panel').getBoundingClientRect().width`) > 72);
  await shot("wrapped-ribbon");
  await send({ type: "restore_workspace", workspace: initial }); await wait();
  await send({ type: "move_panel", panel: "sizes", target: { kind: "edge", edge: "bottom", outer: false } });
  const group = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.active==='sizes').id");
  await send({ type: "move_panel", panel: "layers", target: { kind: "tab", group, index: null } });
  await click(tab("layers"));
  const animationHeights = await evaluate(`new Promise(resolve=>{
    const root=document.querySelector('.expanded-panel'), heights=[root.getBoundingClientRect().height];
    layerApp.dispatch({type:'select_panel_tab',group:${group},panel:'sizes'});
    const start=performance.now(); function sample(){heights.push(root.getBoundingClientRect().height);if(performance.now()-start<260)requestAnimationFrame(sample);else resolve(heights);}requestAnimationFrame(sample);
  })`);
  assert.ok(Math.abs(animationHeights.at(-1) - animationHeights[0]) > 10, "tabs should have different content heights");
  assert.ok(animationHeights.some(h=>Math.abs(h-animationHeights[0])>1 && Math.abs(h-animationHeights.at(-1))>1), "size changes must include intermediate frames");
  await shot("animated-tab-switch");
  await send({ type: "restore_workspace", workspace: initial });
  await wait();
  await send({ type: "invoke", command: "zen_mode" });
  await click(tab("sizes"));
  assert.equal(await expanded(), "sizes");
  // First canvas tap closes only the drawer; second tap hides chrome.
  await clickAt({ x: 850, y: 450 }); assert.equal(await expanded(), null);
  assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), false);
  await clickAt({ x: 850, y: 450 });
  assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), true);
  await send({ type: "restore_workspace", workspace: initial });
  console.log("Web customization: native pointer menus/tabs, dynamic toolbars, picker validation, live controls, four-edge drawers and restore passed.");
}
