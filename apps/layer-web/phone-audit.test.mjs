import assert from 'node:assert/strict';
import test from 'node:test';
import {runInNewContext} from 'node:vm';
import {measurePhoneAuditElements,phoneAuditMatrix,dialogAuditMatrix,dialogAuditRows,selectDialogAuditRows,dialogAuditSpecifications,phoneAuditFrameSpecifications,phoneAuditDrawerSpecifications,takeStrokeRecording} from './phone-audit.mjs';
import {strokeRecordingControl} from './stroke-recording.js';

test('collecting a recording cannot leave its completion prompt over the next contact',t=>{
  const saved={document:globalThis.document,setInterval:globalThis.setInterval};
  t.after(()=>Object.assign(globalThis,saved));
  let tick,recording=false,ready=false,dialogs=0;
  globalThis.setInterval=callback=>{tick=callback;};
  globalThis.document={createElement:()=>({append(){},showModal(){dialogs++;}}),body:{append(){}}};
  const app={
    stroke_recording_status:()=>({recording,ready,label:'',elapsed_seconds:0,raw_events:1}),
    start_stroke_recording(){assert.equal(ready,false);recording=true;},
    stop_stroke_recording(){recording=false;ready=true;},
    stroke_recording_data:()=>new Uint8Array([4,8,15,16,23,42]),
    stroke_recording_saved(){ready=false;},
  };
  strokeRecordingControl(app,{dataset:{}});
  app.start_stroke_recording();tick();
  const bytes=takeStrokeRecording(app);
  tick();
  assert.equal(dialogs,0,'Node file I/O must not expose a stopped, unclaimed recording to the completion controller');
  assert.deepEqual(bytes,[4,8,15,16,23,42]);
  assert.equal(ready,false);
  app.start_stroke_recording();tick();
  assert.equal(dialogs,0);
});

test('phone frame IDs preserve authored tiles and expose actual slider parts',()=>{
  const specs=phoneAuditFrameSpecifications([{id:88,control:{kind:'color'}},{id:91,control:{kind:'divider'}},{id:204,control:{kind:'command',command:'eraser'}}]);
  assert.deepEqual(specs.filter(s=>s.id.startsWith('tile.')).map(s=>[s.id,s.selector]),[
    ['tile.settings','#phone-tool-settings'],['tile.88','#phone-tool-88'],['tile.204','#phone-tool-204']]);
  assert.equal(new Set(specs.map(s=>s.id)).size,specs.length);
  assert.equal(specs.find(s=>s.id==='capy').selector,'#phone-frame #zen-capy');
  for(const name of ['size','strength']){
    const track=specs.find(s=>s.id===`slider.${name}.track`),handle=specs.find(s=>s.id===`slider.${name}.handle`);
    assert.equal(track.role,'trough');assert.equal(handle.role,'slider');
    assert.ok(track.selector.includes('.interval-trough'));assert.ok(handle.selector.includes('.toolbar-slider-thumb'));
  }
  assert.deepEqual(phoneAuditFrameSpecifications([]).filter(s=>/^tile\.\d+$/.test(s.id)),[]);
});

test('phone audit covers both themes at DPR1 and actual 16pt enlarged text',()=>{
  assert.equal(phoneAuditMatrix.length,10);
  assert.deepEqual([...new Set(phoneAuditMatrix.map(c=>c.viewport.join('x')))],['412x891','891x412','320x568','320x320','280x560']);
  for(const entry of phoneAuditMatrix){
    assert.equal(entry.dpr,1);
    assert.equal(entry.text_scale,entry.text_pt/11);
    assert.equal(entry.text_pt,entry.viewport.join('x')==='320x320'?16:11);
  }
});

test('phone audit measures DOM allocations and keeps missing separate from hidden',()=>{
  const original=globalThis.getComputedStyle;
  globalThis.getComputedStyle=node=>node.style;
  try{
    const node=(bounds,visible=true)=>({id:'actual',tagName:'DIV',style:{visibility:'visible',display:'block'},
      getClientRects:()=>visible?[bounds]:[],getBoundingClientRect:()=>bounds});
    const visible=node({x:-2.25,y:8.5,width:48.75,height:47.25}),hidden=node({x:999,y:999,width:44,height:44},false);
    const root={querySelectorAll:selector=>({'.visible':[visible],'.hidden':[hidden]}[selector]||[])};
    const spec=(id,selector,optional=false)=>({id,role:'tile',selector,optional});
    const result=measurePhoneAuditElements([spec('tile.1','.visible'),spec('tile.2','.hidden'),spec('tile.3','.absent'),spec('native.close','.absent',true)],root);
    assert.deepEqual(result.elements.map(e=>[e.id,e.bounds,e.visible]),[['tile.1',[-2.25,8.5,48.75,47.25],true],['tile.2',null,false]]);
    assert.deepEqual(result.missing,['tile.3']);
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.visible'),spec('tile.1','.hidden')],root),/Duplicate/);
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.many')],{querySelectorAll:()=>[visible,hidden]}),/Ambiguous/);
    visible.getBoundingClientRect=()=>({x:NaN,y:0,width:48,height:48});
    assert.throws(()=>measurePhoneAuditElements([spec('tile.1','.visible')],root),/Nonfinite/);
  } finally {globalThis.getComputedStyle=original;}
});

test('drawer allocations distinguish the Color stage from its painted disc',t=>{
  const css=globalThis.CSS;globalThis.CSS={escape:value=>value};t.after(()=>globalThis.CSS=css);
  const nodes=Object.fromEntries(['drawer','column','stage','color','disc'].map(id=>[id,{id}]));
  const drawer=nodes.drawer;drawer.dataset={drawer:'tool'};
  drawer.querySelectorAll=selector=>selector===':scope > .drawer-contents > .drawer-column'?[nodes.column]:[];
  drawer.querySelector=selector=>({'.color-wheel-stage':nodes.stage,'.color-wheel-control':nodes.color,'.color-wheel-control canvas':nodes.disc}[selector]??null);
  const root={querySelectorAll:selector=>selector==='.content-drawer'?[drawer]:selector.startsWith('#')?[nodes[selector.slice(1)]]:[]};
  const specs=phoneAuditDrawerSpecifications(root);
  assert.equal(new Set(specs.map(spec=>spec.id)).size,specs.length);
  assert.deepEqual(specs.filter(spec=>spec.id==='drawer.color.wheel'||spec.id==='drawer.color.disc').map(spec=>[spec.id,spec.role,spec.selector]),[
    ['drawer.color.wheel','color-wheel','#stage'],['drawer.color.disc','context','#disc'],
  ]);
});

test('dialog baseline covers existing desktop widths in both themes without phone-sized viewports',()=>{
  assert.equal(dialogAuditMatrix.length,8);
  assert.deepEqual([...new Set(dialogAuditMatrix.map(test=>test.viewport[0]))],[600,700,800,1024]);
  assert.ok(dialogAuditMatrix.every(test=>test.viewport[1]===900&&test.dpr===1&&test.text_pt===11&&test.text_scale===1));
  for(const width of [600,700,800,1024])assert.deepEqual(dialogAuditMatrix.filter(test=>test.viewport[0]===width).map(test=>test.theme),['light','dark']);
});


test('dialog rows share routes across desktop and phone and include all W6a families',()=>{
  const rows=dialogAuditRows.filter(row=>row.batch==='w6a');
  assert.equal(rows.length,15);assert.equal(new Set(dialogAuditRows.map(row=>row.id)).size,dialogAuditRows.length);
  assert.ok(rows.every(row=>row.openaction&&row.rootselector));
  assert.deepEqual(rows.find(row=>row.id==='shortcut-editor').variants.map(row=>row.id),['shortcut-recording','shortcut-conflict']);
  assert.equal(rows.find(row=>row.id==='preferences-shortcuts-direct').requestedpage,'shortcuts');
  assert.equal(rows.find(row=>row.id==='preferences-shortcuts-category').openaction[1].action.id,'Modifier keys');
  assert.equal(rows.find(row=>row.id==='shortcut-editor').openaction[1].action.id,'command.Redo');
  assert.equal(rows.find(row=>row.id==='shortcut-editor').variants[0].openaction.action.id,'command.Hand');
  assert.equal(rows.find(row=>row.id==='choose-action').openaction[1].action.trigger,'touch.tap.2');
  assert.deepEqual(JSON.parse(rows.find(row=>row.id==='keymap-import').openaction[1].action.text),{format:'capycanvas-keymap',version:1,keymap:{id:'photoshop',revision:1}});
  assert.deepEqual(dialogAuditRows.filter(row=>row.baseline).map(row=>row.id),['preferences-root','preferences-appearance','new-drawing','color']);
});

test('W6c catch-up rows cover the seven forms reachable without a photo',()=>{
  const ids=['rename-drawing-preset','rename-paper','save-export-preset','new-palette','rename-palette','delete-palette','profile-library'];
  const rows=ids.map(id=>dialogAuditRows.find(row=>row.batch==='w6c'&&row.id===id));
  assert.ok(rows.every(row=>row&&typeof row.openaction==='function'&&typeof row.closeaction==='function'&&row.rootselector.includes('[open]')));
});

test('dialog row selection validates the chosen batch and retains its existing variants',()=>{
  assert.deepEqual(selectDialogAuditRows({batch:'w6c'},'this-workspace-details, saved-toolbar-details').map(row=>row.id),['this-workspace-details','saved-toolbar-details']);
  const [drawing]=selectDialogAuditRows({batch:'w6b'},'new-drawing');assert.equal(drawing,dialogAuditRows.find(row=>row.id==='new-drawing'));assert.equal(drawing.variants.length,4);
  assert.throws(()=>selectDialogAuditRows({batch:'w6c'},'new-drawing'),/Unknown dialog row new-drawing in batch w6c/);
  assert.throws(()=>selectDialogAuditRows(null,'new-drawing'),/requires a selected dialog batch/);
  assert.throws(()=>selectDialogAuditRows({batch:'w6b'},''),/requires nonempty row IDs/);
});

test('W6c routes retain manager lists, details, forms and blocked checks in one inventory',()=>{
  const rows=dialogAuditRows.filter(row=>row.batch==='w6c');
  for(const id of ['workspaces','layout-history','this-workspace','saved-toolbars','this-workspace-details','saved-toolbar-details','new-workspace','new-toolbar','save-toolbar','replace-toolbar','rename-saved-toolbar','delete-saved-toolbar','update-saved-toolbar','rename-toolbar','duplicate-toolbar','delete-toolbar','reset-workspace','reset-brushes','unsaved-changes','rename-workspace','delete-workspace','save-as-new-workspace','delete-inactive-workspace','add-tools','insert-tools'])assert.ok(rows.some(row=>row.id===id&&row.openaction&&row.rootselector));
  assert.equal(rows.find(row=>row.id==='rename-workspace').aliasof,'workspaces');
  assert.equal(rows.find(row=>row.id==='legacy-toolbar-manager').aliasof,'this-workspace');
  assert.match(rows.find(row=>row.id==='legacy-toolbar-manager').blocked,/Managed workspace/);
  for(const row of rows.filter(row=>row.blocked)){assert.equal(row.openaction,undefined);assert.equal(row.rootselector,undefined);assert.ok(row.blocked.length);}
});

test('manager form routes use the selected row and the actual enabled offered action',async()=>{
  const action={type:'save_toolbar',value:'authored-toolbar'},inputs=[],clicks=[],guards=[];
  const view={rows:[{id:'authored-row',actions:[]}],details:{actions:[{enabled:false,action:{type:'save_toolbar',value:'disabled'}},{enabled:true,action} ]}};
  const layerApp={app:{workspace_input:json=>inputs.push(JSON.parse(json)),workspace_view:()=>JSON.stringify(view)},wake(){}};
  const context={evaluate:source=>runInNewContext(source,{layerApp}),wait:async condition=>guards.push(condition),settle:async()=>{},click:async selector=>{clicks.push(selector);inputs.push({type:'select',id:'authored-row'},{type:'open_details'});}};
  await dialogAuditRows.find(row=>row.id==='save-toolbar').openaction(context);
  assert.deepEqual(inputs,[{type:'open',page:'this_workspace'},{type:'select',id:'authored-row'},{type:'open_details'},{type:'form',action}]);
  assert.deepEqual(clicks,['.workspace-manager[open] .workspace-choice[data-id="authored-row"]']);
  assert.ok(guards.some(condition=>condition.includes('checkVisibility()')),'The chosen native row must be mapped before opening its details');
  view.details.actions=[];
  await assert.rejects(()=>dialogAuditRows.find(row=>row.id==='save-toolbar').openaction(context),/Manager offers save_toolbar/);
});

test('New Drawing capture variants use the retained form controls and declare their drawer state',async()=>{
  const row=dialogAuditRows.find(row=>row.id==='new-drawing');
  assert.deepEqual(row.drawingstate,{kind:null,save:false});
  const expected=[['#new-drawing-texture'],['#new-drawing-paper-cancel','#new-drawing-presets-button'],['#new-drawing-save-as'],['#new-drawing-save-cancel','#new-drawing-browser-cancel','#new-drawing-paper-color']];
  for(const [index,variant] of row.variants.entries()){
    const clicks=[];await variant.openaction({click:async selector=>clicks.push(selector),send:()=>assert.fail('Retained form actions must not use the gated outer dispatcher')});
    assert.deepEqual(clicks,expected[index]);assert.ok(variant.drawingstate.kind);assert.equal(variant.drawingstate.save,index===2);
  }
});

test('Rename Drawing Preset uses actual naming input and the offered context menu',async()=>{
  const row=dialogAuditRows.find(row=>row.id==='rename-drawing-preset'),clicks=[],events=[];
  const state={new_drawing:{drawer:{kind:'presets',save:null,tiles:[{own:true,name:'Canvas study'}]},copy:{rename:'Rename'}}};
  const node={select(){},scrollIntoView(){},getBoundingClientRect:()=>({x:0,y:0,width:100,height:30})};
  const browser={layerApp:{state:()=>state},document:{querySelector:()=>node,querySelectorAll:()=>[{firstChild:{textContent:'Rename'}}]}};
  await row.openaction({send:async action=>assert.deepEqual(action,{type:'invoke',command:'new_document'}),
    evaluate:async source=>JSON.parse(JSON.stringify(runInNewContext(source,browser))??'null'),settle:async()=>{},
    call:async(type,event)=>events.push([type,event]),click:async selector=>{clicks.push(selector);if(selector==='#new-drawing-save-as')state.new_drawing.drawer.save={name:'Canvas study'};if(selector==='#new-drawing-save-confirm')state.new_drawing.drawer.save=null;if(selector.includes('nth-child'))state.new_drawing.drawer.save={name:'Canvas study'};}});
  assert.deepEqual(clicks,['#new-drawing-presets-button','#new-drawing-save-as','#new-drawing-save-confirm','#new-drawing-dialog .new-drawing-menu:popover-open button:nth-child(1)']);
  assert.equal(events[0][0],'Input.insertText');assert.equal(events[0][1].text,'Canvas study');assert.deepEqual(events.slice(1).map(([,event])=>event.button),['right','right']);
});

test('dialog allocations use only shared shell part attributes and require the header',()=>{
  const root={querySelector:()=>({querySelector:selector=>selector==='[data-dialog-part="end"]'||selector==='[data-dialog-part="panes"]'?{}:null})};
  const specs=dialogAuditSpecifications('#actual-dialog[open]',root);
  assert.deepEqual(specs.map(spec=>spec.id),['page','page.header','page.end','page.panes']);
  assert.equal(specs[1].optional,undefined);
  assert.deepEqual(dialogAuditSpecifications('#missing',{querySelector:()=>null}).map(spec=>spec.id),['page','page.header']);
  const alert={getAttribute:()=> 'alertdialog',querySelector:()=>null};
  assert.deepEqual(dialogAuditSpecifications('#alert',{querySelector:()=>alert}).map(spec=>spec.id),['page']);
  assert.ok(specs.slice(1).every(spec=>spec.selector.includes('[data-dialog-part=')));
});
