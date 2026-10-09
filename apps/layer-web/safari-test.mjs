import assert from 'node:assert/strict';
import {checkContactBrushes} from './contact-brushes.test.mjs';

const driver = process.env.SAFARIDRIVER_URL || 'http://127.0.0.1:4444';
async function request(path, body, method = 'POST') {
  const response = await fetch(driver + path, {
    method, headers: {'Content-Type': 'application/json'},
    ...(body === undefined ? {} : {body: JSON.stringify(body)}),
  });
  const {value} = await response.json();
  if (!response.ok || value?.error) throw Error(JSON.stringify(value));
  return value;
}
const {sessionId} = await request('/session', {capabilities: {alwaysMatch: {browserName: 'safari'}}});
const session = (path, body, method) => request(`/session/${sessionId}${path}`, body, method);
async function evaluate(expression) {
  const result = await session('/execute/async', {
    script: `const done=arguments[arguments.length-1];
      Promise.resolve().then(()=>eval(${JSON.stringify(expression)}))
        .then(value=>done({value:value??null}),error=>done({failure:String(error)}));`,
    args: [],
  });
  if (result.failure) throw Error(result.failure);
  return result.value;
}
const settle = () => evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(null))))');
async function call(method, parameters) {
  if (method === 'Input.dispatchMouseEvent') {
    if (parameters.type === 'mousePressed') {
      await session('/window', {handle: await session('/window', undefined, 'GET')});
    }
    const actions = [{type: 'pointerMove', origin: 'viewport',
      x: Math.round(parameters.x), y: Math.round(parameters.y), duration: 0}];
    if (parameters.type === 'mousePressed') actions.push({type: 'pointerDown', button: 0});
    if (parameters.type === 'mouseReleased') actions.push({type: 'pointerUp', button: 0});
    await session('/actions', {actions: [{type: 'pointer', id: 'brush',
      parameters: {pointerType: 'mouse'}, actions}]});
    return {};
  }
  if (method === 'Page.captureScreenshot') {
    const screenshot = await session('/screenshot', undefined, 'GET');
    const data = await evaluate(`(async()=>{
      const image=new Image();image.src='data:image/png;base64,${screenshot}';await image.decode();
      const clip=${JSON.stringify(parameters.clip)},scale=image.width/innerWidth;
      const canvas=document.createElement('canvas');
      canvas.width=Math.round(clip.width);canvas.height=Math.round(clip.height);
      canvas.getContext('2d').drawImage(image,clip.x*scale,clip.y*scale,
        clip.width*scale,clip.height*scale,0,0,canvas.width,canvas.height);
      return canvas.toDataURL().split(',')[1];
    })()`);
    return {data};
  }
  throw Error(`Unsupported Safari command: ${method}`);
}

try {
  await session('/timeouts', {script: 150000});
  await session('/window/rect', {width: 1440, height: 1000});
  await session('/url', {url: process.env.LAYER_WEB_URL || 'http://127.0.0.1:4173'});
  await evaluate(`new Promise((resolve,reject)=>{
    const end=performance.now()+120000;
    function poll(){
      if(document.body.dataset.gpu==='unavailable')return reject(Error(document.body.innerText));
      if(window.layerApp?.app.startup_progress().every(Boolean))return resolve();
      if(performance.now()>end)return reject(Error('Safari startup timed out'));
      setTimeout(poll,50);
    }poll();
  })`);
  await evaluate(`window.safariGpuErrors=[];
    layerApp.canvas.getContext('webgpu').getConfiguration().device
      .addEventListener('uncapturederror',event=>safariGpuErrors.push(event.error.message));`);
  process.env.LAYER_BRUSH_PRESETS ||= '1,2,3,5';
  const directory = process.env.LAYER_TEST_ARTIFACTS || 'artifacts/contact-brushes/safari';
  for (const theme of ['light', 'dark']) {
    process.env.LAYER_TEST_ARTIFACTS = `${directory}/${theme}`;
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
    await checkContactBrushes({call, evaluate, settle});
    assert.deepEqual(await evaluate('safariGpuErrors'), []);
    console.log(`Safari ${theme}: startup, visible strokes and Undo/Redo passed`);
  }
} finally {
  await session('', undefined, 'DELETE');
}
