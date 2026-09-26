import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Production DOM, shared sampling and GPU loupe. CDP preserves pointer kinds;
// device performance additionally uses Android's OS stylus injector.
export async function checkColorPicker({call,evaluate,settle}) {
  const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/color-picker/web';await mkdir(output,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const key=async(key,code,windowsVirtualKeyCode)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode});await settle();};
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+15000;function check(){if(${expression})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,20)}check()})`);
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const shot=async name=>{await new Promise(r=>setTimeout(r,350));const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${output}/${name}.png`,Buffer.from(png.data,'base64'));};
  const mouse=async(type,point,device='pen',buttons=0)=>call('Input.dispatchMouseEvent',{type,...point,pointerType:device,button:type==='mouseMoved'?'none':'left',buttons,clickCount:1,force:buttons?.7:0});
  const tap=async(point,device='mouse')=>{await mouse('mousePressed',point,device,1);await mouse('mouseReleased',point,device);};
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const buttonPoint=async selector=>{const b=await rect(selector);return{x:b.x+b.width/2,y:b.y+b.height/2};};
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:painter'}));null`);
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:painter'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await settle();
  await invoke('brush');await invoke('fit_canvas');
  await key('i','KeyI',73);assert.equal((await state()).layer_tools.tool,'pick_visible');
  await key('Escape','Escape',27);assert.equal((await state()).layer_tools.tool,'paint');
  const brush=(await state()).brush.preset;
  await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Alt',code:'AltLeft',windowsVirtualKeyCode:18,modifiers:1});await settle();
  assert.equal((await state()).layer_tools.tool,'pick_visible');
  await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Alt',code:'AltLeft',windowsVirtualKeyCode:18});await settle();
  assert.equal((await state()).layer_tools.tool,'paint');assert.equal((await state()).brush.preset,brush);
  const picker='button[aria-label="Color Picker"]';
  const tile=await buttonPoint(picker);
  const order=await evaluate(`(()=>{const layout=layerApp.state().workspace.layout;return layout.panels.find(p=>p.content.tiles?.some(t=>t.control.kind==='brush_size_slider')).content.tiles.map(t=>t.control.kind==='command'?t.control.command:t.control.kind)})()`);
  assert.deepEqual(order,['brush_size_slider','color_picker','brush_opacity_slider','undo','redo']);
  await tap(tile);await settle();assert.equal((await state()).layer_tools.tool,'pick_visible');
  await tap(tile);await settle();
  await wait('layerApp.state().customization.drawer?.compact');
  assert.deepEqual((await state()).customization.drawer.columns,[['tool_settings']]);
  assert.equal((await state()).customization.drawer.dismissal,'explicit');
  const sizes=await evaluate(`[...document.querySelector('[data-picker-setting="Sample size"]').options].map(o=>Number(o.value))`);
  assert.deepEqual(sizes,[1,5,15,51,101]);
  await evaluate(`(()=>{const n=document.querySelector('[data-picker-setting="Sample size"]');n.value='101';n.dispatchEvent(new Event('change',{bubbles:true}));})()`);await settle();
  assert.equal((await state()).color_picker.sample_width,101);
  await shot('sketch-settings');
  await invoke('eyedropper');await send({type:'set_color_sample_size',width:1});
  const point=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
  await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:60});await send({type:'set_color',rgba:[1,0,0,1]});
  await mouse('mousePressed',{x:point.x-12,y:point.y},'pen',1);await mouse('mouseMoved',point,'pen',1);await mouse('mouseMoved',{x:point.x+12,y:point.y},'pen',1);await mouse('mouseReleased',point);await settle();
  await send({type:'set_color',rgba:[0,1,0,1]});const original=(await state()).colors;
  await invoke('eyedropper');await mouse('mouseMoved',point);await wait('layerApp.state().color_picker.preview!=null');
  assert.deepEqual((await state()).colors,original,'hover is reversible');
  assert.ok((await state()).color_picker.preview.rgba[0]>.8&&(await state()).color_picker.preview.rgba[1]<.2,'sample real red paint');
  await shot('glass-hover');
  await mouse('mousePressed',point,'pen',1);await settle();assert.deepEqual((await state()).colors,original,'pen down does not accept');
  await mouse('mouseReleased',point);await wait('layerApp.state().layer_tools.tool==="paint"');
  assert.ok((await state()).colors.foreground.rgba[0]>.8,'pen lift accepts');
  await send({type:'set_color',rgba:[0,1,0,1]});
  const contact={x:point.x,y:point.y+44};
  await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...contact}]});
  await wait('layerApp.state().color_picker.preview!=null');
  assert.ok((await state()).color_picker.preview.rgba[0]>.8&&(await state()).color_picker.preview.rgba[1]<.2,'touch samples crosshair above the finger');
  await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...contact},{id:2,x:contact.x+100,y:contact.y}]});await settle();
  assert.equal((await state()).color_picker.layer,true,'second finger toggles source');await shot('glass-layer-touch');
  await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[{id:1,...contact}]});
  await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await wait('layerApp.state().layer_tools.tool==="paint"');
  await invoke('eyedropper');await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...point}]});await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
  assert.equal((await state()).layer_tools.tool,'paint','finger tap cancels button picker');
  // Begin a palm contact before pen/mouse toolbar activation. Wait beyond the
  // pending hold, then release/cancel without first touching the canvas by pen.
  const camera=async()=>(await state()).camera;
  for(const ending of ['touchEnd','touchCancel']) {
    const palm={id:10,x:point.x-100,y:point.y};
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[palm]});
    await tap(await buttonPoint(picker),'pen');
    await new Promise(r=>setTimeout(r,650));
    await mouse('mouseMoved',point,'pen');await settle();
    assert.ok((await state()).layer_tools.tool.startsWith('pick_'),'toolbar picker stays active through old hold');
    await call('Input.dispatchTouchEvent',{type:ending,touchPoints:[]});
    await key('Escape','Escape',27);
    const before=await camera();
    for(let id=11;id<13;id++) {
      const p={id,x:point.x+40,y:point.y};
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[p]});
      await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...p,x:p.x+60,y:p.y+50}]});
      await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
      assert.deepEqual(await camera(),before,'one fresh finger cannot navigate around a released palm');
    }
    const pair=[palm,{id:13,x:point.x+40,y:point.y}];
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:pair});
    await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[palm,{...pair[1],x:point.x+90,y:point.y+50}]});
    await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
    const after=await camera();assert.notEqual(after.zoom,before.zoom);assert.notEqual(after.rotation,before.rotation);
    await invoke('fit_canvas');
  }
  await send({type:'color_picker',action:{kind:'source',layer:false}});
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:illustrator'}));null`);
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:illustrator'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await settle();
  await send({type:'move_panel',panel:'color',target:{kind:'float',position:[30,70]}});
  assert.ok(await evaluate(`[...document.querySelectorAll('.color-wheel-control')].some(n=>n.getBoundingClientRect().width>128)`),'visible color panel');
  await invoke('eyedropper');await mouse('mouseMoved',point);await wait('layerApp.state().color_picker.preview!=null');
  await evaluate(`window.pickerCounts={fields:0,models:0};for(const [method,key] of [['color_field_pixels','fields'],['state_update','models']]){const old=layerApp.app[method].bind(layerApp.app);layerApp.app[method]=(...args)=>{pickerCounts[key]++;return old(...args)}}`);
  for(let i=0;i<45;i++){await mouse('mouseMoved',{x:point.x-70+i*3,y:point.y});await new Promise(r=>setTimeout(r,8));}
  await settle();assert.deepEqual(await evaluate('pickerCounts'),{fields:0,models:0},'hover never synchronously rasterizes fields or rebuilds workspace');
  await shot('wheel-preview');
  await invoke('eyedropper');
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:illustrator'}));null`);
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:illustrator'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await settle();
  const category=await buttonPoint('button[data-command="eyedropper"]');await tap(category);await settle();await tap(category);await settle();
  assert.deepEqual((await state()).customization.drawer.columns,[['brushes'],['tool_settings']]);
  await send({type:'color_picker',action:{kind:'style',style:'eyedropper'}});await shot('paint-eyedropper-options');
  await invoke('eyedropper');
  console.log('PASS: picker tiles, settings, real sampling, pen lift, touch offset/source/cancel, retained asynchronous wheel preview and category drawer');
}
