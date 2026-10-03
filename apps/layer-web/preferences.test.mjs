import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";

export async function checkPreferences({ call, evaluate, settle, errors }) {
  const dir = "artifacts/ui/preferences";
  await mkdir(dir, { recursive: true });
  const action = async (value) => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const preference = (value) => action({ type: "preferences", action: value });
  const click = async (selector) => { await evaluate(`(() => {const node=document.querySelector(${JSON.stringify(selector)});if(node.matches('input:not([type=hidden]),textarea,select'))node.focus();node.click()})()`); await settle(); };
  const key = async (name, modifiers = {}) => {
    await evaluate(`(() => {for(const type of ['keydown','keyup']) document.activeElement.dispatchEvent(new KeyboardEvent(type,{key:${JSON.stringify(name)},bubbles:true,cancelable:true,...${JSON.stringify(modifiers)}}))})()`);
    await settle();
  };
  const escape = async () => {
    await evaluate(`(() => {
      const down = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
      document.activeElement.dispatchEvent(down);
      const top = [...document.querySelectorAll('dialog[open]')].at(-1);
      if (!down.defaultPrevented && top) top.dispatchEvent(new Event('cancel', { cancelable: true }));
      document.activeElement.dispatchEvent(new KeyboardEvent('keyup', { key: 'Escape', bubbles: true, cancelable: true }));
    })()`);
    await settle();
  };
  const reload = async (gpu) => {
    const previous = await evaluate("performance.timeOrigin");
    await call("Page.reload");
    for (const deadline = Date.now() + 20000; ; await new Promise(resolve => setTimeout(resolve, 50))) {
      assert.ok(Date.now() < deadline, `reload to ${gpu} failed`);
      if (await evaluate(`performance.timeOrigin !== ${previous} && !!window.layerApp && document.body.dataset.gpu === '${gpu}'`).catch(() => false)) break;
    }
  };
  const capture = async (name) => {
    await evaluate("document.activeElement?.blur()"); await settle();
    const shot = await call("Page.captureScreenshot", { format: "png" });
    await writeFile(`${dir}/web-${name}.png`, Buffer.from(shot.data, "base64"));
  };
  await call("Emulation.setDeviceMetricsOverride", { width: 1200, height: 900, deviceScaleFactor: 1, mobile: false });
  await action({ type: "restore_workspace", workspace: await evaluate(`(() => {
    const workspace = structuredClone(layerApp.state().workspace), tabs = (id, panels) => ({ kind: "tabs", id, panels, active: panels[0], tab_style: "icon" });
    Object.assign(workspace.layout, { bands: [{ id: 40, edge: "left", extent: 252, root: tabs(41, ["brushes"]) }, { id: 42, edge: "left", extent: 252, root: tabs(43, ["sizes"]) },
      { id: 44, edge: "right", extent: 252, root: tabs(45, ["layers", "properties"]) }, { id: 46, edge: "top", extent: 36, root: tabs(47, ["toolbar"]) }],
      floating: [], collapsed: [], column_scroll: [], fit_tab_groups: [], fit_height_groups:[], column_stacks:[], next_id: Math.max(48, workspace.layout.next_id) });
    workspace.zen_mode = false;
    return workspace;
  })()`) });
  assert.equal(await evaluate("layerApp.app.catalog().app_name"), "Capy Canvas");
  assert.equal(await evaluate("document.querySelector('#header [data-command=settings] svg').dataset.asset"), "settings");
  const points = await evaluate("layerApp.app.catalog().text_size_pt");
  assert.equal(points, 11);
  const metrics = await evaluate(`(() => {
    const style = selector => getComputedStyle(document.querySelector(selector));
    const height = selector => Math.max(...[...document.querySelectorAll(selector)].map(n => n.getBoundingClientRect().height));
    return { fonts:['.dock-tab','.size-controls .number-title','.size-button','.number-entry','#view-info','#document-title'].map(s=>parseFloat(style(s).fontSize)),
      step:parseFloat(style('.panel .number-step svg').width), tool:parseFloat(style('.toolbar-controls .tile-button svg').width),
      tile:height('.tile-button'), preview:height('.brush-preview'),
      slider:height('.size-controls input[type=range]'), layerIconButton:height('.layer-flags button')};
  })()`);
  for (const size of metrics.fonts) assert.ok(Math.abs(size - points * 4 / 3) < .02, `panel text ${size} should be ${points}pt`);
  assert.equal(metrics.step, 16);
  assert.equal(metrics.tool, 16); assert.equal(metrics.tile, 36); assert.equal(metrics.preview, 40); assert.equal(metrics.slider, 24); assert.equal(metrics.layerIconButton, 24);
  assert.ok(await evaluate("(() => { const button = document.querySelector('#zen-button'); return Math.abs(button.querySelector('svg').getBoundingClientRect().width - button.getBoundingClientRect().height * 440 / 512) < .02; })()"), 'Capy button uses the enlarged favicon proportions');
  assert.equal(await evaluate("document.querySelector('#zen-button svg').dataset.asset"), 'zen-looking-up');
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='zen_mode').shortcut"), 'Tab');
  await evaluate("document.activeElement?.blur()");
  await key('Tab');
  assert.equal(await evaluate("layerApp.state().workspace.zen_mode"), true);
  await key('Tab');
  assert.equal(await evaluate("layerApp.state().workspace.zen_mode"), false);
  await capture('typography');
  assert.equal(await evaluate("getComputedStyle(document.querySelector('#size-number .number-labels')).paddingLeft"), '6px', 'panel labels mirror the value inset');
  assert.ok(await evaluate(`(() => {
    const track=document.querySelector('#size-number .number-track'), [minus,bar,plus]=[...track.children].map(n=>n.getBoundingClientRect());
    return minus.height===24 && plus.height===24 && bar.left===minus.right+6 && bar.right+6===plus.left;
  })()`), 'compact panel track has symmetric 6px gaps before the step buttons');
  // The corner-guard journey opts into edge reveal explicitly.
  await evaluate("layerApp.dispatch({type:'restore_settings',settings:{...layerApp.state().settings,zen_show_capy:false,zen_reveal_at_edges:true}})");
  // Real host events: activation hides immediately, even while the pointer
  // remains over the Zen button, and the fixed corner guard survives refresh.
  await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:24,clientY:24,pointerType:'mouse',bubbles:true}))");
  await click('#zen-button');
  assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), true);
  await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:299,clientY:24,pointerType:'mouse',bubbles:true}))");
  assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), true);
  await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:600,clientY:450,pointerType:'mouse',bubbles:true})); window.dispatchEvent(new PointerEvent('pointermove',{clientX:24,clientY:24,pointerType:'mouse',bubbles:true}))");
  assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), false);
  await click('#zen-button');
  // Total Zen, edge reveal, generic image choices
  // and deep linking use real DOM controls backed by the shared preference API.
  const zenVisible = () => evaluate("(async () => { const n=document.querySelector('#zen-button'); await Promise.all(n.getAnimations().map(a=>a.finished)); return getComputedStyle(n).pointerEvents === 'auto' && getComputedStyle(n).opacity === '1'; })()");
  const zenMenu = () => evaluate(`(() => { const n=document.querySelector('#zen-button'); n.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:24,clientY:36})); })()`);
  const chooseMenu = async text => {
    await evaluate(`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(n=>n.textContent===${JSON.stringify(text)}).click()`); await settle();
  };
  const originalWorkspace = await evaluate('layerApp.state().workspace');
  await action({ type: 'move_panel', panel: 'sizes', target: { kind: 'float', position: [500, 340] } });
  for (const theme of ['dark', 'light']) {
    await action({ type: 'set_theme', theme });
    await zenMenu(); await chooseMenu('Change icon…');
    assert.equal(await evaluate('layerApp.app.preferences().reveal'), 'zen_icon');
    assert.equal(await evaluate("document.querySelector('#settings').getBoundingClientRect().height"), 744);
    assert.equal(await evaluate("document.querySelector('.preferences-content > footer')"), null);
    assert.ok(await evaluate(`(() => {
      const n=document.querySelector('.preference-image-tiles'), r=n.getBoundingClientRect();
      const tiles=[...n.querySelectorAll('button')].map(n=>n.getBoundingClientRect());
      return tiles.length===4 && tiles.every(t=>t.width===64&&t.height===64&&t.top>=0&&t.bottom<innerHeight)
        && Math.abs((tiles[0].left+tiles[3].right)/2-(r.left+r.right)/2)<1
        && [...n.querySelectorAll('svg')].every(n=>n.getBoundingClientRect().width===48);
    })()`), 'centered four-tile selector is revealed in the taller dialog');
    for (const [index, name] of ['looking-up','facing-forward','bathing','sleeping'].entries()) {
      await click(`.preference-image-tiles [data-choice="${index}"]`);
      assert.equal(await evaluate("document.querySelector('#zen-button svg').dataset.asset"), `zen-${name}`);
      assert.equal(await evaluate("document.querySelector('.preference-image-tiles [aria-pressed=true]').dataset.choice"), String(index));
      await capture(`zen-icons-${theme}-${index}`);
    }
    await click('#close-settings');
    await click('#zen-button');
    assert.equal(await zenVisible(), false, 'Total Zen hides the Capy with the rest of the chrome');
    await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:600,clientY:450,bubbles:true}));window.dispatchEvent(new PointerEvent('pointermove',{clientX:24,clientY:24,bubbles:true}))");
    assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), false);
    assert.ok(await zenVisible(), 'Edge reveal restores controls');
    assert.equal(await evaluate("getComputedStyle(document.querySelector('.floating-panel')).pointerEvents"), 'auto');
    // A touch hold must open settings, not trigger the button's exit click.
    await call('Input.setIgnoreInputEvents', { ignore: false });
    await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: 24, y: 24 }] });
    await evaluate('new Promise(resolve=>setTimeout(resolve,700))');
    await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await call('Input.setIgnoreInputEvents', { ignore: true });
    await settle();
    assert.ok(await evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')"));
    assert.ok(await evaluate('layerApp.state().workspace.zen_mode'));
    await capture(`zen-menu-${theme}`);
    await chooseMenu('Change icon…');
    await click('#close-settings');
    await action({ type: 'invoke', command: 'zen_mode' });
    await click('#zen-button');
    await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:600,clientY:450,bubbles:true}));window.dispatchEvent(new PointerEvent('pointermove',{clientX:24,clientY:24,bubbles:true}))");
    await settle();
    assert.equal(await evaluate("document.querySelector('#workspace').classList.contains('zen-hidden')"), false);
    await evaluate("window.dispatchEvent(new PointerEvent('pointermove',{clientX:100,clientY:24,bubbles:true}))");
    await capture(`zen-active-${theme}`);
    await click('#zen-button');
  }
  await preference({ type: 'reset', id: 'zen_icon' });
  await action({ type: 'restore_workspace', workspace: originalWorkspace });
  assert.ok(await evaluate("document.elementFromPoint(24,24).closest('#zen-button') !== null"), 'the visible header must not intercept the persistent button');
  await call('Input.setIgnoreInputEvents', { ignore: false });
  for (const enabled of [true, true, false]) {
    await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: 24, y: 24 }] });
    await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    await settle();
    assert.equal(await evaluate('layerApp.state().workspace.zen_mode'), enabled, 'a hidden control first reveals, then activates on the next tap');
  }
  await call('Input.setIgnoreInputEvents', { ignore: true });
  // Values are plain editing buttons. The slider uses the
  // exact Rust mapping and stepping keeps using display units.
  await click('#size-number .number-value');
  assert.ok(await evaluate(`(() => {const n=document.querySelector('#size-number .number-entry');const r=parseFloat(getComputedStyle(n).borderRadius);return n.getBoundingClientRect().width<100 && r>4 && r<=12;})()`), 'short values use compact rounded editors');
  await evaluate("document.querySelector('#size-number .number-entry').value='45/2'"); await key('Enter');
  assert.equal(await evaluate('layerApp.state().brush.diameter'), 22.5);
  await click('#size-number [aria-label="Increase Brush size"]');
  assert.equal(await evaluate('layerApp.state().brush.diameter'), 23.5);
  await evaluate("const slider=document.querySelector('#size-number input[type=range]');slider.value=.5;slider.dispatchEvent(new Event('input',{bubbles:true}))");
  assert.equal(await evaluate('layerApp.state().brush.diameter'), 32);
  for (const theme of ["dark", "light"]) {
    await action({ type: "set_theme", theme });
    await click('.header-menu[data-menu="view"] summary');
    assert.equal(await evaluate('document.querySelector(\'.header-menu[data-menu="view"] [data-command="toggle_theme"]\')'), null);
    await click('.header-menu[data-menu="view"] summary');
    const menus = await evaluate("layerApp.app.editor_models(0,0).application_menus");
    for (const spec of menus) {
      // Dynamic workspace menus have their own real-pointer suite.
      if (spec.id === "window") continue;
      const selector = `.header-menu[data-menu="${spec.id}"]`;
      await click(`${selector} summary`);
      const items = await evaluate(`[...document.querySelector(${JSON.stringify(selector)}).querySelector('.popover').children].map(n=>n.tagName==='HR'?'separator':n.querySelector('.menu-label').textContent)`);
      assert.deepEqual(items, spec.model.sections.filter(section => section.length).flatMap((section, i) => [...(i ? ['separator'] : []), ...section.map(item => item.label)]));
      assert.ok(await evaluate(`[...document.querySelector(${JSON.stringify(selector)}).querySelectorAll('hr')].every(n=>getComputedStyle(n).height==='1px'&&getComputedStyle(n).backgroundColor!=='rgba(0, 0, 0, 0)')`));
      await capture(`menu-${spec.label.toLowerCase()}-${theme}`);
      await click(`${selector} summary`);
    }
    await click('#header [data-command="settings"]');
    assert.equal(await evaluate("document.querySelector('#settings').getBoundingClientRect().width"), 1000);
    assert.ok(await evaluate(`(() => { const sidebar=document.querySelector('.preferences-sidebar').getBoundingClientRect(), title=document.querySelector('.preferences-sidebar h2').getBoundingClientRect(); return Math.abs(title.left+title.width/2-sidebar.left-sidebar.width/2)<1; })()`), "sidebar title centers independently of the search button");
    assert.ok(await evaluate(`(() => { const sidebar=document.querySelector('.preferences-sidebar').getBoundingClientRect(), content=document.querySelector('.preferences-content').getBoundingClientRect(); return Math.abs(sidebar.bottom-content.bottom)<1; })()`), "sidebar extends alongside the content footer");
    assert.deepEqual(await evaluate("layerApp.app.preferences().pages.map(p=>p.id)"), ["appearance", "canvas", "color", "input", "shortcuts", "about"]);
    for (const page of ["appearance", "canvas", "color", "input", "shortcuts", "about"]) {
      await click(`[data-settings-page="${page}"]`);
      assert.ok(await evaluate(`(() => [...document.querySelectorAll('.preferences-page:not([hidden]) input.number-slider')].every(slider => {
        const row=slider.closest('.number-control'), header=row.querySelector('.number-header').getBoundingClientRect(), track=row.querySelector('.number-track').getBoundingClientRect();
        const labels=row.querySelector('.number-labels').getBoundingClientRect(), value=row.querySelector('.number-value-box').getBoundingClientRect(), title=row.querySelector('.number-title');
        return track.top>=header.bottom && Math.abs(track.width-header.width)<1 && track.width<=600 && slider.getBoundingClientRect().height===32
          && value.height===34 && Math.abs(labels.top+labels.height/2-value.top-value.height/2)<1 && Math.abs(value.right-header.right)<1
          && labels.left===header.left && getComputedStyle(row.querySelector('.number-labels')).paddingLeft==='0px'
          && getComputedStyle(title).whiteSpace==='nowrap' && getComputedStyle(title).textOverflow==='ellipsis' && title.title===title.textContent;
      }))()`), 'sliders span the capped row; right-aligned values center against the complete label block');
      await capture(`${page}-${theme}`);
      assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"), page);
      assert.equal(await evaluate("layerApp.app.preferences().page"), page);
      assert.ok(await evaluate(`[...document.querySelectorAll('.preferences-page:not([hidden]) label,.preferences-page:not([hidden]) p,.preferences-page:not([hidden]) h3,.preferences-page:not([hidden]) input,.preferences-page:not([hidden]) .settings-info,.preferences-page:not([hidden]) .settings-link')].every(n=>Math.abs(parseFloat(getComputedStyle(n).fontSize)-${points * 4 / 3}/(n.matches('.preference-text p,.number-description')?1.2:1))<.02)`), "settings titles use the shared size; subtitles follow Adwaita's smaller font");
      if (page === "input") {
        await click('.preference-choice:has(#setting-cursor) summary');
        assert.equal(await evaluate("document.querySelectorAll('.preference-choice:has(#setting-cursor) [role=option] svg').length"), 10);
        await capture(`cursor-choices-${theme}`);
        await click('.preference-choice:has(#setting-cursor) [data-choice="1"]');
        assert.equal(await evaluate("layerApp.state().settings.cursor"), "cross");
        await preference({ type: "reset", id: "cursor" });
      }
      if (page === "about") {
        assert.ok(await evaluate(`layerApp.app.preferences().pages.flatMap(p=>p.groups.flatMap(g=>g.rows)).filter(r=>r.kind.type==='link').every(row=>{
          const a=document.querySelector('#setting-'+row.id.replaceAll('_','-'));
          return a.tagName==='A'&&a.textContent===row.kind.label&&a.href===row.kind.url&&a.getAttribute('aria-label')===row.title&&a.target==='_blank'&&a.relList.contains('noopener')&&a.relList.contains('noreferrer');
        })`), "About links render the Rust labels and URLs and preserve the drawing tab");
        assert.equal(await evaluate("document.querySelectorAll('[data-page=about] a.settings-link').length"), 2);
      }
    }
    await action({ type: "close_settings" });
  }
  // Real text fields: invalid/incomplete edits never alter the accepted palette.
  for (const [theme, color] of [['dark', '#1C2C3C'], ['light', '#C0B49C']]) {
    await action({ type: 'set_theme', theme });
    await action({ type: 'open_settings', page: 'appearance' });
    const selector = `#setting-${theme}-base`;
    const custom = `[data-preference=${theme}_base] .swatch[data-swatch="4"]`;
    assert.equal(await evaluate(`document.querySelector('${selector}').hidden`), true, 'the hex entry waits for Custom');
    assert.ok(await evaluate(`(() => { const row=document.querySelector('[data-preference=${theme}_base]'), circle=document.querySelector('${custom}'); return circle.getBoundingClientRect().height < row.getBoundingClientRect().height; })()`), 'base circles share the title row');
    await click(custom);
    assert.equal(await evaluate(`document.activeElement===document.querySelector('${selector}')`), true);
    assert.equal(await evaluate(`document.querySelector('${selector}').value`), theme === 'dark' ? '#333333' : '#b8b8b8');
    await evaluate(`document.querySelector('${selector}').value='invalid'`); await key('Enter');
    assert.ok(await evaluate('layerApp.app.preferences().error'));
    await evaluate(`document.querySelector('${selector}').value='${color}'`); await key('Enter');
    assert.equal(await evaluate('layerApp.app.preferences().error ?? null'), null);
    assert.equal(await evaluate(`layerApp.state().settings.${theme}_base`), color.toLowerCase());
    assert.equal(await evaluate("document.querySelector('meta[name=theme-color]').content"), color.toLowerCase());
    assert.ok(await evaluate(`(() => {
      const palette=layerApp.state().palette, probe=document.body.appendChild(document.createElement('i'));
      const css=color=>{probe.style.background=color;return getComputedStyle(probe).backgroundColor};
      const glass=name=>css('rgb('+palette.glass[name].slice(0,3).map(v=>v*255).join(' ')+' / '+palette.glass[name][3]+')');
      const surfaces=[['.panel-preview:has(> .dock-tabs) > .panel-frame',glass('panel')],['.dock-tabs',glass('strip')],
        ['#settings',css(palette.settings)],['.preferences-sidebar',css(palette.sidebar)],['${selector}',css(palette.input)]];
      probe.remove();
      return surfaces.every(([selector,color])=>getComputedStyle(document.querySelector(selector)).backgroundColor===color);
    })()`), 'all surfaces bind the core palette');
    await capture(`custom-base-${theme}`);
    const defaultColor = theme === 'dark' ? '#333333' : '#b8b8b8';
    const openReset = () => evaluate(`(() => { const row=document.querySelector('${selector}').closest('[data-preference]'), r=row.getBoundingClientRect(); row.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:r.left+30,clientY:r.top+20})); })()`);
    if (theme === 'dark') {
      const point = await evaluate(`(() => { const r=document.querySelector('${selector}').closest('[data-preference]').getBoundingClientRect(); return {x:r.left+30,y:r.top+20}; })()`);
      await call('Input.setIgnoreInputEvents', { ignore: false });
      await call('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [point] });
      await evaluate('new Promise(resolve=>setTimeout(resolve,650))');
      await call('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await call('Input.setIgnoreInputEvents', { ignore: true });
      assert.equal(await evaluate("document.querySelector('#preference-context-menu').matches(':popover-open')"), true, 'long press opens the reset menu without activating the row');
    } else await openReset();
    assert.equal(await evaluate("document.querySelector('#preference-context-menu [data-reset]').disabled"), false);
    assert.equal(await evaluate("document.querySelector('#preference-context-menu .shortcut-hint').textContent"), defaultColor);
    assert.ok(await evaluate(`(() => { const a=document.querySelector('#preference-context-menu .command-label'), b=document.querySelector('#preference-context-menu .shortcut-hint'); return b.getBoundingClientRect().left>a.getBoundingClientRect().right && getComputedStyle(b).opacity==='0.55'; })()`));
    await capture(`reset-${theme}`);
    await click('#preference-context-menu [data-reset]');
    assert.equal(await evaluate(`layerApp.state().settings.${theme}_base`), defaultColor);
    await openReset();
    assert.equal(await evaluate("document.querySelector('#preference-context-menu [data-reset]').disabled"), true);
    await key('Escape');
    assert.equal(await evaluate("document.querySelector('#settings').open"), true, 'Escape only closes the context menu');
    assert.equal(await evaluate(`document.querySelector('${selector}').hidden`), true, 'a preset hides the entry');
    await click(custom); await evaluate(`document.querySelector('${selector}').value='${color}'`); await key('Enter');
    await evaluate(`document.querySelector('${selector}').value=''`);
    assert.equal(await evaluate(`layerApp.state().settings.${theme}_base`), color.toLowerCase());
    await key('Enter');
    assert.equal(await evaluate(`document.querySelector('${selector}').value`), defaultColor);
    await click(`[data-preference=${theme}_base] .swatch[data-swatch="0"]`);
    assert.equal(await evaluate(`layerApp.state().settings.${theme}_base`), theme === 'dark' ? '#1f1f1f' : '#a4a4a4');
    await click(custom); await evaluate(`document.querySelector('${selector}').value='${color}'`); await key('Enter');
    await action({ type: 'close_settings' });
    await capture(`custom-workspace-${theme}`);
  }
  await action({ type: 'restore_settings', settings: { ...await evaluate('layerApp.state().settings'), dark_base: '#333333', light_base: '#b8b8b8' } });
  for (const theme of ['dark', 'light']) {
    await action({ type: 'set_theme', theme });
    await action({ type: 'open_settings', page: 'appearance' });
    const swatch = index => `[data-preference=accent] .swatch[data-swatch="${index}"]`;
    assert.deepEqual(await evaluate("[...document.querySelector('[data-page=appearance] .preference-group').children].map(l=>l.dataset.preference).slice(-3)"),
      ['dark_base', 'light_base', 'accent']);
    assert.equal(await evaluate(`document.querySelector('${swatch(0)}').getAttribute('aria-label')`), 'Blue', 'the web has no system accent');
    assert.equal(await evaluate(`document.querySelector('${swatch(0)}').getAttribute('aria-checked')`), 'true');
    assert.ok(await evaluate(`(() => { const row=document.querySelector('[data-preference=accent]').getBoundingClientRect(), a=document.querySelector('${swatch(0)}').getBoundingClientRect(), b=document.querySelector('${swatch(9)}').getBoundingClientRect(); return Math.abs((a.left-row.left)-(row.right-b.right))<=2; })()`), 'accent circles are centered');
    await click(swatch(5));
    assert.equal(await evaluate('layerApp.state().settings.accent'), '#e62d42');
    assert.ok(await evaluate(`(() => { const p=layerApp.state().palette, rgb=hex=>'rgb('+hex.slice(1).match(/../g).map(v=>parseInt(v,16)).join(', ')+')';
      return p.accent==='#e62d42' && getComputedStyle(document.body).getPropertyValue('--accent').trim()===p.accent
        && getComputedStyle(document.querySelector('${swatch(5)}')).backgroundColor===rgb('#e62d42'); })()`));
    await click(swatch(9));
    assert.equal(await evaluate('document.activeElement.id'), 'setting-accent');
    await evaluate("document.querySelector('#setting-accent').value='#12ab56'"); await key('Enter');
    assert.equal(await evaluate('layerApp.state().palette.accent'), '#12ab56');
    assert.equal(await evaluate(`document.querySelector('${swatch(9)}').getAttribute('aria-checked')`), 'true');
    await capture(`accent-custom-${theme}`);
    await click(swatch(0));
    assert.equal(await evaluate('layerApp.state().settings.accent ?? null'), null, 'Blue is the web default');
    assert.equal(await evaluate("document.querySelector('#setting-accent').hidden"), true);
    await action({ type: 'close_settings' });
    assert.ok(await evaluate(`(() => { const c=layerApp.state().palette.glass.switcher_selection, pressed=document.querySelector('#header .workspace-switcher button[aria-pressed="true"]'), probe=document.body.appendChild(document.createElement('i'));
      probe.style.background='rgb('+c.slice(0,3).map(v=>v*255).join(' ')+' / '+c[3]+')'; const expected=getComputedStyle(probe).backgroundColor; probe.remove();
      return !pressed || getComputedStyle(pressed).backgroundColor===expected; })()`), 'the switcher uses the core switcher selection');
  }
  await click('#header [data-command="settings"]');
  await evaluate("document.querySelector('[data-settings-page=appearance]').focus()");
  await key("P", { shiftKey: true });
  assert.equal(await evaluate("document.activeElement.id"), "settings-search");
  assert.equal(await evaluate("document.querySelector('#settings-search').value"), "P");
  // Real text input after focus must append, not replace the first character.
  await call("Input.insertText", { text: "ressure" }); await settle();
  assert.equal(await evaluate("layerApp.app.preferences().query"), "Pressure");
  assert.deepEqual(await evaluate(`(() => {
    const dialog=document.createElement("dialog"), input=document.createElement("input");
    let cancellations=0; dialog.append(input); document.body.append(dialog);
    dialog.addEventListener("cancel", event => { event.preventDefault(); cancellations++; });
    dialog.showModal(); input.focus();
    input.dispatchEvent(new CompositionEvent("compositionstart", {data:"字",bubbles:true}));
    dialog.dispatchEvent(new Event("cancel", {cancelable:true})); const during=cancellations;
    input.dispatchEvent(new CompositionEvent("compositionend", {data:"字",bubbles:true}));
    dialog.dispatchEvent(new Event("cancel", {cancelable:true}));
    dialog.close(); dialog.remove(); return [during,cancellations];
  })()`), [0,1], "native dialog cancel preserves tracked composition and resumes afterward");
  await evaluate(`(() => { const search=document.querySelector("#settings-search"); search.dispatchEvent(new CompositionEvent("compositionstart",{data:"圧",bubbles:true})); search.dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",isComposing:false,bubbles:true,cancelable:true})); document.querySelector("#settings").dispatchEvent(new Event("cancel",{cancelable:true})); })()`);
  assert.equal(await evaluate("document.querySelector('#settings').open && !document.querySelector('#settings-search').hidden"), true, "preedit candidate Escape preserves Settings search");
  await evaluate(`document.querySelector("#settings-search").dispatchEvent(new CompositionEvent("compositionend",{data:"圧",bubbles:true}))`);
  await key("Escape");
  assert.equal(await evaluate("document.querySelector('#settings-search').hidden"), true);
  await click('.preferences-search-toggle');
  assert.equal(await evaluate("document.querySelector('#settings-search').hidden"), false);
  await evaluate("const emptySearch=document.querySelector('#settings-search');emptySearch.value='no-such-preference';emptySearch.dispatchEvent(new Event('input'))");
  assert.equal(await evaluate("document.querySelector('.preferences-empty').hidden"), false);
  await evaluate("const search=document.querySelector('#settings-search');search.value='pressure response';search.dispatchEvent(new Event('input'))");
  assert.ok(await evaluate("[...document.querySelectorAll('.preferences-search-results small')].every(n=>Math.abs(parseFloat(getComputedStyle(n).fontSize)-parseFloat(getComputedStyle(n.parentElement).fontSize)/1.2)<.02)"), 'search-result descriptions use the same Adwaita subtitle size');
  assert.equal(await evaluate("document.querySelectorAll('.preferences-search-results button').length"), 1);
  await capture("search");
  await click('.preferences-search-results button');
  assert.equal(await evaluate("layerApp.app.preferences().page"), "input");
  assert.equal(await evaluate("document.querySelector('.preferences-page:not([hidden])').dataset.page"), "input");
  await click('[data-settings-page="input"]');
  assert.equal(await evaluate("document.querySelector('#settings-search').value"), "");
  assert.equal(await evaluate("document.querySelector('#setting-platform-prediction').checked"), true);
  const nativeAvailable = await evaluate("typeof PointerEvent.prototype.getPredictedEvents === 'function'");
  assert.equal(await evaluate("document.querySelector('#setting-platform-prediction').disabled"), !nativeAvailable);
  assert.ok(await evaluate(`(() => {
    const rows=layerApp.app.preferences().pages.find(p=>p.id==='input').groups.flatMap(g=>g.rows);
    return rows[rows.findIndex(r=>r.id==='feedback')+1].id==='platform_prediction';
  })()`));
  assert.equal(await evaluate("document.querySelector('#setting-prediction-horizon .number-slider').disabled"), nativeAvailable);
  if (nativeAvailable) await click('#setting-platform-prediction');
  assert.equal(await evaluate("document.querySelector('#setting-prediction-horizon .number-value').textContent"), '16 ms', 'units appear beside numeric values');
  await click('#setting-prediction-horizon .number-value');
  await evaluate("document.querySelector('#setting-prediction-horizon .number-entry').value='4*2 ms'");
  await key('Enter');
  assert.equal(await evaluate("layerApp.state().settings.prediction_ms"), 8, 'expressions accept displayed units');
  await click('#setting-prediction-horizon .number-value');
  await evaluate("document.querySelector('#setting-prediction-horizon .number-entry').value='32'"); await key('Enter');
  await click('#setting-prediction-horizon .number-value');
  await evaluate("document.querySelector('#setting-prediction-horizon .number-entry').value=''");
  assert.equal(await evaluate('layerApp.state().settings.prediction_ms'), 32);
  await key('Enter');
  assert.equal(await evaluate('layerApp.state().settings.prediction_ms'), 16);
  await click('#setting-feedback');
  assert.equal(await evaluate("document.querySelector('#setting-prediction-horizon .number-slider').disabled"), true);
  await click('#setting-feedback');
  await click('#setting-pressure .number-value');
  assert.ok(await evaluate(`(() => { const field=document.querySelector('#setting-pressure .number-entry'), css=getComputedStyle(field); return field.getBoundingClientRect().height===34 && css.paddingLeft==='9px' && css.paddingRight==='9px' && parseFloat(css.borderRadius)===(CSS.supports('corner-shape','squircle')?12:6.48); })()`), 'settings editors use full-size Adwaita spacing with control squircles');
  await evaluate("document.querySelector('#setting-pressure .number-entry').value='1.5'");
  await key("Enter");
  await click('#setting-pressure .number-value');
  await key("b");
  assert.equal(await evaluate("layerApp.app.preferences().query"), "", "number editing does not start global search");
  assert.equal(await evaluate("layerApp.app.preferences().capture ?? null"), null);
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5);
  await action({ type: "open_settings", page: "shortcuts" });
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5, "deep link preserves accepted settings");
  const prefs = (expression) => evaluate(`(() => { const view = layerApp.app.preferences(); return ${expression}; })()`);
  const visible = (selector) => evaluate(`(() => { const node = document.querySelector(${JSON.stringify(selector)}); return !!node && node.getClientRects().length > 0; })()`);
  const type = async (selector, value) => { await evaluate(`(() => { const node=document.querySelector(${JSON.stringify(selector)}); node.focus(); node.value=${JSON.stringify(value)}; node.dispatchEvent(new Event('input')); })()`); await settle(); };
  assert.equal(await evaluate("document.querySelector('#shortcut-categories [data-category]').dataset.category"), "Modifier keys", "modifier keys come first");
  assert.ok(await evaluate(`(() => { const rows=['#shortcuts-search','#shortcut-context','#shortcut-show'].map(s=>document.querySelector(s).getBoundingClientRect()); return rows.every(r=>Math.abs(r.top+r.height/2-(rows[0].top+rows[0].height/2))<2) && rows[0].left<rows[1].left && rows[1].left<rows[2].left; })()`), "search and both filters share one line");
  assert.deepEqual(await evaluate("[...document.querySelectorAll('#shortcut-show [data-choice]')].map(o=>o.textContent)"), ["All actions", "With shortcuts", "Customized"]);
  await capture("shortcuts");
  await type('#shortcuts-search', 'z');
  for (const id of ["Undo", "Redo", "UndoWorkspace", "RedoWorkspace"]) assert.ok(await visible(`[data-shortcut="command.${id}"]`), `Z finds ${id}`);
  assert.equal(await visible('[data-shortcut="command.ZoomIn"]'), false, "one letter finds keys, not names");
  await type('#shortcuts-search', '');
  const shortcutKeyBeforePreedit = await prefs('view.shortcut_page.key');
  await evaluate(`(() => { const node=document.querySelector('#shortcuts-search'); node.focus(); node.dispatchEvent(new CompositionEvent('compositionstart',{data:'字',bubbles:true})); node.dispatchEvent(new KeyboardEvent('keydown',{key:'z',ctrlKey:true,isComposing:false,bubbles:true,cancelable:true})); })()`);
  assert.equal(await prefs('view.shortcut_page.key'), shortcutKeyBeforePreedit, 'candidate modifiers do not record a shortcut lookup');
  await evaluate(`document.querySelector('#shortcuts-search').dispatchEvent(new CompositionEvent('compositionend',{data:'字',bubbles:true}))`);
  await evaluate("(() => { const node=document.querySelector('#shortcuts-search'); node.focus(); node.dispatchEvent(new KeyboardEvent('keydown',{key:'z',ctrlKey:true,bubbles:true,cancelable:true})); })()"); await settle();
  assert.equal(await prefs("view.shortcut_page.key"), "Ctrl+Z", "pressing a shortcut in search looks it up");
  assert.ok(await visible('[data-shortcut="command.Undo"]'));
  assert.equal(await visible('[data-shortcut="command.Redo"]'), false);
  await type('#shortcuts-search', 'eraser');
  assert.ok(await evaluate("[...document.querySelectorAll('[data-shortcut]')].filter(r=>r.getClientRects().length).every(row=>row.textContent.toLowerCase().includes('eraser'))"));
  assert.equal(await evaluate(`document.querySelector('[data-shortcut="brush.3"] p').textContent`), "Brush for the Eraser tool", "same-named brushes say what they are");
  await type('#shortcuts-search', '');
  await click('[data-category="Tools"]');
  assert.equal(await evaluate("document.querySelector('#settings-title').textContent"), "Tools");
  assert.ok(await visible('#shortcut-category-back') && await visible('[data-shortcut="tools.paint"]') && !(await visible('#shortcut-categories')));
  await capture("category-tools");
  await click('[data-shortcut="tools.paint"] .shortcut-choose');
  assert.equal(await evaluate("document.querySelector('#shortcut-editor').open"), true);
  assert.equal(await evaluate("document.querySelector('#shortcut-editor h2').textContent"), "Paint tools", "the action name is in the title bar");
  await click('#remove-shortcut-0');
  assert.deepEqual(await prefs("view.shortcut_editor.bindings"), []);
  assert.ok(await visible('[data-shortcut="tools.paint"] .icon-button'), "customized rows offer reset");
  await click('#shortcut-editor-reset');
  assert.deepEqual(await prefs("view.shortcut_editor.bindings"), ["B"]);
  await click('#add-shortcut');
  assert.ok(await visible('#shortcut-recording'), "recording happens inside the editor");
  await key("Control", { ctrlKey: true });
  assert.equal(await prefs("view.capture.chord ?? null"), null);
  await key("w", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#confirm-shortcut').disabled"), true);
  await key("e");
  assert.equal(await prefs("view.capture.conflict"), "Eraser");
  assert.equal(await evaluate("document.querySelector('#confirm-shortcut').textContent"), "Reassign");
  await capture("shortcut-conflict");
  await click('#confirm-shortcut');
  assert.equal(await evaluate("layerApp.state().settings.shortcuts['command.Eraser'].length"), 0);
  assert.deepEqual(await prefs("view.shortcut_editor.bindings"), ["B", "E"], "adding keeps the existing shortcut");
  assert.equal(await evaluate(`document.querySelector('[data-shortcut="tools.paint"] .shortcut-hint').textContent`), "B / E");
  await capture("shortcut-editor");
  await click('#add-shortcut');
  await escape();
  assert.equal(await prefs("view.capture ?? null"), null, "Escape cancels recording");
  assert.equal(await evaluate("document.querySelector('#shortcut-editor').open"), true, "and keeps the editor");
  await escape();
  assert.equal(await evaluate("document.querySelector('#shortcut-editor').open"), false);
  for (const theme of ["light", "dark"]) {
    await action({ type: "set_theme", theme });
    await capture(`shortcut-modified-${theme}`);
  }
  await click('#shortcut-category-back');
  assert.ok(await visible('#shortcut-categories'));
  await type('#shortcuts-search', 'preferences');
  await click('[data-shortcut="command.Settings"] .shortcut-choose');
  await click('#add-shortcut');
  await key("j", { ctrlKey: true });
  await click('#confirm-shortcut');
  await escape();
  await type('#shortcuts-search', '');

  await click('[data-category="Modifier keys"]');
  assert.deepEqual(await evaluate("[...document.querySelectorAll('#modifier-keys [data-modifier]')].map(r=>r.dataset.modifier)"), ["Space", "Alt"]);
  await capture("modifier-keys");
  await click('[data-modifier="Alt"]');
  assert.equal(await evaluate("document.querySelector('#settings-title').textContent"), "Alt", "a modifier key slides in instead of a popup");
  assert.equal(await evaluate("document.querySelector('#modifier-same').checked"), false, "Alt differs by tool");
  assert.equal(await prefs("view.modifier_editor.actions.length"), 11, "one row per kind of tool");
  await click('#modifier-action-selection');
  assert.equal(await evaluate("document.querySelector('#action-picker').open"), true);
  assert.equal(await evaluate("document.querySelector('#action-picker h2').textContent"), "Alt · Selection tools");
  assert.match(await evaluate("document.querySelector('#action-picker-description').textContent"), /^Holding Alt/);
  await capture("modifier-picker");
  await escape();
  assert.equal(await evaluate("document.querySelector('#action-picker').open"), false, "Escape closes the picker");
  await click('#modifier-action-selection');
  await click('[id="action-command.Move"]');
  assert.equal(await evaluate("layerApp.state().settings.hold_keys.find(h=>h.key.key==='alt').actions.selection"), "command.Move");
  await capture("modifier-alt");
  await click('#shortcut-category-back');
  assert.equal(await evaluate("document.querySelector('#settings-title').textContent"), "Modifier keys");
  await click('#add-modifier-key');
  assert.equal(await evaluate("document.querySelector('#modifier-key').open"), true);
  await key("Alt", { altKey: true });
  assert.equal(await evaluate("document.querySelector('#confirm-shortcut').textContent"), "Open", "an existing modifier key opens instead");
  await click('#cancel-shortcut');
  await click('#add-modifier-key');
  await key("F13");
  await capture("modifier-new");
  await click('#confirm-shortcut');
  assert.equal(await evaluate("document.querySelector('#settings-title').textContent"), "F13");
  await click('#modifier-action-all');
  await click('[id="action-command.Pencil"]');
  assert.equal(await evaluate("layerApp.state().settings.hold_keys.find(h=>h.key.key==='f13').actions.drawing"), "command.Pencil");
  await click('#shortcut-category-back');
  await click('#shortcut-category-back');
  assert.ok(await visible('#shortcut-categories'));

  await preference({ type: "page", page: "input" });
  assert.deepEqual(await evaluate("[...document.querySelectorAll('#triggers-pen-buttons [data-trigger]')].map(r=>r.dataset.trigger)"), ["pen.button.primary", "pen.button.secondary"]);
  assert.ok(await visible('#triggers-touch-gestures') && await visible('.preference-row:has(#setting-eraser-tool)'));
  await capture("pen-and-input");
  await click('[data-trigger="pen.button.primary"]');
  assert.equal(await evaluate("document.querySelector('#settings-title').textContent"), "Lower side button");
  await click('#pen-button-same');
  await click('#pen-button-action-selection');
  await type('#action-picker-search', 'undo');
  await click('[id="action-command.Undo"]');
  assert.equal(await evaluate("layerApp.state().settings.pen_buttons['pen.button.primary'].selection"), "command.Undo");
  await capture("pen-button");
  await click('#shortcut-category-back');
  assert.equal(await evaluate(`document.querySelector('[data-trigger="pen.button.primary"] .shortcut-hint').textContent`), "Undo");
  assert.equal(await evaluate(`document.querySelector('[data-trigger="pen.button.primary"] p').textContent`), "With selection tools");
  await click('[data-trigger="touch.tap.4"]');
  assert.equal(await evaluate("document.querySelector('#action-picker').open"), true, "touch gestures open the picker directly");
  await escape();
  await preference({ type: "reset_trigger", trigger: "pen.button.primary" });
  await click('#close-settings');
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5);
  assert.equal(await evaluate("layerApp.state().requests.length"), 0);
  assert.equal(await evaluate("layerApp.state().commands.find(c=>c.id==='settings').shortcut"), "Ctrl+, / Ctrl+J");
  await action({ type: "invoke", command: "eraser" });
  await evaluate("layerApp.canvas.focus()"); await key("e");
  assert.equal(await evaluate("layerApp.state().brush.tool"), "brush", "E now selects the paint tools");
  await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true);
  await action({ type: "close_settings" });
  const saved = await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1'))");
  assert.deepEqual(saved.shortcuts["tools.paint"].map(c => c.key), ["b", "e"]);
  await reload('ready');
  assert.deepEqual(await evaluate("layerApp.state().settings"), saved);
  const launchTag = await evaluate("layerApp.app.language_tag()");
  assert.equal(await evaluate("document.documentElement.lang"), launchTag);
  const { identifier: languageScript } = await call('Page.addScriptToEvaluateOnNewDocument', {
    source: 'Object.defineProperty(navigator,"languages",{configurable:true,value:["ja-JP","zh-TW","ko","en"]});',
  });
  await action({ type:'open_settings', page:'appearance' });
  await click('[data-preference=language] summary');
  await click('[data-preference=language] [data-choice="1"]');
  await action({ type:'close_settings' });
  assert.deepEqual(await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1')).language"), { Explicit:'en' });
  await evaluate('new Promise(r=>{function wait(){if(layerApp.app.language_pending())setTimeout(wait,10);else r();}wait();})');
  assert.equal(await evaluate("layerApp.app.language_tag()"), 'en', 'language preferences update the active context');
  await reload('ready');
  assert.deepEqual(await evaluate("layerApp.state().settings.language"), { Explicit:'en' });
  assert.equal(await evaluate("document.documentElement.lang"), 'en', 'an explicit choice determines document language');
  await evaluate('window.dispatchEvent(new Event("languagechange"))');
  assert.equal(await evaluate("layerApp.app.language_tag()"), 'en', 'an explicit choice survives browser language notifications');
  await action({ type:'open_settings', page:'appearance' });
  await click('[data-preference=language] summary');
  await click('[data-preference=language] [data-choice="0"]');
  await action({ type:'close_settings' });
  await call('Page.removeScriptToEvaluateOnNewDocument', { identifier:languageScript });

  await evaluate("layerApp.canvas.focus()"); await key("j", { ctrlKey: true });
  assert.equal(await evaluate("document.querySelector('#settings').open"), true, "restored shortcuts execute");
  await preference({ type: "page", page: "shortcuts" });
  await preference({ type: "reset_all_shortcuts" });
  await action({ type: "close_settings" });
  assert.deepEqual(await evaluate("layerApp.state().settings.shortcuts"), {}, "confirmed reset is saved without a global Apply");
  await action({ type: "open_settings", page: "canvas" });
  await call("Emulation.setDeviceMetricsOverride", { width: 640, height: 600, deviceScaleFactor: 1, mobile: false });
  await capture("narrow");
  assert.equal(await evaluate("getComputedStyle(document.querySelector('.preferences-sidebar')).display"), "none");
  await click('.preferences-back');
  assert.notEqual(await evaluate("getComputedStyle(document.querySelector('.preferences-sidebar')).display"), "none");
  await click('[data-settings-page="appearance"]');
  assert.equal(await evaluate("layerApp.app.preferences().page"), "appearance");
  await evaluate("document.querySelector('#close-settings').focus()");
  await key("p");
  assert.notEqual(await evaluate("getComputedStyle(document.querySelector('.preferences-sidebar')).display"), "none", "typing reveals a collapsed sidebar");
  assert.equal(await evaluate("document.activeElement.id"), "settings-search");
  assert.equal(await evaluate("document.querySelector('#settings-search').value"), "p");
  assert.equal(await evaluate("document.querySelector('#status').textContent"), "");
  await action({ type: 'set_theme', theme: 'dark' });
  await preference({ type: 'edit', id: 'dark_base', value: '#1c2c3c' });
  await action({ type: 'close_settings' });
  const { identifier } = await call('Page.addScriptToEvaluateOnNewDocument', {
    source: 'navigator.gpu.requestAdapter = async () => null;',
  });
  await reload('unavailable');
  assert.equal(await evaluate('layerApp.state().settings.dark_base'), '#1c2c3c', 'custom colors persist across reload');
  assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).backgroundColor"), 'rgb(28, 44, 60)');
  assert.equal(await evaluate("document.querySelector('meta[name=theme-color]').content"), '#1c2c3c');
  await action({ type: 'open_settings', page: 'appearance' });
  await preference({ type: 'edit', id: 'dark_base', value: '#333333' });
  assert.equal(await evaluate("getComputedStyle(document.querySelector('#gpu-notice')).backgroundColor"), 'rgb(51, 51, 51)', 'settings still update without a GPU');
  await call('Page.removeScriptToEvaluateOnNewDocument', { identifier });
  for (const error of errors.splice(0)) assert.match(error, /^GPU canvas unavailable:/, "Only the injected adapter failure is expected");
  console.log("PASS: native-model settings pages, themes, adaptive sidebar, search, dependencies, key recording/conflicts, immediate persistence and executable restored shortcuts");
}

// Generate GTK fixtures with native_settings_typography first. These compare
// actual allocations and Pango font sizes, not duplicated expected CSS values.
export async function checkSettingsParity({ call, evaluate, settle }) {
  const dir = 'artifacts/ui/settings-audit', differences = [];
  await call('Emulation.setDeviceMetricsOverride', { width: 1280, height: 960, deviceScaleFactor: 1, mobile: false });
  await evaluate("layerApp.dispatch({type:'open_settings',page:'appearance'})");
  const pages = await evaluate("layerApp.app.preferences().pages.map(p=>p.id)");
  for (const theme of ['dark', 'light']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);
    for (const page of pages) {
      await evaluate(`layerApp.dispatch({type:'open_settings',page:'${page}'})`);
      await settle();
      assert.equal(await evaluate("document.querySelector('#status').textContent"), '', 'settings render without a caught UI error');
      const native = JSON.parse(await readFile(`${dir}/gtk-${page}-${theme}.json`, 'utf8'));
      const web = await evaluate(`(() => {
        const content=document.querySelector('.preferences-content').getBoundingClientRect();
        const bounds=n=>{const r=n.getBoundingClientRect();return [r.left-content.left,r.top-content.top,r.width,r.height];};
        const rows = layerApp.app.preferences().pages.find(p=>p.id==='${page}').groups.flatMap(g=>g.rows).map(row=>{
          const field=document.querySelector('#setting-'+row.id.replaceAll('_','-')), node=field.closest('.preference-row');
          return {id:row.id,bounds:bounds(node),labels:[...node.querySelectorAll('.preference-text label,.preference-text p,.number-title,.number-description')].map(n=>({text:n.textContent,bounds:bounds(n),font_px:parseFloat(getComputedStyle(n).fontSize)}))};
        });
        if ('${page}' === 'shortcuts') {
          rows.push({id:'shortcuts-search',bounds:bounds(document.querySelector('#shortcuts-search')),labels:[]});
          for (const node of document.querySelectorAll('.shortcut-row')) rows.push({id:'shortcut-'+node.dataset.shortcut,bounds:bounds(node),labels:[...node.querySelectorAll('.preference-text > span,.preference-text p')].map(n=>({text:n.textContent,bounds:bounds(n),font_px:parseFloat(getComputedStyle(n).fontSize)}))});
        }
        return rows;
      })()`);
      await writeFile(`${dir}/web-${page}-${theme}.json`, JSON.stringify(web, null, 2));
      const shot = await call('Page.captureScreenshot', { format: 'png' });
      await writeFile(`${dir}/web-${page}-${theme}.png`, Buffer.from(shot.data, 'base64'));
      const check = (id, what, actual, expected, tolerance = 1.1) => {
        if (Math.abs(actual - expected) > tolerance) differences.push(`${theme}/${page}/${id} ${what}: web ${actual.toFixed(2)}, GTK ${expected.toFixed(2)}`);
      };
      let nativeOnlyHeight = 0;
      for (const row of native.rows) {
        const actual = web.find(r=>r.id===row.id);
        // New Window is a native-only command, so later web rows sit higher.
        if (row.id === 'shortcut-command.NewWindow') {
          assert.equal(actual, undefined);
          nativeOnlyHeight += row.bounds[3]; continue;
        }
        assert.ok(actual, `Web must render ${row.id}`);
        const wholePixelFilterLabels = row.id === 'shortcuts-search' ? 2 : 0;
        row.bounds.forEach((v,i)=>check(row.id,['x','y','width','height'][i],actual.bounds[i],v - (i === 1 ? nativeOnlyHeight : 0),1.1 + (i === 2 ? wholePixelFilterLabels : 0)));
        for (const label of actual.labels) {
          const expected = row.labels.find(l=>l.text===label.text.replace('Use browser','Use Linux'));
          assert.ok(expected, `GTK must render ${label.text}`);
          check(row.id,'font',label.font_px,expected.font_px,.02);
          check(row.id,'label left',label.bounds[0],expected.bounds[0]);
          check(row.id,'label center Y',label.bounds[1]+label.bounds[3]/2,expected.bounds[1]+expected.bounds[3]/2-nativeOnlyHeight);
        }
      }
    }
  }
  assert.deepEqual(differences, [], 'GTK/web settings geometry and typography');
  console.log('PASS: all settings pages match measured GTK row geometry and subtitle fonts in both themes');
}

export async function checkLiveLanguage({call,evaluate,settle}) {
  const dir=process.env.LAYER_TEST_ARTIFACTS||"artifacts/localization-live-switching/web";await mkdir(dir,{recursive:true});
  await evaluate(`window.languageTiming={quanta:[],publications:[],methods:{},longTasks:[]};for(const method of ['state_update','catalog','bootstrap_view','document_delivery_copy','preferences_cached','panel_view','workspace_update','layout_update','editor_models','application_menus','document_tabs']){const original=layerApp.app[method].bind(layerApp.app);layerApp.app[method]=function(...args){const started=performance.now();try{return original(...args);}finally{(languageTiming.methods[method]??=[]).push(performance.now()-started);}};}const prepare=layerApp.app.prepare_language.bind(layerApp.app);layerApp.app.prepare_language=function(...args){const start=performance.now();try{const result=prepare(...args);if(result)queueMicrotask(()=>languageTiming.publications.push({start,duration:performance.now()-start}));return result;}finally{languageTiming.quanta.push(performance.now()-start);}};window.languageObserver=new PerformanceObserver(list=>{for(const task of list.getEntries())languageTiming.longTasks.push({start:task.startTime,duration:task.duration});});languageObserver.observe({type:'longtask',buffered:false});`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const ready=async(tag,session)=>{
    const expression=`new Promise((resolve,reject)=>{const end=performance.now()+20000;function poll(){if(window.layerApp&&!layerApp.app.language_pending()&&document.documentElement.lang===${JSON.stringify(tag)})resolve(true);else if(performance.now()>end)reject(Error('Language publication '+document.documentElement.lang));else setTimeout(poll,10);}poll();})`;
    if(session){const r=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true},session);if(r.exceptionDetails)throw Error(r.exceptionDetails.exception?.description);}
    else await evaluate(expression);
  };
  const choice=async(index,tag)=>{await evaluate(`document.querySelector('[data-preference=language] summary').click();document.querySelector('[data-preference=language] [data-choice="${index}"]').click()`);await ready(tag);await settle();};
  await send({type:'invoke',command:'new_document'});
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){const create=document.querySelector('[data-document-action=create]');if(create){for(const entry of document.querySelectorAll('[data-document-field=width],[data-document-field=height]')){entry.value='96';entry.dispatchEvent(new Event('input',{bubbles:true}));}create.click();resolve();}else if(performance.now()>end)reject(Error('New drawing'));else setTimeout(poll,20);}poll();})`);
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function poll(){if(layerApp.app.document_tabs(0).tabs.length===2&&!layerApp.documents.busy()&&layerApp.app.brush_ready())resolve();else if(performance.now()>end)reject(Error('Second drawing'));else setTimeout(poll,20);}poll();})`);
  for(const theme of ['light','dark']) {
    await send({type:'set_theme',theme});
    await send({type:'open_settings',page:'appearance'});
    await evaluate('window.languageOwner={app:layerApp.app,canvas:layerApp.canvas,settings:document.querySelector("#settings"),language:document.querySelector("#setting-language"),time:performance.timeOrigin};');
    for(const [index,tag] of [[1,'en'],[2,'ja'],[3,'zh-Hans'],[4,'zh-Hant'],[5,'ko'],[1,'en']]) {
      await choice(index,tag);
      assert.equal(await evaluate('languageOwner.app===layerApp.app&&languageOwner.canvas===layerApp.canvas&&languageOwner.settings===document.querySelector("#settings")&&languageOwner.language===document.querySelector("#setting-language")&&languageOwner.time===performance.timeOrigin'),true);
      assert.equal(await evaluate('document.querySelector("#settings-title").textContent===layerApp.app.preferences().pages.find(p=>p.id==="appearance").title'),true);
      const numbered={en:id=>`Untitled ${id}`,ja:id=>`無題 ${id}`,'zh-Hans':id=>`未命名${id}`,'zh-Hant':id=>`未命名${id}`,ko:id=>`제목 없음 ${id}`};
      const tabs=await evaluate('layerApp.app.document_tabs(0).tabs.map(t=>({id:String(t.id),title:t.title}))');
      assert.deepEqual(tabs.map(t=>t.title),tabs.map(t=>numbered[tag](t.id)),'active and parked generated captions use the published language');

    }
    await send({type:'close_settings'});
    await send({type:'invoke',command:'canvas_size'});
    await evaluate(`(()=>{const n=document.querySelector('[data-canvas-size=width]');n.entry.focus();n.entry.value='１２+invalid';n.entry.dispatchEvent(new Event('input',{bubbles:true}));n.entry.setSelectionRange(1,4);window.languageDraft={number:n,entry:n.entry,value:n.entry.value,start:n.entry.selectionStart,end:n.entry.selectionEnd,history:layerApp.state().document_file.epoch};})()`);
    const before=await evaluate('layerApp.state().tabs.map(t=>[t.width,t.height])');
    const saved=await evaluate('JSON.stringify({...layerApp.state().settings,language:{Explicit:"ja"}})');
    await evaluate(`localStorage.setItem('layer.preferences.v1',${JSON.stringify(saved)})`);
    await evaluate(`languageDraft.entry.dispatchEvent(new CompositionEvent('compositionstart',{bubbles:true,data:'１２'}))`);
    await send({type:'restore_saved_settings',saved});
    await new Promise(resolve=>setTimeout(resolve,200));
    assert.equal(await evaluate('document.documentElement.lang'),'en','Publication waits for an active preedit');
    assert.equal(await evaluate('languageDraft.entry.value'), '１２+invalid');
    await evaluate(`languageDraft.entry.dispatchEvent(new CompositionEvent('compositionend',{bubbles:true,data:'１２'}))`);
    await ready('ja');await settle();
    assert.deepEqual(await evaluate('layerApp.state().tabs.map(t=>[t.width,t.height])'),before);
    assert.equal(await evaluate(`languageDraft.number===document.querySelector('[data-canvas-size=width]')&&languageDraft.entry===document.activeElement&&languageDraft.entry.value===languageDraft.value&&languageDraft.entry.selectionStart===languageDraft.start&&languageDraft.entry.selectionEnd===languageDraft.end`),true,'switch preserves dirty numeric entry, focus and selection');
    assert.equal(await evaluate('document.querySelector("#canvas-size-title").textContent===layerApp.state().layer_tools.canvas_size.title'),true);
    const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${theme}-ja-size.png`,Buffer.from(shot.data,'base64'));
    await send({type:'canvas_size',action:{op:'cancel'}});
  }
  const {targetId}=await call('Target.createTarget',{url:await evaluate('location.href')},null);
  const {sessionId}=await call('Target.attachToTarget',{targetId,flatten:true},null);
  try {
    await ready('ja',sessionId);
    await send({type:'open_settings',page:'appearance'});
    await choice(4,'zh-Hant');await ready('zh-Hant',sessionId);
    await evaluate(`layerApp.dispatch({type:'preferences',action:{type:'edit',id:'language',value:2}});layerApp.dispatch({type:'preferences',action:{type:'edit',id:'language',value:5}})`);
    await ready('ko');await ready('ko',sessionId);
    await call('Runtime.evaluate',{expression:"layerApp.dispatch({type:'open_settings',page:'appearance'})"},sessionId);
    await Promise.all([
      evaluate("layerApp.dispatch({type:'preferences',action:{type:'edit',id:'language',value:2}})"),
      call('Runtime.evaluate',{expression:"layerApp.dispatch({type:'preferences',action:{type:'edit',id:'language',value:4}})"},sessionId),
    ]);
    const latest=await evaluate("JSON.parse(localStorage.getItem('layer.preferences.v1')).language.Explicit");
    await ready(latest);await ready(latest,sessionId);
    await call('Page.bringToFront',{},sessionId);await call('Page.bringToFront');
    await ready(latest);await ready(latest,sessionId);
    await evaluate('Object.defineProperty(navigator,"languages",{configurable:true,value:["fr","ja-JP","ko"]});window.dispatchEvent(new Event("languagechange"));');
    await call('Runtime.evaluate',{expression:'Object.defineProperty(navigator,"languages",{configurable:true,value:["fr","ja-JP","ko"]});window.dispatchEvent(new Event("languagechange"));'},sessionId);
    await choice(0,'ja');await ready('ja',sessionId);
    await choice(1,'en');await ready('en',sessionId);
  } finally {await call('Target.closeTarget',{targetId},null);}
  await send({type:'close_settings'});
  await writeFile(`${dir}/timing.json`,JSON.stringify(await evaluate('languageObserver.disconnect();({maximumQuantum:Math.max(...languageTiming.quanta),quanta:languageTiming.quanta.length,publications:languageTiming.publications,methods:Object.fromEntries(Object.entries(languageTiming.methods).map(([key,values])=>[key,{calls:values.length,total:values.reduce((a,b)=>a+b,0),maximum:Math.max(...values)}])),longTasks:languageTiming.longTasks,memory:performance.memory?{used:performance.memory.usedJSHeapSize,total:performance.memory.totalJSHeapSize}:null})'),null,2));
}
