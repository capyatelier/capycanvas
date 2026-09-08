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
  for (const theme of ["dark", "light"]) {
    await action({ type: "set_theme", theme });
    await click('#header-end [data-command="settings"]');
    assert.deepEqual(await evaluate("layerApp.app.preferences().pages.map(p=>p.id)"), ["appearance", "canvas", "input", "shortcuts", "about"]);
    for (const page of ["appearance", "canvas", "input", "shortcuts", "about"]) {
      await click(`[data-settings-page="${page}"]`);
      await capture(`${page}-${theme}`);
      assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"), page);
      assert.equal(await evaluate("layerApp.app.preferences().page"), page);
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
  await click('[data-shortcut="command.Brush"] .shortcut-choose');
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
  await click('[data-shortcut="command.Settings"] .shortcut-choose');
  await key("j", { ctrlKey: true });
  await click('#confirm-shortcut');
  await click('#apply-settings');
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5);
  assert.equal(await evaluate("layerApp.state().requests.length"), 0);
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='settings').shortcut"), "Ctrl+J");
  await action({ type: "invoke", command: "eraser" });
  await evaluate("layerApp.canvas.focus()"); await key("e");
  assert.equal(await evaluate("layerApp.state().brush.tool"), "brush");
  await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true);
  await action({ type: "cancel_settings" });
  const saved = await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1'))");
  assert.equal(saved.shortcuts["command.Brush"][0].key, "e");
  await call("Page.reload");
  await evaluate("new Promise((resolve,reject)=>{const start=performance.now();function ready(){if(window.layerApp)resolve();else if(performance.now()-start>20000)reject(new Error('reload failed'));else setTimeout(ready,50)}ready()})");
  assert.deepEqual(await evaluate("layerApp.state().settings"), saved);
  await evaluate("layerApp.canvas.focus()"); await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true, "restored shortcuts execute");
  await preference({ type: "page", page: "shortcuts" });
  await preference({ type: "reset_all_shortcuts" });
  await action({ type: "cancel_settings" });
  assert.equal(await evaluate("layerApp.state().settings.shortcuts['command.Brush'][0].key"), "e", "cancel reset keeps applied bindings");
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
