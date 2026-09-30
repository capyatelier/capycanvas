import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

export async function checkColorMixing({call, evaluate, settle}) {
  const directory = process.env.LAYER_TEST_ARTIFACTS ?? 'artifacts/color-mixing/web';
  await mkdir(directory, {recursive: true});
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
  const send = async action => {await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle(); await pause(200);};
  const point = async (p, device, type) => {
    await call('Input.dispatchMouseEvent', {type, ...p, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1, pointerType: device, force: .7});
    await settle();
  };
  const contact = async (selector, device) => {
    const p = await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if (device === 'touch') {
      await call('Input.dispatchTouchEvent', {type: 'touchStart', touchPoints: [{id: 1, ...p}]});
      await call('Input.dispatchTouchEvent', {type: 'touchEnd', touchPoints: []});
    } else {await point(p, device, 'mousePressed'); await point(p, device, 'mouseReleased');}
    await settle(); await pause(250);
  };
  const command = id => evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(id)})`);
  const option = id => `[data-tool-action="${id}"]`;
  const choices = ['color_mix_oklab', 'color_mix_linear', 'color_mix_classic'];
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  const theme = await evaluate('layerApp.state().settings.theme ?? null');
  try {
    await send({type: 'invoke', command: 'fit_canvas'});
    const center = await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
    const stroke = async (y, device) => {
      const revision = await evaluate('Number(layerApp.state().document_file.revision)');
      await point({x: center.x - 120, y: center.y + y}, device, 'mousePressed');
      for (let x = -100; x <= 120; x += 20) await point({x: center.x + x, y: center.y + y}, device, 'mouseMoved');
      await point({x: center.x + 120, y: center.y + y}, device, 'mouseReleased');
      await wait(`Number(layerApp.state().document_file.revision)>${revision}`);
    };
    await send({type: 'select_brush', id: 1});
    await send({type: 'set_brush_size', value: 60});
    for (const [y, rgba] of [[-30, [0.9, 0.05, 0.05, 1]], [30, [0.05, 0.3, 0.9, 1]]]) {
      await send({type: 'set_color', rgba});
      await stroke(y, 'pen');
    }
    await wait(`!!document.querySelector('[data-tool-setting="flow"]')`);
    assert.equal(await evaluate(`!!document.querySelector('[data-tool-action^="color_mix_"]')`), false, 'G-Pen does not mix paint');
    await send({type: 'select_brush', id: 24});
    await wait(`!!document.querySelector(${JSON.stringify(option('color_mix_oklab'))})`);
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('[data-tool-action^="color_mix_"]')].map(n=>n.dataset.toolAction)`), choices);
    assert.equal((await command('color_mix_oklab')).selected, true, 'mixing brushes default to Oklab');
    for (const [device, id] of [['mouse', 'color_mix_classic'], ['touch', 'color_mix_linear'], ['pen', 'color_mix_oklab']]) {
      await contact(option(id), device);
      for (const other of choices) {
        assert.equal((await command(other)).selected, other === id, `${device}: ${id}`);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(option(other))}).getAttribute('aria-pressed')`), String(other === id));
      }
      assert.match(await evaluate(`document.querySelector(${JSON.stringify(option(id))}).textContent`), /mixing$/);
    }
    for (const name of ['light', 'dark']) {
      await send({type: 'set_theme', theme: name});
      await pause(150);
      await writeFile(`${directory}/color-mixing-${name}.png`, Buffer.from((await call('Page.captureScreenshot', {format: 'png'})).data, 'base64'));
    }
    for (const [i, id] of choices.entries()) {
      await send({type: 'invoke', command: id});
      await stroke(-40 + i * 40, 'pen');
      assert.equal((await command(id)).selected, true, `${id} holds while painting`);
    }
    console.log('PASS color mixing: mouse, touch and pen choices, Oklab default, hidden for G-Pen, strokes in each space, light and dark');
  } catch (error) {
    await writeFile(`${directory}/failure.png`, Buffer.from((await call('Page.captureScreenshot', {format: 'png'})).data, 'base64'));
    throw error;
  } finally {
    if (theme) await send({type: 'set_theme', theme});
  }
}
