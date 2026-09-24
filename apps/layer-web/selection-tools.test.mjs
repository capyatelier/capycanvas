import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {checkPaintableSelections} from './selection-masks.test.mjs';

// Run on the dedicated test origin: canvas contacts exercise the real GPU queue.
export async function checkSelectionTools({call, evaluate, settle}) {
  const wait = expression => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const send = async action => {
    await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await settle();
    await new Promise(resolve=>setTimeout(resolve,350));
  };
  const invoke = command => send({type:'invoke',command});
  const workspace = async command => {
    await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(command))});null`);
    await wait('!JSON.parse(layerApp.app.workspace_view()).busy && !JSON.parse(layerApp.app.workspace_view()).dirty');
    await settle();
  };
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  await evaluate('navigator.wakeLock?.request("screen").then(lock=>{window.selectionTestWake=lock}).catch(()=>{})');
  const original = await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  const theme = await evaluate('layerApp.state().settings.theme ?? null');
  await workspace({type:'switch',id:'builtin:workspace:painter'});
  await workspace({type:'form',kind:'new'});
  await workspace({type:'submit',name:`Selection test ${Date.now()}`});
  const created = await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  assert.notEqual(created,'builtin:workspace:painter');
  const point = async (p, device='pen', type='mousePressed') => {
    await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:device,force:.7});
    await settle();
  };
  const contact = async (selector, device='mouse') => {
    const p = await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    if(device==='touch') {
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...p}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    } else {await point(p,device);await point(p,device,'mouseReleased');}
    await settle(); await new Promise(resolve=>setTimeout(resolve,350));
  };
  const headers = await evaluate('layerApp.state().workspace.layout.header.zones.flat()');
  const header = command => `[data-header-item="${headers.find(e=>e.item.control?.command===command).id}"] .header-tool`;
  const mode = id => `.content-drawer [data-tool-action="${id}"]`;
  const selected = id => evaluate(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(id)}).selected`);
  const modes = ['selection_new','selection_add','selection_subtract','selection_intersect'];
  const tools = ['rectangle_select','ellipse_select','lasso','polygon_select','auto_select','color_select','selection_brush'];
  try {
    await invoke('select'); await contact(header('select'));
    if(!await evaluate('!!layerApp.state().customization.drawer'))await contact(header('select'));
    assert.deepEqual(await evaluate('layerApp.state().customization.drawer.columns'),[['tools'],['tool_settings']]);
    const choices = await evaluate('layerApp.state().tool_set.subtools');
    assert.equal(choices.length,7);
    await evaluate('window.selectionDrawer=document.querySelector(".content-drawer")');
    for (const [i, choice] of choices.entries()) {
      const selector = `.content-drawer [data-tool-choice="${choice.label}"]`;
      assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().height>=44`));
      await contact(selector,['mouse','touch','pen'][i%3]);
      assert.ok(await evaluate('selectionDrawer===document.querySelector(".content-drawer")'));
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(header('select'))}).querySelector('svg').dataset.asset`),choice.icon);
      const brush=choice.icon==='selection-brush';
      for(const id of brush?['selection_add','selection_subtract']:modes) {
        await contact(mode(id),['mouse','touch','pen'][i%3]);
        assert.ok(await selected(id));
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(mode(id))}).textContent.trim()`),'');
      }
      assert.ok(await evaluate(`!!document.querySelector('[data-tool-setting=${brush?'selection_brush_size':'selection_feather'}]')`));
      if(!brush)assert.ok(await evaluate('!!document.querySelector("[data-tool-action=selection_antialias]")'));
      const tops=await evaluate('[...document.querySelectorAll(".selection-modes button")].map(n=>n.getBoundingClientRect().top)');
      assert.ok(tops.every(y=>Math.abs(y-tops[0])<1),'Modes fit one row');
    }
    await invoke('rectangle_select'); await contact(mode('selection_new')); await contact(mode('selection_fixed_size'));
    for(const id of ['selection_width','selection_height']) assert.ok(await evaluate(`!!document.querySelector('[data-tool-setting=${id}]')`));
    await contact(mode('selection_fixed_size'));
    await invoke('color_select');
    for(const id of ['tolerance','expansion','smoothing']) assert.ok(await evaluate(`!!document.querySelector('[data-tool-setting=${id}]')`));
    // A short native Tool viewport makes slider-started scrolling observable.
    await evaluate(`(()=>{const s=document.querySelector('.content-drawer [data-tool-setting=tolerance] .number-slider');window.sliderScroll=s.closest('.drawer-column');sliderScroll.style.maxHeight='170px';sliderScroll.style.overflowY='auto';})()`);
    for (const device of ['touch','pen']) {
      await send({type:'set_tool_setting',id:'tolerance',value:.1});
      const before=await evaluate('layerApp.state().tool_settings.find(s=>s.id==="tolerance").value');
      const start=await evaluate(`(()=>{sliderScroll.scrollTop=0;const s=sliderScroll.querySelector('[data-tool-setting=tolerance] .number-slider');s.scrollIntoView({block:'nearest'});const b=s.getBoundingClientRect();return{x:b.x+b.width*.7,y:b.y+b.height/2,top:sliderScroll.scrollTop};})()`);
      const move=async(type,p=start)=>{
        if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,x:p.x,y:p.y}]});
        else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],x:p.x,y:p.y,button:'left',buttons:type==='up'?0:1,pointerType:'pen',force:.7});
        await new Promise(r=>setTimeout(r,60));
      };
      await move('down'); await settle();
      assert.notEqual(await evaluate('layerApp.state().tool_settings.find(s=>s.id==="tolerance").value'),before,`${device} edits on press`);
      for(const dy of [20,45,80])await move('move',{x:start.x,y:start.y-dy});
      await move('up',{x:start.x,y:start.y-80});await settle();
      assert.equal(await evaluate('layerApp.state().tool_settings.find(s=>s.id==="tolerance").value'),before,`${device} restores the slider when scrolling`);
      assert.ok(await evaluate('sliderScroll.scrollTop')>start.top+10,`${device} pans from the slider`);
      await evaluate(`sliderScroll.scrollTop=${start.top}`);await settle();
      await move('down');await move('move',{x:start.x+20,y:start.y});await move('up',{x:start.x+20,y:start.y});await settle();
      assert.notEqual(await evaluate('layerApp.state().tool_settings.find(s=>s.id==="tolerance").value'),before,`${device} keeps horizontal edits`);
    }
    await evaluate("sliderScroll.style.maxHeight='';sliderScroll.style.overflowY='';sliderScroll.scrollTop=0;delete window.sliderScroll");
    for(const id of ['selection_visible','selection_editing','selection_reference']) assert.ok(await evaluate(`!!document.querySelector('[data-tool-action=${id}]')`));
    await mkdir('artifacts/selection-web',{recursive:true});
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});
      const shot=await call('Page.captureScreenshot',{format:'png'});
      await writeFile(`artifacts/selection-web/select-${theme}.png`,Buffer.from(shot.data,'base64'));
    }
    await contact(header('drawing_brush'),'pen');
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(header('select'))}).querySelector('svg').dataset.asset`),'color-select');
    await contact(header('select'),'touch'); assert.ok(await selected('color_select'));
    await contact(header('select')); // close the drawer before canvas contacts
    await invoke('fit_canvas');
    const center=await evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()');
    const at=(x,y)=>({x:center.x+x,y:center.y+y});
    for(const command of tools) {
      if(await evaluate('layerApp.state().layer_tools.has_selection')) await invoke('deselect');
      await invoke(command);
      if(command==='selection_brush')await invoke('selection_add');
      await point(at(-60,-50));
      if(command==='polygon_select') {
        await point(at(-60,-50),'pen','mouseReleased');
        for(const p of [at(60,-50),at(60,50)]) {await point(p);await point(p,'pen','mouseReleased');}
        await invoke('complete_selection');
      } else {
        if(!['auto_select','color_select'].includes(command)) {
          for(const p of [at(60,-50),at(60,50),at(-60,50)]) await point(p,'pen','mouseMoved');
        }
        await point(command==='lasso'?at(-60,50):['auto_select','color_select'].includes(command)?at(-60,-50):at(60,50),'pen','mouseReleased');
      }
      await wait('layerApp.state().layer_tools.has_selection');
      await invoke('undo'); assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),false);
      await invoke('redo'); assert.equal(await evaluate('layerApp.state().layer_tools.has_selection'),true);
    }
    await invoke('deselect');
    await checkPaintableSelections({call,evaluate,settle,send,invoke,point,at});
    await workspace({type:'switch',id:'builtin:workspace:photographer'});
    const commands=await evaluate('layerApp.state().commands');
    assert.ok(tools.every(id=>commands.some(c=>c.id===id)));
    const layout=await evaluate('JSON.stringify(layerApp.state().workspace.layout)');
    for(const id of tools.filter(id=>id!=='selection_brush')) assert.ok(layout.includes(`"${id}"`),`Photo toolbar ${id}`);
    console.log('PASS selection drawers, seven tools/options, mode icons, mouse/touch/pen, remembered icon, GPU gestures, undo/redo, Photo defaults');
  } catch(error) {
    await mkdir('artifacts/selection-web',{recursive:true});
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile('artifacts/selection-web/failure.png',Buffer.from(shot.data,'base64'));
    console.error(error);
    console.error('Selection state',await evaluate('JSON.stringify({tool:layerApp.state().layer_tools,drawer:layerApp.state().customization.drawer},(_,v)=>typeof v==="bigint"?String(v):v)'));
    throw error;
  } finally {
    await send({type:'set_theme',theme});
    await workspace({type:'switch',id:original});
    await workspace({type:'form',kind:'delete',id:created});
    await workspace({type:'submit',name:''});
    await evaluate('delete window.selectionDrawer;window.selectionTestWake?.release();delete window.selectionTestWake');
  }
}
