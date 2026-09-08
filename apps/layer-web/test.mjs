// Real Chrome + Wasm + WebGPU smoke/conformance test. No browser framework.
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import assert from "node:assert/strict";
import { checkParity } from "./parity.mjs";
import { checkPreferences } from "./preferences.test.mjs";
import { checkPwa, servePackage } from "./pwa.test.mjs";
import { checkGpuStartup } from "./gpu.test.mjs";

const packageHost = process.argv.includes("--package") ? await servePackage() : null;

const profile = await mkdtemp(join(tmpdir(), "layer-chrome-"));
const chrome = spawn(
  process.env.CHROME || "google-chrome",
  [
    "--ozone-platform=wayland",
    "--remote-debugging-pipe",
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    // GTK reference PNGs are sRGB; do not bake the monitor's gamma into captures.
    "--force-color-profile=srgb",
    "--enable-gpu",
    "--enable-unsafe-webgpu",
    "--use-angle=vulkan",
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
    )
      errors.push([event.params.entry.text, event.params.entry.url].filter(Boolean).join(" "));
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
    `(async () => { const image = new Image(); image.src = ${JSON.stringify("data:image/png;base64,")} + ${JSON.stringify(shot.data)}; await image.decode(); const canvas = document.createElement('canvas'); canvas.width=image.width; canvas.height=image.height; const ctx=canvas.getContext('2d'); ctx.drawImage(image,0,0); const rgba=ctx.getImageData(0,0,canvas.width,canvas.height).data; let white=0; for(let i=0;i<rgba.length;i+=4) if(rgba[i]>245 && rgba[i+1]>245 && rgba[i+2]>245) white++; return {white,total:rgba.length/4}; })()`,
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
  if (process.argv.includes("--gpu-startup")) {
    assert.ok(packageHost, "Use --package --gpu-startup to test the built distribution");
    await checkGpuStartup({ call, evaluate, settle, canvasPixels, url: packageHost.url });
    assert.deepEqual(errors, []);
  } else if (packageHost && !process.argv.includes("--preferences") && !process.argv.includes("--parity")) {
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
    await click('[data-command="add_layer"]');
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
      await evaluate("layerApp.state().settings_draft == null"),
      true,
    );
    await click('[data-command="settings"]');
    await evaluate(
      `(() => { const input=document.querySelector('#setting-pressure'); input.value='1.5'; input.dispatchEvent(new Event('input')); })()`,
    );
    assert.equal(
      await evaluate("layerApp.state().settings_draft.pressure_gamma"),
      1.5,
    );
    await click("#apply-settings");
    assert.equal(
      await evaluate("layerApp.state().settings.pressure_gamma"),
      1.5,
    );
    assert.equal(
      await evaluate("layerApp.state().settings_draft == null"),
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
    // Set up adjacent docks; native drag/drop and tab insertion tested below.
    await evaluate(
      `layerApp.dispatch({type:"move_panel",panel:"sizes",target:{kind:"edge",edge:"right",outer:false}})`,
    );
    await settle();
    assert.equal(
      await evaluate(
        'layerApp.state().workspace.layout.bands.filter(b => b.edge === "right").length',
      ),
      2,
    );
    assert.ok(
      await evaluate(
        `document.querySelector('[data-panel="sizes"]').getBoundingClientRect().right <= document.querySelector('[data-panel="layers"]').getBoundingClientRect().left`,
      ),
    );
    await evaluate(
      `layerApp.dispatch({type:"move_panel",panel:"brushes",target:{kind:"tab",group:8}})`,
    );
    await settle();
    assert.equal(
      await evaluate(
        'document.querySelector("[data-panel=brushes] .dock-tabs").querySelectorAll(".dock-tab").length',
      ),
      2,
    );
    await evaluate(
      `Array.from(document.querySelectorAll('[data-panel="brushes"] .dock-tab')).find(b => b.textContent === 'Layers').click()`,
    );
    await settle();
    assert.equal(
      await evaluate(
        'document.querySelector("[data-panel=layers] .layer-row") !== null',
      ),
      true,
    );
    const slot = await evaluate(
      `(() => {const b=document.querySelector('[data-panel="layers"] .dock-tab').getBoundingClientRect();return{x:b.x+5,y:b.y+5};})()`,
    );
    // A real browser drop on the header reorders tabs, not a split above them.
    for (const type of ["dragEnter", "dragOver", "drop"])
      await call("Input.dispatchDragEvent", {
        type,
        ...slot,
        data: {
          items: [
            {
              mimeType: "text/layer-dock",
              data: JSON.stringify({ kind: "panel", panel: "brushes" }),
            },
          ],
          dragOperationsMask: 16,
        },
      });
    await settle();
    assert.deepEqual(
      await evaluate(
        "layerApp.app.layout(1440,1000).groups.find(g=>g.id===8).panels",
      ),
      ["brushes", "layers"],
    );
    // Tools as a tab has no inner grip; its tab-bar grip moves the entire group.
    await evaluate(
      `layerApp.dispatch({type:"move_panel",panel:"toolbar",target:{kind:"tab",group:8}})`,
    );
    await settle();
    const groupBefore = await evaluate(
      "layerApp.app.layout(1440,1000).groups.find(g=>g.id===8)",
    );
    assert.ok(
      await evaluate(
        "document.querySelector('[data-panel=toolbar] .toolbar-controls > .panel-grip').hidden",
      ),
    );
    assert.ok(
      await evaluate(
        `(() => {const s=document.querySelector('[data-panel=toolbar] .toolbar-controls'),b=s.getBoundingClientRect(),t=s.querySelector('.tile-button').getBoundingClientRect();return t.x>=b.x+4 && t.y===b.y+4;})()`,
      ),
      "tabbed Tools retains its content inset",
    );
    assert.deepEqual(
      await evaluate(
        `(() => { window.groupGrip=document.querySelector('[data-panel=toolbar] .dock-tabs > .panel-grip'); const data=new DataTransfer(); groupGrip.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:data})); return JSON.parse(data.getData('text/layer-dock')); })()`,
      ),
      { kind: "group", group: 8 },
    );
    const groupDrop = {
      x: 720,
      y: 995,
      data: {
        items: [
          {
            mimeType: "text/layer-dock",
            data: JSON.stringify({ kind: "group", group: 8 }),
          },
        ],
        dragOperationsMask: 16,
      },
    };
    for (const type of ["dragEnter", "dragOver"])
      await call("Input.dispatchDragEvent", { type, ...groupDrop });
    assert.equal(
      await evaluate("document.querySelector('.drop-indicator').hidden"),
      false,
    );
    await call("Input.dispatchDragEvent", { type: "drop", ...groupDrop });
    await evaluate(
      "groupGrip.dispatchEvent(new DragEvent('dragend',{bubbles:true}))",
    );
    await settle();
    const groupAfter = await evaluate(
      "layerApp.app.layout(1440,1000).groups.find(g=>g.id===8)",
    );
    assert.deepEqual(groupAfter.panels, groupBefore.panels);
    assert.equal(groupAfter.active, groupBefore.active);
    assert.ok(
      await evaluate(
        "layerApp.state().workspace.layout.bands.some(b=>b.edge==='bottom'&&b.root.id===8)",
      ),
    );
    await evaluate(`layerApp.dispatch({type:"invoke",command:"reset_layout"})`);
    await settle();
    // Actual captured pointer drag goes through the one shared resize action.
    assert.equal(
      await evaluate("document.querySelectorAll('.dock-menu').length"),
      0,
    );
    assert.equal(
      await evaluate(
        "document.querySelector('#header-start > button').dataset.command",
      ),
      "zen_mode",
    );
    assert.equal(
      await evaluate(
        "document.querySelector('#header-start > button').textContent.trim()",
      ),
      "",
    );
    assert.ok(
      await evaluate(
        `(() => {const t=document.querySelector('[data-panel="toolbar"]').getBoundingClientRect(),s=document.querySelector('#canvas-status').getBoundingClientRect();return t.x===s.x && t.width===s.width;})()`,
      ),
    );
    assert.ok(
      await evaluate(
        `Array.from(document.querySelectorAll('.toolbar-controls > .tile-button')).every(n=>{const b=n.getBoundingClientRect();return b.width===36&&b.height===36;})`,
      ),
    );
    const initialDockExtent = await evaluate(
      "layerApp.state().workspace.layout.bands.find(b=>b.id===3).extent",
    );
    const divider = await evaluate(
      `(() => {const n=Array.from(document.querySelectorAll('.divider')).find(n=>n.divider.id===3);const r=n.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`,
    );
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      ...divider,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    for (let i = 1; i <= 8; i++)
      await call("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: divider.x + i * 8,
        y: divider.y,
        button: "left",
        buttons: 1,
      });
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: divider.x + 64,
      y: divider.y,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await settle();
    assert.ok(
      Math.abs(
        (await evaluate(
          "layerApp.state().workspace.layout.bands.find(b=>b.id===3).extent",
        )) -
          (initialDockExtent + 64),
      ) < 1,
      "drag should resize the left dock by exactly 64 logical pixels",
    );
    const drop = await evaluate(
      `(() => {const r=document.querySelector('[data-panel="layers"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`,
    );
    for (const type of ["dragEnter", "dragOver", "drop"])
      await call("Input.dispatchDragEvent", {
        type,
        ...drop,
        data: {
          items: [
            {
              mimeType: "text/layer-dock",
              data: JSON.stringify({ kind: "panel", panel: "sizes" }),
            },
          ],
          dragOperationsMask: 16,
        },
      });
    await settle();
    assert.ok(
      await evaluate(
        'layerApp.app.layout(1440,1000).groups.some(g=>g.panels.includes("layers")&&g.panels.includes("sizes"))',
      ),
    );
    await evaluate(`layerApp.dispatch({type:"invoke",command:"reset_layout"})`);
    await settle();
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
      [600, 125, true],
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
    await evaluate("layerApp.dispatch({type:'cancel_settings'})");
    await click('[data-command="zen_mode"]');
    await click('[data-command="toggle_panels"]');
    assert.deepEqual(
      await evaluate("layerApp.state().camera.translation"),
      geometry.view.translation,
    );
    await click('[data-command="toggle_panels"]');
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
          "getComputedStyle(document.querySelector('.size-controls .spin')).backgroundColor",
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
      assert.deepEqual(spacing.gaps, [2, 2, 2, 2, 2]);
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
      const strip=document.querySelector('.toolbar-controls'),panel=strip.parentElement,box=strip.getBoundingClientRect(),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),tabGrip=document.querySelector('.dock-tabs>.panel-grip').getBoundingClientRect(),zen=document.querySelector('#header-start>button').getBoundingClientRect();
      return {gripSize:[grip.width,grip.height],tabGripSize:[tabGrip.width,tabGrip.height],overflow:[panel.scrollWidth-panel.clientWidth,panel.scrollHeight-panel.clientHeight],top:box.top,above:zen.top,below:box.top-zen.bottom};
    })()`);
      assert.deepEqual(chromeGeometry.gripSize, chromeGeometry.tabGripSize);
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
    // Standalone ribbons grow across their axis when their length runs out.
    const ribbonGeometry = () =>
      evaluate(`(() => {
    const strip=document.querySelector('.toolbar-controls'),b=strip.getBoundingClientRect(),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),horizontal=strip.dataset.axis==='horizontal';
    return {width:b.width,height:b.height,fits:[...strip.querySelectorAll('.tile-button')].every(node=>{const t=node.getBoundingClientRect();return t.x>=b.x&&t.y>=b.y&&t.right<=b.right&&t.bottom<=b.bottom&&(horizontal?t.right+2<=grip.x:t.bottom+2<=grip.y);})};
  })()`);
    for (const vertical of [false, true]) {
      if (vertical)
        await evaluate(`
      layerApp.dispatch({type:'move_panel',panel:'toolbar',target:{kind:'edge',edge:'left',outer:true}});
      layerApp.dispatch({type:'move_panel',panel:'sizes',target:{kind:'edge',edge:'top',outer:true}});
    `);
      await call("Emulation.setDeviceMetricsOverride", {
        width: vertical ? 1200 : 680,
        height: vertical ? 480 : 900,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await settle();
      const wrapped = await ribbonGeometry();
      assert.equal(vertical ? wrapped.width : wrapped.height, 74);
      assert.ok(wrapped.fits);
      await call("Emulation.setDeviceMetricsOverride", {
        width: 1200,
        height: 900,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await settle();
      const unwrapped = await ribbonGeometry();
      assert.equal(vertical ? unwrapped.width : unwrapped.height, 36);
    }
    await evaluate("layerApp.dispatch({type:'invoke',command:'reset_layout'})");
    await settle();
    // Real side drop: preserve each panel's width, taking space from the canvas.
    const side = await evaluate(
      `(() => {const t=document.querySelector('[data-panel=layers]').getBoundingClientRect(),s=document.querySelector('[data-panel=sizes]').getBoundingClientRect();return{x:t.right-5,y:t.y+t.height/2,target:t.width,source:s.width};})()`,
    );
    for (const type of ["dragEnter", "dragOver", "drop"])
      await call("Input.dispatchDragEvent", {
        type,
        x: side.x,
        y: side.y,
        data: {
          items: [
            {
              mimeType: "text/layer-dock",
              data: JSON.stringify({ kind: "panel", panel: "sizes" }),
            },
          ],
          dragOperationsMask: 16,
        },
      });
    await settle();
    for (const [panel, expected] of [
      ["layers", side.target],
      ["sizes", side.source],
    ])
      assert.ok(
        Math.abs(
          (await evaluate(
            `document.querySelector('[data-panel=${panel}]').getBoundingClientRect().width`,
          )) - expected,
        ) < 1,
      );
    const sideShot = await call("Page.captureScreenshot", {
      format: "png",
      clip: reviewClip,
    });
    await writeFile(
      "artifacts/ui/web-side-dock.png",
      Buffer.from(sideShot.data, "base64"),
    );
    await click('[data-command="reset_layout"]');
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'dark'})");
    for (const panel of ["brushes", "sizes", "toolbar"])
      await evaluate(
        `layerApp.dispatch({type:'move_panel',panel:${JSON.stringify(panel)},target:{kind:'tab',group:8}})`,
      );
    await evaluate(
      "layerApp.dispatch({type:'select_panel_tab',group:8,panel:'brushes'})",
    );
    await settle();
    const append = await evaluate(
      `(() => {const p=document.querySelector('[data-group="8"]'),b=p.getBoundingClientRect(),g=p.querySelector('.panel-grip').getBoundingClientRect(),list=p.querySelector('.tab-list');return{x:b.x+b.width/2,y:b.y+b.height/2,top:b.y,grip:g.x,overflow:list.scrollWidth>list.clientWidth};})()`,
    );
    assert.ok(append.overflow);
    const appendData = {
      items: [
        {
          mimeType: "text/layer-dock",
          data: JSON.stringify({ kind: "panel", panel: "layers" }),
        },
      ],
      dragOperationsMask: 16,
    };
    await evaluate(
      `(() => {window.appendTab=[...document.querySelectorAll('[data-group="8"] .dock-tab')].find(t=>t.textContent==='Layers');const data=new DataTransfer();appendTab.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:data}));})()`,
    );
    for (const type of ["dragEnter", "dragOver"])
      await call("Input.dispatchDragEvent", {
        type,
        x: append.x,
        y: append.y,
        data: appendData,
      });
    const marker = await evaluate(
      `(() => {const n=document.querySelector('.drop-indicator'),b=n.getBoundingClientRect();return {hidden:n.hidden,width:b.width,y:b.y,right:b.right,kind:n.dataset.kind};})()`,
    );
    assert.equal(marker.hidden, false);
    assert.equal(marker.kind, "tab");
    assert.equal(marker.width, 3);
    assert.equal(marker.y, append.top);
    assert.ok(Math.abs(marker.right - append.grip) < 1);
    const tabShot = await call("Page.captureScreenshot", {
      format: "png",
      clip: reviewClip,
    });
    await writeFile(
      "artifacts/ui/web-tab-insertion.png",
      Buffer.from(tabShot.data, "base64"),
    );
    await call("Input.dispatchDragEvent", {
      type: "drop",
      x: append.x,
      y: append.y,
      data: appendData,
    });
    await evaluate(
      "appendTab.dispatchEvent(new DragEvent('dragend',{bubbles:true}))",
    );
    await settle();
    assert.equal(
      await evaluate(
        "layerApp.app.layout(1200,900).groups.find(g=>g.id===8).panels.at(-1)",
      ),
      "layers",
    );
    await click('[data-command="reset_layout"]');
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
      clip: reviewClip,
    });
    await writeFile(
      "artifacts/ui/web-zoom.png",
      Buffer.from(zoomShot.data, "base64"),
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS: hardware Wasm/WebGPU ink, pen pressure, controls/layers/undo, settings apply/cancel, dock moves/tabs/drag/resize, two-touch camera, Zen fade/reveal without viewport change, dark/light/Zen screenshots",
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
