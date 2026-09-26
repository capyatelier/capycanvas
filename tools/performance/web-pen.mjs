// Real Chrome input -> DOM -> Wasm -> WebGPU. Use a dedicated test origin.
// See docs/development/web-pen-huion-2026-09-20.md for device setup and measurements.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { promisify } from "node:util";
import { connectTab } from "../cdp.mjs";

const exec = promisify(execFile);
const options = new Set(process.argv.slice(2));
for (const option of options)
  assert(["--os-input", "--reload", "--navigator", "--profile", "--trace", "--picker"].includes(option), `Unknown option: ${option}`);
const osInput = options.has("--os-input");
const picker = options.has("--picker");
const sampleWidth = Number(process.env.LAYER_PICKER_SAMPLE_SIZE || 1);
assert([1,5,15,51,101].includes(sampleWidth), "LAYER_PICKER_SAMPLE_SIZE must be a picker option");
const adb = process.env.ADB || "adb";
const penHelper = process.env.LAYER_PEN_HELPER || "/data/local/tmp/capy-web-pen.dex";
assert(/^\/data\/local\/tmp\/[a-zA-Z0-9._-]+$/.test(penHelper), "LAYER_PEN_HELPER must be a device temporary filename");
const serial = process.env.LAYER_DEVICE_SERIAL || process.env.CAPY_ANDROID_SERIAL;
if (osInput) assert(serial, "Set LAYER_DEVICE_SERIAL or CAPY_ANDROID_SERIAL for --os-input");
const endpoint = process.env.LAYER_DEVICE_CDP || "http://127.0.0.1:9258";
const url = process.env.LAYER_WEB_URL || "http://127.0.0.1:4198/";
const output = process.env.LAYER_TEST_ARTIFACTS || "artifacts/web-pen";
const label = process.env.LAYER_PEN_LABEL || "baseline";
assert(/^[a-zA-Z0-9_-]+$/.test(label), "LAYER_PEN_LABEL must be a filename label");
const duration = Number(process.env.LAYER_PEN_DURATION || 5000);
const repeats = Number(process.env.LAYER_PEN_REPEATS || 3);
const size = Number(process.env.LAYER_PEN_SIZE || 1024);
const speed = Number(process.env.LAYER_PEN_SPEED || 1);
const transparency = process.env.LAYER_PEN_TRANSPARENCY;
assert([undefined, "off", "low", "medium", "high"].includes(transparency), "LAYER_PEN_TRANSPARENCY must be off, low, medium or high");
for (const [name, value] of Object.entries({ duration, repeats, size }))
  assert(Number.isSafeInteger(value) && value > 0, `${name} must be a positive integer`);
assert(Number.isFinite(speed) && speed > 0, "speed must be positive and finite");
const profile = options.has("--profile"), trace = options.has("--trace");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const summary = values => {
  if (!values.length) return null;
  const sorted = values.toSorted((a, b) => a - b);
  const percentile = p => sorted[Math.floor((sorted.length - 1) * p)];
  return { count: sorted.length, p50: percentile(.5), p95: percentile(.95), p99: percentile(.99), max: sorted.at(-1) };
};
const inject = (x, y, rx, ry, hz, seconds, mode="stroke") => exec(adb, ["-s", serial, "shell",
  `CLASSPATH=${penHelper} app_process -Xmx64m / AndroidPenMotion ${x} ${y} ${rx} ${ry} ${mode} ${hz} ${seconds}`,
], { timeout: seconds * 1000 + 30000 });

await mkdir(output, { recursive: true });
const cdp = await connectTab(endpoint, tab => tab.url === url, { timeout: 180000 });
const { call, evaluate, errors } = cdp;
// Functions sent here run in the page and must use only their arguments/globals.
const inPage = (fn, ...args) => evaluate(`(${fn})(...${JSON.stringify(args)})`);
const waitFor = condition => evaluate(`new Promise((resolve,reject)=>{
  const deadline=performance.now()+120000;
  function check(){
    if(${condition})resolve();
    else if(performance.now()>deadline)reject(Error('Timed out: '+${JSON.stringify(condition)}));
    else setTimeout(check,50);
  }check();
})`);
let profiling = false, tracing = false;
try {
  for (const domain of ["Page", "Runtime"]) await call(`${domain}.enable`);
  // Runtime.enable replays old exceptions. Keep all errors from this run's reload.
  errors.length = 0;
  await call("Page.bringToFront");
  if (options.has("--reload")) {
    await call("Runtime.evaluate", { expression: "void 0", userGesture: true });
    await call("Page.reload", { ignoreCache: true });
    await delay(1000);
  }
  await inPage(() => {
    window.penBenchRecovery = setInterval(() => {
      [...document.querySelectorAll("dialog[open] button")].find(node => node.textContent === "Keep for Later")?.click();
    }, 100);
  });
  await waitFor("window.layerApp?.app.brush_ready() && layerApp.app.startup_progress().every(Boolean) && JSON.parse(layerApp.app.workspace_view()).ready");
  await delay(500);
  await inPage(() => {
    const app = layerApp.app.constructor.create(document.createElement("canvas"));
    try { layerApp.dispatch({ type: "restore_workspace", workspace: app.state().workspace }); }
    finally { app.free(); }
  });
  await delay(500);
  await inPage(() => layerApp.dispatch({ type: "invoke", command: "new_document" }));
  await waitFor("!!document.querySelector('.document-dialog input[type=number]')");
  await inPage(size => {
    [...document.querySelectorAll(".document-dialog input[type=number]")].slice(0, 2).forEach(node => node.value = size);
    [...document.querySelectorAll(".document-dialog button")].find(node => node.textContent === "Create").click();
  }, size);
  await waitFor(`!layerApp.state().document_file.busy && layerApp.state().tabs.find(tab=>tab.active)?.width===${size} && layerApp.app.brush_ready()`);
  await inPage((panel, transparency) => {
    layerApp.dispatch({ type: "select_brush", id: 1 });
    layerApp.dispatch({ type: "set_brush_size", value: 18 });
    layerApp.dispatch({ type: "restore_settings", settings: {
      ...layerApp.state().settings, feedback: true, platform_prediction: false, prediction_ms: 16,
      ...(transparency ? { transparency } : {}),
    } });
    layerApp.dispatch({ type: "invoke", command: "fit_canvas" });
    const group = layerApp.app.layout(innerWidth, innerHeight).groups.find(group => group.panels.includes("stats"));
    // Selecting the already-active tab would open its configuration popup.
    if (group.active !== panel) layerApp.dispatch({ type: "select_panel_tab", group: group.id, panel });
  }, options.has("--navigator") ? "navigator" : "stats", transparency);
  await waitFor("layerApp.app.brush_ready() && layerApp.app.startup_progress().every(Boolean)");
  await delay(1500);
  const info = await inPage(() => JSON.parse(JSON.stringify({
    agent: navigator.userAgent, viewport: [innerWidth, innerHeight, devicePixelRatio],
    camera: layerApp.app.camera(), brush: layerApp.state().brush, settings: layerApp.state().settings,
    startup: layerApp.startupTimes,
    capabilities: { raw: "onpointerrawupdate" in window, prediction: typeof PointerEvent.prototype.getPredictedEvents },
  }, (_, value) => typeof value === "bigint" ? Number(value) : value)));
  if(picker) {
    await inPage(()=>layerApp.dispatch({type:'move_panel',panel:'color',target:{kind:'float',position:[30,80]}}));
    await delay(500);
  }
  const origin = await inPage(picker => {
    const camera = layerApp.app.camera(), rect = layerApp.canvas.getBoundingClientRect();
    const area = camera.work_area, scale = rect.width / camera.viewport[0];
    // Stay to the right of the floating wheel for both visibility states.
    // Crossing a panel intentionally hides the loupe and invalidates a pacing comparison.
    return { x: rect.x + (area[0] + area[2] * (picker ? .77 : .5)) * scale,
      y: rect.y + (area[1] + area[3] / 2) * scale, radius: Math.min(area[2] * (picker ? .16 : .32), area[3] * .32) * scale };
  }, picker);
  await inPage(() => {
    const bench = window.penBench = { original: {}, frames: [], calls: {}, events: [], raf: [],
      running: true, lastInput: null, signal: new AbortController() };
    for (const name of ["frame", "pen", "input", "cursor_input", "workspace_update", "state_update", "color_field_pixels"]) {
      const original = layerApp.app[name];
      bench.original[name] = original;
      bench.calls[name] = [];
      layerApp.app[name] = function(...args) {
        const start = performance.now();
        if (name === "pen") {
          for (let i = 0; i < args[0].length; i += 11)
            if (!(args[0][i + 9] & 1)) bench.lastInput = args[0][i + 8];
        }
        if (name === "cursor_input" && args[0].length) bench.lastInput=args[0][6];
        try { return original.apply(this, args); }
        finally {
          const end = performance.now();
          bench.calls[name].push(end - start);
          if (name === "frame") bench.frames.push({ clock: args[0], start, end, input: bench.lastInput });
        }
      };
    }
    for (const type of ["pointerdown", "pointerrawupdate", "pointermove", "pointerup"])
      layerApp.canvas.addEventListener(type, event => bench.events.push({
        type, time: event.timeStamp, arrival: performance.now(), coalesced: event.getCoalescedEvents?.().length ?? 0,
      }), { capture: true, signal: bench.signal.signal });
    const tick = time => {
      if (!bench.running) return;
      bench.raf.push(time);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  let screenOffset;
  if (osInput) {
    await inPage(() => {
      window.penCalibration = null;
      layerApp.canvas.addEventListener("pointerdown", event => {
        window.penCalibration = { x: event.clientX, y: event.clientY };
      }, { once: true, signal: penBench.signal.signal });
    });
    // Huion landscape calibration point; see the device setup documentation.
    await inject(1200, 800, 1, 1, .5, 1);
    const point = await evaluate("window.penCalibration");
    assert(point, "Calibration contact must reach the canvas");
    screenOffset = { x: 1201 - point.x * info.viewport[2], y: 800 - point.y * info.viewport[2] };
    info.screenOffset = screenOffset;
    info.input = picker ? "Android shell stylus hover replay, 200 Hz" : "Android shell stylus replay, 200 Hz, pressure 1";
    await delay(500);
  } else info.input = "CDP pen replay, nominal 200 Hz, varying pressure";
  if(picker) {
    await inPage(()=>layerApp.dispatch({type:'set_brush_size',value:220}));
    for(let i=0;i<4;i++) {
      await inPage(rgba=>layerApp.dispatch({type:'set_color',rgba}),[[1,0,0,1],[0,1,0,1],[0,0,1,1],[1,0,1,1]][i]);
      const point={x:origin.x+origin.radius*Math.cos(i*Math.PI/2),y:origin.y+origin.radius*.65*Math.sin(i*Math.PI/2)};
      for(const type of ['mousePressed','mouseMoved','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...point,x:point.x+(type==='mouseMoved'?5:0),button:'left',buttons:type==='mouseReleased'?0:1,pointerType:'pen',force:type==='mouseReleased'?0:1});
      await delay(150);
    }
    await inPage(()=>{
      layerApp.dispatch({type:'color_picker',action:{kind:'style',style:'glass'}});
      layerApp.dispatch({type:'color_picker',action:{kind:'source',layer:false}});
      layerApp.dispatch({type:'invoke',command:'eyedropper'});
    });
    await inPage(width=>layerApp.dispatch({type:'set_color_sample_size',width}),sampleWidth);
  }
  const stroke = async (run, milliseconds) => {
    if (osInput) {
      const scale = info.viewport[2];
      return inject(origin.x * scale + screenOffset.x, origin.y * scale + screenOffset.y,
        origin.radius * scale, origin.radius * scale * .65, .5 * speed, Math.max(1, Math.round(milliseconds / 1000)),picker?"hover":"stroke");
    }
    const send = (type, time) => call("Input.dispatchMouseEvent", {
      type, x: origin.x + origin.radius * Math.sin(time * 3.2),
      y: origin.y + origin.radius * .65 * Math.sin(time * 4.7 + run * .31),
      button: picker?"none":"left", buttons: picker||type === "mouseReleased" ? 0 : 1, pointerType: "pen",
      force: picker||type === "mouseReleased" ? 0 : .65 + .3 * Math.sin(time * 1.7),
    });
    if(!picker)await send("mousePressed", 0);
    const begin = performance.now(), inflight = [];
    let inputError;
    try {
      while (performance.now() - begin < milliseconds && !inputError) {
        const elapsed = performance.now() - begin;
        const request = send("mouseMoved", elapsed / 1000 * speed);
        request.catch(error => { inputError = error; });
        inflight.push(request);
        await delay(Math.max(0, 5 - (performance.now() - begin - elapsed)));
      }
      await Promise.all(inflight);
    } finally { if(!picker)await send("mouseReleased", milliseconds / 1000 * speed); }
  };
  await stroke(0, 1500);
  await delay(800);
  if (profile) {
    await call("Profiler.enable");
    await call("Profiler.setSamplingInterval", { interval: 1000 });
    await call("Profiler.start");
    profiling = true;
  }
  if (trace) {
    await call("Tracing.start", { categories: "devtools.timeline,blink.user_timing,cc,viz,gpu,input,latencyInfo,disabled-by-default-devtools.timeline", transferMode: "ReturnAsStream" });
    tracing = true;
  }
  const runs = [];
  for (let run = 0; run < repeats*(picker?2:1); run++) {
    if(picker){
      await inPage(visible=>layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'color',visible}}),run%2===1);
      await delay(500);
      assert.equal(await inPage(()=>[...document.querySelectorAll('.color-wheel-control')].some(n=>n.getBoundingClientRect().width>128)),run%2===1,"Wheel visibility must match the measured condition");
    }
    await inPage(() => {
      penBench.frames = []; penBench.events = []; penBench.raf = []; penBench.lastInput = null;
      for (const values of Object.values(penBench.calls)) values.length = 0;
    });
    const before = await evaluate("Number(layerApp.state().document_file.revision)");
    const glassBefore = await evaluate("layerApp.app.backdrop_frames?.() ?? [0, 0]");
    await stroke(run + 1, duration);
    await delay(500);
    const data = await inPage(() => JSON.parse(JSON.stringify({
      frames: penBench.frames, events: penBench.events, raf: penBench.raf, calls: penBench.calls,
      stats: layerApp.app.renderer_stats(), revision: layerApp.state().document_file.revision,
      glass: layerApp.app.backdrop_frames?.() ?? [0, 0],
    }, (_, value) => typeof value === "bigint" ? Number(value) : value)));
    await writeFile(`${output}/${label}-latest.json`, JSON.stringify({ info, before, data, errors }, null, 2));
    if(picker){
      assert.equal(data.revision,before,"Hover cannot change paint");
      assert.equal(data.calls.color_field_pixels.length,0,"Hover cannot rasterize a wheel on the UI thread");
      assert.equal(data.calls.state_update.length,0,"Hover cannot rebuild workspace models");
    }
    else assert(data.revision > before, "Stroke must commit real paint");
    const down = picker?data.events.find(event=>event.type==="pointermove")?.arrival:data.events.find(event => event.type === "pointerdown")?.arrival;
    const up = picker?data.events.findLast(event=>event.type==="pointermove")?.arrival:data.events.find(event => event.type === "pointerup")?.arrival;
    assert(Number.isFinite(down) && up > down, "Stroke must deliver both endpoints");
    const frames = data.frames.filter(frame => frame.start >= down && frame.start <= up);
    const raf = data.raf.filter(time => time >= down && time <= up);
    assert(frames.length, "Stroke must submit drawing frames");
    data.summary = {
      ...(picker?{wheel:run%2===1,sample_width:sampleWidth}:{}),
      frames: frames.length, updates_per_second: frames.length / ((up - down) / 1000),
      glass: { computed: data.glass[0] - glassBefore[0], reused: data.glass[1] - glassBefore[1] },
      frame_cpu_ms: summary(frames.map(frame => frame.end - frame.start)),
      frame_interval_ms: summary(frames.slice(1).map((frame, i) => frame.start - frames[i].start)),
      raf_interval_ms: summary(raf.slice(1).map((time, i) => time - raf[i])),
      input_to_submit_ms: summary(frames.filter(frame => frame.input !== null).map(frame => frame.end - frame.input)),
      event_age_ms: summary(data.events.filter(event => ["pointermove", "pointerrawupdate"].includes(event.type)).map(event => event.arrival - event.time)),
      diagnostics_cpu_ms: summary(data.stats.samples), diagnostics_gpu_ms: summary(data.stats.gpu_samples),
      calls: Object.fromEntries(Object.entries(data.calls).map(([name, values]) => [name, summary(values)])),
    };
    runs.push(data);
    console.log(label, run, JSON.stringify(data.summary));
    await writeFile(`${output}/${label}.json`, JSON.stringify({ info, label, size, speed, duration, profile, trace, runs, errors }, null, 2));
  }
  if (profiling) {
    const result = await call("Profiler.stop");
    profiling = false;
    await writeFile(`${output}/${label}.cpuprofile`, JSON.stringify(result.profile));
  }
  if (tracing) {
    const done = cdp.once("Tracing.tracingComplete", 180000);
    await call("Tracing.end");
    tracing = false;
    const { stream } = await done;
    let chunks = "";
    try {
      for (;;) {
        const result = await call("IO.read", { handle: stream, size: 1024 * 1024 });
        chunks += result.data;
        if (result.eof) break;
      }
    } finally { await call("IO.close", { handle: stream }); }
    await writeFile(`${output}/${label}.trace.json`, chunks);
  }
  const shot = await call("Page.captureScreenshot", { format: "png" });
  await writeFile(`${output}/${label}.png`, Buffer.from(shot.data, "base64"));
  assert.deepEqual(errors, []);
} finally {
  if (profiling) await call("Profiler.stop").catch(() => {});
  if (tracing) await call("Tracing.end").catch(() => {});
  await inPage(() => {
    if(layerApp.state().layer_tools.tool.startsWith?.("pick_"))layerApp.dispatch({type:"invoke",command:"eyedropper"});
    clearInterval(window.penBenchRecovery);
    if (window.penBench) {
      penBench.running = false;
      penBench.signal.abort();
      for (const [name, original] of Object.entries(penBench.original)) layerApp.app[name] = original;
      delete window.penBench;
    }
    delete window.penCalibration;
    delete window.penBenchRecovery;
  }).catch(() => {});
  await cdp.close();
}
