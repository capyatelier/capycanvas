import assert from "node:assert/strict";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { crc32 } from "node:zlib";

export async function checkPalettes({ call, evaluate, settle, reload }) {
  const output = process.env.LAYER_TEST_ARTIFACTS || "artifacts/palettes/web";
  await mkdir(output, { recursive: true });
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const idle = async () => {
    await pause(80);
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+15000;(function poll(){const v=JSON.parse(layerApp.app.workspace_view());if(v?.ready&&!v.busy&&!v.switcher_busy)resolve();else if(performance.now()>end)reject(Error('Workspace busy'));else setTimeout(poll,30);})();})`);
    await settle();
  };
  const wait = async (expression, message = expression, timeout = 10000) => {
    const end = Date.now() + timeout;
    while (!(await evaluate(expression))) { assert.ok(Date.now() < end, `Timed out: ${message}`); await pause(40); }
  };
  const shot = async name => { const r = await call("Page.captureScreenshot", { format: "png" }); await writeFile(`${output}/${name}.png`, Buffer.from(r.data, "base64")); };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await idle(); };
  const library = () => evaluate("JSON.parse(JSON.stringify(layerApp.state().colors.library,(_,v)=>typeof v==='bigint'?Number(v):v))");
  const active = async () => { const l = await library(); return l.palettes.find(p => p.id === l.active); };
  const panelView = () => evaluate("JSON.parse(JSON.stringify(layerApp.app.palette_panel(),(_,v)=>typeof v==='bigint'?Number(v):v))");
  const rect = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()`);
  const viewport = async () => {
    const read = () => evaluate("[visualViewport.offsetLeft,visualViewport.offsetTop,visualViewport.height,innerHeight,!!document.activeElement?.matches('input,textarea')]");
    for (let last = await read(), i = 0; i < 20; i++) {
      if (!last[1] && last[2] >= last[3] - 1 && !last[4]) return last;
      await pause(200); const next = await read();
      if (JSON.stringify(next) === JSON.stringify(last)) return next;
      last = next;
    }
    return read();
  };
  const center = async (selector, fx = .5, fy = .5) => { const [dx, dy] = await viewport(), r = await rect(selector); return { x: r.x + r.width * fx - dx, y: r.y + r.height * fy - dy }; };
  const visible = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});return !!n&&n.getClientRects().length>0&&!n.closest('[hidden]');})()`);
  const row = async name => { const selector = `${P} .palette-choice[data-name="${name}"]`; await evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollIntoView({block:'nearest'})`); await settle(); return selector; };
  const menuOpen = () => evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')");
  const menuLabels = () => evaluate("[...document.querySelectorAll('.panel-context-menu .menu-label')].map(n=>n.textContent)");
  const closeMenu = () => evaluate("document.querySelector('.panel-context-menu').hidePopover()");
  const menuItem = label => evaluate(`[...document.querySelectorAll('.panel-context-menu button')].find(n=>n.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})?.click()`);
  let device = "mouse", point = { x: 0, y: 0 };
  const pointer = async (type, p = point, button = "left") => {
    point = p;
    if (device === "touch") await call("Input.dispatchTouchEvent", { type: { down: "touchStart", move: "touchMove", up: "touchEnd", cancel: "touchCancel" }[type], touchPoints: ["up", "cancel"].includes(type) ? [] : [{ id: 1, ...p }] });
    else await call("Input.dispatchMouseEvent", { type: { down: "mousePressed", move: "mouseMoved", up: "mouseReleased" }[type], ...p, button, buttons: type === "up" ? 0 : button === "right" ? 2 : 1, clickCount: 1, pointerType: device, force: type === "up" ? 0 : .5 });
    await pause(25);
  };
  const tap = async (selector, kind = "mouse") => {
    device = kind; const p = await center(selector);
    await evaluate(`window.paletteTap=new Promise(resolve=>{const done=()=>{clearTimeout(timer);document.removeEventListener('click',done,true);resolve();};const timer=setTimeout(done,1000);document.addEventListener('click',done,true);})`);
    if (kind !== "touch") await pointer("move", p);
    await pointer("down", p); await pointer("up", p);
    await evaluate("paletteTap"); await idle();
  };
  const key = async (key, modifiers = 0, code = key) => {
    const vk = { Escape: 27, Enter: 13, F10: 121, z: 90, Z: 90, y: 89 }[key] ?? key.toUpperCase().charCodeAt(0);
    const text = key.length === 1 && !(modifiers & 2) ? { text: key } : key === "Enter" ? { text: "\r" } : {};
    await call("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: vk, modifiers, ...text });
    await call("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: vk, modifiers });
    await idle();
  };
  const type = async text => { await call("Input.insertText", { text }); await idle(); };
  const workspaceView = () => evaluate("JSON.parse(layerApp.app.workspace_view())");
  const switchTo = async title => {
    const row = (await workspaceView()).switcher.find(r => r.title === title);
    await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify({ type: "switch", id: row.id }))});null`); await idle();
    await wait(`JSON.parse(layerApp.app.workspace_view()).id===${JSON.stringify(row.id)}`);
    await idle();
  };
  const tabs = group => evaluate(`[...document.querySelectorAll('.dock-group[data-group="${group}"] .tab-list .dock-tab')].map(n=>n.dataset.panel)`);
  const groupOf = panel => evaluate(`[...document.querySelectorAll('.dock-group')].find(n=>n.querySelector('.tab-list .dock-tab[data-panel="${panel}"]'))?.dataset.group`);
  const P = ".dock-group .palettes-panel";
  const tile = index => `${P} .palette-swatches > .palette-tile:nth-child(${index + 1})`;
  const order = async () => (await active()).swatches.map(s => s.id);
  const selectTab = async (group, panel) => { if (!(await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.some(g=>g.id===${group}&&g.active===${JSON.stringify(panel)})`))) await send({ type: "select_panel_tab", group, panel }); };
  const selectPalettes = async () => { const group = Number(await groupOf("palettes")); await selectTab(group, "palettes"); return group; };

  if (await evaluate("layerApp.state().workspace.zen_mode")) await send({ type: "invoke", command: "zen_mode" });
  await switchTo("Paint");
  if (await evaluate("layerApp.state().customization.expanded != null")) await send({ type: "customize", action: { type: "close_expanded" } });
  const colorGroup = Number(await groupOf("color"));
  assert.deepEqual(await tabs(colorGroup), ["color", "palettes"], "Palettes follows Color in Paint");
  const modelTabs = panel => evaluate(`(l=>[...l.groups.map(g=>g.panels),...l.collapsed.flatMap(c=>c.groups.map(g=>g.icons.map(i=>i.panel)))].find(p=>p.includes(${JSON.stringify(panel)})))(layerApp.app.layout(innerWidth,innerHeight))`);
  assert.deepEqual(await modelTabs("brushes"), ["brushes", "stats"], "Diagnostics follows Tool Set");
  assert.deepEqual(await modelTabs("navigator"), ["navigator", "proof"], "Proof follows Navigator");
  const measurement = await evaluate("JSON.parse(JSON.stringify(layerApp.app.panel_measurements().find(m=>m.panel==='palettes')))");
  assert.ok(measurement.content_height > 0 && measurement.scroll.unit_height === 44 && measurement.scroll.fixed_height > 0, `palette measurement: ${JSON.stringify(measurement)}`);
  const groupRect = () => rect(`.dock-group[data-group="${colorGroup}"]`);
  await selectTab(colorGroup, "color");
  const fitted = await groupRect();
  await selectPalettes();
  assert.deepEqual(await groupRect(), fitted, "choosing Palettes does not resize the fitted Color group");
  await selectTab(colorGroup, "color");
  assert.deepEqual(await groupRect(), fitted);
  await selectPalettes();
  let lib = await library();
  const starters = ["Ocean Study", "Pixel Arcade", "Dark Fantasy", "Pop Art", "Candy Pastels", "Riso Print", "Synthwave", "Seventies Print", "Woodblock", "Ink"];
  assert.deepEqual(lib.palettes.map(p => p.name).filter(n => starters.includes(n)), starters, "ten starter palettes");
  for (const leftover of lib.palettes.filter(p => /^(Studies|Pop Art Copy|Krita Set)( \d+)?$/.test(p.name))) await send({ type: "color", action: { op: "library", action: { op: "remove_palette", id: Number(leftover.id) } } });
  lib = await library();
  const ocean = lib.palettes.find(p => p.name === "Ocean Study");
  for (const swatch of ocean.swatches.filter(s => /^Harbor Mist/.test(s.name))) await send({ type: "color", action: { op: "library", action: { op: "remove", id: Number(swatch.id) } } });
  if (lib.active !== ocean.id) await send({ type: "color", action: { op: "library", action: { op: "select_palette", id: Number(ocean.id) } } });
  lib = await library();
  if (!lib.history.length) assert.equal(await evaluate(`document.querySelectorAll('${P} .palette-history .palette-empty').length`), 5, "empty history placeholders");
  const initialOrder = await order();
  assert.equal(await evaluate(`document.querySelectorAll('${P} .palette-swatches > .palette-tile[data-id]').length`), initialOrder.length);
  assert.ok(await visible(`${P} .palette-swatches > .palette-add`), "trailing add tile");
  const panelRect = await rect(P);
  assert.ok(panelRect.width >= 280, "minimum palette width");
  const columns = await evaluate(`getComputedStyle(document.querySelector('${P} .palette-swatches')).gridTemplateColumns.split(' ').length`);
  assert.equal(columns, Math.floor((panelRect.width - 16 + 4) / 44));
  await shot("paint-dark");

  let view = await panelView();
  const used = view.swatches.findIndex((s, i) => i >= 2 && JSON.stringify(s.color) !== JSON.stringify(view.history[0]?.color));
  const history = JSON.stringify(lib.history);
  await tap(tile(used));
  assert.equal(await evaluate(`document.querySelector('${tile(used)}').classList.contains('selected')`), true, "tap selects the swatch");
  assert.equal(await evaluate(`document.querySelector('${P} .palette-name').textContent`), view.swatches[used].name);
  assert.equal(JSON.stringify((await library()).history), history, "choosing a swatch is not history");
  await evaluate('layerApp.dispatch({type:"select_layer",id:layerApp.state().layers.find(l=>l.label==="Current ink").id})');
  await wait("layerApp.app.brush_ready()");
  const canvas = await rect("#canvas");
  device = "pen";
  await pointer("move", { x: canvas.x + canvas.width * .45, y: canvas.y + canvas.height * .5 });
  await pointer("down");
  for (let i = 1; i <= 6; i++) await pointer("move", { x: canvas.x + canvas.width * (.45 + i * .01), y: canvas.y + canvas.height * .5 });
  await pointer("up");
  await wait(`JSON.stringify(layerApp.state().colors.library.history[0])===${JSON.stringify(JSON.stringify(view.swatches[used].color))}`, "painting records history");
  await wait(`document.querySelector('${P} .palette-history .palette-recent')?.title===${JSON.stringify(view.swatches[used].detail.replace(view.swatches[used].name, "Recently used"))}`);
  view = await panelView();
  assert.equal(JSON.stringify(view.history[0].color), JSON.stringify(view.swatches[used].color), "history stores the exact used definition");
  const usedHistory = JSON.stringify((await library()).history);

  const footer = await rect(`${P} .palette-footer`);
  await tap(`${P} [data-palette-history=expand]`);
  assert.ok(await visible(`${P} .palette-expanded`), "history expands");
  assert.equal(await evaluate(`document.querySelector('${P} .palette-normal').inert`), true, "covered controls are inert");
  assert.deepEqual(await rect(`${P} .palette-footer`), footer, "footer stays in place");
  assert.deepEqual(await rect(P), panelRect, "panel height is stable");
  await shot("history-expanded");
  await tap(`${P} [data-palette-history=collapse]`);
  assert.equal(await visible(`${P} .palette-expanded`), false);
  await tap(`${P} [data-palette-history=expand]`);
  await key("Escape");
  assert.equal(await visible(`${P} .palette-expanded`), false, "Escape collapses history first");

  await send({ type: "color", action: { op: "definition", color: { space: "DisplayP3", rgba: [0.9, 0.3, 0.2, 1] } } });
  const count = (await active()).swatches.length;
  await tap(`${P} .palette-add`);
  lib = await library();
  let palette = lib.palettes.find(p => p.id === lib.active);
  assert.equal(palette.swatches.length, count + 1, "+ adds the current color");
  assert.equal(palette.swatches.at(-1).color.space, "DisplayP3", "wide-gamut definition is retained");
  assert.equal(await evaluate(`[...document.querySelectorAll('${P} .palette-swatches > .palette-tile[data-id]')].at(-1).classList.contains('selected')`), true, "added swatch becomes selected");
  assert.equal(JSON.stringify(lib.history), usedHistory, "saving is not history");

  await tap(`${P} .palette-name`);
  assert.ok(await visible(`${P} .palette-editor`));
  await evaluate(`document.querySelector('${P} .palette-editor').select()`);
  await type("Harbor   Mist ");
  await key("Enter");
  lib = await library(); palette = lib.palettes.find(p => p.id === lib.active);
  assert.equal(palette.swatches.at(-1).name, "Harbor Mist", "names are normalized by Rust");
  assert.equal(await visible(`${P} .palette-editor`), false);
  await tap(tile(0));
  await tap(`${P} .palette-name`);
  await evaluate(`document.querySelector('${P} .palette-editor').select()`);
  await type("harbor mist");
  await key("Enter");
  assert.ok(await visible(`${P} .palette-editor`), "duplicate names keep the editor open");
  assert.match(await evaluate(`document.querySelector('${P} .palette-message').textContent`), /already has that name/);
  assert.equal(await evaluate(`document.querySelector('${P} .palette-editor').classList.contains('error')`), true);
  await key("Escape");
  assert.equal(await visible(`${P} .palette-editor`), false, "Escape cancels editing");
  assert.equal(await visible(`${P} .palette-message`), false);
  assert.equal((await active()).swatches[0].name, palette.swatches[0].name);

  await tap(`${P} .palette-selector`, "touch");
  assert.ok(await visible(`${P} .palette-chooser`), "selector opens the chooser");
  assert.equal(await evaluate(`document.activeElement.dataset.name`), "Ocean Study", "touch focuses the active row, not search");
  await key("Escape");
  await tap(`${P} .palette-selector`);
  assert.ok(await visible(`${P} .palette-chooser`), "selector opens the chooser");
  assert.equal(await evaluate(`document.activeElement===document.querySelector('${P} .palette-search')`), true, "search is focused");
  assert.equal(await evaluate(`document.querySelectorAll('${P} .palette-choice').length`), (await library()).palettes.length);
  assert.equal(await evaluate(`document.querySelector('${P} .palette-choice[aria-selected=true]').dataset.name`), "Ocean Study", "active row is highlighted");
  await shot("chooser");
  await type("zzz");
  assert.ok(await visible(`${P} .palette-no-results`), "no matching palettes");
  await evaluate(`(()=>{const s=document.querySelector('${P} .palette-search');s.value='';s.dispatchEvent(new Event('input'));})()`);
  await type("ink");
  assert.deepEqual(await evaluate(`[...document.querySelectorAll('${P} .palette-choice')].filter(n=>!n.hidden).map(n=>n.dataset.name)`), ["Ink"]);
  await tap(`${P} .palette-choice[data-name="Ink"]`);
  assert.equal((await active()).name, "Ink", "choosing a palette selects it");
  assert.equal(await visible(`${P} .palette-chooser`), false, "and closes the chooser");
  await tap(`${P} .palette-selector`);
  await key("Escape");
  assert.equal(await visible(`${P} .palette-chooser`), false, "Escape closes the chooser");
  await tap(`${P} .palette-selector`);
  await evaluate(`(()=>{const s=document.querySelector('${P} .palette-search');s.value='';s.dispatchEvent(new Event('input'));})()`);
  await tap(`${P} .palette-choice[data-name="Ocean Study"]`);

  device = "mouse";
  await pointer("move", await center(tile(1)));
  await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
  assert.equal(await menuOpen(), true, "secondary click opens the swatch menu");
  assert.deepEqual(await menuLabels(), ["Rename Color…", "Remove Color", "Undo Color Reorder", "Redo Color Reorder"]);
  assert.deepEqual(await evaluate("[...document.querySelectorAll('.panel-context-menu button')].map(n=>n.disabled)"), [false, false, true, true]);
  await shot("color-menu");
  await closeMenu();
  device = "mouse"; await pointer("move", await center(tile(1))); await pointer("down"); await pause(700); await pointer("up"); await idle();
  assert.equal(await menuOpen(), false, "mouse hold does not open a menu");
  assert.equal(await evaluate(`document.querySelector('${tile(1)}').classList.contains('selected')`), true, "a stationary mouse release still selects");
  await tap(tile(0));
  for (const kind of ["touch", "pen"]) {
    device = kind; const p = await center(tile(1));
    if (kind === "pen") await pointer("move", p);
    await pointer("down", p); await pause(650);
    assert.equal(await menuOpen(), true, `${kind} hold opens the menu`);
    await pointer("up"); await idle();
    assert.equal(await menuOpen(), true, `${kind} release retains the menu`);
    assert.equal(await evaluate(`document.querySelector('${tile(1)}').classList.contains('selected')`), false, `${kind} held release does not select`);
    await closeMenu();
  }
  await evaluate(`document.querySelector('${tile(1)}').focus()`);
  await key("F10", 8);
  assert.equal(await menuOpen(), true, "Shift+F10 opens the focused swatch menu");
  await closeMenu();
  await tap(`${P} .palette-selector`, "touch");
  device = "mouse";
  await pointer("move", await center(`${P} .palette-choice[data-name="Pop Art"]`));
  await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
  assert.deepEqual(await menuLabels(), ["Rename Palette…", "Export Palette", "Remove Palette…"]);
  await menuItem("Export Palette"); await idle();
  assert.deepEqual(await menuLabels(), ["Capycolor (.capycolor)", "Clip Studio Paint, Photoshop (.aco)", "Procreate (.swatches)", "Affinity, Adobe (.ase)", "Krita, GIMP (.gpl)"]);
  await shot("palette-export-menu");
  await closeMenu();
  device = "touch"; await pointer("down", await center(`${P} .palette-choice[data-name="Pop Art"]`)); await pause(650);
  assert.equal(await menuOpen(), true, "touch hold opens the palette row menu");
  await pointer("up"); await closeMenu();
  device = "touch"; const rowStart = await center(`${P} .palette-choice[data-name="Riso Print"]`);
  await pointer("down", rowStart); await pointer("move", { x: rowStart.x, y: rowStart.y - 60 }); await pause(650); await pointer("up"); await idle();
  assert.equal(await menuOpen(), false, "moving before the hold scrolls instead of opening a row menu");
  assert.equal((await active()).name, "Ocean Study", "a scroll does not choose a palette");
  await key("Escape");

  const drag = async (kind, from, to, { hold = false, cancel = null } = {}) => {
    device = kind;
    const start = await center(tile(from)), end = to == null ? { x: start.x + 400, y: start.y } : await center(tile(to), .5, .5);
    if (kind !== "touch") await pointer("move", start);
    await pointer("down", start);
    if (hold) { await pause(650); assert.equal(await menuOpen(), true, `${kind} hold opens the menu before dragging`); }
    await pointer("move", { x: start.x + 12, y: start.y + 2 });
    assert.equal(await menuOpen(), false, `${kind} movement closes the menu and drags`);
    const steps = 6;
    for (let i = 1; i <= steps; i++) await pointer("move", { x: start.x + (end.x - start.x) * i / steps, y: start.y + (end.y - start.y) * i / steps });
    await pause(60);
    const mid = await evaluate(`(()=>{const g=document.querySelector('.palette-drag-ghost');const r=g?.getBoundingClientRect();return{ghost:!!g,x:r?.x,y:r?.y,moved:[...document.querySelectorAll('${P} .palette-swatches > .palette-tile')].filter(n=>n.style.transform).length,source:document.querySelectorAll('${P} .palette-drag-source').length};})()`);
    assert.ok(mid.ghost, `${kind}: lifted swatch follows the contact`);
    assert.equal(mid.source, 1);
    if (to != null) assert.ok(mid.moved > 0, `${kind}: neighbors slide into the proposed order`);
    if (cancel === "escape") await key("Escape");
    if (cancel === "touch") { await pointer("cancel"); await idle(); }
    else await pointer("up");
    await idle();
    assert.equal(await evaluate("document.querySelectorAll('.palette-drag-ghost').length"), 0, `${kind}: no ghost remains`);
    assert.equal(await evaluate(`[...document.querySelectorAll('${P} .palette-swatches > .palette-tile')].filter(n=>n.style.transform).length`), 0);
    return mid;
  };
  for (const kind of ["mouse", "touch", "pen"]) {
    const before = await order();
    const mid = await drag(kind, 0, 3);
    if (kind === "mouse") await shot("reorder-drop");
    const after = await order();
    assert.deepEqual(after, [before[1], before[2], before[3], before[0], ...before.slice(4)], `${kind}: one drop moves the swatch`);
    assert.equal(JSON.stringify((await library()).history), usedHistory, "reordering is not history");
    assert.ok(mid.x > 0);
    await evaluate(`document.querySelector('${P} .palette-swatches > .palette-tile[data-id="${after[3]}"]').focus()`);
    await key("z", 2);
    assert.deepEqual(await order(), before, `${kind}: Ctrl+Z restores the order in one step`);
    await key("Z", 10);
    assert.deepEqual(await order(), after, `${kind}: Ctrl+Shift+Z redoes`);
    await key("z", 2);
    assert.deepEqual(await order(), before);
  }
  {
    const before = await order();
    await drag("touch", 0, 2, { hold: true });
    assert.deepEqual(await order(), [before[1], before[2], before[0], ...before.slice(3)], "touch hold, menu, then drag reorders");
    device = "mouse"; await pointer("move", await center(tile(0))); await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
    assert.equal(await evaluate("[...document.querySelectorAll('.panel-context-menu button')].find(n=>n.textContent.includes('Undo Color Reorder')).disabled"), false);
    await menuItem("Undo Color Reorder"); await idle();
    assert.deepEqual(await order(), before, "the swatch menu undoes a reorder");
    await drag("pen", 1, null);
    assert.deepEqual(await order(), before, "releasing outside the grid cancels");
    await drag("mouse", 1, 4, { cancel: "escape" });
    assert.deepEqual(await order(), before, "Escape cancels");
    await drag("touch", 1, 4, { cancel: "touch" });
    assert.deepEqual(await order(), before, "a cancelled contact changes nothing");
    const moving = drag("mouse", 0, 2);
    await moving;
    await key("z", 2);
    assert.deepEqual(await order(), before);
  }
  {
    const before = await order();
    device = "mouse"; const start = await center(tile(0)), end = await center(tile(8));
    await pointer("move", start); await pointer("down", start);
    for (let i = 1; i <= 5; i++) await pointer("move", { x: start.x + (end.x - start.x) * i / 5, y: start.y + (end.y - start.y) * i / 5 });
    await pause(200); await shot("reorder-drag");
    await evaluate("window.dispatchEvent(new Event('blur'))"); await pointer("up"); await idle();
    assert.equal(await evaluate("document.querySelectorAll('.palette-drag-ghost').length"), 0, "blur cancels without a ghost");
    assert.deepEqual(await order(), before, "blur leaves the order unchanged");
  }

  const samples = process.env.LAYER_PALETTE_SAMPLES;
  const files = [];
  if (samples) {
    for (const [dir, want] of [["gpl", "gimp3-shipped_Tango.gpl"], ["aco", "nord.aco"], ["ase", "adobecolor_spring_blush.ase"], ["swatches", "p5_worrydoll_Ocean_Dusk_P3.swatches"], ["kpl", "script-generated_wl_Zorn_groups_f32_icc.kpl"]]) {
      const names = await readdir(`${samples}/${dir}`).catch(() => []);
      const name = names.includes(want) ? want : names.find(n => n.endsWith(`.${dir}`));
      if (name) files.push({ name, bytes: await readFile(`${samples}/${dir}/${name}`) });
    }
  }
  await evaluate(`(()=>{window.paletteSaves=[];window.showSaveFilePicker=async options=>({createWritable:async()=>{const chunks=[];return{write:async b=>chunks.push(new Uint8Array(b)),close:async()=>paletteSaves.push({name:options.suggestedName,types:options.types,bytes:[...chunks.flatMap(c=>[...c])]})};}});
    const original=HTMLInputElement.prototype.click;HTMLInputElement.prototype.click=function(){if(this.type==='file'){window.paletteInput=this;return;}return original.call(this);};})()`);
  const exportPalette = async (paletteName, label) => {
    await tap(`${P} .palette-selector`, "touch");
    device = "mouse"; await pointer("move", await center(await row(paletteName)));
    await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
    await menuItem("Export Palette"); await idle(); await menuItem(label);
    await wait(`paletteSaves.length>0`, "export saved");
    const saved = await evaluate("paletteSaves.pop()");
    if (await visible(`${P} .palette-chooser`)) await key("Escape");
    return saved;
  };
  const exported = {};
  for (const [label, extension] of [["Capycolor (.capycolor)", "capycolor"], ["Clip Studio Paint, Photoshop (.aco)", "aco"], ["Procreate (.swatches)", "swatches"], ["Affinity, Adobe (.ase)", "ase"], ["Krita, GIMP (.gpl)", "gpl"]]) {
    const saved = await exportPalette("Pop Art", label);
    assert.equal(saved.name, `Pop Art.${extension}`, `${extension}: suggested file name`);
    assert.deepEqual(Object.values(saved.types[0].accept)[0], [`.${extension}`]);
    assert.ok(saved.bytes.length > 20);
    exported[extension] = saved.bytes;
  }
  {
    await evaluate(`(()=>{window.paletteSavePicker=window.showSaveFilePicker;delete window.showSaveFilePicker;window.paletteDownloads=[];const original=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){if(this.download){paletteDownloads.push(fetch(this.href).then(r=>r.arrayBuffer()).then(b=>({name:this.download,bytes:[...new Uint8Array(b)]})));return;}return original.call(this);};})()`);
    await tap(`${P} .palette-selector`, "touch");
    device = "mouse"; await pointer("move", await center(await row("Pop Art")));
    await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
    await menuItem("Export Palette"); await idle(); await menuItem("Krita, GIMP (.gpl)");
    await wait("paletteDownloads.length>0", "download fallback");
    const download = await evaluate("paletteDownloads.pop()");
    assert.equal(download.name, "Pop Art.gpl", "download fallback without a save picker");
    assert.deepEqual(download.bytes, exported.gpl);
    await evaluate("window.showSaveFilePicker=paletteSavePicker");
    if (await visible(`${P} .palette-chooser`)) await key("Escape");
  }
  assert.deepEqual(exported.aco.slice(0, 2), [0, 1], "ACO starts with its version 1 section");
  assert.equal(String.fromCharCode(...exported.ase.slice(0, 4)), "ASEF");
  assert.deepEqual(exported.swatches.slice(0, 4), [0x50, 0x4b, 3, 4]);
  if (!files.length) {
    for (const extension of ["aco", "ase", "gpl", "swatches"]) files.push({ name: `Pop Art Copy.${extension}`, bytes: Buffer.from(exported[extension]) });
    const xml = Buffer.from('<?xml version="1.0"?><ColorSet version="2.0" name="Krita Set" columns="8"><ColorSetEntry name="Red" spot="false" bitdepth="U8"><RGB r="1" g="0" b="0" space="sRGB"/></ColorSetEntry></ColorSet>');
    files.push({ name: "Krita Set.kpl", bytes: storedZip([["mimetype", Buffer.from("krita/x-colorset")], ["colorset.xml", xml]]) });
  }
  const importFile = async file => {
    const paletteCount = (await library()).palettes.length;
    await tap(`${P} .palette-selector`, "touch");
    await tap(`${P} .palette-library-add`);
    assert.deepEqual(await menuLabels(), ["New Palette…", "Import Palette…"]);
    await menuItem("Import Palette…");
    await wait("window.paletteInput!=null");
    assert.match(await evaluate("paletteInput.accept"), /\.aco.*\.swatches.*\.ase.*\.gpl.*\.kpl/);
    await evaluate(`(()=>{const input=paletteInput;window.paletteInput=null;const t=new DataTransfer();t.items.add(new File([new Uint8Array(${JSON.stringify([...file.bytes])})],${JSON.stringify(file.name)}));input.files=t.files;input.dispatchEvent(new Event('change'));})()`);
    await wait(`layerApp.state().colors.library.palettes.length===${paletteCount + 1}||!document.querySelector('${P} .palette-message').hidden`, `import ${file.name}`);
    assert.equal(await evaluate(`document.querySelector('${P} .palette-message').hidden`), true, `${file.name}: ${await evaluate(`document.querySelector('${P} .palette-message').textContent`)}`);
    assert.equal(await visible(`${P} .palette-chooser`), false, "a successful import closes the chooser");
    const imported = await active();
    assert.ok(imported.swatches.length > 0, `${file.name}: imports colors`);
    return imported;
  };
  const importedPalettes = [];
  for (const file of files) importedPalettes.push(await importFile(file));
  const popArt = (await library()).palettes.find(p => p.name === "Pop Art");
  if (!samples) {
    for (const imported of importedPalettes.slice(0, 3)) assert.deepEqual(imported.swatches.map(s => s.name), popArt.swatches.map(s => s.name), `${imported.name}: names and order round-trip`);
    assert.equal(importedPalettes[3].swatches.length, popArt.swatches.length, "Procreate swatches keep every color");
  }
  await shot("imported");
  for (const imported of importedPalettes) await send({ type: "color", action: { op: "library", action: { op: "remove_palette", id: Number(imported.id) } } });
  const harbor = (await library()).palettes.find(p => p.name === "Ocean Study").swatches.find(s => s.name === "Harbor Mist");
  await send({ type: "color", action: { op: "library", action: { op: "remove", id: Number(harbor.id) } } });
  const badCount = (await library()).palettes.length;
  await importFile({ name: "Broken.aco", bytes: Buffer.from([0, 1, 0, 9, 0]) }).catch(() => {});
  assert.equal((await library()).palettes.length, badCount, "failed imports leave the library unchanged");
  assert.ok(await visible(`${P} .palette-message`), "import failure is reported in the panel");
  await key("Escape");

  await tap(`${P} .palette-selector`, "touch"); await tap(`${P} .palette-library-add`); await menuItem("New Palette…"); await idle();
  assert.ok(await visible("dialog.palette-form[open]"));
  await evaluate("document.querySelector('dialog.palette-form input').select()"); await type("Pop art");
  assert.equal(await evaluate("document.querySelector('dialog.palette-form .suggested-action').disabled"), true, "duplicate palette names are rejected live");
  await evaluate("document.querySelector('dialog.palette-form input').select()"); await type("Studies");
  await key("Enter");
  assert.equal((await active()).name, "Studies");
  await tap(`${P} .palette-selector`, "touch");
  device = "mouse"; await pointer("move", await center(await row("Studies"))); await pointer("down", point, "right"); await pointer("up", point, "right"); await idle();
  await menuItem("Remove Palette…"); await idle();
  assert.match(await evaluate("document.querySelector('dialog.palette-form[open]').textContent"), /Remove “Studies”/);
  await evaluate("document.querySelector('dialog.palette-form .destructive-action').click()"); await idle();
  assert.equal((await library()).palettes.some(p => p.name === "Studies"), false, "removal is confirmed");
  if (await visible(`${P} .palette-chooser`)) await key("Escape");

  const saved = await library();
  await wait("!JSON.parse(layerApp.app.workspace_view()).dirty", "workspace saved", 20000);
  await reload();
  await wait("window.layerApp && document.body.dataset.gpu==='ready'", "reload", 60000);
  await idle();
  const restored = await library();
  assert.deepEqual(restored.palettes, saved.palettes, "palettes survive reload");
  assert.deepEqual(restored.history, saved.history, "history survives reload");

  await switchTo("Sketch");
  await tap(".header-tool.brush-color");
  await wait("!!document.querySelector('.content-drawer .palettes-panel .palette-tile')", "Sketch color drawer shows palettes");
  const drawer = await evaluate(`(()=>{const d=document.querySelector('.content-drawer');const wheel=d.querySelector('.color-wheel-control').getBoundingClientRect(),palettes=d.querySelector('.palettes-panel').getBoundingClientRect();return{below:palettes.top>=wheel.bottom-1,width:palettes.width};})()`);
  assert.ok(drawer.below, "palettes sit below the wheel");
  await pause(300); await shot("sketch-drawer");
  const D = ".content-drawer .palettes-panel";
  {
    const before = await order();
    device = "mouse";
    const start = await center(`${D} .palette-swatches > .palette-tile:nth-child(1)`), end = await center(`${D} .palette-swatches > .palette-tile:nth-child(3)`);
    await pointer("move", start); await pointer("down", start);
    for (let i = 1; i <= 5; i++) await pointer("move", { x: start.x + (end.x - start.x) * i / 5, y: start.y + (end.y - start.y) * i / 5 });
    await pointer("up"); await idle();
    assert.deepEqual(await order(), [before[1], before[2], before[0], ...before.slice(3)], "drawer reordering");
    await evaluate(`document.querySelector('${D} .palette-swatches > .palette-tile').focus()`); await key("z", 2);
    assert.deepEqual(await order(), before);
  }
  await key("Escape");

  await switchTo("Photo");
  await selectPalettes();
  await send({ type: "set_theme", theme: "light" }); await pause(200);
  await shot("photo-light");
  await send({ type: "set_theme", theme: "dark" });
  await shot("photo-dark");

  await switchTo("Paint");
  await send({ type: "move_panel", panel: "palettes", target: { kind: "float", position: [520, 160] } });
  const F = ".dock-group.floating-panel .palettes-panel";
  await wait(`!!document.querySelector('${F} .palette-tile')`);
  {
    const before = await order();
    device = "pen";
    const start = await center(`${F} .palette-swatches > .palette-tile:nth-child(2)`), end = await center(`${F} .palette-swatches > .palette-tile:nth-child(1)`);
    await pointer("move", start); await pointer("down", start);
    for (let i = 1; i <= 5; i++) await pointer("move", { x: start.x + (end.x - start.x) * i / 5, y: start.y + (end.y - start.y) * i / 5 });
    await pointer("up"); await idle();
    assert.deepEqual(await order(), [before[1], before[0], ...before.slice(2)], "floating reordering");
    await shot("float");
    await evaluate(`document.querySelector('${F} .palette-swatches > .palette-tile').focus()`); await key("z", 2);
    assert.deepEqual(await order(), before);
  }
  await send({ type: "invoke", command: "undo_workspace" });
  assert.equal(await evaluate("!!document.querySelector('.dock-group.floating-panel .palettes-panel')"), false, "undo returns the float to its group");

  await switchTo("Paint");
  const tabGroup = Number(await groupOf("color"));
  const names = () => evaluate(`[...document.querySelectorAll('.dock-group[data-group="${tabGroup}"] .tab-list .dock-tab')].map(n=>({panel:n.dataset.panel,name:!n.lastElementChild.hidden&&!n.classList.contains('icon-only-tab')}))`);
  for (const panel of ["layers", "properties", "adjustments", "stats"]) {
    if ((await names()).some(t => !t.name)) break;
    await send({ type: "move_panel", panel, target: { kind: "tab", group: tabGroup, index: null } }); await pause(100); await settle();
  }
  const fit = await names();
  assert.ok(fit.length >= 3);
  const shown = fit.map(t => t.name);
  assert.ok(shown.includes(false), "four tabs do not all fit at the fitted width");
  assert.deepEqual(shown, [...shown].sort((a, b) => b - a), "names are restored from the left");
  await selectTab(tabGroup, fit.at(-1).panel); await pause(100); await settle();
  assert.deepEqual((await names()).map(t => t.name), shown, "selection does not change label priority");
  await shot("automatic-tabs");
  for (let i = 1; i < fit.length - 1; i++) await send({ type: "invoke", command: "undo_workspace" });
  assert.deepEqual(await tabs(tabGroup), ["color", "palettes"]);
  console.log("PASS: Web palettes: placement, fitting, starters, history, expansion, add, naming, chooser, menus, reorder (mouse/touch/pen, hold, cancel, undo/redo), imports, exports, persistence, Sketch drawer, float, themes and automatic tab names");
}

function storedZip(entries) {
  const locals = [], central = [];
  let offset = 0;
  for (const [name, data] of entries) {
    const header = Buffer.alloc(30), path = Buffer.from(name), crc = crc32(data);
    header.writeUInt32LE(0x04034b50, 0); header.writeUInt16LE(20, 4); header.writeUInt32LE(crc, 14);
    header.writeUInt32LE(data.length, 18); header.writeUInt32LE(data.length, 22); header.writeUInt16LE(path.length, 26);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0); entry.writeUInt16LE(20, 4); entry.writeUInt16LE(20, 6); entry.writeUInt32LE(crc, 16);
    entry.writeUInt32LE(data.length, 20); entry.writeUInt32LE(data.length, 24); entry.writeUInt16LE(path.length, 28); entry.writeUInt32LE(offset, 42);
    locals.push(header, path, data); central.push(entry, path); offset += 30 + path.length + data.length;
  }
  const directory = Buffer.concat(central), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}
