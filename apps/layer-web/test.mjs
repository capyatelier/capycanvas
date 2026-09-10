// Real Chrome + Wasm + WebGPU smoke/conformance test. No browser framework.
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import { checkParity } from "./parity.mjs";
import { checkLayers } from "./layers.test.mjs";
import { checkAdjustments } from "./effects.test.mjs";
import { checkPreferences, checkSettingsParity } from "./preferences.test.mjs";
import { checkPwa, servePackage } from "./pwa.test.mjs";
import { checkGpuStartup } from "./gpu.test.mjs";
import { checkCustomization, checkWorkspace, checkTabStyles, checkToolbarManager } from "./customization.test.mjs";

const packageHost = process.argv.includes("--package") ? await servePackage() : null;

const profile = await mkdtemp(join(tmpdir(), "layer-chrome-"));
const chrome = spawn(
  process.env.CHROME || "google-chrome",
  [
    ...(process.argv.includes("--headless")
      ? ["--headless=new", "--ozone-platform=headless"]
      : ["--ozone-platform=wayland"]),
    "--remote-debugging-pipe",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    // GTK reference PNGs are sRGB; do not bake the monitor's gamma into captures.
    "--force-color-profile=srgb",
    "--enable-gpu",
    "--enable-unsafe-webgpu",
    "--use-angle=vulkan",
    // Offscreen Vulkan avoids Wayland's Vulkan swapchain incompatibility while
    // retaining the hardware WebGPU renderer and capturable window contents.
    "--enable-features=Vulkan",
    "--disable-vulkan-surface",
    "--window-size=1440,1000",
    "about:blank",
  ],
  { stdio: ["ignore", "ignore", "pipe", "pipe", "pipe"] },
);
let sequence = 0,
  buffer = "",
  session;
const requests = new Map(),
  errors = [];
chrome.stderr.on("data", (data) => {
  if (process.env.LAYER_TEST_VERBOSE) process.stderr.write(data);
});
chrome.stdio[4].on("data", (data) => {
  buffer += data.toString();
  for (;;) {
    const end = buffer.indexOf("\0");
    if (end < 0) break;
    const event = JSON.parse(buffer.slice(0, end));
    buffer = buffer.slice(end + 1);
    if (event.id) {
      const waiter = requests.get(event.id);
      if (!waiter) continue;
      requests.delete(event.id);
      clearTimeout(waiter.timer);
      if (event.error) waiter.reject(new Error(`${waiter.method}: ${JSON.stringify(event.error)}`));
      else waiter.resolve(event.result);
    } else if (event.method === "Runtime.exceptionThrown")
      errors.push(
        event.params.exceptionDetails.exception?.description ||
          event.params.exceptionDetails.text,
      );
    else if (
      event.method === "Runtime.consoleAPICalled" &&
      event.params.type === "error"
    )
      errors.push(
        event.params.args.map((a) => a.value || a.description).join(" "),
      );
    else if (
      event.method === "Log.entryAdded" &&
      ["error", "warning"].includes(event.params.entry.level) &&
      !event.params.entry.text.includes("favicon")
    ) {
      if (process.env.LAYER_TEST_VERBOSE) process.stderr.write(`${JSON.stringify(event.params.entry)}\n`);
      errors.push([event.params.entry.text, event.params.entry.url].filter(Boolean).join(" "));
    }
  }
});
function call(method, params = {}, sessionId = session) {
  const id = ++sequence;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      requests.delete(id);
      reject(new Error(`CDP timeout: ${method}`));
    }, 30000);
    requests.set(id, { resolve, reject, timer, method });
    chrome.stdio[3].write(
      JSON.stringify({
        id,
        method,
        params,
        ...(sessionId ? { sessionId } : {}),
      }) + "\0",
    );
  });
}
async function evaluate(expression) {
  const result = await call("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });
  if (result.exceptionDetails)
    throw new Error(
      result.exceptionDetails.exception?.description ||
        result.exceptionDetails.text,
    );
  return result.result.value;
}
const settle = () =>
  evaluate(
    "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
  );
async function canvasPixels() {
  const clip = await evaluate(
    "(() => { const r = layerApp.canvas.getBoundingClientRect(); return {x:r.x,y:r.y,width:r.width,height:r.height,scale:1}; })()",
  );
  const shot = await call("Page.captureScreenshot", { format: "png", clip });
  // Inspect the presented framebuffer. Reading a WebGPU canvas backbuffer in a
  // later task can return its newly cleared buffer instead of the visible frame.
  return evaluate(
    `(async () => { const image = new Image(); image.src = ${JSON.stringify("data:image/png;base64,")} + ${JSON.stringify(shot.data)}; await image.decode(); const canvas = document.createElement('canvas'); canvas.width=image.width; canvas.height=image.height; const ctx=canvas.getContext('2d',{willReadFrequently:true}); ctx.drawImage(image,0,0); const rgba=ctx.getImageData(0,0,canvas.width,canvas.height).data; let white=0; for(let i=0;i<rgba.length;i+=4) if(rgba[i]>245 && rgba[i+1]>245 && rgba[i+2]>245) white++; return {white,total:rgba.length/4}; })()`,
  );
}
async function click(selector) {
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);
  await settle();
}
try {
  const target = await call(
    "Target.createTarget",
    { url: "about:blank" },
    null,
  );
  session = (
    await call(
      "Target.attachToTarget",
      { targetId: target.targetId, flatten: true },
      null,
    )
  ).sessionId;
  await call("Runtime.enable");
  await call("Page.enable");
  await call("Log.enable");
  // Keep the test tab focused even when the surrounding desktop is in use.
  await call("Emulation.setFocusEmulationEnabled", { enabled: true });
  if (process.argv.includes("--parity") || process.argv.includes("--preferences"))
    await call("Input.setIgnoreInputEvents", { ignore: true });
  await call("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 1000,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await call("Page.navigate", {
    url: packageHost?.url || process.env.LAYER_WEB_URL || "http://127.0.0.1:4173",
  });
  await evaluate(
    `new Promise((resolve, reject) => { const started = performance.now(); function check() { if (window.layerApp && document.body.dataset.gpu === 'ready') resolve(true); else if (performance.now() - started > 25000) reject(new Error(document.querySelector('#gpu-notice')?.textContent || document.querySelector('#status')?.textContent)); else setTimeout(check, 100); } check(); })`,
  );
  await settle();
  if (process.argv.includes("--adjustments")) {
    await checkAdjustments({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--layers")) {
    await checkLayers({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--toolbar-manager")) {
    await checkToolbarManager({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--tab-styles")) {
    await checkTabStyles({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--settings-audit")) {
    await checkSettingsParity({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--workspace")) {
    await checkWorkspace({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--customization")) {
    await checkCustomization({ call, evaluate, settle, canvasPixels });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--gpu-startup")) {
    assert.ok(packageHost, "Use --package --gpu-startup to test the built distribution");
    await checkGpuStartup({ call, evaluate, settle, canvasPixels, url: packageHost.url });
    assert.deepEqual(errors, []);
  } else if (packageHost && !process.argv.includes("--preferences") && !process.argv.includes("--parity") && !process.argv.includes("--smoke")) {
    await checkPwa({ call, evaluate, settle, canvasPixels, host: packageHost });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--preferences")) {
    await checkPreferences({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else if (process.argv.includes("--parity")) {
    await checkParity({ call, evaluate, settle });
    assert.deepEqual(errors, []);
  } else {
    assert.equal(
      await evaluate("layerApp.state().settings.theme ?? null"),
      null,
    );
    for (const value of ["light", "dark"]) {
      await call("Emulation.setEmulatedMedia", {
        features: [{ name: "prefers-color-scheme", value }],
      });
      await settle();
      assert.equal(await evaluate("layerApp.state().theme"), value);
      assert.equal(await evaluate("document.body.dataset.theme"), value);
    }
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'})");
    await call("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "light" }],
    });
    await settle();
    await call("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "dark" }],
    });
    await settle();
    assert.equal(await evaluate("layerApp.state().theme"), "light");
    await evaluate("layerApp.dispatch({type:'set_theme',theme:null})");
    await settle();
    assert.equal(await evaluate("layerApp.state().theme"), "dark");
    const pixels = await canvasPixels();
    assert.ok(
      pixels.white > pixels.total * 0.1,
      `GPU canvas must visibly contain paper: ${JSON.stringify(pixels)}`,
    );
    assert.equal(await evaluate("layerApp.state().tabs.length"), 1);
    assert.equal(await evaluate("layerApp.state().layers.length"), 2);
    await click('[data-brush="4"]');
    assert.equal(await evaluate("layerApp.state().brush.preset"), 4);
    await click('[data-size="96"]');
    // Temporary Space shortcut must route to camera gestures, never pen records.
    const beforePan = await evaluate("layerApp.state().camera.translation");
    await evaluate(
      "window.penCalls=0; window.panEvents=[]; for(const name of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture']) canvas.addEventListener(name,e=>panEvents.push({name,x:e.clientX,y:e.clientY,id:e.pointerId}),{capture:true}); window.originalPen=layerApp.app.pen; layerApp.app.pen=function(...args){window.penCalls++;return window.originalPen.apply(this,args);}",
    );
    await call("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: " ",
      code: "Space",
      windowsVirtualKeyCode: 32,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: 600,
      y: 450,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 650,
      y: 480,
      button: "left",
      buttons: 1,
    });
    await call("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: " ",
      code: "Space",
      windowsVirtualKeyCode: 32,
    });
    // Releasing Space before the mouse still finishes the same pan, not a stroke.
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 670,
      y: 490,
      button: "left",
      buttons: 1,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 670,
      y: 490,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await settle();
    assert.equal(await evaluate("window.penCalls"), 0);
    assert.deepEqual(
      await evaluate(
        "layerApp.state().camera.translation.map((v,i)=>Math.round(v-" +
          JSON.stringify(beforePan) +
          "[i]))",
      ),
      [70, 40],
      JSON.stringify(await evaluate("window.panEvents")),
    );
    await evaluate("layerApp.app.pen=window.originalPen");
    for (const [modifiers, axis] of [
      [0, 1],
      [8, 0],
    ]) {
      const before = await evaluate(
        "({t:layerApp.state().camera.translation,z:layerApp.state().camera.zoom,r:layerApp.state().camera.rotation})",
      );
      await evaluate(
        "window.lastWheel=null; document.querySelector('#canvas').addEventListener('wheel',e=>window.lastWheel={dx:e.deltaX,dy:e.deltaY,dpi:devicePixelRatio,mode:e.deltaMode},{once:true})",
      );
      await call("Input.dispatchMouseEvent", {
        type: "mouseWheel",
        x: 650,
        y: 450,
        deltaX: 0,
        deltaY: 40,
        modifiers,
      });
      await settle();
      const after = await evaluate(
        "({t:layerApp.state().camera.translation,z:layerApp.state().camera.zoom,r:layerApp.state().camera.rotation})",
      );
      const wheel = await evaluate("window.lastWheel");
      assert.equal(wheel.mode, 0);
      assert.ok(
        Math.abs(
          after.t[axis] -
            before.t[axis] +
            (wheel.dy + (axis === 0 ? wheel.dx : 0)) * wheel.dpi,
        ) < 0.1,
        JSON.stringify({ modifiers, axis, before, after, wheel }),
      );
      assert.equal(after.t[1 - axis], before.t[1 - axis]);
      assert.equal(after.z, before.z);
      assert.equal(after.r, before.r);
    }
    const beforeZoom = await evaluate("layerApp.state().camera.zoom");
    await call("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 650,
      y: 450,
      deltaX: 0,
      deltaY: -40,
      modifiers: 2,
    });
    await settle();
    assert.ok((await evaluate("layerApp.state().camera.zoom")) > beforeZoom);
    await click('[data-command="fit_canvas"]');
    assert.equal(await evaluate("layerApp.state().brush.diameter"), 96);
    await click('.layer-footer [aria-label="New layer"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    await click('[data-command="undo"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 2);
    await click('[data-command="redo"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    await click('[data-command="settings"]');
    assert.equal(
      await evaluate('document.querySelector("#settings").open'),
      true,
    );
    await evaluate('document.querySelector("#settings").close()');
    await settle();
    assert.equal(
      await evaluate("!layerApp.state().settings_open"),
      true,
    );
    await click('[data-command="settings"]');
    await evaluate(
      `(() => { document.querySelector('#setting-pressure .number-value').click(); const input=document.querySelector('#setting-pressure .number-entry'); input.value='1.5'; input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})); })()`,
    );
    assert.equal(
      await evaluate("layerApp.state().settings.pressure_gamma"),
      1.5,
    );
    await click("#close-settings");
    assert.equal(
      await evaluate("layerApp.state().settings.pressure_gamma"),
      1.5,
    );
    assert.equal(
      await evaluate("!layerApp.state().settings_open"),
      true,
    );
    // Draw through Chrome mouse events, exercising DOM capture and Wasm input.
    await evaluate(
      "window.inkEvents=[]; for(const name of ['pointerdown','pointerup','pointercancel']) layerApp.canvas.addEventListener(name,e=>window.inkEvents.push({name,x:e.clientX,y:e.clientY,buttons:e.buttons}),{capture:true});",
    );
    const rect = await evaluate(
      "(() => { const r = layerApp.canvas.getBoundingClientRect(); return {x:r.x,y:r.y,w:r.width,h:r.height}; })()",
    );
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: rect.x + rect.w * 0.22,
      y: rect.y + rect.h * 0.48,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    for (let i = 1; i <= 35; i++) {
      await call("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: rect.x + rect.w * (0.22 + i * 0.015),
        y: rect.y + rect.h * (0.48 + 0.1 * Math.sin(i / 6)),
        button: "left",
        buttons: 1,
      });
    }
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: rect.x + rect.w * 0.745,
      y: rect.y + rect.h * (0.48 + 0.1 * Math.sin(35 / 6)),
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await settle();
    const painted = await canvasPixels();
    assert.ok(
      pixels.white - painted.white > 200,
      `Native mouse input must visibly paint: before=${pixels.white}, after=${painted.white}; ${JSON.stringify(await evaluate("({events:window.inkEvents,dpi:devicePixelRatio,w:layerApp.canvas.width,css:layerApp.canvas.clientWidth,camera:layerApp.state().camera.translation,zoom:layerApp.state().camera.zoom,undo:layerApp.state().commands.find(c=>c.id==='undo')})"))}`,
    );
    // Chrome-generated pen events must preserve pressure through DOM -> Wasm.
    await click('[data-brush="1"]');
    await click('[data-size="96"]');
    const pressureInk = [];
    for (const force of [0.3, 1.0]) {
      const baseline = (await canvasPixels()).white;
      for (let i = 0; i <= 24; i++) {
        await call("Input.dispatchMouseEvent", {
          type:
            i === 0
              ? "mousePressed"
              : i === 24
                ? "mouseReleased"
                : "mouseMoved",
          x: rect.x + rect.w * (0.3 + (0.4 * i) / 24),
          y: rect.y + rect.h * 0.7,
          button: "left",
          buttons: i === 24 ? 0 : 1,
          clickCount: 1,
          pointerType: "pen",
          force,
          tiltX: 15,
          tiltY: 10,
        });
      }
      await settle();
      pressureInk.push(baseline - (await canvasPixels()).white);
      await click('[data-command="undo"]');
    }
    assert.ok(
      pressureInk[0] > 50 && pressureInk[1] > pressureInk[0] * 1.5,
      `pressure must visibly change ink width: ${pressureInk}`,
    );
    await click('[data-brush="4"]');
    await click('[data-size="96"]');
    // Workspace gestures are covered by the shared browser suite below.
    const camera = await evaluate(
      "({zoom:layerApp.state().camera.zoom, rotation:layerApp.state().camera.rotation})",
    );
    const a = { id: 1, x: rect.x + rect.w * 0.4, y: rect.y + rect.h * 0.5 },
      b = { id: 2, x: rect.x + rect.w * 0.6, y: rect.y + rect.h * 0.5 };
    await call("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [a, b],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [
        { ...a, x: a.x - 30, y: a.y - 40 },
        { ...b, x: b.x + 70, y: b.y + 70 },
      ],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    await settle();
    const moved = await evaluate(
      "({zoom:layerApp.state().camera.zoom, rotation:layerApp.state().camera.rotation})",
    );
    assert.ok(
      moved.zoom > camera.zoom * 1.1 &&
        Math.abs(moved.rotation - camera.rotation) > 0.1,
      "Native two-touch input must zoom and rotate the shared camera",
    );
    await click('[data-command="fit_canvas"]');
    await mkdir("artifacts/ui", { recursive: true });
    // Match the GTK review window's logical viewport, without browser chrome.
    await call("Emulation.setDeviceMetricsOverride", {
      width: 1200,
      height: 900,
      deviceScaleFactor: 2,
      mobile: false,
    });
    await settle();
    await click('[data-command="fit_canvas"]');
    const geometryExpression =
      "JSON.parse(JSON.stringify({view:layerApp.state().camera, rect:layerApp.canvas.getBoundingClientRect().toJSON()}, (_,v)=>typeof v==='bigint'?String(v):v))";
    const geometry = await evaluate(geometryExpression);
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await click('[data-command="zen_mode"]');
    await evaluate("new Promise(r=>setTimeout(r,250))");
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      true,
    );
    assert.equal(
      await evaluate(
        "getComputedStyle(document.querySelector('#header')).opacity",
      ),
      "0",
    );
    assert.equal(
      await evaluate("getComputedStyle(layerApp.canvas).outlineStyle"),
      "none",
    );
    assert.deepEqual(await evaluate(geometryExpression), geometry);
    const reviewClip = { x: 0, y: 0, width: 1200, height: 900, scale: 0.5 };
    const zen = await call("Page.captureScreenshot", {
      format: "png",
      clip: reviewClip,
    });
    await writeFile(
      "artifacts/ui/web-zen.png",
      Buffer.from(zen.data, "base64"),
    );
    // No hover first: hidden UI must consume a reveal tap, not paint beneath it.
    // Screenshot scaling can temporarily change emulation/hover; restore the
    // normal pointer position and wait for native hit-testing before input.
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await settle();
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      true,
    );
    assert.equal(
      await evaluate("document.elementFromPoint(20,450).id"),
      "canvas",
    );
    // Exercise the host's pointer handler without a desktop drag: entering a
    // hidden toolbar's old proximity zone must not reveal it any more.
    for (const [x, y, hidden] of [
      [600, 120, true],
      [600, 80, false],
      [600, 120, false],
      [600, 165, true],
      [600, 120, true],
      [600, 820, true], // Empty bottom edge: the status HUD is not a panel.
      [600, 820, true],
      [600, 450, true],
    ]) {
      assert.equal(
        await evaluate(
          `(() => {window.dispatchEvent(new PointerEvent('pointermove',{clientX:${x},clientY:${y},pointerType:'mouse',buttons:0,bubbles:true}));return document.querySelector('#workspace').classList.contains('zen-hidden')})()`,
        ),
        hidden,
        `Zen hover at ${x},${y}`,
      );
    }
    const beforeReveal = await evaluate("String(layerApp.state().revision)");
    await call("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ id: 9, x: 20, y: 450 }],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    await settle();
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    assert.equal(
      await evaluate("String(layerApp.state().revision)"),
      beforeReveal,
    );
    await evaluate("document.querySelector('details').open=true");
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await evaluate("document.querySelector('details').open=false");
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 24,
      y: 24,
    });
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await click('[data-command="settings"]');
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await evaluate("layerApp.dispatch({type:'close_settings'})");
    await click('[data-command="zen_mode"]');
    assert.deepEqual(
      await evaluate("layerApp.state().camera.translation"),
      geometry.view.translation,
    );
    await evaluate("document.activeElement?.blur()");
    for (const theme of ["dark", "light"]) {
      await evaluate(
        `layerApp.dispatch({type:'set_theme', theme:${JSON.stringify(theme)}})`,
      );
      await settle();
      await evaluate(
        "Promise.all([...document.images].map(image=>image.decode()))",
      );
      const surfaces = await evaluate(`(() => {
      const group=document.querySelector('.dock-group:not(.toolbar)'), tab=group.querySelector('.dock-tab[aria-selected=true]');
      return {panel:getComputedStyle(group).backgroundColor, bar:getComputedStyle(group.querySelector('.dock-tabs')).backgroundColor, active:getComputedStyle(tab).backgroundColor, radius:getComputedStyle(tab).borderBottomRightRadius, foot:getComputedStyle(tab,'::after').width};
    })()`);
      assert.equal(
        surfaces.panel,
        theme === "dark" ? "rgb(65, 65, 65)" : "rgb(237, 237, 237)",
      );
      assert.equal(
        surfaces.bar,
        theme === "dark" ? "rgb(46, 46, 46)" : "rgb(222, 222, 222)",
      );
      assert.equal(surfaces.active, surfaces.panel);
      assert.equal(
        await evaluate(
          "getComputedStyle(document.querySelector('.size-controls .number-entry')).backgroundColor",
        ),
        theme === "dark" ? "rgb(51, 51, 51)" : "rgb(250, 250, 250)",
      );
      assert.equal(surfaces.radius, "0px");
      assert.equal(surfaces.foot, "6px");
      const spacing = await evaluate(
        `(() => {const strip=document.querySelector('.toolbar-controls'),box=strip.getBoundingClientRect(),tiles=[...strip.querySelectorAll('.tile-button')].map(n=>n.getBoundingClientRect()),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),tab=document.querySelector('.dock-tab').getBoundingClientRect();return{firstX:tiles[0].x-box.x,firstY:tiles[0].y-box.y,gaps:tiles.slice(1).map((b,i)=>b.x-tiles[i].right),gripRight:box.right-grip.right,gripCenter:grip.y+grip.height/2-box.y-box.height/2,tabHeight:tab.height,tileHeight:tiles[0].height};})()`,
      );
      assert.equal(spacing.firstX, 0);
      assert.equal(spacing.firstY, 0);
      assert.deepEqual(spacing.gaps, Array(7).fill(2));
      assert.equal(
        await evaluate(
          "(()=>{const [a,b]=[...document.querySelectorAll('.brush-choice')].slice(0,2).map(n=>n.getBoundingClientRect());return b.top-a.bottom;})()",
        ),
        2,
      );
      assert.equal(spacing.gripRight, 0);
      assert.equal(spacing.gripCenter, 0);
      assert.equal(spacing.tabHeight, spacing.tileHeight);
      assert.equal(spacing.tabHeight, 36);
      const chromeGeometry = await evaluate(`(() => {
      const strip=document.querySelector('.toolbar-controls'),panel=strip.parentElement,box=strip.getBoundingClientRect(),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),tabGrip=document.querySelector('.dock-tabs>.panel-grip').getBoundingClientRect(),zen=document.querySelector('#zen-button').getBoundingClientRect();
      return {gripSize:[grip.width,grip.height],tabGripSize:[tabGrip.width,tabGrip.height],overflow:[panel.scrollWidth-panel.clientWidth,panel.scrollHeight-panel.clientHeight],top:box.top,above:zen.top,below:box.top-zen.bottom};
    })()`);
      assert.deepEqual(chromeGeometry.gripSize, [20, 36]);
      assert.deepEqual(chromeGeometry.tabGripSize, [20, 24]);
      assert.deepEqual(chromeGeometry.overflow, [0, 0]);
      assert.equal(chromeGeometry.top, 48);
      assert.ok(Math.abs(chromeGeometry.above - chromeGeometry.below) <= 1);
      const shot = await call("Page.captureScreenshot", {
        format: "png",
        clip: reviewClip,
      });
      await writeFile(
        `artifacts/ui/web-${theme}.png`,
        Buffer.from(shot.data, "base64"),
      );
    }
    assert.ok(
      await evaluate(
        `(() => {const n=document.querySelector('#view-info').getBoundingClientRect(),p=document.querySelector('[data-panel=layers]').getBoundingClientRect();return Math.abs(n.bottom-p.bottom)<1;})()`,
      ),
    );
    await checkWorkspace({ call, evaluate, settle });
    await checkCustomization({ call, evaluate, settle, canvasPixels });
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'})");
    await settle();
    // Review the transparent decoration area over zoomed artwork.
    await call("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 600,
      y: 450,
      deltaX: 0,
      deltaY: -924,
      modifiers: 2,
    });
    await settle();
    const zoomShot = await call("Page.captureScreenshot", {
      format: "png",
      clip: { ...reviewClip, scale: 1 }, // The workspace suites restored 1× DPR.
    });
    await writeFile(
      "artifacts/ui/web-zoom.png",
      Buffer.from(zoomShot.data, "base64"),
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS: hardware Wasm/WebGPU ink, pen pressure, controls/layers/undo, settings persistence, dock moves/tabs/drag/resize, two-touch camera, Zen fade/reveal without viewport change, dark/light/Zen screenshots",
    );
  }
} catch (error) {
  await mkdir("artifacts/ui", { recursive: true });
  const failureShot = await call("Page.captureScreenshot", {
    format: "png",
  }).catch(() => null);
  if (failureShot)
    await writeFile(
      "artifacts/ui/web-failure.png",
      Buffer.from(failureShot.data, "base64"),
    );
  console.error(
    "WebGPU diagnostic:",
    await evaluate(
      "(async () => { try { const a = await navigator.gpu?.requestAdapter(); return { available: !!navigator.gpu, adapter: a ? {vendor:a.info.vendor, architecture:a.info.architecture, description:a.info.description, fallback:a.info.isFallbackAdapter} : null}; } catch (e) { return String(e); } })()",
    ).catch(String),
  );
  const info = await call("SystemInfo.getInfo", {}, null).catch(String);
  console.error(
    "Chrome GPU:",
    JSON.stringify({
      devices: info.gpu?.devices,
      features: info.gpu?.featureStatus,
      renderer: info.gpu?.auxAttributes.glRenderer,
    }),
  );
  console.error("Page errors:", errors);
  throw error;
} finally {
  try {
    await call("Browser.close", {}, null);
  } catch {
    chrome.kill("SIGTERM");
  }
  if (chrome.exitCode === null)
    await new Promise((resolve) => chrome.once("exit", resolve));
  await rm(profile, { recursive: true, force: true });
  await packageHost?.close();
}
