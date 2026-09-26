// Refresh timeline, including GPU API calls, main-thread jobs and recovery.
// Use a dedicated test origin; this reloads the selected tab. It never accepts,
// discards or clears recovery records. --probes opens/closes title-bar menus.
// --strokes replays and undoes test strokes on an empty dedicated drawing.
// LAYER_DEVICE_CDP=http://127.0.0.1:9247 LAYER_WEB_URL=http://127.0.0.1:4197/ \
//   node tools/performance/web-refresh.mjs
// --desktop-ui-only launches a temporary Chrome profile without WebGPU. This
// checks the harness/UI path, and is not a GPU or tablet performance result.
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { installRefreshProbe } from './web-refresh-probe.js';
import { replayRefreshStrokes } from './web-refresh-strokes.mjs';

const uiOnly = process.argv.includes('--desktop-ui-only');
const software = process.argv.includes('--desktop-software');
const desktop = uiOnly || software;
const skipCatalog = process.argv.includes('--skip-catalog');
const quietCompiler = process.argv.includes('--quiet-compiler');
const earlyRecovery = process.argv.includes('--early-recovery');
const url = process.env.LAYER_WEB_URL || 'http://127.0.0.1:4197/';
const output = process.env.LAYER_TEST_ARTIFACTS || 'artifacts/web-refresh';
const duration = Number(process.env.LAYER_REFRESH_MS || 60000);
const runs = Number(process.env.LAYER_REFRESH_RUNS || 1);
if (!Number.isSafeInteger(duration) || duration < 1000 || !Number.isSafeInteger(runs) || runs < 1)
  throw Error('LAYER_REFRESH_MS and LAYER_REFRESH_RUNS must be positive integers (capture >= 1000 ms)');
await mkdir(output, { recursive: true });
let chrome, profile, socket, send, session, sequence = 0;
const pending = new Map(), errors = [];
let finishTrace;
function receive(message) {
  if (message.id) {
    const job = pending.get(message.id); if (!job) return;
    pending.delete(message.id); clearTimeout(job.timer);
    message.error ? job.reject(Error(JSON.stringify(message.error))) : job.resolve(message.result);
  } else if (message.method === 'Tracing.tracingComplete') finishTrace?.(message.params);
  else if (message.method === 'Runtime.exceptionThrown') errors.push(message.params);
  else if (message.method === 'Page.javascriptDialogOpening' && message.params.type === 'beforeunload') {
    call('Page.handleJavaScriptDialog', { accept: true }).catch(error => errors.push(String(error)));
  }
}
function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}`)); }, duration + 30000);
    pending.set(id, { resolve, reject, timer });
    send({ id, method, params, ...(session ? { sessionId: session } : {}) });
  });
}
async function evaluate(expression) {
  const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
let preload;
let tracing = false, profiling = false;
let strokeWork;
const strokeAbort = new AbortController();
try {
  if (desktop) {
    profile = await mkdtemp(join(tmpdir(), 'capy-refresh-'));
    chrome = spawn(process.env.CHROME || 'google-chrome', [
      '--headless=new', '--ozone-platform=headless', '--remote-debugging-pipe',
      ...(uiOnly ? ['--disable-gpu'] : ['--use-angle=swiftshader', '--enable-unsafe-webgpu', '--enable-unsafe-swiftshader']),
      `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check',
      '--password-store=basic', '--window-size=1440,1000', 'about:blank',
    ], { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
    chrome.stderr.on('data', () => {});
    let buffer = '';
    chrome.stdio[4].on('data', data => {
      buffer += data.toString();
      for (let index; (index = buffer.indexOf('\0')) >= 0;) {
        receive(JSON.parse(buffer.slice(0, index))); buffer = buffer.slice(index + 1);
      }
    });
    send = message => chrome.stdio[3].write(JSON.stringify(message) + '\0');
    const { targetInfos } = await call('Target.getTargets');
    ({ sessionId: session } = await call('Target.attachToTarget', { targetId: targetInfos.find(t => t.type === 'page').targetId, flatten: true }));
  } else {
    const endpoint = process.env.LAYER_DEVICE_CDP || 'http://127.0.0.1:9247';
    const tabs = await (await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(10000) })).json();
    const tab = tabs.find(t => t.url === url);
    if (!tab) throw Error(`Open dedicated test origin ${url} first`);
    socket = new WebSocket(tab.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
    socket.onmessage = e => receive(JSON.parse(e.data));
    send = message => socket.send(JSON.stringify(message));
  }
  for (const domain of ['Page', 'Runtime', 'Network']) await call(`${domain}.enable`);
  await call('Page.bringToFront');
  ({ identifier: preload } = await call('Page.addScriptToEvaluateOnNewDocument', {
    source: `(${installRefreshProbe.toString()})(${JSON.stringify({ duration, uiOnly, skipCatalog, quietCompiler, earlyRecovery, probes: process.argv.includes('--probes') })})`,
  }));
  for (let run = 0; run < runs; run++) {
    if (process.argv.includes('--timeline')) {
      await call('Tracing.start', {
        categories: 'devtools.timeline,v8,blink.user_timing,gpu,disabled-by-default-gpu.service',
        transferMode: 'ReturnAsStream',
      });
      tracing = true;
    }
    if (process.argv.includes('--profile')) { await call('Profiler.enable'); await call('Profiler.start'); profiling = true; }
    const before = await evaluate('performance.timeOrigin');
    if (desktop && run === 0) await call('Page.navigate', { url });
    else await call('Page.reload', { ignoreCache: process.argv.includes('--bypass-http-cache') });
    // The replay owns input while the observer records this navigation. Capture
    // errors immediately and restore its wrappers before disposing the observer.
    strokeWork = process.argv.includes('--strokes')
      ? replayRefreshStrokes({ call, evaluate, before, output: `${output}/stroke-${run}.json`, signal: strokeAbort.signal })
        .then(value => ({ value }), error => ({ error }))
      : null;
    // Poll from the host: long browser tasks remain visible as gaps and do not
    // prevent periodic progress reports from this runner.
    const end = Date.now() + duration + 30000;
    for (;;) {
      let value;
      try { value = await evaluate(`({ origin: performance.timeOrigin, elapsed: performance.now(), attached: !!window.refreshTrace, times: window.layerApp?.startupTimes })`); }
      catch (error) { if (!/context|navigat/i.test(String(error))) throw error; }
      if (value?.origin !== before && value?.attached && value.elapsed >= duration) break;
      if (Date.now() > end) throw Error('Refresh did not finish within capture window');
      if (value?.attached) console.log(JSON.stringify({ run, elapsed: Math.round(value.elapsed), times: value.times }));
      await new Promise(resolve => setTimeout(resolve, 5000));
    }
    const strokeResult = await strokeWork;
    if (strokeResult?.error) throw strokeResult.error;
    const result = await evaluate(`({ ...refreshTrace, times: layerApp.startupTimes,
      marks: Object.fromEntries(performance.getEntriesByType('mark').map(e=>[e.name,e.startTime])),
      markEvents: performance.getEntriesByType('mark').map(e=>({name:e.name,start:e.startTime})),
      navigation: performance.getEntriesByType('navigation').map(e=>e.toJSON()),
      resources: performance.getEntriesByType('resource').map(e=>e.toJSON()),
      userAgent: navigator.userAgent, viewport: [innerWidth,innerHeight,devicePixelRatio],
      gpuNotice: document.querySelector('#gpu-notice').textContent,
      workspace: JSON.parse(layerApp.app.workspace_view()).name,
    })`);
    result.mode = uiOnly ? 'desktop UI only; GPU disabled' : software ? 'desktop software WebGPU; not hardware timing' : 'attached browser';
    result.ablations = { skipCatalog, quietCompiler, earlyRecovery };
    result.probed = process.argv.includes('--probes'); result.cdpErrors = [...errors];
    result.strokes = !!strokeWork;
    result.instrumentation = { timeline: process.argv.includes('--timeline'), profile: process.argv.includes('--profile') };
    await writeFile(`${output}/refresh-${run}.json`, JSON.stringify(result, null, 2));
    if (process.argv.includes('--profile')) {
      const { profile } = await call('Profiler.stop');
      profiling = false;
      await writeFile(`${output}/refresh-${run}.cpuprofile`, JSON.stringify(profile));
    }
    if (process.argv.includes('--timeline')) {
      const finished = new Promise(resolve => { finishTrace = resolve; });
      await call('Tracing.end'); const { stream } = await finished;
      tracing = false;
      const chunks = [];
      for (;;) { const part = await call('IO.read', { handle: stream }); chunks.push(part.base64Encoded ? Buffer.from(part.data, 'base64') : Buffer.from(part.data)); if (part.eof) break; }
      await call('IO.close', { handle: stream });
      await writeFile(`${output}/refresh-${run}.trace.json`, Buffer.concat(chunks));
    }
    await evaluate('refreshTrace.dispose(); undefined');
    const slowest = result.calls.toSorted((a, b) => b.sync - a.sync).slice(0, 12);
    console.log(JSON.stringify({ run, mode: result.mode, marks: result.marks, dialogs: result.dialogs,
      pipelines: result.gpu.filter(c => c.name.includes('Pipeline')).length, slowest, states: result.states }, null, 2));
    if (run + 1 < runs && !uiOnly && result.times.complete == null) throw Error('Wait for this shader queue to finish before measuring another reload');
  }
} finally {
  strokeAbort.abort();
  await strokeWork;
  if (profiling) await call('Profiler.stop').catch(() => {});
  if (tracing) await call('Tracing.end').catch(() => {});
  if (send) await evaluate('window.refreshTrace?.dispose(); undefined').catch(() => {});
  if (preload) await call('Page.removeScriptToEvaluateOnNewDocument', { identifier: preload }).catch(() => {});
  socket?.close(); chrome?.kill();
  for (const job of pending.values()) clearTimeout(job.timer);
  if (chrome) await new Promise(resolve => chrome.exitCode !== null ? resolve() : chrome.once('exit', resolve));
  if (profile) await rm(profile, { recursive: true, force: true, maxRetries: 8, retryDelay: 250 });
}
