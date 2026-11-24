import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { parseArgs } from 'node:util';
import { connectBrowser, launchChrome } from '../cdp.mjs';
import { trackWasmBuild } from './web-build-provenance.mjs';
import {
  ALIASES, CLOCK_MARK, PEN_HZ, PRESENTED, TOUCH_HZ, WORKSPACES, at, canvasDragPoint, completions, documentNeeds, fillPoint, frameRows,
  gesturePoints, guaranteeSize, idleTimedOut, memoryRows, motionMs, pageClock, panelDragPoint, presentedTimes, sampleCount, selectSteps, stepRecord, stopPageOnTimeout, strokePoint,
  strokePressure, sweepPoint, traceEvents,
} from './bench-web-chain.mjs';

const { values: options } = parseArgs({ options: {
  url: { type: 'string' }, chain: { type: 'string' }, output: { type: 'string' }, profile: { type: 'string' },
  pass: { type: 'string', default: '1' }, cold: { type: 'boolean', default: false }, device: { type: 'string', default: 'web' },
  version: { type: 'string' }, commit: { type: 'string' }, 'wasm-sha256': { type: 'string' }, steps: { type: 'string' },
  document: { type: 'string', multiple: true, default: [] }, window: { type: 'string', default: '1600x1000' },
  'shader-cache': { type: 'string', default: 'empty' }, host: { type: 'string', default: 'linux' }, browser: { type: 'string', default: 'chrome' },
  executable: { type: 'string' }, endpoint: { type: 'string' }, probe: { type: 'string' }, 'memory-interval-ms': { type: 'string', default: '1000' },
  tier: { type: 'string' }, 'refresh-hz': { type: 'string' }, 'trace-steps': { type: 'string' }, 'trace-categories': { type: 'string' },
} });
assert(['linux', 'desktop', 'android'].includes(options.host), `Unknown host ${options.host}`);
if (options.host === 'android' && process.env.CAPY_CDP_PORT) options.endpoint ??= `http://127.0.0.1:${process.env.CAPY_CDP_PORT}`;
for (const name of ['url', 'chain', 'output', 'version', 'probe', options.host === 'android' ? 'endpoint' : 'profile']) assert(options[name], `--${name} is required`);

class Skipped extends Error {}
const now = () => Number(process.hrtime.bigint());
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const until = async target => { const wait = (target - now()) / 1e6; if (wait >= 1) await sleep(Math.floor(wait)); };
const chainBytes = await readFile(options.chain);
const chain = JSON.parse(chainBytes);
assert.equal(chain.schema, 1, 'Unknown chain schema');
const list = text => (text ?? '').split(',').map(item => item.trim()).filter(Boolean);
const wanted = options.steps ? new Set(list(options.steps)) : null;
selectSteps(chain.steps, wanted);
const FRAME_TRACE = ['disabled-by-default-display.framedisplayed', 'blink.user_timing'];
const tracedSteps = new Set(list(options['trace-steps']));
const stepTrace = [...FRAME_TRACE, ...list(options['trace-categories'])];
const files = Object.fromEntries(options.document.map(entry => entry.split('=', 2)));
const [width, height] = options.window.split('x').map(Number);
const output = options.output;
await mkdir(output, { recursive: true });

function instrument() {
  if (!window.GPUQueue || window.capyBench) return;
  const bench = window.capyBench = { submits: [], pen: null, file: null };
  let acquired = null;
  const acquire = GPUCanvasContext.prototype.getCurrentTexture;
  GPUCanvasContext.prototype.getCurrentTexture = function () {
    const texture = acquire.call(this);
    if (this.canvas?.id === 'canvas') acquired = { t: performance.now(), input: bench.pen };
    return texture;
  };
  const submit = GPUQueue.prototype.submit;
  GPUQueue.prototype.submit = function (buffers) {
    const result = submit.call(this, buffers);
    if (acquired) {
      const row = acquired;
      acquired = null;
      row.s = performance.now();
      bench.submits.push(row);
      this.onSubmittedWorkDone().then(() => { row.done = performance.now(); }, () => {});
    }
    return result;
  };
  for (const type of ['pointerdown', 'pointermove', 'pointerrawupdate'])
    addEventListener(type, event => { if (event.pointerType === 'pen') bench.pen = Math.max(bench.pen ?? 0, event.timeStamp); }, { capture: true, passive: true });
  window.showSaveFilePicker = async ({ suggestedName } = {}) =>
    (await navigator.storage.getDirectory()).getFileHandle(suggestedName || 'drawing.capy', { create: true });
  bench.json = value => JSON.parse(JSON.stringify(value ?? null, (_, item) => typeof item === 'bigint' ? Number(item) : item));
  bench.state = () => bench.json(layerApp.state());
  bench.visible = node => { const r = node?.getBoundingClientRect(); return !!node?.isConnected && r.width > 0 && r.height > 0 && !node.closest('[hidden]') ? r : null; };
  bench.center = node => { const r = bench.visible(node); return r && [r.x + r.width / 2, r.y + r.height / 2]; };
  bench.until = (ready, label, timeout) => new Promise((resolve, reject) => {
    const start = performance.now();
    let shown = null;
    const check = () => {
      try {
        const failure = window.layerApp?.app.gpu_failure();
        if (failure) return reject(Error(`Canvas failed: ${failure}`));
        const met = !!window.layerApp && (!ready || ready());
        if (ready && met && shown === null) shown = performance.now();
        if (met && !layerApp.documents.busy() && !layerApp.app.canvas_work_pending()) return resolve({ shown, settled: performance.now() });
        if (performance.now() - start > timeout) return reject(Error(`Timed out waiting for ${label}${shown === null ? '' : ' (shown, not settled)'}`));
      } catch (error) { return reject(error); }
      setTimeout(check, 4);
    };
    check();
  });
  bench.take = (all = false) => {
    const cutoff = performance.now() - 5000, taken = [], kept = [];
    for (const row of bench.submits) (all || row.done != null || row.s < cutoff ? taken : kept).push(row);
    bench.submits = kept;
    return taken;
  };
  bench.area = () => {
    const camera = bench.json(layerApp.app.camera()), rect = layerApp.canvas.getBoundingClientRect(), scale = rect.width / camera.viewport[0], a = camera.work_area;
    return { x: rect.x + a[0] * scale, y: rect.y + a[1] * scale, w: a[2] * scale, h: a[3] * scale, view: { x: rect.x, y: rect.y, scale }, camera };
  };
  bench.editing = () => { const s = bench.state(); return s.layers.find(layer => layer.editing)?.id ?? s.layer_properties.layer; };
  bench.panelShown = panel => !!bench.visible(document.querySelector(`.panel.${panel}-panel`));
  bench.grip = panel => {
    for (const group of document.querySelectorAll('section.dock-group')) {
      if (group.dataset.panel !== panel && !group.querySelector(`.dock-tab[data-panel="${panel}"]`)) continue;
      const center = bench.center(group.querySelector('.panel-grip'));
      if (center) return { group: Number(group.dataset.group), center };
    }
    return null;
  };
  bench.placement = () => { const layout = bench.state().workspace.layout; return JSON.stringify([layout.bands, layout.floating, layout.collapsed]); };
  bench.rightBands = () => bench.state().workspace.layout.bands.filter(band => band.edge === 'right');
  bench.divider = band => {
    const workspace = document.getElementById('workspace').getBoundingClientRect();
    const divider = bench.json(layerApp.app.layout(innerWidth, innerHeight)).dividers.find(d => d.band === band && !d.fixed && d.bounds.height > d.bounds.width);
    return divider && { x: workspace.x + divider.bounds.x, y: workspace.y + divider.bounds.y, w: divider.bounds.width, h: divider.bounds.height };
  };
  bench.dropTarget = (group, previousBand, start) => {
    const workspace = document.getElementById('workspace'), box = workspace.getBoundingClientRect(), divider = previousBand == null ? null : bench.divider(previousBand);
    const candidates = divider ? [44, 56, 68].map(distance => [divider.x - distance, divider.y + divider.h / 2])
      : [start[1], box.y + box.height / 2].map(y => [box.right - 4, y]);
    return [...candidates, start].find(position => {
      const target = bench.json(layerApp.app.drop_hint({ viewport: [workspace.clientWidth, workspace.clientHeight], position, tabs: [], item: { kind: 'group', group }, expansion: null }))?.target;
      return previousBand == null ? target?.kind === 'edge' && target.edge === 'right' && target.outer : target?.kind === 'beside_band' && target.band === previousBand;
    }) ?? null;
  };
  bench.menu = () => ['.header-item[data-kind="menu"] details.header-menu', '.header-menu-labels details.header-menu', '.header-menu-labels-compact']
    .flatMap(selector => [...document.querySelectorAll(selector)]).find(node => bench.visible(node.querySelector('summary')));
  bench.menuShown = () => [...document.querySelectorAll('details.header-menu[open] > .popover')].some(node => bench.visible(node));
}

const quiet = ['--disable-background-networking', '--disable-component-update', '--start-maximized'];
const flags = { desktop: quiet, linux: ['--ozone-platform=wayland', '--force-color-profile=srgb', '--enable-gpu', '--enable-unsafe-webgpu',
  '--use-angle=vulkan', '--enable-features=Vulkan', '--disable-vulkan-surface', '--disable-accelerated-2d-canvas', ...quiet, `--window-size=${width},${height}`] };
async function connectDevice(endpoint) {
  for (const deadline = Date.now() + 20000; ; await sleep(250)) {
    try { return await connectBrowser(endpoint, { timeout: () => 300000 }); }
    catch (error) { if (Date.now() > deadline) throw Error(`No Chrome DevTools endpoint at ${endpoint}: ${error.message}`); }
  }
}
const cdp = options.host === 'android' ? await connectDevice(options.endpoint)
  : await launchChrome(flags[options.host], { executable: options.executable, profile: options.profile, timeout: () => 300000 });
const pageTimeout = stopPageOnTimeout(cdp);
const { call, evaluate } = cdp;
const run = (fn, ...args) => evaluate(`(${fn})(...${JSON.stringify(args)})`);
const pass = { schema: 1, host: 'web', device: options.device, version: options.version, commit: options.commit ?? null, app_id: options.url,
  build: options['wasm-sha256'] ?? null, pass: Number(options.pass), cold: options.cold, clock: 'monotonic', refresh_hz: null, canvas: null,
  chain_sha256: createHash('sha256').update(chainBytes).digest('hex'), tier: options.tier ?? null, shader_cache: options['shader-cache'],
  isolation: options.host === 'android' ? 'incognito context' : 'private profile', browser: { name: options.browser },
  frames_source: null, frames_evidence: null, frames_note: null, trace_offset_ns: null, steps: [], memory_metric: null, memory_end: null, memory_note: null,
  page_errors: [], error: null };
const save = () => writeFile(`${output}/pass.json`, JSON.stringify(pass, null, 1) + '\n');
const samples = [], submits = [], documents = {};
let offsetNs = 0, refreshHz = 60, targetId, context = null, baseline = new Set(), startWorkspace = null, active = null, alive = false, memoryEnd = null, sampling = null;

const toNode = ms => ms == null ? null : Math.round(ms * 1e6 + offsetNs);
async function measureClock() {
  let best = null;
  for (let i = 0; i < 15; i++) {
    const before = now(), page = await evaluate('performance.now()'), after = now();
    if (!best || after - before < best.span) best = { span: after - before, offset: (before + after) / 2 - page * 1e6 };
  }
  offsetNs = Math.round(best.offset);
}
const state = () => run(() => capyBench.state());
const pageWait = (ready, label, timeout = 120000) =>
  evaluate(`capyBench.until(${ready === null ? 'null' : `() => (${ready})`}, ${JSON.stringify(label)}, ${timeout})`);
const waitPage = async (...args) => { const { shown, settled } = await pageWait(...args); return { shown: toNode(shown), settled: toNode(settled) }; };
const failure = () => run(() => window.layerApp?.app.gpu_failure() ?? null);
async function dispatch(action) {
  await run(action => layerApp.dispatch(action), action);
  const failed = await failure();
  if (failed) throw Error(`Canvas failed: ${failed}`);
}
async function command(name) {
  const id = ALIASES[name] ?? name;
  const enabled = await run(id => { const found = capyBench.state().commands.find(command => command.id === id); return found ? found.enabled !== false : null; }, id);
  if (enabled === null) throw new Skipped(`command ${id} is not in this build`);
  if (!enabled) throw Error(`command ${id} is disabled`);
  await dispatch({ type: 'invoke', command: id });
}
const showPanel = (panel, visible) => dispatch({ type: 'customize', action: { type: 'set_panel_visible', panel, visible } });
const panelGroup = panel => run(panel => capyBench.json(layerApp.app.layout(innerWidth, innerHeight)).groups.find(group => group.panels.includes(panel)) ?? null, panel);
async function reveal(panel) {
  let group = await panelGroup(panel);
  if (!group) { await showPanel(panel, true); group = await panelGroup(panel); }
  if (group && group.active !== panel) await dispatch({ type: 'select_panel_tab', group: group.id, panel });
}
const area = () => run(() => capyBench.area());

function send(pen, phase, [x, y], pressure, id = 1) {
  if (pen) return call('Input.dispatchMouseEvent', { type: { down: 'mousePressed', move: 'mouseMoved', up: 'mouseReleased' }[phase], x, y,
    button: 'left', buttons: phase === 'up' ? 0 : 1, clickCount: phase === 'move' ? 0 : 1, pointerType: 'pen', force: pressure });
  return call('Input.dispatchTouchEvent', { type: { down: 'touchStart', move: 'touchMove', up: 'touchEnd' }[phase],
    touchPoints: phase === 'up' ? [] : [{ x, y, id, radiusX: 4, radiusY: 4, force: 1 }] });
}
let touchId = 1;
async function contacts(pen, durationMs, point, { count = 1, pressure = () => 1 } = {}) {
  const hz = pen ? PEN_HZ : TOUCH_HZ, samples = sampleCount(durationMs, hz, count), start = now(), windows = [], sent = [];
  let index = 0;
  const id = touchId++;
  for (let contact = 0; contact < count; contact++) {
    let down;
    for (let sample = 0; sample <= samples; sample++) {
      await until(start + index++ * 1e9 / hz);
      const progress = (contact + sample / samples) / count, phase = sample === 0 ? 'down' : sample === samples ? 'up' : 'move';
      if (sample === 0) down = now();
      sent.push(send(pen, phase, point(progress), phase === 'up' ? 0 : pressure(progress), id));
    }
    windows.push([down, now()]);
  }
  await Promise.all(sent);
  return windows;
}
const tap = async (point, pen = false) => (await contacts(pen, 30, () => point))[0][0];
async function twoFingers(durationMs, flickMs, points) {
  const samples = Math.max(1, Math.round(durationMs * TOUCH_HZ / 1000)), flick = Math.round(flickMs * TOUCH_HZ / 1000);
  const ids = [touchId++, touchId++], sent = [];
  const placed = i => points(Math.min(1, i / samples), flick === 0 ? 0 : Math.min(1, Math.max(0, (i - samples) / flick)))
    .map(([x, y], n) => ({ x, y, id: ids[n], radiusX: 4, radiusY: 4, force: 1 }));
  const touch = (type, touchPoints) => sent.push(call('Input.dispatchTouchEvent', { type, touchPoints }));
  const begin = now(), start = now();
  touch('touchStart', placed(0).slice(0, 1));
  touch('touchStart', placed(0));
  for (let i = 1; i <= samples + flick; i++) {
    await until(start + i * 1e9 / TOUCH_HZ);
    touch('touchMove', placed(i));
  }
  touch('touchEnd', placed(samples + flick).slice(0, 1));
  touch('touchEnd', []);
  const end = now();
  await Promise.all(sent);
  return [begin, end];
}
async function click(point) {
  for (const type of ['mousePressed', 'mouseReleased'])
    await call('Input.dispatchMouseEvent', { type, x: point[0], y: point[1], button: 'left', buttons: type === 'mousePressed' ? 1 : 0, clickCount: 1 });
}

function recorder(record) {
  return {
    motion: ([begin_ns, end_ns]) => record.motion.push({ begin_ns, end_ns }),
    latency: (name, input_ns, { shown, settled }, surface) => record.latency.push({ name, input_ns, ready_ns: shown ?? settled, settled_ns: settled, surface }),
  };
}

async function selectLayer(label) {
  const opened = documents[active];
  const id = label == null ? opened?.paint : label === 'Photo' ? opened?.photo
    : (await state()).layers.find(layer => layer.label === label)?.id;
  if (id == null) {
    if (label == null) return;
    throw new Skipped(`no layer labelled ${label}`);
  }
  await dispatch({ type: 'select_layer', id });
}

const handlers = {
  async launch(step, record, { latency }) {
    const loaded = cdp.once('Page.loadEventFired', 120000);
    const input = now();
    await call('Page.navigate', { url: options.url });
    await loaded;
    const ready = await pageWait(`document.body.dataset.gpu === 'ready' && layerApp.app.brush_ready() && document.querySelector('#gpu-notice').hidden
      && (view => view.ready && !view.busy)(JSON.parse(layerApp.app.workspace_view()))`, 'a ready brush');
    await measureClock();
    latency('launch', input, { shown: toNode(ready.shown), settled: toNode(ready.settled) }, 'canvas');
    alive = true;
    const intervals = await evaluate(`new Promise(resolve => { const times = []; const tick = time => { times.push(time); if (times.length < 61) requestAnimationFrame(tick); else resolve(times.slice(1).map((t, i) => t - times[i]).sort((a, b) => a - b)); }; requestAnimationFrame(tick); })`);
    record.extra.raf_hz = Math.round(10000 / intervals[intervals.length >> 1]) / 10;
    refreshHz = Number(options['refresh-hz'] ?? record.extra.raf_hz);
    pass.refresh_hz = refreshHz;
    pass.browser = { ...pass.browser, ...await run(async () => {
      const adapter = await navigator.gpu.requestAdapter();
      return { agent: navigator.userAgent, viewport: [innerWidth, innerHeight, devicePixelRatio], visibility: document.visibilityState,
        adapter: { vendor: adapter.info.vendor, architecture: adapter.info.architecture, description: adapter.info.description,
          fallback: adapter.isFallbackAdapter ?? adapter.info.isFallbackAdapter ?? false } };
    }) };
    if (pass.browser.adapter.fallback || /swiftshader|llvmpipe|software/i.test(pass.browser.adapter.description)) throw Error(`WebGPU adapter is not hardware: ${JSON.stringify(pass.browser.adapter)}`);
    pass.wasm_sha256 = await provenance.verify();
    startWorkspace = JSON.parse(await evaluate('layerApp.app.workspace_view()')).id;
    Object.assign(record.extra, { workspace: startWorkspace, tabs: (await state()).tabs.length, startup: await run(() => ({ ...layerApp.startupTimes })) });
    await evaluate(`performance.mark(${JSON.stringify(CLOCK_MARK)}).startTime`);
  },
  async open(step, record, { latency }) {
    record.input = 'dispatch';
    const name = step.document, file = files[name], photo = chain.documents[name].kind === 'photo';
    const before = await run(() => { const s = capyBench.state(); return { tabs: s.tabs.map(tab => String(tab.id)), epoch: s.document_file.epoch }; });
    await run(async (url, name) => { const blob = await (await fetch(url)).blob(); capyBench.file = new File([blob], name, { type: blob.type }); },
      `/capy-bench/${encodeURIComponent(file)}`, file);
    const input = now();
    await run(() => { capyBench.opening = null; layerApp.documents.openFiles([capyBench.file]).catch(error => { capyBench.opening = String(error); }); capyBench.file = null; });
    const ready = await waitPage(`(() => {
      if (capyBench.opening) throw Error('Opening failed: ' + capyBench.opening);
      const dialog = document.querySelector('dialog[open]:not(.file-progress)');
      if (dialog) throw Error('Opening showed a dialog: ' + (dialog.querySelector('h1,h2')?.textContent ?? '').trim());
      const s = layerApp.state(), shown = s.tabs.find(tab => tab.active);
      return !!shown && !${JSON.stringify(before.tabs)}.includes(String(shown.id)) && Number(s.document_file.epoch) !== ${JSON.stringify(before.epoch)} && layerApp.app.brush_ready();
    })()`, `${name} to open`);
    latency('open', input, ready, 'canvas');
    const photoLayer = photo ? await run(() => capyBench.editing()) : null;
    if (photo) { await command('add_layer'); await waitPage(null, 'the paint layer'); }
    const { tab, paint, size } = await run(() => { const s = capyBench.state(), shown = s.tabs.find(tab => tab.active); return { tab: String(shown.id), paint: capyBench.editing(), size: [shown.width, shown.height] }; });
    documents[name] = { tab, paint, photo: photoLayer };
    active = name;
    pass.canvas ??= size;
    record.extra.canvas = size;
  },
  async stroke(step, record, { motion }) {
    const preset = step.preset;
    if (!await run(preset => layerApp.app.catalog().brush_categories.some(c => c.brushes.some(brush => brush.id === preset)), preset)) throw new Skipped(`brush preset ${preset} is not in this build`);
    await selectLayer(step.layer ?? null);
    await dispatch({ type: 'select_brush', id: preset });
    await waitPage('layerApp.app.brush_ready()', `brush ${preset}`);
    const { tool, pixels } = await run(() => { const s = capyBench.state(), shown = s.tabs.find(tab => tab.active); return { tool: s.brush.tool, pixels: shown.width * shown.height }; });
    if (['clone', 'heal', 'spot_heal'].includes(tool)) await command('use_reference_below');
    await dispatch({ type: 'set_brush_size', value: step.size === 'guarantee' ? guaranteeSize(pixels) : step.size });
    if (step.zoom === 'fit') await command('fit_canvas');
    else if (typeof step.zoom === 'number') await dispatch({ type: 'set_zoom', zoom: step.zoom });
    await waitPage('layerApp.app.brush_ready()', 'brush ready');
    const a = await area();
    Object.assign(record.extra, await run(() => { const s = capyBench.state(); return { size: s.brush.diameter, zoom: s.camera.zoom }; }));
    if (['clone', 'heal'].includes(tool)) {
      await command('clone_source_arm');
      await tap(at(a, .25, .5), true);
      await waitPage(`!layerApp.state().commands.some(c => c.id === 'clone_source_arm' && c.selected)`, 'clone source');
    }
    const primed = (await state()).document_file.revision, travel = tool === 'liquify' ? .025 : 0;
    await contacts(true, 100, p => at(a, .5 + travel * p, .5));
    await waitPage(`layerApp.state().document_file.revision > ${primed}`, 'priming dab');
    const duration = motionMs(step, chain, refreshHz);
    const windows = await contacts(true, duration, p => strokePoint(a, step.path, p, duration), { count: step.contacts ?? 1, pressure: p => strokePressure(p, duration) });
    const settled = await evaluate(`new Promise(resolve => { const end = performance.now() + ${step.catch_up_cap_ms ?? 3000}; const check = () => !layerApp.app.canvas_work_pending() ? resolve(performance.now()) : performance.now() > end ? resolve(null) : setTimeout(check, 2); check(); })`);
    record.settled_ns = toNode(settled);
    if (settled == null) record.extra.catch_up_capped = true;
    await waitPage(null, 'stroke catch-up', 60000);
    for (const window of windows) { motion(window); record.contacts.push({ down_ns: window[0], up_ns: window[1] }); }
  },
  async gesture(step, record, { motion }) {
    const kind = step.gesture;
    if (!['pan', 'pinch', 'rotate'].includes(kind)) throw new Skipped(`unknown gesture ${kind}`);
    const quick = step.release === 'quick', a = await area(), before = (await state()).camera.revision;
    motion(await twoFingers(motionMs(step, chain, refreshHz), quick ? 100 : 0, (s, flick) => gesturePoints(a, kind, s, flick)));
    if (quick) record.settled_ns = toNode(await evaluate(`new Promise((resolve, reject) => { const end = performance.now() + 30000; let previous = null;
      const check = () => { const camera = JSON.stringify(capyBench.json(layerApp.app.camera()));
        if (camera === previous && !layerApp.app.canvas_work_pending()) return resolve(performance.now());
        if (performance.now() > end) return reject(Error('The camera did not stop'));
        previous = camera; setTimeout(check, 4); }; check(); })`));
    await waitPage(null, 'gesture to settle');
    if ((await state()).camera.revision === before) throw Error(`The ${kind} gesture did not move the camera`);
  },
  async tap(step, record, { latency }) {
    const tool = step.tool, paint = tool === 'fill';
    if (paint && await run(() => capyBench.state().commands.find(c => c.id === 'deselect')?.enabled === true)) await command('deselect');
    await command(tool);
    for (const point of step.points) {
      await waitPage(null, 'idle before tap');
      const revision = paint ? 'Number(layerApp.state().document_file.revision)' : 'Number(layerApp.state().revision)';
      const before = await evaluate(revision), a = await area(), drawing = (await state()).tabs.find(tab => tab.active);
      const target = at(a, point[0], point[1]);
      const input = await tap(paint ? fillPoint(target, a, a.camera, drawing, a.view) : target, true);
      latency(tool, input, await waitPage(`${revision} > ${before}`, `${tool} tap`), step.surface ?? 'canvas');
    }
  },
  async drag(step, record, tools) {
    const target = step.target, duration = motionMs(step, chain, refreshHz);
    if (target === 'canvas' || target.startsWith('object:')) return canvasDrag(step, target, duration, record, tools);
    if (target.startsWith('grip:')) return panelDrag(target.slice(5), duration, record, tools);
    if (target.startsWith('edge:')) return edgeDrag(target.slice(5).replace(/-column$/, ''), duration, record, tools);
    if (target === 'slider:size') return sliderDrag(duration, record, tools);
    if (target.startsWith('list:')) return fling(target.slice(5), duration, record, tools);
    throw new Skipped(`unknown drag target ${target}`);
  },
  async scrub(step, record, { motion }) {
    record.input = 'dispatch';
    const frames = step.frames ?? chain.motion_frames ?? 150;
    const paced = async source => {
      const [begin, end] = await evaluate(`new Promise(resolve => { let index = 0, begin = null; const frames = ${frames}, action = ${source};
        const tick = () => { begin ??= performance.now(); layerApp.dispatch(action(index++)); if (index < frames) requestAnimationFrame(tick); else resolve([begin, performance.now()]); };
        requestAnimationFrame(tick); })`);
      return [toNode(begin), toNode(end)];
    };
    if (step.effect) {
      if (step.effect !== 'curves') throw new Skipped(`no scrub value is defined for ${step.effect}`);
      await selectLayer('Photo');
      await dispatch({ type: 'effect', action: { op: 'insert', effect: step.effect } });
      const layer = (await state()).layer_properties.layer;
      record.extra.effect_layer = layer;
      try {
        motion(await paced(`i => ({ type: 'effect', action: { op: 'gesture', phase: i === 0 ? 'down' : i === frames - 1 ? 'up' : 'move',
          action: { op: 'set', layer: ${layer}, key: 'rgb', value: { kind: 'curve', value: [[0, 0], [.5, .5 + .25 * Math.sin(2 * Math.PI * i / frames)], [1, 1]] } } } })`));
        await waitPage(null, 'curves to settle', 60000);
      } finally { await dispatch({ type: 'layer', action: { op: 'delete', id: layer } }); }
      return;
    }
    const [from, to] = step.layer_opacity;
    await selectLayer(null);
    motion(await paced(`i => ({ type: 'set_layer_opacity', opacity: ${from} + (${to} - ${from}) * i / Math.max(1, frames - 1) })`));
    await waitPage(null, 'opacity to settle', 60000);
  },
  async repeat(step, record, { latency }) {
    const surface = step.surface ?? 'canvas';
    let items;
    if (step.commands) {
      record.input = 'dispatch';
      items = step.commands.map(name => async () => { const input = now(); await command(name); latency(name, input, await waitPage(null, name), surface); });
    } else if (step.open) items = step.open.map(spec => async () => openItem(spec, surface, latency));
    else if (step.workspaces) {
      items = step.workspaces.map(name => async () => {
        const id = name === 'start' ? startWorkspace : WORKSPACES[name] ?? `builtin:workspace:${name}`;
        if (!id) throw new Skipped('the start workspace is unknown');
        if (name !== 'start' && id === startWorkspace) throw new Skipped(`${name} is already the starting workspace`);
        const button = await run(id => capyBench.center(document.querySelector(`[data-workspace-id="${CSS.escape(id)}"]`)), id);
        if (!button) throw new Skipped(`no ${name} workspace switch button is visible`);
        const input = await tap(button);
        latency(name, input, await waitPage(`(view => view.id === ${JSON.stringify(id)} && !view.busy && !view.loading)(JSON.parse(layerApp.app.workspace_view())) && layerApp.app.brush_ready()`, `workspace ${name}`), surface);
      });
    } else if (step.tabs) {
      items = step.tabs.map(name => async () => {
        const opened = documents[name];
        if (!opened) throw new Skipped(`${name} is not open`);
        const pick = await run(tab => capyBench.center(document.querySelector(`.drawing-tab[data-drawing-id="${CSS.escape(tab)}"] .drawing-tab-pick`)), opened.tab);
        let input;
        if (pick) input = await tap(pick);
        else { record.input = 'dispatch'; input = now(); await run(tab => { layerApp.documents.select(tab); }, opened.tab); }
        latency(`tab:${name}`, input, await waitPage(`String(layerApp.app.document_tabs(0).selected) === ${JSON.stringify(opened.tab)} && layerApp.app.brush_ready()`, `tab ${name}`), surface);
        active = name;
      });
    } else throw new Skipped('nothing to repeat');
    for (let round = 0; round < (step.count ?? 1); round++) for (const item of items) await item();
  },
  async save(step, record, { latency }) {
    record.input = 'dispatch';
    const opened = documents[step.document];
    if (!opened) throw new Skipped(`${step.document} is not open`);
    if (String(await evaluate('layerApp.app.document_tabs(0).selected')) !== opened.tab) {
      await run(tab => { layerApp.documents.select(tab); }, opened.tab);
      await waitPage(`String(layerApp.app.document_tabs(0).selected) === ${JSON.stringify(opened.tab)}`, `tab ${step.document}`);
      active = step.document;
    }
    const input = now();
    await command('save_document');
    latency('save', input, await waitPage(`(s => !s.document_file.modified && !s.requests.some(r => r.kind.type === 'document'))(layerApp.state())`, 'save'), 'window');
  },
  async resume(step, record, { latency }) {
    const { targetId: other } = await call('Target.createTarget', { url: 'about:blank', ...(context ? { browserContextId: context } : {}) }, null);
    try {
      await call('Target.activateTarget', { targetId: other }, null);
      const deadline = Date.now() + 10000;
      while (await evaluate('document.visibilityState') !== 'hidden') {
        if (Date.now() > deadline) throw Error('The page did not go to the background');
        await sleep(5);
      }
      await sleep(1000);
      const input = now();
      await call('Target.activateTarget', { targetId }, null);
      latency('resume', input, await waitPage(`document.visibilityState === 'visible' && layerApp.app.brush_ready()`, 'resume', 60000), 'canvas');
    } finally { await call('Target.closeTarget', { targetId: other }, null).catch(() => {}); }
  },
  async close(step, record, { latency }) {
    record.input = 'dispatch';
    const initial = (await state()).tabs.map(tab => String(tab.id));
    for (const id of initial) {
      if (!(await state()).tabs.some(tab => String(tab.id) === id)) continue;
      if (String(await evaluate('layerApp.app.document_tabs(0).selected')) !== id) {
        await run(tab => { layerApp.documents.select(tab); }, id);
        await waitPage(`String(layerApp.app.document_tabs(0).selected) === ${JSON.stringify(id)}`, 'tab to close');
      }
      const input = now();
      await command('close_document');
      const deadline = Date.now() + 120000;
      for (;;) {
        const { open, discard } = await run(id => ({ open: layerApp.state().tabs.some(tab => String(tab.id) === id),
          discard: capyBench.center(document.querySelector('dialog[open] button.destructive-action')) }), id);
        if (!open) break;
        if (discard) await click(discard);
        if (Date.now() > deadline) throw Error('The drawing did not close');
        await sleep(10);
      }
      latency('close', input, await waitPage(null, 'close'), 'window');
    }
  },
};

async function canvasDrag(step, target, duration, record, { motion, latency }) {
  if (step.select === 'all') await command('select_all');
  if (step.command) await command(step.command);
  if (step.tool) await command(step.tool);
  if (target.startsWith('object:')) await selectLayer(target.slice(7));
  await waitPage(null, 'drag target');
  const a = await area(), drawing = (await state()).tabs.find(tab => tab.active);
  const start = target === 'canvas' ? at(a, .5, .5)
    : [a.view.x + (a.camera.translation[0] + a.camera.zoom * drawing.width / 2) * a.view.scale, a.view.y + (a.camera.translation[1] + a.camera.zoom * drawing.height / 2) * a.view.scale];
  motion((await contacts(true, duration, p => canvasDragPoint(a, step.path, p, start)))[0]);
  if (step.then === 'apply') {
    await waitPage(null, 'drag to settle');
    const input = now();
    await command('apply');
    latency('apply', input, await waitPage(null, 'apply'), 'canvas');
  }
}

async function panelDrag(panel, duration, record, { motion }) {
  const temporary = !await run(panel => capyBench.grip(panel), panel);
  try {
    if (temporary) await showPanel(panel, true);
    const grip = await evaluate(`new Promise((resolve, reject) => { const end = performance.now() + 5000; const check = () => { const grip = capyBench.grip(${JSON.stringify(panel)}); if (grip) resolve(grip); else if (performance.now() > end) reject(Error('No visible ${panel} grip')); else setTimeout(check, 10); }; check(); })`);
    const before = await run(() => capyBench.placement()), bands = await run(() => capyBench.rightBands());
    const slot = bands.findIndex(band => band.root.id === grip.group), previousBand = slot > 0 ? bands[slot - 1].id : null;
    const a = await area(), float = at(a, .5, .5), radius = .1 * Math.min(a.w, a.h);
    let drop = slot < 0 ? grip.center : null, asked = slot < 0, answer = null;
    motion((await contacts(false, duration, p => {
      if (p >= .4 && !asked) { asked = true; answer = run((group, band, start) => capyBench.dropTarget(group, band, start), grip.group, previousBand, grip.center).then(found => { drop = found; }); }
      return panelDragPoint(p, grip.center, float, radius, drop);
    }))[0]);
    await answer;
    await waitPage(null, 'panel drop');
    const restored = slot < 0 ? await run(() => capyBench.placement()) === before
      : drop != null && (await run(() => capyBench.rightBands()))[slot]?.root.id === grip.group;
    Object.assign(record.extra, { target: `group-grip-${grip.group}`, returned_in_place: restored });
    if (!restored && !temporary) await command('undo_workspace');
    if (drop == null) throw Error(`No live drop target for the starting ${panel} dock`);
    if (!restored) throw Error(`${panel} did not return to its starting dock`);
  } finally { if (temporary) await showPanel(panel, false); }
}

async function edgeDrag(side, duration, record, { motion }) {
  const band = (await state()).workspace.layout.bands.find(item => item.edge === side);
  if (!band) throw new Skipped(`no ${side} column in this workspace`);
  const divider = await run(band => capyBench.divider(band), band.id);
  if (!divider) throw new Skipped(`the ${side} column has no visible divider`);
  const start = [divider.x + divider.w / 2, divider.y + divider.h / 2];
  motion((await contacts(false, duration, p => sweepPoint(start, p, true, side === 'right' ? -120 : 120)))[0]);
  record.extra.target = `divider-${band.id}`;
}

async function sliderDrag(duration, record, { motion }) {
  const slider = await run(() => { const node = [...document.querySelectorAll('.toolbar-slider.size-track')].find(item => capyBench.visible(item)); const r = node && capyBench.visible(node); return r && { x: r.x, y: r.y, w: r.width, h: r.height }; });
  if (!slider) throw new Skipped('no brush size slider is visible in this workspace');
  const before = (await state()).brush.diameter;
  motion((await contacts(false, duration, p => sweepPoint([slider.x + slider.w / 2, slider.y + slider.h / 2], p, slider.w >= slider.h, 120)))[0]);
  await waitPage(null, 'slider release');
  Object.assign(record.extra, { target: 'toolbar-slider-size', size_before: before, size_after: (await state()).brush.diameter });
}

async function fling(panel, duration, record, { motion }) {
  if (!await run(panel => capyBench.panelShown(panel), panel)) {
    await reveal(panel);
    await waitPage(`capyBench.panelShown(${JSON.stringify(panel)})`, `${panel} panel`, 30000);
  }
  const box = await run(panel => { const r = capyBench.visible(document.querySelector(`.panel.${panel}-panel`).closest('.panel-frame')); return { x: r.x + r.width / 2, y: r.y, h: r.height }; }, panel);
  const begin = now(), start = Date.now();
  await contacts(false, 120, p => [box.x, box.y + box.h * (.75 - .5 * p)]);
  await sleep(Math.max(0, start + duration / 2 - Date.now()));
  await contacts(false, 120, p => [box.x, box.y + box.h * (.25 + .5 * p)]);
  await sleep(Math.max(0, start + duration - Date.now()));
  motion([begin, now()]);
  record.extra.target = `panel-body-${panel}`;
}

async function openItem(spec, surface, latency) {
  if (spec === 'menu:main') {
    const summary = await run(() => capyBench.center(capyBench.menu()?.querySelector('summary')));
    if (!summary) throw new Skipped('no main menu control is visible');
    const input = await tap(summary);
    latency(spec, input, await waitPage('capyBench.menuShown()', 'main menu', 30000), surface);
    await tap(summary);
    await waitPage('!capyBench.menuShown()', 'main menu to close', 30000);
    return;
  }
  const panel = spec.replace(/^panel:/, '');
  if (await run(panel => capyBench.panelShown(panel), panel)) {
    await showPanel(panel, false);
    await waitPage(`!capyBench.panelShown(${JSON.stringify(panel)})`, `${panel} to close`, 30000);
  }
  const input = now();
  await reveal(panel);
  latency(`panel:${panel}`, input, await waitPage(`capyBench.panelShown(${JSON.stringify(panel)})`, `${panel} to open`, 30000), surface);
}

let trace = null, provenance = null;
const traced = [];
async function startTrace(categories) {
  await call('Tracing.start', { transferMode: 'ReturnAsStream', traceConfig: { recordMode: 'recordAsMuchAsPossible', includedCategories: categories } }, null);
  trace = categories;
}
async function endTrace() {
  trace = null;
  const done = cdp.once('Tracing.tracingComplete', 180000);
  await call('Tracing.end', {}, null);
  const { stream } = await done;
  let text = '';
  for (;;) { const chunk = await call('IO.read', { handle: stream, size: 1 << 22 }, null); text += chunk.data; if (chunk.eof) break; }
  await call('IO.close', { handle: stream }, null);
  for (const event of traceEvents(text)) if (event.name === PRESENTED || event.name === CLOCK_MARK) traced.push(event);
  return text;
}
async function retrace(categories, file) {
  const text = trace && await endTrace();
  if (file && text) await writeFile(file, text);
  await startTrace(categories);
}
function startProbe([command, ...args]) {
  const child = spawn(command, args, { stdio: ['pipe', 'pipe', 'inherit'] });
  let buffer = '', pending = null;
  const settle = (method, value) => { const waiting = pending; pending = null; clearTimeout(waiting?.timer); waiting?.[method](value); };
  child.stdout.setEncoding('utf8').on('data', data => {
    buffer += data;
    for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) {
      const line = buffer.slice(0, index);
      buffer = buffer.slice(index + 1);
      try { settle('resolve', JSON.parse(line)); } catch (error) { settle('reject', error); }
    }
  });
  child.on('exit', code => settle('reject', Error(`memory probe exited with ${code}`)));
  child.on('error', error => settle('reject', error));
  return {
    ask: request => new Promise((resolve, reject) => {
      pending = { resolve, reject, timer: setTimeout(() => settle('reject', Error('memory probe did not answer in 30 s')), 30000) };
      child.stdin.write(JSON.stringify(request) + '\n');
    }),
    close: () => new Promise(resolve => { child.once('exit', resolve); child.stdin.end(); if (child.exitCode !== null) resolve(); }),
  };
}
const probe = startProbe(JSON.parse(options.probe));
const processInfo = async () => (await call('SystemInfo.getProcessInfo', {}, null)).processInfo;
async function sample() {
  if (sampling) return;
  sampling = (async () => {
    const t_ns = now(), processes = await processInfo();
    const renderers = processes.filter(item => item.type === 'renderer' && !baseline.has(item.id)).map(item => item.id);
    const gpu = processes.find(item => item.type === 'GPU')?.id ?? null;
    const reply = await probe.ask({ renderers, gpu });
    pass.memory_metric = reply.metric;
    if (reply.note) pass.memory_note ??= reply.note;
    samples.push({ t_ns, os_available_kib: reply.os_available_kib, gpu_kib: reply.gpu_kib,
      renderers: Object.fromEntries(renderers.filter(pid => reply.processes[pid] != null).map(pid => [pid, reply.processes[pid]])),
      gpu_process_rss_kib: gpu == null ? null : reply.processes[gpu] ?? null });
  })().catch(error => { pass.memory_note ??= `memory sample failed: ${error.message}`; }).finally(() => { sampling = null; });
  await sampling;
}

let timer;
try {
  if (options.host === 'android') {
    baseline = new Set((await processInfo().catch(() => [])).filter(item => item.type === 'renderer').map(item => item.id));
    context = (await call('Target.createBrowserContext', {}, null).catch(error => {
      throw Error(`Chrome cannot open an isolated incognito context, so the pass stops instead of using the device profile: ${error.message}`);
    })).browserContextId;
  }
  targetId = await cdp.attachPage(context);
  if (context) await call('Target.activateTarget', { targetId }, null);
  for (const domain of ['Page', 'Runtime']) await call(`${domain}.enable`);
  await call('Browser.grantPermissions', { permissions: ['durableStorage'], ...(context ? { browserContextId: context } : {}) }, null);
  await call('Emulation.setFocusEmulationEnabled', { enabled: true });
  await call('Page.addScriptToEvaluateOnNewDocument', { source: `(${instrument})()` });
  provenance = await trackWasmBuild(call, evaluate, options['wasm-sha256']);
  await startTrace(FRAME_TRACE);
  await sample();
  timer = setInterval(sample, Number(options['memory-interval-ms']));
  let opened = null, stuck = null;
  for (const step of chain.steps) {
    if (step.do === 'open') opened = step.document;
    if (wanted && !wanted.has(step.id)) continue;
    const record = stepRecord(step, now(), stuck);
    if (stuck) { pass.steps.push(record); continue; }
    try {
      const needs = documentNeeds(step, opened);
      if (needs.some(name => !files[name])) throw new Skipped('fixture not provided');
      const missing = step.do === 'open' ? null : needs.find(name => !documents[name]);
      if (missing) throw new Skipped(`${missing} did not open`);
      if (step.do !== 'launch') {
        if (!alive) throw Error('The app is not running');
        await waitPage(null, `idle before ${step.id}`, 60000);
      }
      if (step.do === 'close') memoryEnd = samples.at(-1);
      const handler = handlers[step.do];
      if (!handler) throw new Skipped(`unknown step kind ${step.do}`);
      if (tracedSteps.has(step.id)) await retrace(stepTrace);
      record.begin_ns = now();
      await handler(step, record, recorder(record));
    } catch (error) {
      if (error instanceof Skipped) Object.assign(record, { status: 'skipped', reason: error.message });
      else {
        Object.assign(record, { status: 'error', reason: error.message });
        record.extra.error = error.stack;
        if (alive) await run(() => document.querySelector('dialog[open]') && document.querySelector('dialog[open]').close()).catch(() => {});
      }
    }
    record.end_ns = now();
    if (trace !== FRAME_TRACE) await retrace(FRAME_TRACE, `${output}/trace-${step.id}.json`);
    submits.push(...await run(() => capyBench.take()).catch(() => []));
    const failed = alive && !pageTimeout() ? await failure().catch(() => 'the page is gone') : null;
    const timeout = pageTimeout();
    if (timeout) {
      if (record.status !== 'error') Object.assign(record, { status: 'error', reason: timeout.message });
      if (record.extra.error !== timeout.stack) record.extra.transport_error = timeout.stack;
      pass.error = stuck = timeout.message;
    } else if (idleTimedOut(record.reason)) stuck = record.reason;
    pass.steps.push(record);
    await save();
    console.log(`BENCH_STEP ${step.id} ${record.status} ${record.reason ?? ''}`);
    if (failed && !timeout) { pass.error = `Canvas failed: ${failed}`; break; }
  }
} catch (error) {
  const timeout = pageTimeout();
  pass.error = timeout?.message ?? error.stack;
  if (timeout && !pass.steps.length)
    pass.steps = selectSteps(chain.steps, wanted).map(step => stepRecord(step, now(), timeout.message));
  console.error(error);
} finally {
  clearInterval(timer);
  if (pageTimeout()) await save().catch(error => console.error(error));
  await sampling;
  submits.push(...await run(() => capyBench.take(true)).catch(() => []));
  pass.page_errors = cdp.errors.slice(0, 20);
  if (trace) await endTrace().catch(error => { pass.frames_note = `Chrome trace unavailable: ${error.message}`; });
  await provenance?.close().catch(() => {});
  if (pageTimeout()) pass.error = pageTimeout().message;
  if (context) await call('Target.disposeBrowserContext', { browserContextId: context }, null).catch(() => {});
  await cdp.close().catch(() => {});
  await probe.close();
  const clock = pageClock(traced), presented = clock ? presentedTimes(traced).map(t => t - clock.offset_ns + offsetNs) : [];
  pass.trace_offset_ns = clock ? clock.offset_ns - offsetNs : null;
  const canvas = submits.filter(row => row.s != null).map(row => toNode(row.s)).sort((a, b) => a - b);
  const frames = frameRows(canvas, presented);
  pass.frames_source = frames.evidence === 'presented' ? 'chrome-trace' : 'canvas-submissions';
  pass.frames_evidence = frames.evidence;
  pass.frames_note ??= frames.evidence === 'presented' ? 'canvas frames are the first presented display frame after each canvas submission; window frames are every presented display frame'
    : 'no presented display frames were traced, so canvas frames are WebGPU canvas submissions';
  const finished = submits.filter(row => row.done != null).map(row => ({ complete_ns: toNode(row.done), input_ns: toNode(row.input) }));
  for (const record of pass.steps)
    if (record.contacts.length && chain.steps.find(step => step.id === record.id)?.do === 'stroke')
      record.completions = completions(finished, record.contacts[0].down_ns, record.settled_ns ?? record.end_ns);
  const rows = memoryRows(samples, clock?.pid);
  if (clock == null) pass.memory_note ??= 'the page renderer was not identified, so app_rss_kib is missing';
  if (clock == null && traced.length) pass.frames_note = 'the page clock mark was not traced, so presented frames could not be placed';
  const end = memoryEnd ? memoryRows([memoryEnd], clock?.pid)[0] : rows.at(-1);
  pass.memory_end = end ? Object.fromEntries(Object.entries(end).filter(([key]) => key !== 't_ns')) : null;
  await writeFile(`${output}/frames.jsonl`, frames.rows.map(row => JSON.stringify(row)).join('\n') + (frames.rows.length ? '\n' : ''));
  await writeFile(`${output}/memory.jsonl`, rows.map(row => JSON.stringify(row)).join('\n') + (rows.length ? '\n' : ''));
  await save();
  console.log(`Wrote ${output}`);
}
process.exit(pass.error ? 1 : 0);
