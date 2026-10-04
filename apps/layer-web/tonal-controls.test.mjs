import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {scopesFixture} from './scopes-fixture.mjs';
import {packageOccurrences,packageObject} from './package-fixture.test.mjs';

export async function checkTonalControls({call,evaluate,settle}) {
  const fixture=await scopesFixture({call,evaluate,settle});const {checkpoint,documentPoint,visibleSamples}=fixture;
  try {
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:photographer'}));null`);await fixture.poll(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:photographer'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await fixture.invoke('fit_canvas');
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/tonal-controls-web';await mkdir(directory,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const view=()=>fixture.json('layerApp.state().layer_properties');
  let menuRecorded=false;
  const phase=process.env.LAYER_TONAL_PHASE;
  const enabled=name=>!phase||phase===name;
  const waitContact=async predicate=>{const end=Date.now()+30000;while(Date.now()<end){if(await evaluate(predicate))return;await settle();await new Promise(r=>setTimeout(r,100));}throw Error(`Contact did not settle: ${await evaluate('JSON.stringify({tool:layerApp.state().layer_tools.tool,properties:layerApp.state().layer_properties,error:layerApp.state().host_error})')}`);};
  const ready=async()=>{const end=Date.now()+120000;while(Date.now()<end){await settle();await evaluate('layerApp.app.wait_for_canvas()');if(await evaluate('layerApp.app.brush_ready()&&!layerApp.state().document_file.busy&&!layerApp.state().host_error&&(!layerApp.state().layer_properties.histogram||layerApp.state().tonal_histogram.data!=null)'))return;await new Promise(r=>setTimeout(r,100));}throw Error(await evaluate('JSON.stringify(layerApp.state().layer_properties)'));};
  const press=async selector=>{const p=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const action=async predicate=>{const index=(await view()).actions.findIndex(predicate);assert.ok(index>=0);if((await view()).actions[index].group){await press('[data-property-action-group=calibration]');const geometry=await evaluate(`(()=>{const n=document.querySelector('.toolbar-choice-menu:popover-open'),b=document.querySelector('[data-property-action-group=calibration]').getBoundingClientRect(),r=n.getBoundingClientRect();return{left:r.left,right:r.right,top:r.top,bottom:r.bottom,buttonBottom:b.bottom,width:innerWidth,height:innerHeight}})()`);assert.ok(geometry.left>=0&&geometry.right<=geometry.width&&geometry.top>=0&&geometry.bottom<=geometry.height,'Picker menu fits viewport');assert.ok(Math.abs(geometry.top-geometry.buttonBottom)<=8,'Picker menu anchors to its opener');if(!menuRecorded){const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/calibration-menu-open.png`,Buffer.from(png.data,'base64'));menuRecorded=true;}}await press(`[data-property-action="${index}"]`);assert.ok(await evaluate(`Array.from(document.querySelectorAll('.toolbar-choice-menu[popover]')).every(n=>!n.checkVisibility())`),'Closed sampler menus are not painted');};
  const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});};
  const preview=async previous=>{const end=Date.now()+30000;while(Date.now()<end){if(await evaluate(`layerApp.state().color_picker.preview!=null${previous?`&&JSON.stringify(layerApp.state().color_picker.preview)!==${JSON.stringify(previous)}`:''}`))return;await settle();await new Promise(r=>setTimeout(r,100));}throw Error(`Touch preview timeout: ${await evaluate('JSON.stringify(layerApp.state().color_picker)')}`);};
  const contact=async(device,cancel,drag=false,role=null)=>{
    const originalCurves=drag?JSON.stringify((await view()).controls.filter(c=>c.curve).map(c=>c.value)):null;
    const p=await documentPoint(role==='black'?[32,32]:role==='white'?[224,224]:[128,128]),end={x:p.x,y:p.y-30};
    if(device==='touch'){
      const start=drag?p:{x:p.x,y:p.y+60},finish=drag?end:{x:p.x,y:p.y+44};
      await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{id:1,...start}]});
      if(!drag){await preview();}
      const beforePreview=await evaluate('JSON.stringify(layerApp.state().color_picker.preview)');
      await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{id:1,...finish}]});await settle();
      if(!drag){await preview(beforePreview);assert.ok(await evaluate('layerApp.state().color_picker.preview.rgba.every(Number.isFinite)'),'Touch loupe samples real finite source pixels');}
      await call('Input.dispatchTouchEvent',{type:cancel?'touchCancel':'touchEnd',touchPoints:[]});
    }else{
      await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,pointerType:device,button:'left',buttons:1,clickCount:1,force:.7});
      if(drag)await call('Input.dispatchMouseEvent',{type:'mouseMoved',...end,pointerType:device,button:'left',buttons:1,force:.7});
      if(cancel)await escape();
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...(drag?end:p),pointerType:device,button:'left',buttons:0,clickCount:1});
    }
    if(drag){if(!cancel)await waitContact(`JSON.stringify(layerApp.state().layer_properties.controls.filter(c=>c.curve).map(c=>c.value))!==${JSON.stringify(originalCurves)}`);await escape();}
    await waitContact("layerApp.state().layer_tools.tool!=='pick_visible'&&layerApp.state().layer_tools.tool!=='target_curve'");await ready();
  };
  if(enabled('nested')){
    const original = await checkpoint();
    await evaluate('window.nestedScopeSaved = placementTest.saved.slice()');
    const owner = () => evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
    const select = id => send({type:'layer', action:{op:'select', id, mask:false}});
    const source = await owner();
    await send({type:'layer', action:{op:'new', group:true, clipped:false}});
    const outer = await owner();
    await send({type:'layer', action:{op:'new', group:true, clipped:false}});
    const inner = await owner();
    await send({type:'layer', action:{op:'reparent', id:source, parent:inner, index:0}});
    await select(source);
    await send({type:'effect', action:{op:'insert', effect:'curves'}});
    const target = await owner();
    const input = async () => {
      await select(target);
      await send({type:'effect', action:{op:'select_page', layer:target, page:'red'}});
      await ready();
      await fixture.poll(`layerApp.state().tonal_histogram.status==='Exact' && Number(layerApp.state().tonal_histogram.captured_source?.EffectInput)===${target}`);
      const result = await evaluate('JSON.parse(JSON.stringify(layerApp.state().tonal_histogram, (_,v)=>typeof v==="bigint"?Number(v):v))');
      assert.equal(result.data.pixels + result.data.transparent, 256*256);
      assert.ok(result.data.channels.every(c=>c.bins.length===256));
      return result;
    };
    const bins = result => result.data.channels.map(c=>c.bins);
    const initial = await input();
    assert.ok(initial.data.channels.every(c=>c.bins.filter(n=>n>0).length>1), 'Nested fixture has nonconstant input bins');
    const manifest=await fixture.save(),rows=await fixture.json('layerApp.state().layers'),occurrences=packageOccurrences(manifest);
    const occurrence=id=>occurrences[rows.findIndex(row=>Number(row.id)===id)];
    const children=id=>packageObject(manifest,occurrence(id).data.content.stack).data.entries.map(entry=>entry.ref);
    assert.ok(children(inner).includes(occurrence(source).id));
    assert.ok(children(outer).includes(occurrence(inner).id));
    assert.ok(children(inner).includes(occurrence(target).id));
    for (const position of ['inside', 'outside']) {
      const before = await visibleSamples();
      await select(position==='inside' ? target : outer);
      await send({type:'effect', action:{op:'insert', effect:'hue_saturation'}});
      const upper = await owner();
      await send({type:'effect', action:{op:'set', layer:upper, key:'lightness', value:{kind:'number', value:-35}}});
      const after = await input();
      assert.deepEqual(bins(after), bins(initial), `${position} upper adjustment cannot contaminate nested target input`);
      assert.notDeepEqual(await visibleSamples(), before, `${position} upper adjustment changes actual visible output`);
      assert.deepEqual((await checkpoint()).source, original.source);
    }
    await select(source);
    await send({type:'effect', action:{op:'insert', effect:'film_grain'}});
    const grain = await owner();
    await send({type:'effect', action:{op:'set', layer:grain, key:'animate', value:{kind:'toggle', value:false}}});
    await send({type:'effect', action:{op:'set', layer:grain, key:'time', value:{kind:'number', value:.375}}});
    const frozen = await input(), frozenDocument = await checkpoint();
    assert.notDeepEqual(bins(frozen), bins(initial), 'A lower fixed-phase effect changes actual target input');
    await evaluate('new Promise(resolve=>setTimeout(resolve,500))');await ready();
    const retained = await input();
    assert.deepEqual(bins(retained), bins(frozen), 'Fixed animation phase retains exact target-input bins');
    assert.deepEqual((await checkpoint()).authored, frozenDocument.authored, 'Host time passage does not change fixed-phase source');
    const beforeRetirement = await checkpoint();
    await action(a=>a.action.op==='target_curve');
    await select(source);
    await fixture.poll("layerApp.state().layer_tools.tool!=='target_curve' && layerApp.state().tonal_histogram.data==null");
    await evaluate('new Promise(resolve=>setTimeout(resolve,300))');await settle();
    assert.equal(await evaluate('layerApp.state().tonal_histogram.data==null'), true, 'Retired target cannot publish late embedded data');
    assert.deepEqual((await checkpoint()).authored, beforeRetirement.authored, 'Changing calibration target retires without correction');
    await evaluate(`window.showOpenFilePicker=async()=>[{name:'scope-baseline.capy', getFile:async()=>new File([nestedScopeSaved],'scope-baseline.capy')}];`);
    const epoch = await evaluate('String(layerApp.state().document_file.epoch)');
    await fixture.invoke('open_document');
    await fixture.poll(`String(layerApp.state().document_file.epoch)!==${JSON.stringify(epoch)}`);
    await fixture.idle();await fixture.invoke('fit_canvas');
    assert.deepEqual(await checkpoint(), original);
    await evaluate('delete window.nestedScopeSaved');
  }
  for(const width of process.env.LAYER_TONAL_WIDTH?[Number(process.env.LAYER_TONAL_WIDTH)]:[640,1100])for(const theme of process.env.LAYER_TONAL_THEME?[process.env.LAYER_TONAL_THEME]:['light','dark'])for(const effect of phase==='nested'?[]:process.env.LAYER_TONAL_EFFECT?[process.env.LAYER_TONAL_EFFECT]:['levels','curves','white_balance']){
    await fixture.reopen();await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});await send({type:'set_theme',theme});await send({type:'effect',action:{op:'insert',effect}});await send({type:'customize',action:{type:'set_panel_visible',panel:'properties',visible:true}});const propertiesActive=await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('properties'))?.active==='properties'");if(!propertiesActive)await fixture.click('.dock-group[data-panel=properties] .dock-tab[data-panel=properties]');if(await evaluate("layerApp.state().customization.expanded==='properties'"))await fixture.click('.dock-group[data-panel=properties] .dock-tab[data-panel=properties]');await fixture.poll("layerApp.state().customization.expanded==null&&!document.querySelector('.dock-group[data-panel=properties] .panel-configuration')?.checkVisibility()");await ready();await fixture.invoke('fit_canvas');await ready();
    if(effect==='levels'&&enabled('auto')){
      const before=await checkpoint(),pixelsBefore=await visibleSamples();await action(a=>a.action.op==='auto_levels');await ready();const after=await checkpoint();assert.deepEqual(after.source,before.source);
      assert.notDeepEqual(after.authored,before.authored,'Valid Auto changes the uncorrected bounded-range fixture');assert.notDeepEqual(await visibleSamples(),pixelsBefore,'Auto changes actual rendered artwork pixels');{await send({type:'invoke',command:'undo'});await ready();assert.deepEqual((await checkpoint()).authored,before.authored);await send({type:'invoke',command:'redo'});await ready();assert.deepEqual((await checkpoint()).authored,after.authored);await send({type:'invoke',command:'undo'});await ready();}
    }
    if(enabled('calibration'))for(const role of effect==='white_balance'||width===1100?['gray']:['black','gray','white'])for(const device of width===1100?['pen']:['pen','touch'])for(const cancel of [true,false]){console.log(`Calibration ${effect}/${width}/${theme}/${role}/${device}/${cancel?'cancel':'commit'}`);
      const before=await checkpoint(),pixelsBefore=await visibleSamples();await action(a=>a.action.op==='calibrate'&&a.action.role.toLowerCase()===role);await contact(device,cancel,false,role);const after=await checkpoint();assert.deepEqual(after.source,before.source);
      if(cancel)assert.deepEqual(after.authored,before.authored,'Cancelled calibration has no edit');
      else {assert.notDeepEqual(after.authored,before.authored,'Valid calibration commits a correction');assert.notDeepEqual(await visibleSamples(),pixelsBefore,'Calibration changes actual rendered artwork pixels');await send({type:'invoke',command:'undo'});await ready();assert.deepEqual((await checkpoint()).authored,before.authored,'One Undo restores calibration');await send({type:'invoke',command:'redo'});await ready();assert.deepEqual((await checkpoint()).authored,after.authored);await send({type:'invoke',command:'undo'});await ready();}
    }
    if(effect==='curves'&&enabled('targeted'))for(const page of ['rgb','red']){
      const available=(await view()).pages;const selected=available.find(p=>p.id===page);assert.ok(selected,'Shared Master/Red page exists');await send({type:'effect',action:{op:'select_page',layer:(await view()).layer,page:selected.id}});
      for(const device of ['pen','touch'])for(const cancel of [true,false]){const before=await checkpoint();await action(a=>a.action.op==='target_curve');await contact(device,cancel,true);const after=await checkpoint();assert.deepEqual(after.source,before.source);if(cancel)assert.deepEqual(after.authored,before.authored);else{assert.notDeepEqual(after.authored,before.authored);await send({type:'invoke',command:'undo'});await ready();assert.deepEqual((await checkpoint()).authored,before.authored);await send({type:'invoke',command:'redo'});await ready();assert.deepEqual((await checkpoint()).authored,after.authored);await send({type:'invoke',command:'undo'});await ready();}}
    }
    if(effect==='curves'&&enabled('focus')){
      const v=await view();await send({type:'effect',action:{op:'select_page',layer:v.layer,page:'rgb'}});await ready();
      const p=await evaluate(`(()=>{const r=document.querySelector('.curve-editor').getBoundingClientRect();return{x:r.x+r.width*.4,y:r.y+r.height*.45}})()`);
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await ready();
      const before=await checkpoint();
      await press('[data-curve-axis="input"] .number-value');
      await evaluate(`(()=>{const n=document.querySelector('[data-curve-axis="input"] .number-entry');window.scopeDraft=n;n.value='0.4123456789012345';n.dispatchEvent(new Event('input',{bubbles:true}));n.setSelectionRange(3,9)})()`);
      const binsBefore=JSON.stringify(await fixture.json('layerApp.state().tonal_histogram.data'));
      await send({type:'effect',action:{op:'set',layer:v.layer,key:'curve_1',value:{kind:'curve',value:[[0,0],[.5,.25],[1,1]]}}});
      await fixture.poll(`JSON.stringify(layerApp.state().tonal_histogram.data,(_,v)=>typeof v==='bigint'?Number(v):v)!==${JSON.stringify(binsBefore)}&&layerApp.state().tonal_histogram.data!=null`);await ready();
      assert.ok(await evaluate(`document.activeElement===scopeDraft&&scopeDraft===document.querySelector('[data-curve-axis="input"] .number-entry')`),'Analysis refresh retains the exact numeric field and focus');
      assert.deepEqual(await evaluate('[scopeDraft.value,scopeDraft.selectionStart,scopeDraft.selectionEnd]'),['0.4123456789012345',3,9]);
      await escape();await ready();await send({type:'invoke',command:'undo'});await ready();assert.deepEqual((await checkpoint()).authored,before.authored,'Escape cancels the draft; one Undo restores the sibling curve edit');
      const geometry=await evaluate(`(()=>{const root=document.querySelector('[data-curve-axis="input"]').closest('.dock-group'),r=root.getBoundingClientRect();return[...root.querySelectorAll('[data-curve-axis]')].map(n=>{const b=n.getBoundingClientRect();const value=n.querySelector('.number-value-box').getBoundingClientRect(),cell=n.closest('.curve-coordinate').getBoundingClientRect();return{left:b.left,right:b.right,containerLeft:r.left,containerRight:r.right,valueWidth:value.width,cellWidth:cell.width}})})()`);
      assert.ok(geometry.every(b=>b.left>=b.containerLeft-1&&b.right<=b.containerRight+1),'Precise coordinates stay inside actual dock bounds');assert.ok(geometry.every(b=>b.valueWidth>=b.cellWidth*.8),'Compact value boxes fill their coordinate columns');
    }
    if(phase==='capture'){await action(a=>a.action.op==='calibrate'&&a.action.role.toLowerCase()==='gray');await escape();await waitContact("layerApp.state().layer_tools.tool!=='pick_visible'");if(effect==='curves'){const p=await evaluate(`(()=>{const r=document.querySelector('.curve-editor').getBoundingClientRect();return{x:r.x+r.width*.4,y:r.y+r.height*.45}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await ready();}}
    await fixture.reopen();await fixture.recover();await ready();
    if(effect==='curves'&&phase!=='targeted'){await press('.curve-editor circle:nth-child(2)');await ready();assert.ok(await evaluate(`Array.from(document.querySelectorAll('[data-curve-axis] .number-value')).every(n=>n.textContent.trim().length>0)`),'A selected real knot shows readable coordinate values');}
    const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${effect}-${width}-${theme}.png`,Buffer.from(png.data,'base64'));
    if(effect==='curves'){await evaluate(`(()=>{let n=document.querySelector('.curve-editor');while(n){if(n.scrollHeight>n.clientHeight)n.scrollTop=0;n=n.parentElement}})()`);await settle();const main=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/curves-main-${width}-${theme}.png`,Buffer.from(main.data,'base64'));}
  }
  } finally {await fixture.dispose();}
}
