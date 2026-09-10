import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkToolbarManager({ call, evaluate, settle }) {
  const dir = 'artifacts/ui/toolbar-manager'; await mkdir(dir, { recursive: true });
  await call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 960, deviceScaleFactor: 1, mobile: false });
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const edit = action => send({ type: 'customize', action });
  // Native dialog dismissal groups depend on real user activation, not DOM click().
  const click = async selector => {
    const point = await evaluate(`(() => { const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
    await call('Input.dispatchMouseEvent', {type:'mouseMoved',...point});
    await call('Input.dispatchMouseEvent', {type:'mousePressed',...point,button:'left',clickCount:1});
    await call('Input.dispatchMouseEvent', {type:'mouseReleased',...point,button:'left',clickCount:1}); await settle();
  };
  const model = () => evaluate('layerApp.app.toolbar_manager() ?? null');
  const shot = async name => { const image = await call('Page.captureScreenshot', { format: 'png' }); await writeFile(`${dir}/web-${name}.png`, Buffer.from(image.data, 'base64')); };
  const initial = await evaluate('layerApp.state().workspace');
  for (const theme of ['dark', 'light']) {
    await send({ type: 'restore_workspace', workspace: initial }); await send({ type: 'set_theme', theme });
    for (const name of ['Sketching', 'Painting']) {
      await edit({ type: 'duplicate_toolbar', panel: 'toolbar' });
      await edit({ type: 'toolbar_name', name }); await edit({ type: 'confirm_toolbar' });
    }
    const hidden = await evaluate("layerApp.state().workspace.layout.panels.at(-1).id");
    await edit({ type: 'set_panel_visible', panel: hidden, visible: false });
    const before = await evaluate('layerApp.state().workspace');
    await click('summary[aria-label="Workspace"]');
    assert.deepEqual(await evaluate("[...document.querySelectorAll('#workspace-menu .menu-label')].slice(-2).map(n=>n.textContent)"), ['New Toolbar…','Manage Toolbars…']);
    await click('#workspace-menu button:last-child');
    assert.equal(await evaluate("document.querySelector('#toolbar-manager').open"), true);
    assert.equal(await evaluate("document.querySelector('#delete-managed-toolbar').disabled"), true);
    assert.equal((await model()).toolbars.length, 3);
    await shot(`${theme}-initial`);
    await click(`.managed-toolbars button[data-panel="${hidden}"]`);
    assert.equal((await model()).selected, hidden);
    const selectionColor = () => evaluate("getComputedStyle(document.querySelector('.managed-toolbars [aria-pressed=true]')).backgroundColor");
    const hoveredSelection = await selectionColor();
    await call('Input.dispatchMouseEvent', {type:'mouseMoved',x:640,y:700}); await settle();
    assert.equal(await selectionColor(), hoveredSelection, 'Selection stays highlighted while hovered');
    await shot(`${theme}-selected`);
    await click('#delete-managed-toolbar'); await shot(`${theme}-confirm`);
    await call('Input.dispatchKeyEvent', {type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent', {type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27}); await settle();
    assert.equal(await evaluate("document.querySelector('#toolbar-prompt').open"), false);
    assert.ok(await model(), 'Escape dismisses only the confirmation');
    assert.equal((await model()).selected, hidden);
    assert.deepEqual(await evaluate('layerApp.state().workspace'), before);
    await click('#delete-managed-toolbar'); await click('#confirm-toolbar');
    assert.equal((await model()).toolbars.length, 2);
    assert.equal(await evaluate("document.querySelector('#delete-managed-toolbar').disabled"), true);
    await shot(`${theme}-deleted`);
    while ((await model()).toolbars.length) {
      await click('.managed-toolbars button'); await click('#delete-managed-toolbar'); await click('#confirm-toolbar');
    }
    assert.equal(await evaluate("document.querySelector('#delete-managed-toolbar').disabled"), true);
    await shot(`${theme}-empty`); await click('#toolbar-manager .dialog-close');
    assert.equal(await model(), null);
    await send({type:'invoke',command:'undo_workspace'});
    assert.equal(await evaluate("layerApp.state().workspace.layout.panels.filter(p=>p.content.kind==='toolbar').length"), 1);
  }
  assert.equal(await evaluate("document.querySelector('#status').textContent"), '');
  console.log('PASS: toolbar manager selection, hidden toolbars, cancellation, deletion, empty state, dismissal and undo in both themes');
}

export async function checkTabStyles({ call, evaluate, settle }) {
  const dir = 'artifacts/ui/group-tab-styles'; await mkdir(dir, { recursive: true });
  await call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 960, deviceScaleFactor: 1, mobile: false });
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); };
  const group = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('brushes')).id");
  for (const panel of ['sizes', 'layers']) await send({ type: 'move_panel', panel, target: { kind: 'tab', group } });
  for (const theme of ['dark', 'light']) {
    await send({ type: 'set_theme', theme });
    for (const [style, label] of [['automatic','Automatic'],['active_name','Icons and active tab name'],['icon_name','Icons and names'],['name','Names only'],['icon','Icons only']]) {
      await evaluate(`(() => { const n=document.querySelector('[data-group="${group}"] .dock-tabs > .panel-grip'),r=n.getBoundingClientRect(); n.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.x+5,clientY:r.y+5})); })()`);
      assert.deepEqual(await evaluate("[...document.querySelectorAll('.panel-context-menu:popover-open button')].slice(0,5).map(n=>n.textContent)"), ['Automatic','Icons and active tab name','Icons and names','Names only','Icons only']);
      await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(n=>n.textContent===${JSON.stringify(label)}).click()`);
      for (const active of ['brushes','sizes','layers']) {
        await evaluate(`document.querySelector('.dock-tab[data-panel="${active}"]').click()`); await settle();
        const tabs = await evaluate(`[...document.querySelectorAll('[data-group="${group}"] .dock-tab')].map(n=>({panel:n.dataset.panel,icon:!!n.querySelector('svg'),name:n.textContent.length>0,title:n.title,height:n.getBoundingClientRect().height}))`);
        assert.equal(tabs.length, 3);
        for (const tab of tabs) {
          assert.equal(tab.icon, style !== 'name');
          assert.equal(tab.name, style === 'icon_name' || style === 'name' || (['automatic','active_name'].includes(style) && tab.panel === active));
          assert.ok(tab.title); assert.equal(tab.height, 36);
        }
      }
      await evaluate('new Promise(resolve=>setTimeout(resolve,260))');
      const shot = await call('Page.captureScreenshot', { format: 'png' });
      await writeFile(`${dir}/web-${theme}-${style}.png`, Buffer.from(shot.data, 'base64'));
    }
    await send({type:'customize',action:{type:'set_tab_style',group,style:'automatic'}});
    await send({type:'move_panel',panel:'layers',target:{kind:'float',position:[850,200]}});
    const names = () => evaluate(`[...document.querySelectorAll('[data-group="${group}"] .dock-tab')].map(n=>({icon:!!n.querySelector('svg'),name:n.textContent.length>0}))`);
    assert.deepEqual(await names(), [{icon:true,name:true},{icon:true,name:true}], 'Automatic expands both names when the third tab leaves');
    const shot = await call('Page.captureScreenshot', {format:'png'});
    await writeFile(`${dir}/web-${theme}-automatic-two-tabs.png`, Buffer.from(shot.data, 'base64'));
    await send({type:'move_panel',panel:'layers',target:{kind:'tab',group}});
    assert.equal((await names()).filter(t=>t.name).length, 1, 'Automatic collapses inactive names when a third tab arrives');
  }
  assert.equal(await evaluate("document.querySelector('#status').textContent"), '');
  console.log('PASS: group context choices and active/inactive tab contents in all five styles, both themes');
}

// Real browser pointer sequences exercise capture across DOM reconciliation;
// direct Rust actions are only used to establish each test's starting layout.
export async function checkWorkspace({ call, evaluate, settle }) {
  const dir = "artifacts/ui/workspace-management/web";
  await mkdir(dir, { recursive: true });
  const wait = () => evaluate("new Promise(resolve => setTimeout(resolve, 260))");
  const send = action => evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const customize = action => send({ type: "customize", action });
  const snapshot = () => evaluate("layerApp.state().workspace");
  const group = panel => evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}))`);
  const config = panel => evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id===${JSON.stringify(panel)})`);
  const mouse = (type, p, extra = {}) => call("Input.dispatchMouseEvent", { type, ...p, ...extra });
  const point = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw new Error('Missing '+${JSON.stringify(selector)});const b=n.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
  const clickAt = async (p, count = 1) => {
    await mouse("mouseMoved", p);
    for (let n = 1; n <= count; n++) {
      await mouse("mousePressed", p, { button: "left", clickCount: n });
      await mouse("mouseReleased", p, { button: "left", clickCount: n });
    }
    await wait();
  };
  const click = async selector => clickAt(await point(selector));
  const context = async selector => {
    const p = await point(selector);
    await mouse("mouseMoved", p);
    await mouse("mousePressed", p, { button: "right", clickCount: 1 });
    await mouse("mouseReleased", p, { button: "right", clickCount: 1 });
    await settle();
    assert.equal(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"), true);
  };
  const choose = async (label, selector = ".panel-context-menu:popover-open") => {
    const p = await evaluate(`(()=>{const n=[...document.querySelectorAll(${JSON.stringify(selector + " button")})].find(n=>(n.querySelector('.menu-label')?.textContent||n.textContent)===${JSON.stringify(label)});if(!n)throw new Error('Missing menu item '+${JSON.stringify(label)});if(n.disabled)throw new Error('Disabled menu item '+${JSON.stringify(label)});const b=n.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
    await clickAt(p);
  };
  const workspaceMenu = async () => click('summary[aria-label="Workspace"]');
  const shot = async name => {
    await wait(); const image = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/${name}.png`, Buffer.from(image.data, "base64"));
  };
  const grip = panel => `.dock-group[data-panel="${panel}"] .panel-grip`;
  const tab = panel => `.dock-tab[data-panel="${panel}"]`;
  const edit = async (selector, value) => {
    await click(selector);
    await evaluate(`{const n=document.querySelector(${JSON.stringify(selector)});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('input',{bubbles:true}));}`);
    await settle();
  };
  const drag = async (start, end, during) => {
    await mouse("mouseMoved", start);
    await mouse("mousePressed", start, { button: "left", clickCount: 1 });
    for (let i = 1; i <= 6; i++) {
      await mouse("mouseMoved", { x: start.x + (end.x-start.x)*i/6, y: start.y + (end.y-start.y)*i/6 }, { button: "left", buttons: 1 });
      await settle();
    }
    if (during) await during();
    await mouse("mouseReleased", end, { button: "left", clickCount: 1 });
    await wait();
  };
  await call("Emulation.setDeviceMetricsOverride", { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
  await wait();
  const initial = await snapshot();
  const reset = async () => {
    await evaluate("document.querySelector('.panel-context-menu').hidePopover();document.querySelectorAll('details[open]').forEach(n=>n.open=false)");
    await send({ type: "restore_workspace", workspace: initial }); await wait();
  };
  const float = async (panel, position = [430, 180]) => {
    await send({ type: "move_panel", panel, target: { kind: "float", position } }); await wait(); return group(panel);
  };
  for (const theme of ["dark", "light"]) {
    await reset(); await send({ type: "set_theme", theme });
    for (const style of ["automatic", "active_name", "icon_name", "name", "icon"]) {
      await customize({ type: "set_tab_style", group: (await group("sizes")).id, style });
      await wait();
      const before = await snapshot();
      for (const hidden of [true, false]) {
        await clickAt(await point(grip("sizes")), 2);
        assert.equal((await config("sizes")).hide_tab, hidden, "First double-click toggles docked header");
        assert.equal((await group("sizes")).floating, false);
        assert.equal((await group("sizes")).tabs_visible, !hidden);
        assert.deepEqual((await snapshot()).layout.bands, before.layout.bands, "Dock dimensions remain unchanged");
        await shot(`docked-handle-${style}-${hidden}-${theme}`);
      }
    }
  }
  await reset();
  for (const edge of ["left", "right", "top", "bottom"]) {
    for (const style of ["small", "large", "labeled"]) {
      await reset();
      await customize({ type: "set_tile_style", panel: "toolbar", style });
      await send({ type: "move_panel", panel: "toolbar", target: { kind: "edge", edge, outer: true } });
      await wait();
      const natural = await group("toolbar"), oversized = await snapshot();
      oversized.layout.bands.find(b => b.root.id === natural.id).extent += 120;
      await send({ type: "restore_workspace", workspace: oversized }); await wait();
      await clickAt(await point(grip("toolbar")), 2);
      assert.deepEqual((await group("toolbar")).bounds, natural.bounds, `${edge} ${style}: first double-click restores dock fit`);
      assert.equal((await group("toolbar")).tabs_visible, false);
      await shot(`docked-toolbar-reset-${edge}-${style}`);
    }
  }
  await reset();
  for (const theme of process.argv.includes("--gestures") ? [] : ["dark", "light"]) {
    await reset(); await send({ type: "set_theme", theme });
    await workspaceMenu(); await shot(`workspace-menu-${theme}`);
    const menuModel = await evaluate("layerApp.app.workspace_menu()");
    assert.equal(menuModel.sections.length, 4);
    await choose("Brushes panel", "#workspace-menu"); assert.equal(await group("brushes"), undefined);
    await workspaceMenu(); await choose("Brushes panel", "#workspace-menu"); assert.ok(await group("brushes"));
    await workspaceMenu(); await choose("New Toolbar…", "#workspace-menu");
    await edit("#toolbar-name", `Review ${theme}`);
    await click(".tool-choice:first-child"); await click("#confirm-tools");
    let custom = await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.content.name==='Review ${theme}').id`);
    await context(grip(custom)); await choose(`Rename Review ${theme} toolbar…`);
    assert.equal(await evaluate("document.querySelector('#toolbar-prompt-name').value"), `Review ${theme}`);
    await edit("#toolbar-prompt-name", "Brushes");
    assert.equal(await evaluate("document.querySelector('#confirm-toolbar').disabled"), true);
    await shot(`rename-error-${theme}`);
    await edit("#toolbar-prompt-name", `Study ${theme}`); await click("#confirm-toolbar");
    assert.equal((await config(custom)).content.name, `Study ${theme}`);
    await context(grip(custom)); await choose(`Duplicate Study ${theme} toolbar…`);
    assert.notEqual(await evaluate("document.querySelector('#toolbar-prompt-name').value"), `Study ${theme}`);
    await shot(`duplicate-${theme}`); await click("#confirm-toolbar");
    const duplicate = await evaluate(`layerApp.state().workspace.layout.panels.filter(p=>p.content.kind==='toolbar').at(-1).id`);
    assert.notEqual(custom, duplicate);
    assert.deepEqual((await config(custom)).content.tiles.map(t=>t.control), (await config(duplicate)).content.tiles.map(t=>t.control));
    await workspaceMenu(); await choose("Manage Toolbars…", "#workspace-menu");
    await click(`.managed-toolbars button[data-panel="${duplicate}"]`);
    await click("#delete-managed-toolbar");
    await shot(`delete-${theme}`);
    assert.match(await evaluate("document.querySelector('#toolbar-prompt').textContent"), /Undo Workspace Change/);
    await click("#confirm-toolbar"); assert.equal(await config(duplicate), undefined);
    await click("#toolbar-manager .dialog-close");
    await workspaceMenu(); await choose("Undo Workspace Change", "#workspace-menu"); assert.ok(await config(duplicate));
    await workspaceMenu(); await choose("Redo Workspace Change", "#workspace-menu"); assert.equal(await config(duplicate), undefined);
    await context(grip(custom)); await choose(`Hide Study ${theme} toolbar`);
    assert.equal(await group(custom), undefined); assert.ok(await config(custom));
    await workspaceMenu(); await choose(`Study ${theme} toolbar`, "#workspace-menu"); assert.ok(await group(custom));
    await reset();
    const destination = (await group("layers")).id;
    await context(grip("layers")); await choose("Add built-in panel"); await shot(`add-panel-${theme}`); await choose("Brush size panel");
    assert.equal((await group("sizes")).id, destination);
    assert.equal(await evaluate(`document.querySelector('[data-group="${destination}"] .tab-list').scrollWidth<=document.querySelector('[data-group="${destination}"] .tab-list').clientWidth`), true);
    await context(grip("sizes")); await choose("Add Toolbar"); await shot(`add-toolbar-${theme}`); await choose("Tools toolbar");
    assert.equal((await group("toolbar")).id, destination);
    await context(tab("toolbar"));
    assert.deepEqual(await evaluate("[...document.querySelectorAll('.panel-context-menu .menu-label')].map(n=>n.textContent)"), ["Configure Tools toolbar…", "Hide Tools toolbar"]);
    await choose("Configure Tools toolbar…");
    await shot(`toolbar-configuration-${theme}`);
    assert.equal(await evaluate("document.querySelectorAll('.panel-configuration .toolbar-options button').length>6"), true);
    assert.equal(await evaluate("[...document.querySelectorAll('.panel-configuration .toolbar-options button')].some(n=>n.textContent==='Icons only')"), false);
    await customize({ type: "close_expanded" });
    await context(grip("toolbar")); await choose("Icons only");
    assert.equal(await evaluate(`document.querySelector('${tab("toolbar")} svg').dataset.asset`), await evaluate("layerApp.app.panel_view('toolbar').tiles[0].icon"));
    await reset(); await float("toolbar");
    for (const mode of ["compact", "vertical", "horizontal"]) {
      if (mode !== "compact") await clickAt(await point(grip("toolbar")), 2);
      assert.equal((await snapshot()).layout.floating[0].toolbar_layout, mode);
      for (const [style, label, width, height, glyph] of [["large", "Large Tiles", 72, 72, 32], ["labeled", "Labeled Tiles", 108, 72, 16], ["small", "Small Tiles", 36, 36, 16]]) {
        await context(grip("toolbar")); await choose(label);
        assert.equal((await snapshot()).layout.floating[0].toolbar_layout, mode, "tile size must retain the selected preset");
        const g = await group("toolbar");
        const bounds = await evaluate(`(()=>{const strip=document.querySelector('[data-panel="toolbar"] .toolbar-controls'),b=strip.getBoundingClientRect();return[...strip.querySelectorAll('[data-tile]')].map(n=>{const r=n.getBoundingClientRect(),s=n.querySelector('svg').getBoundingClientRect();return{x:r.x-b.x,y:r.y-b.y,width:r.width,height:r.height,glyph:s.width,label:n.querySelector('.tile-label')?.textContent}})})()`);
        for (const b of bounds) { assert.equal(b.width, width); assert.equal(b.height, height); assert.equal(b.glyph, glyph); assert.ok(b.x>=0&&b.y>=0&&b.x+b.width<=g.bounds.width+.5&&b.y+b.height<=g.bounds.height+.5); if (style === "labeled") assert.ok(b.label); }
        if (mode === "horizontal") assert.ok(bounds.every(b=>b.y===0));
        if (mode === "vertical") assert.ok(bounds.every(b=>b.x===0));
        await shot(`floating-${mode}-${style}-${theme}`);
      }
    }
    console.log(`Web workspace menus and all toolbar layouts passed (${theme}).`);
  }
  await reset();
  // One tab tears off as a group, keeps following after DOM replacement, and
  // undoes as one edit. Pointer capture must live on the stable workspace.
  const beforeDrag = await snapshot();
  await drag(await point(tab("sizes")), { x: 540, y: 440 }, async () => {
    assert.equal((await group("sizes")).floating, true);
    assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"), true);
    assert.equal((await config("sizes")).hide_tab, true);
    await shot("live-panel-tearoff");
  });
  assert.equal((await group("sizes")).floating, true);
  await send({ type: "invoke", command: "undo_workspace" }); assert.deepEqual(await snapshot(), beforeDrag);
  await send({ type: "invoke", command: "redo_workspace" }); await wait();
  await context(grip("sizes")); await choose("Configure Brush size panel…");
  assert.equal(await evaluate("layerApp.state().customization.expanded"), "sizes");
  await customize({ type: "close_expanded" }); await wait();
  // Every hitbox is outside the border, including the top (inside means drag).
  for (const edge of ["left", "right", "top", "bottom", "top_left", "top_right", "bottom_left", "bottom_right"]) {
    const g = await group("sizes"), handle = g.resize_handles.find(h=>h.edge===edge);
    assert.ok(handle, edge);
    const b = handle.bounds, p = {x:b.x+b.width/2,y:b.y+b.height/2};
    assert.ok(p.x<g.bounds.x||p.x>g.bounds.x+g.bounds.width||p.y<g.bounds.y||p.y>g.bounds.y+g.bounds.height);
    const before = await snapshot();
    await drag(p, {x:p.x+(edge.includes("left")?-16:edge.includes("right")?16:0), y:p.y+(edge.includes("top")?-16:edge.includes("bottom")?16:0)});
    assert.notDeepEqual((await group("sizes")).bounds, g.bounds, edge);
    await send({type:"invoke",command:"undo_workspace"}); assert.deepEqual(await snapshot(), before);
  }
  // First double-click after resizing resets; the next toggles tabs.
  let g = await group("sizes");
  const h = g.resize_handles.find(h=>h.edge==="bottom_right").bounds;
  await drag({x:h.x+3,y:h.y+3}, {x:h.x+63,y:h.y+43});
  assert.notEqual((await snapshot()).layout.floating[0].height, undefined);
  await clickAt(await point(grip("sizes")), 2);
  assert.equal((await snapshot()).layout.floating[0].height, undefined);
  assert.equal((await config("sizes")).hide_tab, true);
  await clickAt(await point(grip("sizes")), 2); assert.equal((await config("sizes")).hide_tab, false);
  await shot("panel-cycle-tab-shown");
  await clickAt(await point(grip("sizes")), 2); assert.equal((await config("sizes")).hide_tab, true);
  await shot("panel-cycle-tab-hidden");
  await reset(); await float("toolbar");
  g = await group("toolbar");
  const handle = g.resize_handles.find(h=>h.edge==="bottom_right").bounds;
  await drag({x:handle.x+3,y:handle.y+3},{x:handle.x+73,y:handle.y+53});
  await clickAt(await point(grip("toolbar")), 2);
  assert.equal((await snapshot()).layout.floating[0].height, undefined);
  assert.equal((await snapshot()).layout.floating[0].toolbar_layout, "compact");
  await clickAt(await point(grip("toolbar")), 2);
  assert.equal((await snapshot()).layout.floating[0].toolbar_layout, "vertical");
  // The docked toolbar must not snap back when its original grip is detached.
  await reset();
  await drag(await point(grip("toolbar")), {x:650,y:430}, async () => {
    assert.equal((await group("toolbar")).floating, true); await shot("live-toolbar-tearoff");
  });
  assert.equal((await group("toolbar")).floating, true);
  // Touch uses that same stable capture and core transaction.
  await reset(); await call("Emulation.setTouchEmulationEnabled", {enabled:true,maxTouchPoints:1});
  const touch = (type,p) => call("Input.dispatchTouchEvent",{type,touchPoints:p?[{...p,id:1,radiusX:1,radiusY:1,force:1}]:[]});
  await touch("touchStart", await point(tab("sizes")));
  await touch("touchMove", {x:540,y:440}); await wait();
  assert.equal((await group("sizes")).floating, true);
  await touch("touchMove", {x:620,y:480}); await touch("touchEnd"); await wait();
  assert.equal((await group("sizes")).floating, true);
  await call("Emulation.setTouchEmulationEnabled", {enabled:false}); await shot("touch-floating-panel");
  await reset(); await float("sizes", [450,320]); await float("layers", [720,320]);
  await send({type:"invoke",command:"zen_mode"});
  await mouse("mouseMoved", {x:620,y:600}); await wait();
  const zenHidden = () => evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')");
  assert.equal(await zenHidden(), true);
  await drag(await point(grip("sizes")), {x:570,y:680}, async () => {
    assert.equal(await zenHidden(), true);
    assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"), true);
    assert.equal(await evaluate("getComputedStyle(document.querySelector('.floating-panel')).opacity"), "1");
    await shot("zen-floating-drag");
  });
  assert.equal(await zenHidden(), true);
  // The empty bottom edge cannot capture a drop while docks are hidden.
  await drag(await point(grip("sizes")), {x:570,y:890}, async () => {
    assert.equal(await zenHidden(), true); assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"), true);
  });
  assert.equal((await group("sizes")).floating, true);
  const target = (await group("layers")).bounds;
  await drag(await point(grip("sizes")), {x:target.x+target.width/2,y:target.y+target.height/2}, async () => {
    assert.equal(await zenHidden(), true);
    assert.equal(await evaluate("document.querySelector('.drop-indicator').dataset.kind"), "tab");
    await shot("zen-floating-tab-merge");
  });
  assert.equal((await group("sizes")).id, (await group("layers")).id);
  assert.equal((await group("sizes")).floating, true);
  // An occupied edge reveals docks through the drag, then normal proximity
  // decides visibility on release. No persistent post-drop pin remains.
  const start = await point(grip("sizes"));
  await mouse("mouseMoved", start); await mouse("mousePressed", start, {button:"left",clickCount:1});
  await mouse("mouseMoved", {x:20,y:500}, {button:"left",buttons:1}); await wait();
  assert.equal(await zenHidden(), false);
  await mouse("mouseMoved", {x:620,y:680}, {button:"left",buttons:1}); await wait();
  assert.equal(await zenHidden(), false);
  await mouse("mouseReleased", {x:620,y:680}, {button:"left",clickCount:1}); await wait();
  assert.equal(await zenHidden(), true);
  await reset();
  // Stacked dock dividers use the same stable pointer path as float resizing.
  const divider = await evaluate("layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.axis==='vertical')");
  const p = {x:divider.bounds.x+divider.bounds.width/2,y:divider.bounds.y+divider.bounds.height/2};
  const beforeDivider = await snapshot(), beforeBounds = (await group("sizes")).bounds;
  await drag(p, {x:p.x,y:p.y-40});
  assert.notDeepEqual((await group("sizes")).bounds, beforeBounds);
  await send({type:"invoke",command:"undo_workspace"}); assert.deepEqual(await snapshot(), beforeDivider);
  await reset();
  const toolbar = await float("toolbar");
  await send({type:"move_panel",panel:"sizes",target:{kind:"tab",group:toolbar.id,index:null}}); await wait();
  g = await group("sizes");
  const corner = g.resize_handles.find(h=>h.edge==='bottom_right').bounds;
  await drag({x:corner.x+3,y:corner.y+3},{x:corner.x+70,y:corner.y+60});
  await context(tab("sizes")); await choose("Hide Brush size panel");
  assert.equal((await group("toolbar")).bounds.width, 112);
  assert.equal((await snapshot()).layout.floating[0].height, undefined);
  assert.equal((await group("toolbar")).tabs_visible, false);
  await shot("toolbar-group-collapse");
  // A tab in a multi-tab float moves individually, unlike a singleton tab.
  await reset(); const source = await float("sizes");
  await send({type:"move_panel",panel:"layers",target:{kind:"tab",group:source.id,index:null}}); await wait();
  await drag(await point(tab("layers")), {x:810,y:650});
  assert.notEqual((await group("sizes")).id, (await group("layers")).id);
  assert.equal((await group("layers")).floating, true);
  await reset(); await float("sizes");
  await send({type:"move_panel",panel:"toolbar",target:{kind:"edge",edge:"left",outer:true}}); await wait();
  const narrow = (await group("toolbar")).bounds;
  assert.equal(narrow.width,36);
  await drag(await point(grip("sizes")), {x:narrow.x+18,y:narrow.y+narrow.height/2}, async () => {
    assert.equal(await evaluate("document.querySelector('.drop-indicator').dataset.kind"), "tab");
    await shot("narrow-ribbon-tab-target");
  });
  assert.equal((await group("sizes")).id, (await group("toolbar")).id);
  await reset(); await float("toolbar");
  await drag(await point(grip("toolbar")), {x:600,y:60}, async () => {
    assert.equal(await evaluate("document.querySelector('.drop-indicator').getBoundingClientRect().y"), 48);
    await shot("top-snap-below-header");
  });
  assert.equal((await group("toolbar")).floating, false);
  assert.equal((await group("toolbar")).axis, "horizontal");
  // Keyboard workspace history is separate from drawing undo.
  await evaluate(`{const fire=(type)=>document.body.dispatchEvent(new KeyboardEvent(type,{key:'z',ctrlKey:true,altKey:true,bubbles:true,cancelable:true}));fire('keydown');fire('keyup');}`);
  assert.equal((await group("toolbar")).floating, true);
  await send({type:"invoke",command:"redo_workspace"}); assert.equal((await group("toolbar")).floating, false);
  await reset();
  const left = (await group("sizes")).bounds, right = (await group("layers")).bounds;
  await drag(await point(tab("sizes")), {x:right.x+5,y:right.y+right.height/2});
  assert.ok(Math.abs((await group("sizes")).bounds.width-left.width)<1);
  assert.ok(Math.abs((await group("layers")).bounds.width-right.width)<1);
  await shot("side-dock-widths");
  await reset();
  const tabGroup = (await group("layers")).id;
  for (const panel of ["brushes","sizes","toolbar"])
    await send({type:"move_panel",panel,target:{kind:"tab",group:tabGroup,index:null}});
  // Layers now enforces a six-tile minimum width; use names to exercise
  // overflow deliberately rather than relying on an illegally narrow group.
  await customize({type:"set_tab_style",group:tabGroup,style:"name"});
  await wait();
  const wide = (await group("toolbar")).bounds;
  const edgeDivider = await evaluate(`layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.axis==='horizontal'&&Math.abs(d.bounds.x+d.bounds.width-${wide.x})<1)`);
  const shrinkStart = {x:edgeDivider.bounds.x+edgeDivider.bounds.width/2,y:edgeDivider.bounds.y+edgeDivider.bounds.height/2};
  await drag(shrinkStart, {x:shrinkStart.x+wide.width-180,y:shrinkStart.y});
  assert.equal(await evaluate(`(()=>{const n=document.querySelector('[data-group="${tabGroup}"] .tab-list');return n.scrollWidth>n.clientWidth})()`),true);
  const tight = (await group("toolbar")).bounds;
  await drag(await point(tab("layers")), {x:tight.x+tight.width/2,y:tight.y+tight.height/2}, async () => {
    const marker = await evaluate(`(()=>{const b=document.querySelector('.drop-indicator').getBoundingClientRect(),g=document.querySelector('[data-group="${tabGroup}"] .dock-tabs>.panel-grip').getBoundingClientRect();return{width:b.width,top:b.y,right:b.right,grip:g.x}})()`);
    assert.equal(marker.width,3); assert.equal(marker.top,tight.y); assert.ok(Math.abs(marker.right-marker.grip)<1);
    await shot("overflow-tab-append");
  });
  assert.equal((await group("layers")).panels.at(-1), "layers");
  await reset();
  console.log("Web workspace gestures: real tear-off/touch/capture, eight-edge resize, first double-click reset, layout cycles, docking/history and Zen floating-only merges passed.");
}

export async function checkCustomization({ call, evaluate, settle, canvasPixels }) {
  const dir = "artifacts/ui/workspace-management/web"; await mkdir(dir, { recursive: true });
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
  const menuItem = async (label, selector = ".panel-context-menu:popover-open") => {
    const point = await evaluate(`(()=>{const node=[...document.querySelectorAll(${JSON.stringify(selector + " button")})]
      .find(n=>(n.querySelector('.menu-label')?.textContent || n.textContent)===${JSON.stringify(label)});
      if(!node)throw new Error('Menu item missing: '+${JSON.stringify(label)}); const b=node.getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
    await clickAt(point);
  };
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
    assert.deepEqual(await context(tab("sizes")), ["Show tab bar", "Configure Brush size panel…", "Hide Brush size panel"]);
    await shot(`panel-menu-${theme}`);
    await menuItem('Configure Brush size panel…');
    assert.equal(await evaluate(`!!document.querySelector('${tab("sizes")} svg')`), true);
    assert.equal(await expanded(), "sizes");
    assert.equal(await evaluate("document.querySelectorAll('.expanded-panel').length"), 1);
    assert.deepEqual(await evaluate(`(()=>{const root=document.querySelector('.expanded-panel');return [getComputedStyle(root).filter,...['.panel-preview','.panel-configuration'].map(s=>{const c=getComputedStyle(root.querySelector(s));return [c.boxShadow,c.filter]})];})()`),
      ["drop-shadow(rgba(0, 0, 0, 0.4) 0px 8px 12px)", ["none", "none"], ["none", "none"]],
      "one stronger shadow must wrap both columns, never their seam");
    await evaluate("window.originalPanel=document.querySelector('.sizes-panel');window.originalParent=originalPanel.parentElement;");
    await shot(`expanded-sizes-${theme}`);
    await click('.panel-configuration [data-visible="brush_opacity"]');
    assert.equal(await evaluate("document.querySelector('.sizes-panel [data-control=brush_opacity]').hidden"), false);
    await click('.panel-configuration [data-control=brush_opacity] .number-value');
    await evaluate(`{const input=document.querySelector('.panel-configuration [data-control=brush_opacity] .number-entry');input.value='42';input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));}`);
    assert.ok(Math.abs(await evaluate("layerApp.state().brush.opacity") - .42) < .001);
    assert.equal(await evaluate("document.querySelector('.sizes-panel [data-control=brush_opacity] .number-entry').value"), "42.0");
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
    for (const edge of ["left", "right"]) {
      await send({ type: "move_panel", panel: "sizes", target: { kind: "edge", edge, outer: false } }); await wait();
      await click(tab("sizes")); assert.equal(await expanded(), "sizes");
      const bounds = await evaluate(`(()=>{const root=document.querySelector('.expanded-panel'),p=root.querySelector('.panel-preview').getBoundingClientRect(),c=root.querySelector('.panel-configuration').getBoundingClientRect();return{p:{top:p.top,bottom:p.bottom},c:{top:c.top,bottom:c.bottom},count:root.querySelectorAll('.panel-preview').length};})()`);
      assert.equal(bounds.c.top - bounds.p.top, 36); assert.equal(bounds.c.bottom, bounds.p.bottom); assert.equal(bounds.count, 1);
      await shot(`expanded-${edge}-${theme}`); await click(tab("sizes"));
    }
    await send({ type: "restore_workspace", workspace: initial }); await wait();
    assert.deepEqual(await context('[data-panel="layers"] .dock-tabs > .panel-grip'), ["Automatic", "Icons and active tab name", "Icons and names", "Names only", "Icons only", "Show tab bar", "Add built-in panel", "Add Toolbar", "New Toolbar…"]);
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
    assert.ok((await context(`[data-panel="${custom}"] .toolbar-controls`)).includes("Add Tools…"));
    await menuItem("Add Tools…");
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
  await send({ type: "move_panel", panel: "sizes", target: { kind: "float", position: [450, 180] } });
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
  console.log("Web customization: native pointer menus/tabs, dynamic toolbars, picker validation, live controls, side drawers and restore passed.");
}
