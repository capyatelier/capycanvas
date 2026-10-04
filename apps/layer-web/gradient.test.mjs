import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {readPackage,packageOccurrences} from './package-fixture.test.mjs';
import {scopesFixture} from './scopes-fixture.mjs';

export async function checkGradientSurfaceContracts({evaluate,settle,selectors,preparePreviewFixture}) {
  for(const key of ['toolPanel','toolbarContext','editor','preview'])assert.ok(selectors?.[key],`${key} selector must come from the approved Web port`);
  assert.equal(typeof preparePreviewFixture,'function','The existing host setup must prepare the actual document depth and opaque gradient preview');
  const read=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().tool_extra.find(option=>option.Gradient)?.Gradient,(_,v)=>typeof v==="bigint"?Number(v):v))');
  const definition={interpolation:'Oklab',stops:[{position:0,color:{space:'Srgb',rgba:[.13,.27,.44,1]}},{position:1,color:{space:'Srgb',rgba:[.78,.59,.31,1]}}]};
  const pixels=()=>evaluate(`(()=>{const canvas=document.querySelector(${JSON.stringify(selectors.preview)});if(!(canvas instanceof HTMLCanvasElement))throw Error('Approved preview selector must identify its painted canvas');return{width:canvas.width,height:canvas.height,pixels:Array.from(canvas.getContext('2d',{willReadFrequently:true}).getImageData(0,0,canvas.width,canvas.height).data)}})()`);
  const rowVariance=({width,height,pixels})=>{
    assert.ok(width>=3&&height>=2,'The actual preview must have interior columns and multiple rows');
    let varies=false;
    for(let y=1;y<height;y++)for(let x=1;x<width-1;x++)for(let c=0;c<3;c++)if(pixels[(y*width+x)*4+c]!==pixels[x*4+c])varies=true;
    return varies;
  };
  for(const depth of ['U8','F32']) {
    await preparePreviewFixture({depth,definition});await settle();
    const control=await read();assert.equal(control.gradient.destination.kind,'tool');
    const canonical=value=>({...value,stops:value.stops.map(stop=>({...stop,position:Math.fround(stop.position),color:{...stop.color,rgba:stop.color.rgba.map(Math.fround)}}))});
    assert.deepEqual(canonical(control.value.value),canonical(definition),'Preview preparation preserves the authored float definition');
    assert.equal(Object.hasOwn(control.value.value,'dither'),false);
    assert.equal(Object.hasOwn(control.gradient,'shape'),false);assert.equal(Object.hasOwn(control.gradient,'shapes'),false);
    for(const root of [selectors.toolPanel,selectors.toolbarContext]) {
      const editors=await evaluate(`(()=>{const root=document.querySelector(${JSON.stringify(root)});return root?[...root.querySelectorAll(${JSON.stringify(selectors.editor)})].map(node=>({visible:node.getClientRects().length>0,text:node.textContent})):[]})()`);
      assert.ok(editors.some(editor=>editor.visible),'Tool panel and contextual toolbar expose the shared gradient editor');
      for(const [,label] of control.gradient.interpolations)assert.ok(editors.some(editor=>editor.text.includes(label)),`Shared interpolation label ${label} is presented`);
    }
    const actual=await pixels();assert.equal(rowVariance(actual),depth==='U8','Integer preview dithers across rows; float preview leaves each column unchanged');
    assert.deepEqual(await pixels(),actual,'Always-on preview dithering is deterministic');
    assert.deepEqual((await read()).value.value,control.value.value,'Preview rendering never changes authored float stops');
  }
  const constant={interpolation:'Oklab',stops:definition.stops.map(stop=>({...stop,color:definition.stops[0].color}))};
  await preparePreviewFixture({depth:'U8',definition:constant});await settle();
  assert.equal(rowVariance(await pixels()),false,'A constant integer gradient has no dither noise');
}

export async function checkGradientDefinitions({call,evaluate,settle}) {
  const fixture=await scopesFixture({call,evaluate,settle});
  const directory=process.env.LAYER_TEST_ARTIFACTS??'/tmp/capy-gradients-web';await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=command=>send({type:'invoke',command});
  const key=async(key,code=key,windowsVirtualKeyCode)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode});await settle();};
  const click=async selector=>{const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});const x=r.x+r.width/2,y=r.y+r.height/2,hit=document.elementFromPoint(x,y);if(!hit||!(n===hit||n.contains(hit)))throw Error('Obstructed '+${JSON.stringify(selector)});return{x,y}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const select=async(selector,index)=>{await click(selector);await key('Home','Home',36);for(let i=0;i<index;i++)await key('ArrowDown','ArrowDown',40);await key('Enter','Enter',13);};
  const typeValue=async(selector,text,finish='Enter')=>{await click(selector);for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});await call('Input.insertText',{text});await key(finish,finish,finish==='Tab'?9:finish==='Escape'?27:13);};
  const capture=async name=>{const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(data,'base64'));};
  const phase=process.env.LAYER_GRADIENT_PHASE;
  let tool=false;
  const read=()=>evaluate(`JSON.parse(JSON.stringify(${tool?"layerApp.state().tool_extra.find(option=>option.Gradient)?.Gradient":"layerApp.state().layer_properties.controls.find(c=>c.gradient)"},(_,v)=>typeof v==='bigint'?Number(v):v))`);
  const edit=async change=>{
    const control=await read();
    assert.ok(control?.gradient?.destination,'The shared editor publishes its current owner');
    await send({type:'effect',action:{op:'gradient',target:control.gradient.destination,edit:change}});
  };
  const definition=async()=> (await read()).value.value;
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}+' '+JSON.stringify({file:layerApp.state().document_file,error:layerApp.state().host_error,text:document.body.innerText.slice(-1000)},(_,v)=>typeof v==='bigint'?String(v):v)));else setTimeout(check,25);}check();})`);
  const archive=async expected=>{
    await evaluate(`window.gradientFiles={open:window.showOpenFilePicker,save:window.showSaveFilePicker};
      window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(v){gradientFiles.bytes=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){},async abort(){}}}});
      window.showOpenFilePicker=async()=>[{name:'gradient.capy',async getFile(){return new File([gradientFiles.bytes],'gradient.capy')}}];`);
    try {
      const selected=(await read()).gradient.destination.layer;
      const rows=await evaluate('JSON.parse(JSON.stringify(layerApp.state().layers,(_,v)=>typeof v==="bigint"?Number(v):v))');
      await invoke('save_document_as');await wait('!layerApp.state().document_file.busy&&!layerApp.state().document_file.modified');
      const saved=await readPackage(evaluate,'gradientFiles.bytes'),occurrences=packageOccurrences(saved);
      const target=occurrences[rows.findIndex(row=>row.id===selected)].id;
      const epoch=await evaluate('String(layerApp.state().document_file.epoch)');
      await invoke('open_document');await wait(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(epoch)}`);await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
      const reopenedRows=await evaluate('JSON.parse(JSON.stringify(layerApp.state().layers,(_,v)=>typeof v==="bigint"?Number(v):v))');
      await send({type:'layer',action:{op:'select',id:reopenedRows[occurrences.findIndex(row=>row.id===target)].id,mask:false}});
      await wait('layerApp.state().layer_properties.controls.some(c=>c.gradient)');
      assert.deepEqual(await definition(),expected,'Native archive reopening preserves HDR, alpha, stops and interpolation');
    } finally {await evaluate('window.showOpenFilePicker=gradientFiles.open;window.showSaveFilePicker=gradientFiles.save;delete window.gradientFiles');}
  };
  const pointer=async(device,phase,p)=>{
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd',cancel:'touchCancel'}[phase],touchPoints:['up','cancel'].includes(phase)?[]:[{id:71,...p,radiusX:1,radiusY:1,force:.65}]});
    else if(phase==='cancel')await evaluate(`document.querySelector('.gradient-editor').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,pointerType:${JSON.stringify(device)},bubbles:true}))`);
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[phase],...p,button:'left',buttons:phase==='up'?0:1,clickCount:1,pointerType:device});
    await settle();
  };
  const toolToolbar=async()=>{
    const tiles=await evaluate('layerApp.state().workspace.layout.panels.find(p=>p.id==="commands").content.tiles.map(t=>t.id)');
    for(const tile of tiles)await send({type:'customize',action:{type:'remove_tool',panel:'commands',tile}});
    await send({type:'customize',action:{type:'insert_tools',panel:'commands',before:null}});await send({type:'customize',action:{type:'picker_select',control:{kind:'tool_options',style:{text:true,sliders:true}},selected:true}});await send({type:'customize',action:{type:'confirm_tools'}});
    await send({type:'move_panel',panel:'commands',target:{kind:'edge',edge:'top',outer:true}});
  };
  const toolLayout=async()=>{
    for(const panel of ['properties','layers','histogram','waveform','navigator','color','palettes','tools'])await send({type:'customize',action:{type:'set_panel_visible',panel,visible:false}});
    await send({type:'customize',action:{type:'set_panel_visible',panel:'tool_settings',visible:true}});
    await send({type:'move_panel',panel:'tool_settings',target:{kind:'edge',edge:'right',outer:false}});await invoke('fit_canvas');
  };
  const measureGradientMotion=async()=>{
  const motion=[];
  await edit({kind:'stop',index:null,position:.5,color:null,remove:false});
  for(const kind of ['stop','geometry'])for(const device of ['mouse','touch','pen']) {
    const revision=await evaluate('String(layerApp.state().document_file.revision)'),before=await definition(),p=await evaluate(kind==='stop'?`(()=>{const n=document.querySelector('[data-control=tool_settings] [data-gradient-stop="1"]'),r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`:`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.4)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.4)*r.height/c.viewport[1]}})()`);
    if(kind==='geometry')assert.equal(await evaluate(`document.elementFromPoint(${p.x},${p.y})===layerApp.canvas`),true,'Measured geometry contact hits the actual canvas');
    await evaluate(`window.gradientMotion={frames:[],previews:[],frame:layerApp.app.frame,color:layerApp.app.color_ui,put:CanvasRenderingContext2D.prototype.putImageData};
      layerApp.app.frame=function(...args){const start=performance.now();try{return gradientMotion.frame.apply(this,args)}finally{gradientMotion.frames.push([start,performance.now()-start])}};
      layerApp.app.color_ui=function(request){const start=performance.now(),result=gradientMotion.color.call(this,request);if(request.type==='gradient'){const end=performance.now();gradientMotion.pending={start,end,size:request.image.size,depth:request.image.depth};}return result;};
      CanvasRenderingContext2D.prototype.putImageData=function(...args){const start=performance.now();try{return gradientMotion.put.apply(this,args)}finally{if(this.canvas.matches('canvas.gradient-preview')&&gradientMotion.pending){const q=gradientMotion.pending;gradientMotion.previews.push({...q,projection_ms:q.end-q.start,conversion_ms:start-q.end,put_ms:performance.now()-start});gradientMotion.pending=null;}}};`);
    const contact=(phase,point)=>device==='touch'?call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[phase],touchPoints:phase==='up'?[]:[{id:88,...point}]}):call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[phase],...point,button:'left',buttons:phase==='up'?0:1,clickCount:1,pointerType:device});
    try {
      await contact('down',p);await evaluate('gradientMotion.begin=performance.now()');
      const started=performance.now(),pending=[];let inputs=0;
      while(performance.now()-started<5000){pending.push(contact('move',{x:p.x+(kind==='stop'?30:20)*Math.sin((performance.now()-started)/250),y:p.y}));inputs++;await new Promise(resolve=>setTimeout(resolve,4));}
      await Promise.all(pending);await evaluate('gradientMotion.end=performance.now()');await contact('up',{x:p.x+20,y:p.y});await settle();
      if(kind==='stop')assert.notDeepEqual(await definition(),before,'Native active stop motion commits a changed position');
      else assert.deepEqual(await definition(),before,'Geometry motion preserves the tool gradient definition');
      if(kind==='stop'||device==='touch')assert.equal(await evaluate('String(layerApp.state().document_file.revision)'),revision,'Tool setting and touch navigation motion do not edit the artwork');
      else assert.notEqual(await evaluate('String(layerApp.state().document_file.revision)'),revision,'Native geometry motion commits painted artwork');
      motion.push({kind,device,inputs,...await evaluate('({begin:gradientMotion.begin,end:gradientMotion.end,frames:gradientMotion.frames.filter(f=>f[0]>=gradientMotion.begin&&f[0]<=gradientMotion.end),previews:gradientMotion.previews.filter(p=>p.start>=gradientMotion.begin&&p.end<=gradientMotion.end)})')});
      if(kind==='stop')await edit({kind:'position',index:1,operation:{type:'value',value:before.stops[1].position}});
      else if(device==='touch')await invoke('fit_canvas');else await invoke('undo');
      assert.deepEqual(await definition(),before,'Restoring the measured motion preserves the other tool settings');
    } finally {await evaluate('layerApp.app.frame=gradientMotion.frame;layerApp.app.color_ui=gradientMotion.color;CanvasRenderingContext2D.prototype.putImageData=gradientMotion.put;delete window.gradientMotion');}
  }
  await writeFile(`${directory}/motion.json`,JSON.stringify({reference_hardware:false,frame_fields:['start_ms','cpu_ms'],runs:motion},null,2));
  };
  let measured=false;
  await wait('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  if(phase!=='preview')for(const width of process.env.LAYER_GRADIENT_WIDTH?[Number(process.env.LAYER_GRADIENT_WIDTH)]:[640,1100])for(const theme of process.env.LAYER_GRADIENT_THEME?[process.env.LAYER_GRADIENT_THEME]:['light','dark']) {
    await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:photographer'}));null`);
    await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:photographer'&&!JSON.parse(layerApp.app.workspace_view()).busy`);
    await invoke('fit_canvas');
    await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();
    await send({type:'move_panel',panel:'properties',target:{kind:'edge',edge:'right',outer:false},viewport:[width,800]});
    const propertiesBounds=await evaluate(`(()=>{const r=document.querySelector('.dock-group[data-panel=properties]').getBoundingClientRect();return {x:r.x,width:r.width,height:r.height}})()`);assert.ok(propertiesBounds.width>=200,JSON.stringify(propertiesBounds));
    await send({type:'set_theme',theme});
    tool=false;await invoke('hand');
    for(const effect of ['gradient_map','gradient_fill']) {
      await send({type:'effect',action:{op:'insert',effect}});
      let control=await read();
      assert.equal(control.gradient.destination.kind,'effect');
      assert.equal(control.gradient.destination.layer,await evaluate('Number(layerApp.state().layer_properties.layer)'));
      assert.equal(control.gradient.destination.epoch,await evaluate('Number(layerApp.state().layer_properties.epoch)'));
      assert.deepEqual(control.gradient.interpolations.map(([value])=>value),['Oklab','LinearRgb','Classic']);
      assert.ok(control.gradient.interpolations.every(([,label])=>label.length>0));
      assert.equal(Object.hasOwn(control.value.value,'dither'),false);
      assert.equal(Object.hasOwn(control.gradient,'shape'),false);assert.equal(Object.hasOwn(control.gradient,'shapes'),false);
      const editor='.dock-group[data-panel=properties] .gradient-editor';
      assert.ok(await evaluate(`!!document.querySelector(${JSON.stringify(editor+' .gradient-preview')})`),'Properties presents the common canvas editor');
      if(effect==='gradient_fill'){
        assert.equal(await evaluate('layerApp.state().layer_properties.controls[0].key'),'style','Fill Shape is the first shared control');
        const bounds=await evaluate(`(()=>{const root=document.querySelector('.dock-group[data-panel=properties]'),shape=root.querySelector('[data-property-key=style]'),editor=root.querySelector('.gradient-editor');return{shape:shape.getBoundingClientRect().toJSON(),editor:editor.getBoundingClientRect().toJSON(),visible:shape.checkVisibility()}})()`);
        assert.ok(bounds.visible&&bounds.shape.height>=24&&bounds.shape.bottom<=bounds.editor.top+1,'The actual Fill Shape control is visible above the gradient editor');
      }
      for(const [index,[value]] of control.gradient.interpolations.entries()) {
        await select(editor+' [data-gradient-interpolation]',index);
        assert.equal((await definition()).interpolation,value);
      }
      const color={space:'Srgb',rgba:[.25,2,.125,.375]};
      await edit({kind:'stop',index:null,position:.5,color,remove:false});
      const authored=await definition();
      await click(editor+' [data-gradient-action=reverse]');assert.deepEqual((await definition()).stops.map(stop=>stop.color),authored.stops.map(stop=>stop.color).reverse());
      await click(editor+' [data-gradient-action=reverse]');
      const exact=await definition();
      assert.deepEqual(exact.stops[1].color,color,'Authored HDR and alpha survive in the shared definition');
      await click(editor+' .gradient-stops button:nth-child(2)');
      await select(editor+' [data-gradient-interpolation]',1);
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(editor+' .gradient-stops button.selected')}).dataset.gradientStop`),'1','Interpolation retains the selected interior stop');
      await invoke('undo');assert.deepEqual(await definition(),exact);
      await click(editor+' [data-gradient-position] .number-value');
      await call('Input.dispatchKeyEvent',{type:'keyDown',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});await call('Input.dispatchKeyEvent',{type:'keyUp',key:'a',code:'KeyA',windowsVirtualKeyCode:65,modifiers:2});
      await call('Input.insertText',{text:'47.1 %'});await key('Enter','Enter',13);
      const numeric=await definition();
      if(Math.abs(numeric.stops[1].position-.471)>=1e-7){await capture(`numeric-failure-${width}-${theme}`);console.log('Gradient numeric state',numeric,await evaluate(`document.querySelector(${JSON.stringify(editor+' [data-gradient-position]')}).outerHTML`));}
      assert.ok(Math.abs(numeric.stops[1].position-.471)<1e-7,'Native numeric stop entry uses the shared percent parser');
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(editor+' .gradient-stops button.selected')}).dataset.gradientStop`),'1','Numeric commit retains the selected interior stop');
      await invoke('undo');assert.deepEqual(await definition(),exact,'One Undo restores numeric stop commit');
      await click(editor+' .gradient-stops button:nth-child(2)');
      for(let i=0;i<4;i++)await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39,autoRepeat:i>0});
      await settle();const held=await definition();assert.notDeepEqual(held,exact,'Held native arrow moves selected stop');
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowRight',code:'ArrowRight',windowsVirtualKeyCode:39});await settle();assert.deepEqual(await definition(),held,'Native arrow release preserves the last admitted stop position');
      await invoke('undo');assert.deepEqual(await definition(),exact,'Held native arrow is one Undo');
      await archive(exact);
      await evaluate('layerApp.restartGpu()');await wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()');await settle();assert.deepEqual(await definition(),exact,'Renderer recreation preserves gradient stops');
      await capture(`${effect}-${width}-${theme}`);
      for(const device of ['mouse','touch','pen']) {
        const at=await evaluate(`(()=>{const n=document.querySelectorAll('.dock-group[data-panel=properties] .gradient-stops button')[1];n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2,width:n.closest('.gradient-strip').querySelector('.gradient-ramp').getBoundingClientRect().width}})()`);
        const end={x:at.x+12,y:at.y},release={x:at.x+16,y:at.y};
        await pointer(device,'down',at);await pointer(device,'move',end);if(device==='touch')await pointer(device,'move',release);await pointer(device,'up',release);
        const moved=await definition();
        assert.notDeepEqual(moved,exact,`${device} native stop drag reaches the shared editor`);
        assert.ok(Math.abs(moved.stops[1].position-Math.fround(exact.stops[1].position+16/at.width))<1e-7,`${device} release adopts the final native coordinate without restoring stale published position: ${JSON.stringify({actual:moved.stops[1].position,expected:Math.fround(exact.stops[1].position+16/at.width),at})}`);
        await invoke('undo');assert.deepEqual(await definition(),exact,'One contact creates one undo step');
        await invoke('redo');assert.deepEqual(await definition(),moved);
        await invoke('undo');
        await pointer(device,'down',at);await pointer(device,'move',end);
        if(device==='touch')await pointer(device,'cancel',end);
        else {
          await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
          await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
          await pointer(device,'up',end);
        }
        assert.deepEqual(await definition(),exact,'Cancelled native contact restores exact authored stops');
      }
      for(let i=1;i<40;i++)await edit({kind:'stop',index:null,position:i/41,color:null,remove:false});
      control=await read();assert.equal(control.value.value.stops.length,32);assert.equal(control.gradient.can_add,false);
      await edit({kind:'reset'});assert.equal((await definition()).stops.length,2);assert.equal((await read()).gradient.can_add,true);
    }
    await invoke('add_layer');
    await toolToolbar();
    const propertiesBefore=await evaluate('JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v==="bigint"?String(v):v)');
    await invoke('gradient');tool=true;
    assert.equal(await evaluate('JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v==="bigint"?String(v):v)'),propertiesBefore,'Tool gradient publication preserves the selected layer Properties');
    const control=await read();assert.equal(control.gradient.destination.kind,'tool');
    assert.equal(Object.hasOwn(control.gradient,'shape'),false);assert.equal(Object.hasOwn(control.gradient,'shapes'),false);
    const choices=await evaluate('JSON.parse(JSON.stringify(layerApp.state().tool_extra,(_,v)=>typeof v==="bigint"?Number(v):v))');
    assert.equal(choices[0].Choice.id,'gradient-shape');assert.equal(choices[0].Choice.items.length,3,'Photo Gradient exposes all three shape variants');assert.ok(choices[1].Gradient);
    await toolLayout();
    assert.equal(await evaluate(`document.querySelector('[data-control=tool_settings]')?.checkVisibility()`),true,'The actual Tool Settings panel is visible');
    for(const [index,item] of choices[0].Choice.items.entries()) {
      assert.equal(item.enabled,true);await click(`[data-control=tool_settings] [data-toolbar-segment=gradient-shape-${index}]`);
      const selected=await evaluate('JSON.parse(JSON.stringify(layerApp.state().tool_extra[0].Choice.items.filter(item=>item.selected),(_,v)=>typeof v==="bigint"?Number(v):v))');
      assert.equal(selected.length,1);assert.deepEqual(selected[0].action,item.action);
    }
    const geometry=await evaluate(`(()=>{const root=document.querySelector('[data-control=tool_settings]'),editor=root.querySelector('.gradient-editor'),shape=[...root.querySelectorAll('[data-toolbar-choice]')].find(n=>n.dataset.toolbarChoice==='gradient-shape');return{root:root.getBoundingClientRect().toJSON(),editor:editor.getBoundingClientRect().toJSON(),shape:shape?.getBoundingClientRect().toJSON(),segments:[...shape.querySelectorAll('button')].map(b=>({button:b.getBoundingClientRect().toJSON(),icon:b.querySelector('svg')?.getBoundingClientRect().toJSON(),visible:b.checkVisibility(),hit:b.contains(document.elementFromPoint(b.getBoundingClientRect().x+b.getBoundingClientRect().width/2,b.getBoundingClientRect().y+b.getBoundingClientRect().height/2))})),html:root.innerHTML}})()`);
    assert.ok(geometry.shape&&geometry.shape.bottom<=geometry.editor.top+1,'Tool Settings shape is above the gradient editor');
    assert.equal(geometry.segments.length,choices[0].Choice.items.length);
    for(const segment of geometry.segments)assert.ok(segment.visible&&segment.hit&&segment.button.height>=24&&segment.icon?.width>=12&&segment.icon.height>=12&&segment.button.top>=geometry.root.top&&segment.button.bottom<=geometry.root.bottom&&segment.button.left>=geometry.root.left&&segment.button.right<=geometry.root.right,JSON.stringify(geometry));
    await capture(`tool-${width}-${theme}`);
    await click('[data-toolbar-component=tool_options] [data-toolbar-choice=tool] > button');await click('.toolbar-choice-menu button:nth-child(3)');
    await click('.toolbar-gradient > button');
    assert.equal(await evaluate(`!!document.querySelector('.toolbar-editor-popover:popover-open .gradient-editor')`),true,'Toolbar popup presents the common gradient editor');
    assert.equal(await evaluate(`!!document.querySelector('.toolbar-editor-popover:popover-open [data-toolbar-choice]')`),false,'Toolbar shape remains outside the gradient popup');
    const popupOriginal=await definition();await click('.toolbar-editor-popover:popover-open [data-gradient-action=reverse]');assert.deepEqual((await definition()).stops.map(stop=>stop.color),popupOriginal.stops.map(stop=>stop.color).reverse());await click('.toolbar-editor-popover:popover-open [data-gradient-action=reverse]');assert.deepEqual(await definition(),popupOriginal);
    await capture(`toolbar-popup-${width}-${theme}`);
    await key('Tab','Tab',9);await key('Tab','Tab',9);
    assert.equal(await evaluate(`document.activeElement===document.querySelector('.toolbar-editor-popover:popover-open .gradient-strip')`),true,'Native Tab reaches the idle gradient strip');
    await key('Escape','Escape',27);
    const popupStillOpen=await evaluate(`!!document.querySelector('.toolbar-editor-popover:popover-open')`);
    if(popupStillOpen)console.log('Gradient popup Escape',await evaluate(`({active:document.activeElement.outerHTML,popup:document.querySelector('.toolbar-editor-popover:popover-open').outerHTML,tool:layerApp.state().layer_tools.tool})`));
    assert.equal(popupStillOpen,false,'Native Escape dismisses the gradient toolbar popup');
    const unreversed=await definition();
    await edit({kind:'reverse'});
    assert.deepEqual((await definition()).stops.map(stop=>stop.color),unreversed.stops.map(stop=>stop.color).reverse(),'Reverse mutates stop order rather than an effect parameter');
    assert.equal(Object.hasOwn((await read()).gradient,'reverse'),false);
    for(const [slot,color] of [['background',{space:'ProPhoto',rgba:[2,.125,.5,.375]}],['foreground',{space:'DisplayP3',rgba:[.125,2,.5,.625]}]]) {
      await send({type:'color',action:{op:'set_slot',slot,color}});
      await send({type:'color',action:{op:'select',slot}});
      const selected=await evaluate(`JSON.parse(JSON.stringify(layerApp.state().colors[${JSON.stringify(slot)}]))`);
      await click('[data-control=tool_settings] [data-gradient-action=use-color]');
      assert.deepEqual((await definition()).stops[0].color,selected,'Use Current Color uses the selected paint definition with its HDR/profile/alpha');
    }
    const retained=await definition();
    await invoke('hand');
    assert.equal(await evaluate('layerApp.state().tool_extra.some(option=>option.Gradient)'),false,'Switching tools retires gradient tool options');
    await invoke('gradient');
    assert.equal(await evaluate('layerApp.state().tool_extra.some(option=>option.Gradient)'),true);
    assert.deepEqual(await definition(),retained);
    const region=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.3)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.4)*r.height/c.viewport[1],dx:Math.min(80,a[2]*r.width/c.viewport[0]*.3)}})()`);
    for(const device of ['mouse','touch','pen']) {
      const revision=()=>evaluate('String(layerApp.state().document_file.revision)');
      assert.equal(await evaluate(`document.elementFromPoint(${region.x},${region.y})===layerApp.canvas`),true,'Gradient geometry contact hits the actual canvas');
      const before=await revision(),start={x:region.x,y:region.y},end={x:region.x+region.dx,y:region.y+10};
      const contact=async phase=>{
        const p=phase==='down'?start:end;
        if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd',cancel:'touchCancel'}[phase],touchPoints:['up','cancel'].includes(phase)?[]:[{id:72,...p,radiusX:1,radiusY:1,force:.65}]});
        else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[phase],...p,button:'left',buttons:phase==='up'?0:1,clickCount:1,pointerType:device});
        await settle();
      };
      await contact('down');await contact('move');await contact('up');
      if(device==='touch'){assert.equal(await revision(),before,'A finger navigates without painting a gradient');await invoke('fit_canvas');continue;}
      assert.notEqual(await revision(),before,`${device} native geometry contact commits the reflected tool`);
      await invoke('undo');assert.deepEqual(await definition(),retained,'Undoing painted geometry preserves retained tool settings');
      await invoke('redo');await invoke('undo');
      await contact('down');await contact('move');
      const draft=await revision();
      if(device==='touch')await contact('cancel');
      else {
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await contact('up');
      }
      assert.equal(await revision(),draft,'Cancelling geometry cannot commit a document operation');
    }
    if(phase!=='ui'&&!measured){await measureGradientMotion();measured=true;}
  }
  if(phase==='preview'){
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:photographer'}));null`);
    await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:photographer'&&!JSON.parse(layerApp.app.workspace_view()).busy`);
    await toolToolbar();
  }
  if(phase!=='ui'&&phase!=='motion')await checkGradientSurfaceContracts({evaluate,settle,selectors:{toolPanel:'[data-control=tool_settings]',toolbarContext:'.toolbar-editor-popover:popover-open',editor:'.gradient-editor',preview:'.toolbar-editor-popover:popover-open .gradient-preview'},preparePreviewFixture:async({depth,definition:expected})=>{
    const epoch=await evaluate('String(layerApp.state().document_file.epoch)');
    await invoke('new_document');await wait(`!!document.querySelector('.document-dialog [data-document-field=depth]')`);
    for(const field of ['width','height'])await typeValue(`.document-dialog [data-document-field=${field}]`,'64');
    const index=await evaluate(`[...document.querySelector('.document-dialog [data-document-field=depth]').options].findIndex(option=>option.value===${JSON.stringify(depth)})`);assert.ok(index>=0);
    await select('.document-dialog [data-document-field=depth]',index);await click('.document-dialog [data-document-action=create]');
    await wait(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(epoch)}&&!layerApp.documents.busy()&&layerApp.app.brush_ready()&&layerApp.app.document_color().depth===${JSON.stringify(depth)}`);
    await invoke('gradient');tool=true;await toolLayout();await edit({kind:'reset'});await edit({kind:'interpolation',value:expected.interpolation});
    for(const [index,stop] of expected.stops.entries())await edit({kind:'stop',index,position:stop.position,color:stop.color,remove:false});
    if(depth==='F32') {
      const before=await definition();await click('[data-control=tool_settings] .property-color');await wait(`!!document.querySelector('.color-dialog[open]')`);
      await typeValue('.color-dialog [data-color-field="3"]','37.5','Tab');
      await typeValue('.color-dialog .color-entry-group > .color-entry input:not([data-color-field])','4','Tab');
      await click('.color-dialog footer .suggested-action');await wait(`!document.querySelector('.color-dialog[open]')`);
      const changed=await definition();assert.equal(changed.stops[0].color.rgba[3],.375);assert.ok(changed.stops[0].color.rgba.slice(0,3).some(value=>value>1),'Native color entry preserves HDR above one');
      await click('[data-control=tool_settings] .property-color');await wait(`!!document.querySelector('.color-dialog[open]')`);await typeValue('.color-dialog [data-color-field="3"]','12.5','Tab');await click('.color-dialog footer button:first-child');
      assert.deepEqual(await definition(),changed,'Native color dialog Cancel preserves the previous HDR and alpha');
      await edit({kind:'stop',index:0,position:before.stops[0].position,color:before.stops[0].color,remove:false});assert.deepEqual(await definition(),before);
    }
    await click('.toolbar-gradient > button');await capture(`preview-${depth}`);
  }});
  await fixture.dispose();
}
