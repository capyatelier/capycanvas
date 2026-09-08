import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkPreferences({ call, evaluate, settle }) {
  const dir = "artifacts/ui/preferences";
  await mkdir(dir, { recursive: true });
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const preference = (value) => action({ type: "preferences", action: value });
  const click = async (selector) => { await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`); await settle(); };
  const key = async (name, modifiers = {}) => {
    await evaluate(`(() => {for(const type of ['keydown','keyup']) (document.querySelector('#shortcut-capture').open ? document.querySelector('#shortcut-capture') : document.activeElement).dispatchEvent(new KeyboardEvent(type,{key:${JSON.stringify(name)},bubbles:true,cancelable:true,...${JSON.stringify(modifiers)}}))})()`);
    await settle();
  };
  const capture = async (name) => {
    await evaluate("document.activeElement?.blur()"); await settle();
    const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/web-${name}.png`, Buffer.from(shot.data, "base64"));
  };
  await call("Emulation.setDeviceMetricsOverride", { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
  assert.equal(await evaluate("layerApp.app.catalog().app_name"), "Capy Canvas");
  assert.equal(await evaluate("document.querySelector('#header-end [data-command=settings] svg').dataset.asset"), "settings");
  const points = await evaluate("layerApp.app.catalog().text_size_pt");
  assert.equal(points, 11);
  for (const legacySize of [9, 11, 13]) {
    await evaluate(`layerApp.dispatch({type:'restore_settings',settings:{...layerApp.state().settings,panel_text_pt:${legacySize}}})`);
    await settle();
    assert.equal(await evaluate("'panel_text_pt' in layerApp.state().settings"), false);
    const metrics = await evaluate(`(() => {
      const style = selector => getComputedStyle(document.querySelector(selector));
      return { fonts:['.dock-tab','.brush-list h3','.size-button','.spin input','#view-info','#document-title'].map(s=>parseFloat(style(s).fontSize)),
        step:parseFloat(style('.panel .spin button svg').width), tool:parseFloat(style('.tile-button svg').width),
        tile:document.querySelector('.tile-button').getBoundingClientRect().height,
        preview:document.querySelector('.brush-preview').getBoundingClientRect().height,
        slider:document.querySelector('.size-controls input[type=range]').getBoundingClientRect().height,
        layerIconButton:document.querySelector('.layer-tools button').getBoundingClientRect().height};
    })()`);
    for (const size of metrics.fonts) assert.ok(Math.abs(size - points * 4 / 3) < .02, `panel text ${size} should be ${points}pt`);
    assert.ok(Math.abs(metrics.step - points * 4 / 3 * 1.16) < .1);
    assert.equal(metrics.tool, 16); assert.equal(metrics.tile, 36); assert.equal(metrics.preview, 40); assert.equal(metrics.slider, 28); assert.equal(metrics.layerIconButton, 28);
  }
  await capture('typography');
  await action({ type: "open_settings", page: "appearance" });
  assert.equal(await evaluate("document.querySelector('#setting-panel-text-size')"), null);
  await action({ type: "cancel_settings" });
  for (const theme of ["dark", "light"]) {
    await action({ type: "set_theme", theme });
    const darkMode = '.header-menu [data-command="toggle_theme"]';
    assert.equal(await evaluate(`document.querySelector('${darkMode} .command-label').textContent`), 'Dark Mode');
    assert.equal(await evaluate(`document.querySelector('${darkMode}').getAttribute('aria-pressed')`), String(theme === 'dark'));
    await click(darkMode);
    assert.equal(await evaluate(`document.querySelector('${darkMode}').getAttribute('aria-pressed')`), String(theme !== 'dark'));
    assert.equal(await evaluate('document.body.dataset.theme'), theme === 'dark' ? 'light' : 'dark');
    await click(darkMode);
    const menus = await evaluate("layerApp.app.catalog().menus");
    for (const [index, spec] of menus.entries()) {
      const selector = `.header-menu:nth-of-type(${index + 1})`;
      await click(`${selector} summary`);
      const items = await evaluate(`[...document.querySelector(${JSON.stringify(selector)}).querySelector('.popover').children].map(n=>n.tagName==='HR'?'separator':n.dataset.command)`);
      assert.deepEqual(items, spec.sections.flatMap((section, i) => i ? ['separator', ...section] : section));
      assert.ok(await evaluate(`[...document.querySelector(${JSON.stringify(selector)}).querySelectorAll('hr')].every(n=>getComputedStyle(n).height==='1px'&&getComputedStyle(n).backgroundColor!=='rgba(0, 0, 0, 0)')`));
      await capture(`menu-${spec.label.toLowerCase()}-${theme}`);
      await click(`${selector} summary`);
    }
    await click('#header-end [data-command="settings"]');
    assert.ok(await evaluate(`(() => { const sidebar=document.querySelector('.preferences-sidebar').getBoundingClientRect(), content=document.querySelector('.preferences-content').getBoundingClientRect(); return Math.abs(sidebar.bottom-content.bottom)<1; })()`), "sidebar extends alongside the content footer");
    assert.deepEqual(await evaluate("layerApp.app.preferences().pages.map(p=>p.id)"), ["appearance", "canvas", "input", "shortcuts", "about"]);
    for (const page of ["appearance", "canvas", "input", "shortcuts", "about"]) {
      await click(`[data-settings-page="${page}"]`);
      await capture(`${page}-${theme}`);
      assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"), page);
      assert.equal(await evaluate("layerApp.app.preferences().page"), page);
      assert.ok(await evaluate(`[...document.querySelectorAll('.preferences-page:not([hidden]) label,.preferences-page:not([hidden]) p,.preferences-page:not([hidden]) h3,.preferences-page:not([hidden]) input,.preferences-page:not([hidden]) .settings-info,.preferences-page:not([hidden]) .settings-link')].every(n=>Math.abs(parseFloat(getComputedStyle(n).fontSize)-${points * 4 / 3})<.02)`), "all settings text uses the shared size");
      if (page === "canvas") {
        await click('.preference-choice summary');
        assert.equal(await evaluate("document.querySelectorAll('.preference-options [role=option] svg').length"), 5);
        await capture(`cursor-choices-${theme}`);
        await click('.preference-options [data-choice="2"]');
        assert.equal(await evaluate("layerApp.state().settings_draft.cursor"), "cross");
        await preference({ type: "edit", id: "cursor", value: 0 });
      }
      if (page === "about") {
        assert.ok(await evaluate(`layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).filter(r=>r.kind.type==='link').every(row=>{
          const a=document.querySelector('#setting-'+row.id.replaceAll('_','-'));
          return a.tagName==='A'&&a.textContent===row.kind.label&&a.href===row.kind.url&&a.getAttribute('aria-label')===row.title&&a.target==='_blank'&&a.relList.contains('noopener')&&a.relList.contains('noreferrer');
        })`), "About links render the Rust labels and URLs and preserve the drawing tab");
        assert.equal(await evaluate("document.querySelectorAll('[data-page=about] a.settings-link').length"), 2);
      }
    }
    await action({ type: "cancel_settings" });
  }
  await click('#header-end [data-command="settings"]');
  await click('.preferences-search-toggle');
  assert.equal(await evaluate("document.querySelector('#settings-search').hidden"), false);
  await preference({ type: "register_action", definition: {
    id: "custom.test-size", label: "Test size", repeat: false,
    action: { kind: "action", action: { type: "set_brush_size", value: 42 } },
  } });
  assert.equal(await evaluate("document.querySelectorAll('[data-shortcut=\"custom.test-size\"]').length"), 1);
  await action({ type: "cancel_settings" });
  await click('#header-end [data-command="settings"]');
  assert.equal(await evaluate("document.querySelectorAll('[data-shortcut=\"custom.test-size\"]').length"), 0, "canceled custom actions must not leave stale rows");
  await evaluate("const emptySearch=document.querySelector('#settings-search');emptySearch.value='no-such-preference';emptySearch.dispatchEvent(new Event('input'))");
  assert.equal(await evaluate("document.querySelector('.preferences-empty').hidden"), false);
  await evaluate("const search=document.querySelector('#settings-search');search.value='pressure response';search.dispatchEvent(new Event('input'))");
  assert.equal(await evaluate("document.querySelectorAll('.preferences-search-results button').length"), 1);
  await capture("search");
  await click('.preferences-search-results button');
  assert.equal(await evaluate("layerApp.app.preferences().page"), "input");
  assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"), "input");
  await click('[data-settings-page="input"]');
  assert.equal(await evaluate("document.querySelector('#settings-search').value"), "");
  assert.equal(await evaluate("document.querySelector('#setting-platform-prediction').checked"), true);
  await click('#setting-feedback');
  assert.equal(await evaluate("document.querySelector('#setting-prediction-horizon').disabled"), true);
  await click('#setting-feedback');
  await evaluate("const pressure=document.querySelector('#setting-pressure');pressure.value='1.5';pressure.dispatchEvent(new Event('input'));pressure.focus()");
  await key("b");
  assert.equal(await evaluate("layerApp.app.preferences().capture ?? null"), null);
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1);
  await action({ type: "open_settings", page: "shortcuts" });
  assert.equal(await evaluate("layerApp.state().settings_draft.pressure_gamma"), 1.5, "deep link preserves draft");
  await evaluate("const shortcutsSearch=document.querySelector('#shortcuts-search');shortcutsSearch.value='eraser';shortcutsSearch.dispatchEvent(new Event('input'))");
  assert.ok(await evaluate("[...document.querySelectorAll('[data-shortcut]:not([hidden])')].every(row=>row.textContent.toLowerCase().includes('eraser'))"));
  await preference({ type: "search_shortcuts", query: "" });
  await click('[data-shortcut="command.Brush"] .shortcut-choose');
  assert.equal(await evaluate("document.querySelector('#shortcut-editor').open"), true);
  assert.equal(await evaluate("document.querySelector('#shortcut-capture').open"), false);
  await click('#shortcut-editor .preference-row button');
  assert.deepEqual(await evaluate("layerApp.app.preferences().shortcut_editor.bindings"), []);
  await click('#shortcut-editor footer button:first-child');
  assert.deepEqual(await evaluate("layerApp.app.preferences().shortcut_editor.bindings"), ["B"]);
  await click('#add-shortcut');
  await key("Control", { ctrlKey: true });
  assert.equal(await evaluate("layerApp.app.preferences().capture.chord ?? null"), null);
  await key("w", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#confirm-shortcut').disabled"), true);
  await key("e");
  assert.equal(await evaluate("layerApp.app.preferences().capture.conflict"), "Eraser");
  assert.equal(await evaluate("document.querySelector('#confirm-shortcut').textContent"), "Replace Shortcut");
  await capture("shortcut-conflict");
  assert.ok(await evaluate("document.querySelector('#shortcut-capture').getBoundingClientRect().height < 320"), "recording prompt must be a compact centered sheet");
  await click('#confirm-shortcut');
  assert.equal(await evaluate("layerApp.state().settings_draft.shortcuts['command.Eraser'].length"), 0);
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='brush').shortcut"), "B");
  assert.deepEqual(await evaluate("layerApp.app.preferences().shortcut_editor.bindings"), ["B", "E"]);
  await capture("shortcut-editor");
  await click('#close-shortcut-editor');
  await click('[data-shortcut="command.Settings"] .shortcut-choose');
  await click('#add-shortcut');
  await key("j", { ctrlKey: true });
  await click('#confirm-shortcut');
  await click('#close-shortcut-editor');
  await click('#apply-settings');
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5);
  assert.equal(await evaluate("layerApp.state().requests.length"), 0);
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='settings').shortcut"), "Ctrl+, / Ctrl+J");
  await action({ type: "invoke", command: "eraser" });
  await evaluate("layerApp.canvas.focus()"); await key("e");
  assert.equal(await evaluate("layerApp.state().brush.tool"), "brush");
  await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true);
  await action({ type: "cancel_settings" });
  const saved = await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1'))");
  assert.deepEqual(saved.shortcuts["command.Brush"].map(c => c.key), ["b", "e"]);
  await call("Page.reload");
  await evaluate("new Promise((resolve,reject)=>{const start=performance.now();function ready(){if(window.layerApp)resolve();else if(performance.now()-start>20000)reject(new Error('reload failed'));else setTimeout(ready,50)}ready()})");
  assert.deepEqual(await evaluate("layerApp.state().settings"), saved);
  await evaluate("layerApp.canvas.focus()"); await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true, "restored shortcuts execute");
  await preference({ type: "page", page: "shortcuts" });
  await preference({ type: "reset_all_shortcuts" });
  await action({ type: "cancel_settings" });
  assert.equal(await evaluate("layerApp.state().settings.shortcuts['command.Brush'][1].key"), "e", "cancel reset keeps applied bindings");
  await action({ type: "open_settings", page: "canvas" });
  await call("Emulation.setDeviceMetricsOverride", { width: 640, height: 600, deviceScaleFactor: 1, mobile: false });
  await capture("narrow");
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.preferences-sidebar')).display"), "none");
  await click('.preferences-back');
  assert.notEqual(await evaluate("getComputedStyle(document.querySelector('.preferences-sidebar')).display"), "none");
  await click('[data-settings-page="appearance"]');
  assert.equal(await evaluate("layerApp.app.preferences().page"), "appearance");
  assert.equal(await evaluate("document.querySelector('#status').textContent"), "");
  console.log("PASS: native-model settings pages, themes, adaptive sidebar, search, dependencies, key recording/conflicts, transactions, persistent reload and executable restored shortcuts");
}
