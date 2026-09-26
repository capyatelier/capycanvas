import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

export async function checkCommandBar({call, evaluate, settle}) {
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  const key = async (key, code, windowsVirtualKeyCode, modifiers=0) => {
    await call('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode,modifiers});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode,modifiers:0});
  };
  const open = async () => { await key('k','KeyK',75,2); await wait(`document.querySelector('#command-bar').open`); };
  const query = text => evaluate(`(()=>{const e=document.querySelector('#command-search');e.value=${JSON.stringify(text)};e.dispatchEvent(new Event('input',{bubbles:true}));return null;})()`);
  const capture = async name => {
    if (!process.env.LAYER_TEST_ARTIFACTS) return;
    await settle(); await new Promise(r=>setTimeout(r,150));
    await mkdir(process.env.LAYER_TEST_ARTIFACTS,{recursive:true});
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${process.env.LAYER_TEST_ARTIFACTS}/${name}.png`,Buffer.from(shot.data,'base64'));
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
  await capture('command-bar-web-light');
  await query('brush size'); await key('Enter','Enter',13);
  assert.equal(await evaluate('layerApp.state().command_search.parameter.id'),'tool_setting.size');
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
  await capture('command-bar-web-touch');
  const point = await evaluate('(()=>{const r=document.querySelector("#command-result-0").getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()');
  await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...point}]});
  await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await wait('!document.querySelector("#command-bar").open');
  assert.equal(await evaluate('layerApp.state().brush.tool'),'eraser');
  await call('Emulation.setTouchEmulationEnabled',{enabled:false});
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  console.log('Web command search keyboard, numeric, focus, touch and dismissal passed');
}
