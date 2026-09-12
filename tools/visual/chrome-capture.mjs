// Deterministic shared-editor capture, using an isolated local Chrome profile.
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, extname, sep, dirname, basename } from 'node:path';
import { once } from 'node:events';
import assert from 'node:assert/strict';
import {captureToolbarFixture} from './toolbar-fixture.mjs';
import {captureWorkspaceTabs} from './workspace-tabs.mjs';
import {captureHeaderControls} from './header-controls.mjs';
import {captureControlColors} from './control-colors.mjs';
import {captureToolActions} from './tool-actions.mjs';
import {captureWindowsEditor} from './windows-editor.mjs';
const [widthArg='1200', heightArg='900', scaleArg='2', output='artifacts/ui/parity', theme='light', scenario='initial', fixturePath] = process.argv.slice(2);
const width = Number(widthArg), height = Number(heightArg), scale = Number(scaleArg);
assert(Number.isInteger(width) && width > 0 && Number.isInteger(height) && height > 0);
assert(Number.isFinite(scale) && scale > 0);
assert(['light', 'dark'].includes(theme));
assert(['initial', 'canvas-under-header', 'layer-added', 'filter-properties', 'panel-configuration', 'partial-zen', 'toolbar-tiles', 'workspace-tabs', 'header-controls', 'control-colors', 'tool-actions', 'windows-editor'].includes(scenario));
await mkdir(output, {recursive:true});
const root = resolve('apps/layer-web');
const server = createServer(async (req, res) => {
  try {
    const path = resolve(root, '.' + decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
    if (path !== root && !path.startsWith(root + sep)) { res.writeHead(403); res.end(); return; }
    const file = path === root ? join(root, 'index.html') : path;
    res.setHeader('Content-Type', ({'.html':'text/html', '.js':'text/javascript', '.wasm':'application/wasm', '.css':'text/css', '.svg':'image/svg+xml', '.png':'image/png', '.json':'application/json'})[extname(file)] || 'application/octet-stream');
    res.end(await readFile(file));
  } catch { res.writeHead(404); res.end(); }
});
server.listen(0, '127.0.0.1');
await once(server, 'listening');
const profileRoot = await realpath(tmpdir());
const profile = await mkdtemp(join(profileRoot, 'capy-parity-chrome-'));
const chrome = spawn(process.env.CAPY_CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', [
  '--headless=new', '--remote-debugging-pipe', `--user-data-dir=${profile}`,
  '--no-first-run', '--no-default-browser-check', '--force-color-profile=srgb',
  '--enable-gpu', '--enable-unsafe-webgpu', 'about:blank',
], {stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe']});
let seq = 0, buffer = '', session, stderr = '';
const requests = new Map(), errors = [];
chrome.stderr.on('data', data => { stderr += data.toString(); });
chrome.stdio[4].on('data', data => {
  buffer += data.toString();
  for (let end; (end = buffer.indexOf('\0')) >= 0;) {
    const event = JSON.parse(buffer.slice(0, end)); buffer = buffer.slice(end + 1);
    if (event.id) {
      const req = requests.get(event.id); if (!req) continue;
      requests.delete(event.id); clearTimeout(req.timer);
      event.error ? req.reject(new Error(JSON.stringify(event.error))) : req.resolve(event.result);
    } else if (event.method === 'Runtime.exceptionThrown') errors.push(event.params.exceptionDetails);
  }
});
function call(method, params = {}, sessionId = session) {
  return new Promise((resolve, reject) => {
    const id = ++seq, timer = setTimeout(() => { requests.delete(id); reject(new Error(`Timeout ${method}: ${stderr.slice(-2000)}`)); }, 30000);
    requests.set(id, {resolve, reject, timer});
    chrome.stdio[3].write(JSON.stringify({id, method, params, ...(sessionId ? {sessionId} : {})}) + '\0');
  });
}
async function evaluate(expression) {
  const r = await call('Runtime.evaluate', {expression, awaitPromise: true, returnByValue: true});
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
try {
  const target = await call('Target.createTarget', {url:'about:blank'}, null);
  session = (await call('Target.attachToTarget', {targetId:target.targetId, flatten:true}, null)).sessionId;
  await call('Runtime.enable'); await call('Page.enable');
  await call('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor:scale, mobile:false});
  await call('Emulation.setEmulatedMedia', {features:[{name:'prefers-reduced-motion', value:'reduce'}]});
  const component = ['toolbar-tiles', 'control-colors', 'tool-actions'].includes(scenario);
  await call('Page.navigate', {url:`http://127.0.0.1:${server.address().port}${component?'/workspace-chrome.js':''}`});
  if (component) {
    const capture = {'toolbar-tiles':captureToolbarFixture,'control-colors':captureControlColors,'tool-actions':captureToolActions}[scenario];
    await capture({fixture:JSON.parse(await readFile(fixturePath,'utf8')),width,height,scale,output,theme,evaluate,call});
    assert.deepEqual(errors,[]);
  } else {
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp&&document.body.dataset.gpu==='ready')resolve(true);else if(performance.now()-start>25000)reject(new Error(document.querySelector('#gpu-notice')?.textContent||'GPU startup timeout'));else setTimeout(check,100);}check();})`);
  if (scenario === 'windows-editor') {
    await captureWindowsEditor({manifest:JSON.parse(await readFile(fixturePath,'utf8')),output,evaluate,call});
    assert.deepEqual(errors,[]);
  } else if (scenario === 'header-controls') {
    await captureHeaderControls({manifest: JSON.parse(await readFile(fixturePath, 'utf8')), output, evaluate, call});
    assert.deepEqual(errors, []);
  } else if (scenario === 'workspace-tabs') {
    await captureWorkspaceTabs({fixture: JSON.parse(await readFile(fixturePath, 'utf8')), output, evaluate, call});
    assert.deepEqual(errors, []);
  } else {
  const captures = [];
  for (const selectedTheme of [theme]) {
    await evaluate(`layerApp.dispatch({type:'system_theme_changed',theme:'${selectedTheme}'}); layerApp.dispatch({type:'invoke',command:'fit_canvas'});`);
    if (scenario === 'canvas-under-header') await evaluate(`for(let i=0;i<4;i++) layerApp.dispatch({type:'invoke',command:'zoom_in'});`);
    if (scenario === 'layer-added') await evaluate(`layerApp.dispatch({type:'layer',action:{op:'new',group:false,clipped:false}});`);
    if (scenario === 'panel-configuration') await evaluate(`layerApp.dispatch({type:'customize',action:{type:'show_all_controls',panel:'sizes'}});`);
    if (scenario === 'partial-zen') await evaluate(`layerApp.dispatch({type:'invoke',command:'zen_mode'});`);
    if (scenario === 'filter-properties') await evaluate(`
      layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'adjustments',visible:true}});
      layerApp.dispatch({type:'effect',action:{op:'insert',effect:'gaussian_blur'}});
      layerApp.dispatch({type:'effect',action:{op:'set',layer:layerApp.state().layer_properties.layer,key:'sigma',value:{kind:'number',value:5}}});
      layerApp.dispatch({type:'effect',action:{op:'insert',effect:'curves'}});
      layerApp.dispatch({type:'effect',action:{op:'insert',effect:'gradient_map'}});
      layerApp.dispatch({type:'filter_picker',action:{op:'search',query:'Gradient Map'}});
    `);
    await evaluate(`document.fonts.ready.then(()=>Promise.all([...document.images].map(i=>i.decode())))`);
    // GPU attachment precedes staged compilation and thumbnail readback. A
    // fixed delay can capture empty previews and produce a misleading diff.
    await evaluate(`new Promise((resolve,reject)=>{
      const start=performance.now();
      function check(){
        const previews=[...document.querySelectorAll('.layer-thumbnail canvas')].filter(c=>{
          const r=c.getBoundingClientRect(); return r.width>0&&r.height>0&&r.bottom>0&&r.top<innerHeight;
        });
        const ready=previews.every(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.every((v,i)=>i%4!==3||v===255));
        if(layerApp.startupTimes.complete!==null&&ready) requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)));
        else if(performance.now()-start>25000) reject(new Error('Staged GPU startup or visible thumbnails did not settle'));
        else setTimeout(check,100);
      } check();
    })`);
    const shot = await call('Page.captureScreenshot', {format:'png', fromSurface:true});
    const png = Buffer.from(shot.data, 'base64');
    assert.equal(png.readUInt32BE(16), Math.round(width * scale)); assert.equal(png.readUInt32BE(20), Math.round(height * scale));
    const file = `${output}/web-${theme}-${width}x${height}@${scale}x${scenario==='initial'?'':'-'+scenario}.png`;
    await writeFile(file,png); captures.push(file);
  }
  const metrics = await evaluate(`(async()=>{const a=await navigator.gpu.requestAdapter();return {width:innerWidth,height:innerHeight,scale:devicePixelRatio,gpu:document.body.dataset.gpu,adapter:{vendor:a.info.vendor,architecture:a.info.architecture,isFallbackAdapter:a.info.isFallbackAdapter},canvas:[layerApp.canvas.width,layerApp.canvas.height]};})()`);
  assert.equal(metrics.adapter.isFallbackAdapter,false);
  assert.deepEqual(errors,[]);
  await writeFile(`${output}/chrome-capture.json`,JSON.stringify({scenario,metrics,captures,errors},null,2));
  console.log(JSON.stringify({scenario,metrics,captures,errors},null,2));
  }
  }
} catch (error) {
  if (errors.length) console.error(JSON.stringify({browserExceptions: errors}));
  throw error;
} finally {
  const exited = once(chrome,'exit');
  chrome.kill(); await exited;
  server.closeAllConnections(); await new Promise(resolve=>server.close(resolve));
  // Delete only the exact generated directory under the canonical temp root.
  // Refuse a redirected/replaced profile path, including Windows junctions.
  const cleanup = await realpath(profile);
  assert.equal(cleanup, profile);
  assert.equal(dirname(cleanup), profileRoot);
  assert.ok(basename(cleanup).startsWith('capy-parity-chrome-'));
  await rm(cleanup,{recursive:true,force:true});
}
