// Deterministic shared-editor capture, using an isolated local Chrome profile.
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve, extname, sep } from 'node:path';
import { once } from 'node:events';
import assert from 'node:assert/strict';
import { launchChrome } from '../cdp.mjs';
import {captureToolbarFixture} from './toolbar-fixture.mjs';
import {captureWorkspaceTabs} from './workspace-tabs.mjs';
import {captureHeaderControls} from './header-controls.mjs';
import {captureControlColors} from './control-colors.mjs';
import {captureToolActions} from './tool-actions.mjs';
import {captureWindowsEditor} from './windows-editor.mjs';
import {captureNumberControls} from './number-controls.mjs';
import {captureColorPanels} from './color-panel.mjs';
import {captureIcons} from './icons.mjs';
import {captureChoices} from './choices.mjs';
import {captureInlineNumbers} from './inline-numbers.mjs';
const [widthArg='1200', heightArg='900', scaleArg='2', output='artifacts/ui/parity', theme='light', scenario='initial', fixturePath] = process.argv.slice(2);
const width = Number(widthArg), height = Number(heightArg), scale = Number(scaleArg);
assert(Number.isInteger(width) && width > 0 && Number.isInteger(height) && height > 0);
assert(Number.isFinite(scale) && scale > 0);
assert(['light', 'dark'].includes(theme));
assert(['initial', 'canvas-under-header', 'paint-expanded', 'paint-canvas-under-header', 'sketch', 'photo', 'layer-added', 'filter-properties', 'panel-configuration', 'partial-zen', 'toolbar-tiles', 'workspace-tabs', 'header-controls', 'control-colors', 'tool-actions', 'windows-editor', 'number-controls', 'color-panel', 'icons', 'choices', 'inline-numbers'].includes(scenario));
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
const cdp = await launchChrome(['--headless=new', '--force-color-profile=srgb', '--enable-gpu', '--enable-unsafe-webgpu'],
  {executable: process.env.CAPY_CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const {call, evaluate, errors} = cdp;
try {
  await cdp.attachPage();
  await call('Runtime.enable'); await call('Page.enable');
  await call('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor:scale, mobile:false});
  await call('Emulation.setEmulatedMedia', {features:[{name:'prefers-reduced-motion', value:'reduce'}]});
  if (scenario === 'header-controls') {
    await call('Page.addScriptToEvaluateOnNewDocument', {source: `
      Object.defineProperty(navigator,'getBattery',{configurable:true,value:async()=>
        Object.assign(new EventTarget(),{level:0.85,charging:false})});
    `});
  }
  const component = ['toolbar-tiles', 'control-colors', 'tool-actions', 'number-controls', 'color-panel', 'icons', 'choices', 'inline-numbers'].includes(scenario);
  await call('Page.navigate', {url:`http://127.0.0.1:${server.address().port}${component?'/workspace-chrome.js':''}`});
  if (component) {
    if (scenario==='color-panel'||scenario==='icons'||scenario==='choices'||scenario==='inline-numbers') {
      const capture={'icons':captureIcons,'color-panel':captureColorPanels,'choices':captureChoices,'inline-numbers':captureInlineNumbers}[scenario];
      await capture({manifest:JSON.parse(await readFile(fixturePath,'utf8')),fixturePath,output,evaluate,call});
    } else {
      const capture = {'toolbar-tiles':captureToolbarFixture,'control-colors':captureControlColors,'tool-actions':captureToolActions,'number-controls':captureNumberControls}[scenario];
      await capture({fixture:JSON.parse(await readFile(fixturePath,'utf8')),width,height,scale,output,theme,evaluate,call});
    }
    assert.deepEqual(errors,[]);
  } else {
  await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function check(){if(window.layerApp&&document.body.dataset.gpu==='ready')resolve(true);else if(performance.now()-start>25000)reject(new Error(document.querySelector('#gpu-notice')?.textContent||'GPU startup timeout'));else setTimeout(check,100);}check();})`);
  // GPU attachment can precede the asynchronous workspace lease. Dispatching
  // theme/layout actions while the session is read-only creates a recovery
  // message and an invalid reference even if storage later finishes normally.
  await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();
    function check(){
      const workspace=JSON.parse(layerApp.app.workspace_view());
      if(workspace?.error)reject(new Error(workspace.error));
      else if(workspace?.ready&&!workspace.busy)resolve(true);
      else if(performance.now()-start>25000)reject(new Error('Workspace ownership did not settle'));
      else setTimeout(check,50);
    }check();
  })`);
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
  const preset = scenario.startsWith('paint') ? 'illustrator' : {sketch:'painter', photo:'photographer'}[scenario];
  if (preset) {
    // Native capture metadata records reserved space for OS window controls.
    // Apply it through the shared layout action; the canvas stays full size.
    if (fixturePath) {
      const {workspace_bottom=0} = JSON.parse(await readFile(fixturePath, 'utf8'));
      assert(Number.isFinite(workspace_bottom) && workspace_bottom >= 0 && workspace_bottom < height);
      await evaluate(`layerApp.dispatch({type:'measure_workspace_bottom',inset:${workspace_bottom}})`);
    }
    await evaluate(`layerApp.dispatch({type:'workspace_manager',command:{type:'switch',id:'builtin:workspace:${preset}'}})`);
    await evaluate(`new Promise((resolve,reject)=>{
      const start=performance.now();
      function check(){
        const workspace=JSON.parse(layerApp.app.workspace_view());
        if(workspace?.error)reject(new Error(workspace.error));
        else if(workspace?.id==='builtin:workspace:${preset}'&&workspace.ready&&!workspace.busy)resolve(true);
        else if(performance.now()-start>25000)reject(new Error('Workspace preset did not settle'));
        else setTimeout(check,50);
      }check();
    })`);
  }
  const captures = [];
  for (const selectedTheme of [theme]) {
    await evaluate(`layerApp.dispatch({type:'system_theme_changed',theme:'${selectedTheme}'}); layerApp.dispatch({type:'invoke',command:'fit_canvas'});`);
    if (scenario === 'canvas-under-header' || scenario === 'paint-canvas-under-header') await evaluate(`for(let i=0;i<4;i++) layerApp.dispatch({type:'invoke',command:'zoom_in'});`);
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
      let previous,stable=0;
      function check(){
        const previews=[...document.querySelectorAll('.layer-thumbnail canvas')].filter(c=>{
          const r=c.getBoundingClientRect(); return r.width>0&&r.height>0&&r.bottom>0&&r.top<innerHeight;
        });
        const ready=previews.every(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.every((v,i)=>i%4!==3||v===255));
        const state=layerApp.state();
        const signature=JSON.stringify([layerApp.app.layout(innerWidth,innerHeight),state.workspace.layout.measurements,state.camera],(_,v)=>typeof v==='bigint'?v.toString():v);
        stable=signature===previous?stable+1:0;previous=signature;
        const workspace=JSON.parse(layerApp.app.workspace_view());
        if(workspace?.error)reject(new Error(workspace.error));
        else if(layerApp.startupTimes.complete!==null&&ready&&stable>=3&&workspace?.ready&&!workspace.busy) requestAnimationFrame(()=>requestAnimationFrame(()=>resolve(true)));
        else if(performance.now()-start>25000) reject(new Error('Staged GPU startup or visible thumbnails did not settle'));
        else setTimeout(check,100);
      } check();
    })`);
    const health=await evaluate(`({workspace:JSON.parse(layerApp.app.workspace_view()),status:document.getElementById('status').textContent.trim()})`);
    assert.equal(health.workspace.ready,true);assert.equal(health.workspace.error,null);
    assert.equal(health.status,'','A capture with an application error/status message is not a settled reference');
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
  try { console.error(JSON.stringify(await evaluate("({gpu:document.body.dataset.gpu,notice:document.querySelector('#gpu-notice')?.textContent})"))); } catch {}
  if (errors.length) console.error(JSON.stringify({browserExceptions: errors}));
  throw error;
} finally {
  await cdp.close();
  server.closeAllConnections(); await new Promise(resolve=>server.close(resolve));
}
