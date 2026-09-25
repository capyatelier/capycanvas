// Focused visual/DOM conformance, using the same hardware browser as test.mjs.
// GTK reference measurements come from native_web_parity_reference, not CSS.
import assert from "node:assert/strict";
import { checkPenRendering } from "./pen-rendering.test.mjs";
import { mkdir, readFile, writeFile } from "node:fs/promises";

export async function checkParity({ call, evaluate, settle }) {
  const dir = "artifacts/ui/parity";
  await mkdir(dir, { recursive: true });
  const native = JSON.parse(await readFile(`${dir}/gtk-dark.json`, "utf8"));
  const tabs = (id, panel) => ({ kind: "tabs", id, panels: [panel], active: panel, tab_style: "automatic" });
  const fixture = { version: 1, zen_mode: false, layout: { next_id: 9, bands: [
    { id: 3, edge: "left", extent: 232, root: { kind: "split", id: 4, axis: "vertical", fraction: .68, first: tabs(5, "brushes"), second: tabs(6, "sizes") } },
    { id: 7, edge: "right", extent: 232, root: { kind: "tabs", id: 8, panels: ["layers", "adjustments", "properties"], active: "layers", tab_style: "automatic" } },
    { id: 1, edge: "top", extent: 42, root: tabs(2, "toolbar") },
  ] } };
  const restoreFixture = async (zen_mode = false) => { await evaluate(`layerApp.dispatch({type:'restore_workspace',workspace:${JSON.stringify({ ...fixture, zen_mode })}})`); await settle(); };
  await restoreFixture();
  const catalog = await evaluate("layerApp.app.catalog()");
  const settings = await evaluate("layerApp.state().settings");
  assert.deepEqual(
    await evaluate(`(() => {
    const n=document.querySelector('#size-number .number-entry');
    return [getComputedStyle(n).fontVariantNumeric,...[...n.closest('.number-control').querySelectorAll('button')].map(b=>getComputedStyle(b).borderLeftWidth)];
  })()`),
    ["tabular-nums", "0px", "0px", "0px"],
  );
  assert.deepEqual(
    await evaluate(
      `[...document.querySelectorAll('.toolbar-controls > .tile-button svg')].map(s=>s.dataset.asset)`,
    ),
    ["brush", "eraser", "lasso", "move", "undo", "redo", "colors", "opacity"],
  );
  const menus = await evaluate("layerApp.app.editor_models(0,0).application_menus.map(m=>m.id)");
  assert.deepEqual(
    await evaluate("[...document.querySelectorAll('.header-menu[data-menu]')].map(m=>m.dataset.menu)"),
    menus,
  );
  assert.deepEqual(
    await evaluate("[...document.querySelectorAll('.brushes-control .tool-choice-button')].map(n=>n.dataset.toolChoice)"),
    await evaluate("(({groups,subtools})=>[...groups,...subtools].map(i=>i.label))(layerApp.state().tool_panels.brushes ?? layerApp.state().tool_set)"),
  );
  for (const [id, spec] of [
    ["size-number", catalog.brush_size],
    [".sizes-panel [data-control=brush_opacity] .number-control", catalog.opacity],
    ["layer-opacity", catalog.opacity],
  ]) {
    assert.deepEqual(
      await evaluate(
        `(() => {const n=document.querySelector(${JSON.stringify(id.startsWith(".") ? id : `#${id}`)}).querySelector('input[type=range]');return [Number(n.min),Number(n.max),n.step]})()`,
      ),
      [0, 1, "any"],
    );
  }
  // Real DOM shortcut wiring uses Rust's modifier guards and repeat policy.
  assert.deepEqual(
    await evaluate(`(() => {
    document.activeElement?.blur();
    layerApp.dispatch({type:'invoke',command:'eraser'});
    const fire = (type,key,extra={}) => layerApp.canvas.dispatchEvent(new KeyboardEvent(type,{key,bubbles:true,cancelable:true,...extra}));
    fire('keydown','b',{ctrlKey:true});
    const modified = layerApp.state().brush.tool;
    fire('keyup','b',{ctrlKey:true});
    fire('keydown','b');
    const bare = layerApp.state().brush.tool;
    fire('keyup','b');
    fire('keydown','Tab'); fire('keydown','Tab',{repeat:true});
    const repeated = layerApp.state().workspace.zen_mode;
    fire('keyup','Tab'); fire('keydown','Tab'); fire('keyup','Tab');
    return [modified,bare,repeated,layerApp.state().workspace.zen_mode];
  })()`),
    ["eraser", "brush", true, false],
  );
  await call("Emulation.setDeviceMetricsOverride", {
    width: 1200,
    height: 900,
    deviceScaleFactor: native.scale ?? 1,
    mobile: false,
  });
  await settle();
  await call("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
  const action = async (value) => {
    await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`);
    await settle();
  };
  const click = async (selector) => {
    await evaluate(
      `document.querySelector(${JSON.stringify(selector)}).click()`,
    );
    await settle();
  };
  const capture = async (name) => {
    await evaluate(
      "document.activeElement?.blur(); Promise.all([...document.images].map(i=>i.decode()))",
    );
    await settle();
    const shot = await call("Page.captureScreenshot", {
      format: "png",
      clip: {
        x: 0,
        y: 0,
        width: await evaluate("innerWidth"),
        height: await evaluate("innerHeight"),
        scale: 1 / (native.scale ?? 1),
      },
    });
    await writeFile(`${dir}/web-${name}.png`, Buffer.from(shot.data, "base64"));
  };
  await action({ type: "invoke", command: "fit_canvas" });
  const metrics = {};
  for (const theme of ["dark", "light"]) {
    await action({ type: "invoke", command: "pen" });
    await action({ type: "system_theme_changed", theme });
    await capture(theme);
    metrics[theme] = await evaluate(`(() => {
      const selectors = ['#document-title','#zen-button','.header-menu > summary','.tool-groups .tool-choice-button','.tool-subtools .tool-choice-button[data-brush]','.size-controls','.size-controls .number-control','.size-button','.layer-row','.layer-row > button','.layer-name','.layer-thumbnail:not([hidden])','#layer-opacity','#view-info','.dock-group','.dock-tab'];
      return Object.fromEntries(selectors.map(s=>[s,[...document.querySelectorAll(s)].map(n=>{const b=n.getBoundingClientRect(),c=getComputedStyle(n);return {bounds:[b.x,b.y,b.width,b.height],font:c.font,color:c.color,background:c.backgroundColor}})]));
    })()`);
    await click('[data-command="settings"]');
    await capture(`settings-${theme}`);
    assert.deepEqual(
      await evaluate(
        `(() => {const b=document.querySelector('#settings').getBoundingClientRect();return [b.x,b.y,b.width,b.height]})()`,
      ),
      [100, 78, 1000, 744],
    );
    await click("#close-settings");
    assert.equal(
      await evaluate("layerApp.state().settings_open"),
      false,
    );
  }
  await writeFile(
    `${dir}/web-metrics.json`,
    JSON.stringify(metrics, null, 2) + "\n",
  );
  // GTK rounds child allocations to integer pixels; CSS retains fractional
  // split sizes. Allow at most one logical pixel for that rounding difference.
  const nodes = [];
  const flatten = (n) => {
    nodes.push(n);
    n.children.forEach(flatten);
  };
  flatten(native);
  const nativeHeader = nodes.filter((n) => (n.css.includes("chrome-control") || n.css.includes("capy-button")) && n.bounds[0] < 600)
    .sort((a, b) => a.bounds[0] - b.bounds[0]);
  const webHeader = [
    ...metrics.dark["#zen-button"],
    ...metrics.dark[".header-menu > summary"].filter((n) => n.bounds[2] > 0),
  ];
  for (const header of [nativeHeader, webHeader]) {
    assert.equal(header.length, 1 + menus.length);
    assert.deepEqual(header[0].bounds, [6, 6, 36, 36]);
    header.slice(1).forEach((n, i) => {
      assert.deepEqual([n.bounds[1], n.bounds[3]], [11, 26]);
      assert.ok(Math.abs(n.bounds[0] - header[i].bounds[0] - header[i].bounds[2] - (i ? 2 : 10)) <= 1);
    });
  }
  webHeader.forEach((n, i) => assert.ok(Math.abs(n.bounds[2] - nativeHeader[i].bounds[2]) <= 1));
  const nativePanels = nodes.filter((n) => n.css.includes("dock-panel"));
  const leftPanel = nativePanels[0].bounds;
  const rightPanel = nativePanels.find((n) => n.bounds[0] > 900).bounds;
  const topRibbon = nativePanels.find((n) =>
    n.css.includes("tool-strip"),
  ).bounds;
  assert.equal(leftPanel[0], 6);
  assert.equal(1200 - rightPanel[0] - rightPanel[2], 6);
  assert.equal(900 - rightPanel[1] - rightPanel[3], 6);
  assert.equal(topRibbon[0] - leftPanel[0] - leftPanel[2], 6);
  assert.equal(
    topRibbon[1] - nativeHeader[0].bounds[1] - nativeHeader[0].bounds[3],
    6,
  );
  const close = nodes.find((n) => n.css.includes("close")).children[0].bounds;
  assert.deepEqual(close.slice(2), [24, 24]);
  assert.ok(close[1] >= 10 && close[1] <= 14);
  assert.ok(
    1200 - close[0] - close[2] >= 10 && 1200 - close[0] - close[2] <= 14,
  );
  const nativeFonts = (n, interior = false) => {
    interior ||= n.css.includes("dock-panel");
    if (
      (interior && (n.type === "GtkLabel" || n.type === "GtkText")) ||
      n.css.includes("status-bubble")
    )
      assert.ok(
        Math.abs(parseFloat(n.font.split(" ").at(-1)) - 11 * 4 / 3) < .02,
        `${n.name}: ${n.font}`,
      );
    n.children.forEach((c) => nativeFonts(c, interior));
  };
  nativeFonts(native);
  const webFonts = await evaluate(`(() => {
    const target=getComputedStyle(document.querySelector('.size-button')).fontSize;
    return {target, different:[...document.querySelectorAll('.panel button,.panel input,.panel label,.panel span,#view-info')].filter(n=>getComputedStyle(n).fontSize!==target).map(n=>n.className||n.id), tab:getComputedStyle(document.querySelector('.dock-tab')).fontSize};
  })()`);
  assert.ok(Math.abs(parseFloat(webFonts.target) - 11 * 4 / 3) < .02);
  assert.deepEqual(webFonts.different, []);
  assert.equal(webFonts.tab, webFonts.target);
  const toolSizes = await evaluate(`(() => {
    const size=n=>{const b=n.getBoundingClientRect();return [b.width,b.height]};
    return {tiles:[...document.querySelectorAll('.toolbar-controls > .tile-button')].map(size), icons:[...document.querySelectorAll('.toolbar-controls > .tile-button svg')].map(size), columns:[...document.querySelectorAll('.layer-row > button')].map(size)};
  })()`);
  for (const size of toolSizes.tiles) assert.deepEqual(size, [36, 36]);
  for (const size of toolSizes.icons) assert.deepEqual(size, [16, 16]);
  assert.ok(toolSizes.columns.length > 0);
  for (const size of toolSizes.columns) assert.deepEqual(size, [24, 36]);
  const nativeColumns = nodes.filter((n) => n.css.includes("layer-column"));
  assert.equal(nativeColumns.length, toolSizes.columns.length);
  for (const n of nativeColumns) assert.deepEqual(n.bounds.slice(2), [24, 36]);
  const comparisons = [
    [".tool-groups .tool-choice-button", (n) => n.css.includes("tool-group")],
    [".tool-subtools .tool-choice-button[data-brush]", (n) => n.css.includes("brush-choice")],
    [".size-controls .number-control", (n) => n.name === "brush-size"],
    [".size-button", (n) => n.css.includes("size-preset")],
    [".layer-row", (n) => n.css.includes("layer-row")],
    [".layer-row > button", (n) => n.css.includes("layer-column")],
    [".layer-name", (n) => n.css.includes("layer-name")],
    [".layer-thumbnail:not([hidden])", (n) => n.css.includes("layer-thumbnail")],
    ["#layer-opacity", (n) => n.type === "CapyNumberControl" && n.bounds[0] > 900],
    ["#view-info", (n) => n.css.includes("status-bubble")],
  ];
  const differences = [];
  for (const [selector, predicate] of comparisons) {
    const actual = metrics.dark[selector];
    const expected = nodes.filter(predicate);
    assert.equal(actual.length, expected.length, selector);
    actual.forEach((n, i) =>
      n.bounds.forEach((value, axis) => {
        const difference = Math.abs(value - expected[i].bounds[axis]);
        if (difference > 1)
          differences.push({
            selector,
            index: i,
            axis,
            web: value,
            gtk: expected[i].bounds[axis],
          });
      }),
    );
  }
  await writeFile(
    `${dir}/geometry-differences.json`,
    JSON.stringify(differences, null, 2) + "\n",
  );
  assert.deepEqual(
    differences,
    [],
    "native/web control geometry (one-pixel tolerance)",
  );
  const insets = await evaluate(`(() => {
    const tab=document.querySelector('.dock-tab'), range=document.createRange();range.selectNodeContents(tab);
    const textInset=range.getBoundingClientRect().left-tab.getBoundingClientRect().left;
    return [...document.querySelectorAll('.panel-grip')].map(n=>({textInset,inkInset:n.getBoundingClientRect().right-(n.querySelector('svg').getBoundingClientRect().left+11.6)}));
  })()`);
  for (const { textInset, inkInset } of insets) {
    assert.equal(textInset, 8);
    assert.ok(Math.abs(inkInset - textInset) < 0.01);
  }
  // Probe state-dependent CSS too: screenshots without hover missed the old
  // bright-blue divider and grip-button backgrounds.
  await call("DOM.enable");
  await call("CSS.enable");
  const { root } = await call("DOM.getDocument");
  for (const selector of [".divider", ".panel-grip"]) {
    const { nodeId } = await call("DOM.querySelector", {
      nodeId: root.nodeId,
      selector,
    });
    await call("CSS.forcePseudoState", {
      nodeId,
      forcedPseudoClasses: ["hover", "active", "focus", "focus-visible"],
    });
    assert.deepEqual(
      await evaluate(
        `(() => {const c=getComputedStyle(document.querySelector(${JSON.stringify(selector)}));return [c.backgroundColor,c.outlineStyle,c.boxShadow]})()`,
      ),
      ["rgba(0, 0, 0, 0)", "none", "none"],
    );
    await call("CSS.forcePseudoState", { nodeId, forcedPseudoClasses: [] });
  }
  // Compare real shadow pixels as well as CSS parameters; GTK/Skia blur
  // rounding can differ slightly. Chrome tags captures with its output ICC
  // profile, so decode both hosts' PNGs in the page to compare sRGB values.
  // GTK's render-node capture extends evenly past the window for panel shadows.
  const toolbar = nodes.find((n) => n.css.includes("tool-strip")).bounds;
  const points = [
    [500, 100],
    [970, 400],
    ...Array.from({ length: 12 }, (_, i) => [234 + i, 400]),
    ...Array.from({ length: 16 }, (_, i) => [500, toolbar[1] + toolbar[3] + i]),
  ];
  const pixels = async (name, samples = points) => {
    const png = await readFile(`${dir}/${name}.png`);
    return evaluate(
      `(async () => {const image=new Image();image.src='data:image/png;base64,${png.toString("base64")}';await image.decode();const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const ctx=c.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);const dx=(image.width-1200)/2,dy=(image.height-900)/2;return ${JSON.stringify(samples)}.map(([x,y])=>Array.from(ctx.getImageData(x+dx,y+dy,1,1).data).slice(0,3));})()`,
    );
  };
  const shadow = {};
  for (const theme of ["dark", "light"]) {
    const gtk = await pixels(`gtk-${theme}`),
      web = await pixels(`web-${theme}`);
    shadow[theme] = points.map((point, i) => ({
      point,
      gtk: gtk[i],
      web: web[i],
    }));
  }
  await writeFile(
    `${dir}/shadow-samples.json`,
    JSON.stringify(shadow, null, 2) + "\n",
  );
  for (const theme of ["dark", "light"]) {
    const maxDelta = Math.max(
      ...shadow[theme].flatMap((s) =>
        s.gtk.map((v, i) => Math.abs(v - s.web[i])),
      ),
    );
    assert.ok(
      maxDelta <= 3,
      `${theme} panel/shadow pixels differ by ${maxDelta}/255`,
    );
    // GTK symbolic recoloring applies per SVG shape: classes only on a
    // parent <g> incorrectly filled outlines. The eye keeps a clear iris
    // around a filled pupil despite GTK/Skia's different antialiasing.
    const eyes = nativeColumns.filter((_, i) => i % 2 === 0).flatMap(({ bounds: [x, y, w, h] }) =>
      [[x - 3, y + h / 2], [x + w / 2 + 2, y + h / 2], [x + w / 2, y + h / 2]]);
    const foreground = metrics[theme][".layer-row > button"][0].color.match(/[\d.]+/g).slice(0, 3).map(Number);
    const distance = (a, b) => Math.max(...a.map((v, i) => Math.abs(v - b[i])));
    for (const host of ["gtk", "web"]) {
      const samples = await pixels(`${host}-${theme}`, eyes);
      for (let i = 0; i < samples.length; i += 3) {
        const [background, iris, pupil] = samples.slice(i, i + 3), contrast = distance(background, foreground);
        assert.ok(distance(background, iris) < .5 * contrast, `${host} ${theme} eye ${i / 3} must have a clear iris`);
        assert.ok(distance(pupil, foreground) < .5 * contrast, `${host} ${theme} eye ${i / 3} must have a filled pupil`);
      }
    }
  }

  // Pen-first chrome: no browser selection/tap/focus decorations, but a real
  // copyable title and editable values; no aria-hidden on interactive controls.
  assert.deepEqual(
    await evaluate(`(() => {
    const selectors=['#canvas','.brush-preview','.panel-grip','.header-menu summary','.dock-tab'];
    return selectors.map(s=>{const n=document.querySelector(s);n.focus?.();const c=getComputedStyle(n);return [c.userSelect,c.webkitTapHighlightColor,c.outlineStyle]});
  })()`),
    Array(5).fill(["none", "rgba(0, 0, 0, 0)", "none"]),
  );
  assert.deepEqual(
    await evaluate(
      `['#document-title','#size-number .number-entry','#view-info'].map(s=>getComputedStyle(document.querySelector(s)).userSelect)`,
    ),
    ["text", "text", "text"],
  );
  assert.equal(
    await evaluate(
      `(() => { const n=document.querySelector('#document-title');const range=document.createRange();range.selectNodeContents(n);const s=getSelection();s.removeAllRanges();s.addRange(range);const text=s.toString();s.removeAllRanges();return text; })()`,
    ),
    "Untitled · 2048 × 1536",
  );
  assert.equal(
    await evaluate("document.querySelector('.brush-preview').draggable"),
    false,
  );
  await evaluate(
    `const menus=[...document.querySelectorAll('.header-menu')];menus[0].open=true;menus[1].open=true;`,
  );
  assert.equal(
    await evaluate("document.querySelectorAll('.header-menu[open]').length"),
    1,
  );
  await evaluate(
    "window.dismissPenCalls=0; window.dismissOriginalPen=layerApp.app.pen; layerApp.app.pen=function(...args){dismissPenCalls++; return dismissOriginalPen.apply(this,args)};",
  );
  await evaluate(
    "canvas.dispatchEvent(new PointerEvent('pointerdown',{pointerId:901,pointerType:'pen',button:0,buttons:1,clientX:600,clientY:450,bubbles:true,cancelable:true}));",
  );
  assert.equal(
    await evaluate("document.querySelectorAll('.header-menu[open]').length"),
    0,
  );
  assert.equal(await evaluate("dismissPenCalls"), 0);
  await evaluate(
    "[...document.querySelectorAll('.toolbar-controls > .tile-button')].find(t=>t.querySelector('svg[data-asset=opacity]')).querySelector('button').click()",
  );
  await settle();
  assert.ok(await evaluate("layerApp.state().customization.drawer"));
  assert.equal(
    await evaluate("(({left,right,top,bottom})=>300>=left&&300<=right&&800>=top&&800<=bottom)(document.querySelector('.content-drawer').getBoundingClientRect())"),
    false,
  );
  await evaluate(
    "canvas.dispatchEvent(new PointerEvent('pointerdown',{pointerId:902,pointerType:'pen',button:0,buttons:1,clientX:300,clientY:800,bubbles:true,cancelable:true}));",
  );
  await settle();
  assert.equal(await evaluate("layerApp.state().customization.drawer ?? null"), null);
  assert.equal(await evaluate("dismissPenCalls"), 0);
  await evaluate(
    "layerApp.app.pen=dismissOriginalPen; delete window.dismissOriginalPen;",
  );

  const beforeStep = await evaluate("layerApp.state().brush.diameter");
  await click('.size-controls [aria-label="Increase Brush size"]');
  assert.equal(await evaluate("layerApp.state().brush.diameter"), beforeStep + catalog.brush_size.step);
  await click('[data-size="96"]');
  assert.equal(
    await evaluate("document.querySelector('#size-number .number-entry').value"),
    "96.0",
  );
  await evaluate(
    `window.savedSlider=document.querySelector('#layer-opacity input[type=range]');savedSlider.value=.4;savedSlider.dispatchEvent(new Event('input'));`,
  );
  assert.equal(
    await evaluate("document.querySelector('#layer-opacity input[type=range]')===savedSlider"),
    true,
  );
  assert.ok(
    Math.abs(
      (await evaluate("layerApp.state().layer_tools.editing_layer.opacity")) -
        0.4,
    ) < 0.001,
  );
  await click('.layer-footer [aria-label="New layer"]');
  assert.equal(await evaluate("layerApp.state().layers.length"), 3);
  await evaluate("layerApp.dispatch({type:'layer',action:{op:'delete_selected'}})");
  await click('[data-command="settings"]');
  await click('[data-settings-page="input"]');
  await click('#setting-pressure .number-value');
  await evaluate("document.querySelector('#setting-pressure .number-entry').value='1.5'");
  assert.equal(
    await evaluate(
      "document.querySelector('#setting-pressure .number-entry').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}))",
    ),
    false,
  );
  assert.equal(
    await evaluate("document.querySelector('#settings').open"),
    true,
  );
  await click('#close-settings');
  assert.equal(await evaluate("layerApp.state().settings.pressure_gamma"), 1.5);
  await click('[data-command="settings"]');
  await click('[data-settings-page="input"]');
  await click('#setting-pressure [aria-label="Increase Pressure response"]');
  await click("#close-settings");
  assert.ok(
    Math.abs(
      (await evaluate("layerApp.state().settings.pressure_gamma")) - 1.55,
    ) < 0.001,
  );

  // Geometry, pressure, transformed mask and DPI cases live in shared Rust
  // cursor tests. Browser suites share the actual GPU pixel/input checks,
  // which inject CDP input that this suite otherwise ignores.
  await restoreFixture();
  await call("Input.setIgnoreInputEvents", { ignore: false });
  await checkPenRendering({call, evaluate, settle});
  await call("Input.setIgnoreInputEvents", { ignore: true });
  await action({ type: "restore_settings", settings });

  // No desktop input injection required: exercise host DOM listeners directly
  // for Zen/scroll, and the same shared actions used by native docking controls.
  await action({ type: "system_theme_changed", theme: "dark" });
  await action({ type: "restore_settings", settings: { ...settings, zen_reveal_at_edges: true } });
  await click("#zen-button");
  // Read the synchronous listener result in the same task, before compositor
  // hover events from the real desktop can replace the test pointer position.
  const point = (x, y) =>
    evaluate(`(() => {
    window.dispatchEvent(new PointerEvent('pointermove',{clientX:${x},clientY:${y},pointerType:'pen'}));
    return document.querySelector('#workspace').classList.contains('zen-hidden');
  })()`);
  assert.equal(await point(600, 450), true);
  assert.equal(await point(600, 95), true);
  assert.equal(await point(600, 79), false);
  assert.equal(await point(600, 115), false);
  assert.equal(await point(600, 150), false); // 80px keep-visible margin below ribbon.
  assert.equal(await point(600, 170), true);
  assert.equal(await point(600, 899), true); // HUD alone is not a bottom panel.
  assert.equal(await point(1, 450), false);
  assert.equal(await point(600, 450), true);
  assert.equal(await point(1199, 450), false);
  for (const panel of ["brushes", "sizes"])
    await action({
      type: "move_panel",
      panel,
      target: { kind: "tab", group: 8 },
    });
  assert.equal(await point(600, 450), true);
  assert.equal(await point(1, 450), true); // No remaining left dock.
  for (const panel of ["brushes", "sizes", ...fixture.layout.bands[1].root.panels])
    await action({type:"customize",action:{type:"set_panel_visible",panel,visible:false}});
  await action({
    type: "move_panel",
    panel: "toolbar",
    target: { kind: "edge", edge: "bottom", outer: false },
  });
  assert.equal(await point(600, 450), true);
  assert.equal(await point(1199, 450), true); // No remaining right dock.
  assert.equal(await point(600, 899), false); // Bottom dock now exists.
  await action({type:"customize",action:{type:"set_panel_visible",panel:"toolbar",visible:false}});
  assert.equal(await point(600, 450), true);
  assert.equal(await point(600, 899), true);
  assert.equal(await point(600, 1), false); // Header always remains reachable.
  await restoreFixture(true);
  assert.equal(await point(600, 450), true);
  await capture("zen");
  await click("#zen-button");
  assert.equal(await evaluate("layerApp.state().workspace.zen_mode"), false);
  const camera = await evaluate(
    "(({zoom,translation})=>({zoom,translation}))(layerApp.state().camera)",
  );
  await evaluate(
    "canvas.dispatchEvent(new WheelEvent('wheel',{deltaY:40,clientX:600,clientY:450,cancelable:true}))",
  );
  const scrolled = await evaluate(
    "(({zoom,translation})=>({zoom,translation}))(layerApp.state().camera)",
  );
  assert.equal(scrolled.zoom, camera.zoom);
  assert.ok(scrolled.translation[1] < camera.translation[1]);
  await evaluate(
    "canvas.dispatchEvent(new WheelEvent('wheel',{deltaY:40,shiftKey:true,clientX:600,clientY:450,cancelable:true}))",
  );
  assert.ok(
    (await evaluate("layerApp.state().camera.translation[0]")) <
      scrolled.translation[0],
  );
  await evaluate(
    "canvas.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,ctrlKey:true,clientX:600,clientY:450,cancelable:true}))",
  );
  assert.ok((await evaluate("layerApp.state().camera.zoom")) > camera.zoom);

  const fitTiles = () =>
    evaluate(`(() => {
    const strip=document.querySelector('.toolbar-controls'), b=strip.getBoundingClientRect(), grip=strip.querySelector('.panel-grip').getBoundingClientRect();
    const horizontal=strip.dataset.axis==='horizontal';
    return {width:b.width,height:b.height,fits:[...strip.querySelectorAll('.tile-button')].every(n=>{const t=n.getBoundingClientRect();return t.x>=b.x&&t.y>=b.y&&t.right<=b.right&&t.bottom<=b.bottom&&(horizontal?t.right+2<=grip.x:t.bottom+2<=grip.y)})};
  })()`);
  await call("Emulation.setDeviceMetricsOverride", {
    width: 680,
    height: 900,
    deviceScaleFactor: native.scale ?? 1,
    mobile: false,
  });
  await settle();
  assert.ok((await fitTiles()).height > 36);
  assert.equal((await fitTiles()).fits, true);
  await capture("ribbon-wrap");
  await call("Emulation.setDeviceMetricsOverride", {
    width: 1200,
    height: 900,
    deviceScaleFactor: native.scale ?? 1,
    mobile: false,
  });
  await settle();
  assert.equal((await fitTiles()).height, 36);
  await action({
    type: "move_panel",
    panel: "toolbar",
    target: { kind: "edge", edge: "left", outer: true },
  });
  assert.equal((await fitTiles()).width, 36);
  assert.equal((await fitTiles()).fits, true);
  assert.deepEqual(
    await evaluate(`(() => {
    const zen=document.querySelector('#zen-button').getBoundingClientRect();
    const tile=document.querySelector('.toolbar-controls > .tile-button').getBoundingClientRect();
    return [zen.x, tile.x, zen.width, tile.width, zen.x+zen.width/2, tile.x+tile.width/2];
  })()`),
    [6, 6, 36, 36, 24, 24],
  );
  const bottomInset = await evaluate(`(() => {
    const grip=document.querySelector('.toolbar-controls > .panel-grip');
    return grip.getBoundingClientRect().bottom-(grip.querySelector('svg').getBoundingClientRect().top+11.6);
  })()`);
  assert.ok(Math.abs(bottomInset - 8) < 0.01);
  await capture("vertical-ribbon");
  await restoreFixture();
  await action({ type: "invoke", command: "fit_canvas" });
  const group = await evaluate(
    "layerApp.app.layout(1200,900).groups.find(g=>g.panels.includes('brushes')).id",
  );
  await action({
    type: "move_panel",
    panel: "toolbar",
    target: { kind: "tab", group, index: 1 },
  });
  assert.equal(
    await evaluate(
      "document.querySelector('[data-panel=toolbar] .toolbar-controls > .panel-grip').hidden",
    ),
    true,
  );
  assert.equal(
    await evaluate(
      "!!document.querySelector('[data-panel=toolbar] .dock-tabs > .panel-grip')",
    ),
    true,
  );
  await capture("tabbed-tools");
  // Restore the serialized workspace into a separate real Wasm UI session,
  // not just back into the session that created its IDs and topology.
  const savedWorkspace = await evaluate("layerApp.state().workspace");
  const savedLayout = await evaluate("layerApp.app.layout(1200,900)");
  const fresh = await evaluate(`(async () => {
    const surface=document.createElement('canvas');surface.width=1200;surface.height=900;
    const session=layerApp.app.constructor.create(surface);
    try {
      session.dispatch({type:'restore_workspace',workspace:${JSON.stringify(savedWorkspace)}});
      return {workspace:session.state().workspace,layout:session.layout(1200,900)};
    } finally {session.free();}
  })()`);
  assert.deepEqual(fresh.workspace, savedWorkspace);
  // The work area also reflects this host's measured window bar, which is
  // runtime presentation rather than serialized workspace state.
  const topology = ({ work_area, ...layout }) => layout;
  assert.deepEqual(topology(fresh.layout), topology(savedLayout));
  await restoreFixture();
  await action({ type: "restore_workspace", workspace: savedWorkspace });
  assert.deepEqual(
    await evaluate("layerApp.app.layout(1200,900)"),
    savedLayout,
  );
  assert.equal(
    await evaluate(
      "!!document.querySelector('[data-panel=toolbar] .dock-tabs > .panel-grip')",
    ),
    true,
  );
  assert.equal(
    await evaluate(`(() => {
    const before=JSON.stringify(layerApp.state().workspace);
    try {layerApp.app.dispatch({type:'restore_workspace',workspace:{...layerApp.state().workspace,version:999}});return false;}
    catch {return before===JSON.stringify(layerApp.state().workspace);}
  })()`),
    true,
  );
  await restoreFixture();
  await action({ type: "restore_settings", settings });
  await action({ type: "invoke", command: "fit_canvas" });
  assert.equal(
    await evaluate("document.querySelector('#status').textContent"),
    "",
  );
  console.log(
    "PASS: GTK/web geometry ≤1px incl. Tool Set groups/brush rows, dark/light glass/shadow pixels ≤3/255, settings captures, nonselectable chrome/no focus halos, copyable title, menu/drawer dismissal without ink, live controls, immediate settings persistence, GPU pen cursor, occupied-edge Zen 80/80, wheel modifiers, ribbon wrapping/tabbed grips, fresh Wasm workspace restore",
  );
}
