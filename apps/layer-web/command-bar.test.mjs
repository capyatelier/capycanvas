import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

export async function checkCommandBar({call, evaluate, settle}) {
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  const key = async (key, code, windowsVirtualKeyCode, modifiers=0) => {
    await call('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode,modifiers});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode,modifiers:0});
  };
  const open = async () => {
    for (let attempt = 0; attempt < 10; attempt++) {
      await wait(`layerApp.state().commands.some(c=>c.id==='search_commands'&&c.enabled)`);
      await key('k','KeyK',75,2);
      if (await evaluate(`new Promise(resolve=>{const end=performance.now()+1000;(function check(){if(document.querySelector('#command-bar').open||performance.now()>end)resolve(document.querySelector('#command-bar').open);else setTimeout(check,30);})();})`)) return;
    }
    assert.fail('Primary+K opens command search once the canvas is idle');
  };
  const query = text => evaluate(`(()=>{const e=document.querySelector('#command-search');e.value=${JSON.stringify(text)};e.dispatchEvent(new Event('input',{bubbles:true}));return null;})()`);
  const detail = async () => {
    const [text, shared] = await evaluate(`[document.querySelector('#command-detail').textContent,layerApp.state().command_search.detail]`);
    assert.equal(text, shared);
    return text;
  };
  const style = await evaluate('layerApp.app.catalog().command_search_style');
  const top = height => Math.min(style.top_max, Math.max(style.top_min, height / 5));
  const settled = async () => { await wait(`!document.querySelector('#command-bar').getAnimations().length`); await settle(); };
  const bounds = () => evaluate(`document.querySelector('#command-bar').getBoundingClientRect().toJSON()`);
  const capture = async name => {
    await settle(); await new Promise(r=>setTimeout(r,150));
    const {data}=await call('Page.captureScreenshot',{format:'png'});
    if (process.env.LAYER_TEST_ARTIFACTS) {
      await mkdir(process.env.LAYER_TEST_ARTIFACTS,{recursive:true});
      await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/${name}.png`,Buffer.from(data,'base64'));
    }
    return data;
  };
  for (let i=0;i<3;i++) {
    await open();
    assert.equal(await evaluate('document.activeElement.id'),'command-search');
    await call('Input.insertText',{text:'pencil'});
    await key('Enter','Enter',13);
    assert.equal(await evaluate('layerApp.state().brush.tool'),'pencil');
    assert.equal(await evaluate('document.querySelector("#command-bar").open'),false);
  }
  await evaluate(`layerApp.dispatch({type:'set_theme',theme:'light'}); null`);
  await open(); await query('undo');
  assert.equal(await evaluate('layerApp.state().command_search.results[0].id'),'command.undo');
  assert.equal(await detail(),'Nothing to undo');
  assert.equal(await evaluate('layerApp.state().command_search.results[0].disabled_reason'),'Nothing to undo');
  await capture('command-bar-web-light');
  await query('brush size');
  assert.equal(await evaluate('layerApp.state().command_search.results[0].id'),'tool_setting.size');
  assert.match(await detail(),/Current .*Range /);
  assert.equal(await detail(),await evaluate('layerApp.state().command_search.results[0].description'));
  await key('Enter','Enter',13);
  assert.equal(await evaluate('layerApp.state().command_search.parameter.id'),'tool_setting.size');
  assert.equal(await detail(),await evaluate('layerApp.state().command_search.parameter.description'));
  assert.equal(await evaluate('document.querySelector(".command-unit").textContent'),'px');
  await query('24'); await key('Enter','Enter',13);
  assert.equal(await evaluate('layerApp.state().brush.diameter'),24);
  await evaluate(`layerApp.dispatch({type:'set_theme',theme:'dark'}); null`);
  await open(); await query('select');
  await key('ArrowDown','ArrowDown',40);
  assert.equal(await evaluate('Number(layerApp.state().command_search.selected)'),1);
  assert.equal(await evaluate('document.querySelector("#command-search").getAttribute("aria-activedescendant")'),'command-result-1');
  await capture('command-bar-web-dark');
  await key('Escape','Escape',27);
  await open();
  const before = await evaluate('String(layerApp.state().document_file.revision)');
  await call('Input.dispatchMouseEvent',{type:'mousePressed',x:720,y:800,button:'left',clickCount:1});
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:720,y:800,button:'left',clickCount:1});
  assert.equal(await evaluate('document.querySelector("#command-bar").open'),false);
  assert.equal(await evaluate('String(layerApp.state().document_file.revision)'),before);
  // Query changes must leave the editor's retained controls alone.
  await open();
  const timings = await evaluate(`(async()=>{
    const panel=document.querySelector('.dock-group'),samples=[];
    for(let i=0;i<20;i++){
      const start=performance.now(),e=document.querySelector('#command-search');
      e.value=['select','undo','brush size','pencil'][i%4];e.dispatchEvent(new Event('input',{bubbles:true}));
      await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));samples.push(performance.now()-start);
      if(panel!==document.querySelector('.dock-group'))throw Error('Search rebuilt workspace controls');
    }
    return samples.sort((a,b)=>a-b);
  })()`);
  console.log('Web query to two animation frames p95 (ms):',timings[18]);
  await key('Escape','Escape',27);
  await call('Emulation.setDeviceMetricsOverride',{width:420,height:820,deviceScaleFactor:1,mobile:true});
  await call('Emulation.setTouchEmulationEnabled',{enabled:true,maxTouchPoints:5});
  await open(); await query('eraser');
  assert.equal(await evaluate('getComputedStyle(document.querySelector("#command-bar")).top'),'16px');
  await capture('command-bar-web-touch');
  const point = await evaluate('(()=>{const r=document.querySelector("#command-result-0").getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()');
  await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...point}]});
  await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await wait('!document.querySelector("#command-bar").open');
  assert.equal(await evaluate('layerApp.state().brush.tool'),'eraser');
  await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  for (const height of [700, 1000]) {
    await call('Emulation.setDeviceMetricsOverride',{width:1440,height,deviceScaleFactor:1,mobile:false});
    await open(); await settled();
    assert.equal((await bounds()).y, top(height), `the bar sits a fifth down a ${height}px viewport`);
    await key('Escape','Escape',27);
  }
  console.log('Web command search keyboard, numeric, focus, touch and dismissal passed');
  await checkGlass({call, evaluate, settle, key, open, query, wait, settled, bounds, capture, style});
}

async function checkGlass({call, evaluate, settle, key, open, query, wait, settled, bounds, capture, style}) {
  const send = action => evaluate(`layerApp.dispatch(${JSON.stringify(action)}); null`);
  const [width, height] = await evaluate('[innerWidth, innerHeight]');
  await send({type:'invoke',command:'fit_canvas'});
  for (let i=0;i<3;i++) await send({type:'invoke',command:'zoom_in'});
  await open(); await query('select'); await settled();
  const full = await bounds();
  await key('Escape','Escape',27);
  await send({type:'select_brush',id:1});
  await send({type:'set_color',rgba:[0,0,0,1]});
  await send({type:'set_brush_size',value:6 / await evaluate('layerApp.state().camera.zoom')});
  for (let i=0;i<=54;i++) {
    const x = width / 2 - 351 + 13 * i;
    for (const [type, y] of [['mousePressed', full.y - 90], ['mouseMoved', full.y + full.height / 2], ['mouseMoved', full.y + full.height + 60], ['mouseReleased', full.y + full.height + 60]])
      await call('Input.dispatchMouseEvent',{type,x,y,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
  }
  await settle();
  await evaluate("window.glassProbe={set:layerApp.app.set_glass,boxes:[]};layerApp.app.set_glass=(boxes,...rest)=>{glassProbe.boxes=Array.from(boxes);return glassProbe.set.call(layerApp.app,boxes,...rest);};null");
  const region = async r => (await evaluate(`(()=>{const c=layerApp.canvas.getBoundingClientRect(),b=glassProbe.boxes;return Array.from({length:b.length/9},(_,i)=>b.slice(i*9,i*9+9)).map(g=>[g[0]+c.x,g[1]+c.y,...g.slice(2)]);})()`))
    .find(g => [r.x, r.y, r.width, r.height].every((v, i) => Math.abs(v - g[i]) < .5));
  const rows = (data, ys, [left, right]) => evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();
    const canvas=new OffscreenCanvas(image.width,image.height),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);
    return ${JSON.stringify(ys.map(Math.round))}.map(y=>{const d=context.getImageData(${Math.round(left)},y,${Math.round(right-left)},1).data,row=[];
      for(let i=0;i<d.length;i+=4)row.push([d[i]/255,d[i+1]/255,d[i+2]/255]);return row;});})()`);
  const mean = row => [0,1,2].map(i => row.reduce((sum, p) => sum + p[i], 0) / row.length);
  const luma = p => .2126 * p[0] + .7152 * p[1] + .0722 * p[2];
  const sharpness = row => row.slice(1).reduce((sum, p, i) => sum + Math.abs(luma(p) - luma(row[i])), 0) / (row.length - 1);
  const close = (a, b, tolerance) => a.every((v, i) => Math.abs(v - b[i]) <= tolerance);
  for (const [theme, level] of [['dark','off'],['dark','low'],['dark','high'],['light','medium'],['light','high'],['light','off']]) {
    const name = `command-bar-glass-${theme}-${level}`;
    await send({type:'set_theme',theme});
    await send({type:'preferences',action:{type:'edit',id:'transparency',value:['off','low','medium','high'].indexOf(level)}});
    await open(); await query('select'); await settled();
    const body = await bounds(), span = [body.x + 16, body.x + body.width - 16];
    const colors = await evaluate(`(()=>{const p=layerApp.state().palette,probe=document.body.appendChild(document.createElement('i'));
      const css=c=>{probe.style.background=c;return getComputedStyle(probe).backgroundColor;},g=p.glass.panel;
      const r={glass:css('rgb('+g.slice(0,3).map(v=>v*255).join(' ')+' / '+g[3]+')'),panel:css(p.panel),input:css(p.input),
        bar:getComputedStyle(document.querySelector('#command-bar')).backgroundColor,field:getComputedStyle(document.querySelector('.command-search-field')).backgroundColor,
        radius:getComputedStyle(document.querySelector('#command-bar')).borderTopLeftRadius};probe.remove();return r;})()`);
    assert.equal(colors.bar, colors.glass, `${name}: the bar uses the panel glass fill`);
    assert.equal(colors.field, colors.input, `${name}: the search field stays opaque`);
    assert.equal(colors.radius, `${style.radius}px`);
    if (level === 'off') {
      assert.equal(colors.bar, colors.panel, `${name}: Off is the opaque panel color`);
      assert.equal(await region(body), undefined, `${name}: Off publishes no glass`);
    } else assert.deepEqual((await region(body))?.slice(4), [...Array(4).fill(style.radius), 0], `${name}: the open bar publishes a round glass region`);
    const [behind, ...inside] = await rows(await capture(name), [body.y - 60, body.y + 6, body.y + body.height - 6], span);
    assert.ok(sharpness(behind) > .05, `${name}: stripes surround the bar`);
    const glass = await evaluate('layerApp.state().palette.glass.panel');
    const expected = mean(behind).map((v, i) => glass[i] * glass[3] + v * (1 - glass[3]));
    for (const row of inside) {
      assert.ok(close(mean(row), expected, .04), `${name}: ${mean(row)} vs ${expected}`);
      assert.ok(sharpness(row) < .01, `${name}: glass must blur, not show, the stripes (${sharpness(row)})`);
    }
    await query('zzzz'); await wait('layerApp.state().command_search.results.length===0'); await settled();
    const shrunk = await bounds();
    assert.ok(shrunk.height + 60 < body.height);
    if (level !== 'off') {
      assert.ok(await region(shrunk), `${name}: the region follows the resized bar`);
      assert.equal(await region(body), undefined, `${name}: the resized bar leaves no stale region`);
    }
    const [resized, vacated] = await rows(await capture(`${name}-empty`), [shrunk.y + shrunk.height - 6, body.y + body.height - 6], span);
    assert.ok(close(mean(resized), expected, .04), `${name}: resized bar keeps its glass`);
    assert.ok(sharpness(vacated) > .05, `${name}: no stale blur below the resized bar`);
    await key('Escape','Escape',27);
    await wait('!document.querySelector("#command-bar").open');
    const closed = await rows(await capture(`${name}-closed`), [body.y + 6, shrunk.y + shrunk.height - 6], span);
    assert.equal(await region(shrunk), undefined, `${name}: closing removes the region`);
    for (const row of closed) assert.ok(sharpness(row) > .05, `${name}: no stale blur after closing`);
  }
  await evaluate('layerApp.app.set_glass=glassProbe.set;delete window.glassProbe;null');
  console.log(`Web command bar glass passed at ${width}x${height}`);
}
