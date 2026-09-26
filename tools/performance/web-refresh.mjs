// Refresh timeline, including GPU API calls, main-thread jobs and recovery.
// Use a dedicated test origin; this reloads the selected tab. It never accepts,
// discards or clears recovery records. --probes opens/closes title-bar menus.
// --strokes replays and undoes test strokes on an empty dedicated drawing.
// LAYER_DEVICE_CDP=http://127.0.0.1:9247 LAYER_WEB_URL=http://127.0.0.1:4197/ \
//   node tools/performance/web-refresh.mjs
// --desktop-ui-only launches a temporary Chrome profile without WebGPU. This
// checks the harness/UI path, and is not a GPU or tablet performance result.
import { mkdir, writeFile } from 'node:fs/promises';
import { connectTab, launchChrome } from '../cdp.mjs';
import { installRefreshProbe } from './web-refresh-probe.js';
import { replayRefreshStrokes } from './web-refresh-strokes.mjs';

const uiOnly = process.argv.includes('--desktop-ui-only');
const software = process.argv.includes('--desktop-software');
const desktop = uiOnly || software;
const url = process.env.LAYER_WEB_URL || 'http://127.0.0.1:4197/';
const output = process.env.LAYER_TEST_ARTIFACTS || 'artifacts/web-refresh';
const duration = Number(process.env.LAYER_REFRESH_MS || 60000);
const runs = Number(process.env.LAYER_REFRESH_RUNS || 1);
if (!Number.isSafeInteger(duration) || duration < 1000 || !Number.isSafeInteger(runs) || runs < 1)
  throw Error('LAYER_REFRESH_MS and LAYER_REFRESH_RUNS must be positive integers (capture >= 1000 ms)');
await mkdir(output, { recursive: true });
const options = { timeout: duration + 30000 };
const cdp = desktop
  ? await launchChrome(['--headless=new', '--ozone-platform=headless',
    ...(uiOnly ? ['--disable-gpu'] : ['--use-angle=swiftshader', '--enable-unsafe-webgpu', '--enable-unsafe-swiftshader']),
    '--window-size=1440,1000'], options)
  : await connectTab(process.env.LAYER_DEVICE_CDP || 'http://127.0.0.1:9247', t => t.url === url, options);
const { call, evaluate, errors } = cdp;
let preload;
let tracing = false, profiling = false;
let strokeWork;
const strokeAbort = new AbortController();
try {
  if (desktop) await cdp.attachPage();
  for (const domain of ['Page', 'Runtime', 'Network']) await call(`${domain}.enable`);
  await call('Page.bringToFront');
  ({ identifier: preload } = await call('Page.addScriptToEvaluateOnNewDocument', {
    source: `(${installRefreshProbe.toString()})(${JSON.stringify({ duration, uiOnly, probes: process.argv.includes('--probes') })})`,
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
      const finished = cdp.once('Tracing.tracingComplete', duration + 30000);
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
  await evaluate('window.refreshTrace?.dispose(); undefined').catch(() => {});
  if (preload) await call('Page.removeScriptToEvaluateOnNewDocument', { identifier: preload }).catch(() => {});
  await cdp.close();
}
