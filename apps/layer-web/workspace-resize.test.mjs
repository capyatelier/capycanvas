import {nativeScreenEvents} from './native-input-fixture.mjs';
import assert from "node:assert/strict";
import { mkdir, writeFile, rename } from "node:fs/promises";
import { existsSync } from "node:fs";

export async function checkWorkspaceConstraints({call,evaluate,settle,reload}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/workspace-constraints';
  await mkdir(directory,{recursive:true});
  const axes=process.env.LAYER_CONSTRAINT_AXES==='1';
  const reports=[],viewports=axes?[[1257,691]]:[[1257,691],[823,1257],[800,600],[1024,600],[1280,720],[1440,1000],[640,360]];
  const read=expression=>evaluate(`JSON.parse(JSON.stringify((${expression}),(_,v)=>typeof v==='bigint'?String(v):v))`);
  const poll=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+25000;function check(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(check,50)}check()})`);
  const idle=async()=>{await settle();await poll(`(v=>v.ready&&!v.busy&&!v.dirty)(JSON.parse(layerApp.app.workspace_view()))`);await evaluate('new Promise(r=>setTimeout(r,100))');await settle();};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await idle();};
  const manager=async action=>{await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(action))});null`);await idle();};
  const size=async(width,height)=>{await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:0,mobile:false});await idle();};
  const snapshot=()=>read('layerApp.app.workspace_persistence()');
  const group=panel=>read(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}))`);
  const pointer=async(start,end=start)=>{
    for(const[type,p,buttons]of[['mousePressed',start,1],['mouseMoved',end,1],['mouseReleased',end,0]]){await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons,clickCount:1});await settle();}
    await idle();
  };
  const target=async(selector,partial=false)=>{
    await evaluate(`(()=>{const n=[...document.querySelectorAll(${JSON.stringify(selector)})].find(n=>{const r=n.getBoundingClientRect();return r.width>0&&r.height>0&&!n.disabled});if(!n)throw Error('Missing visible enabled '+${JSON.stringify(selector)});window.workspaceConstraintTarget=n;n.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'});})()`);await settle();
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+5000;let previous,stable=0;function frame(){const node=window.workspaceConstraintTarget;if(!node?.isConnected){reject(Error('Target retired before input'));return;}const b=node.getBoundingClientRect(),current=[b.x,b.y,b.width,b.height];stable=b.width>0&&b.height>0&&previous?.every((v,i)=>Math.abs(v-current[i])<.01)?stable+1:0;previous=current;if(stable>=3)resolve(true);else if(performance.now()>end)reject(Error('Target did not settle before input'));else requestAnimationFrame(frame);}requestAnimationFrame(frame);})`);
    const p=await read(`(()=>{const n=window.workspaceConstraintTarget,r=n.getBoundingClientRect(),g=n.closest('.dock-group,.content-drawer')?.getBoundingClientRect();let top=Math.max(0,r.top),bottom=Math.min(innerHeight,r.bottom);for(let a=n.parentElement;a;a=a.parentElement)if(/auto|scroll|hidden|clip/.test(getComputedStyle(a).overflowY)){const b=a.getBoundingClientRect();top=Math.max(top,b.top);bottom=Math.min(bottom,b.bottom);}const x=r.x+r.width/2,y=${partial}?(top+bottom)/2:r.y+r.height/2,hit=document.elementFromPoint(x,y);return{x,y,width:r.width,height:r.height,visibleHeight:Math.max(0,bottom-top),hit:n.contains(hit),contained:r.left>=Math.max(0,g?.left??0)-1&&r.right<=Math.min(innerWidth,g?.right??innerWidth)+1&&(${partial}||bottom-top>=r.height-1),disabled:n.disabled,obscuredBy:n.contains(hit)?null:hit?.outerHTML.slice(0,500),dialogs:[...document.querySelectorAll('dialog[open]')].map(n=>n.textContent.slice(0,500)),viewport:{inner:[innerWidth,innerHeight],visual:[visualViewport.offsetLeft,visualViewport.offsetTop,visualViewport.width,visualViewport.height,visualViewport.scale],screen:[screen.width,screen.height],dpr:devicePixelRatio}}})()`);
    assert.ok(p.width>0&&p.height>0&&p.hit&&p.contained&&!p.disabled,`${selector} has a reachable complete target: ${JSON.stringify(p)}`);
    if(partial)assert.ok(p.visibleHeight>=35.5,`${selector} has a useful visible graph viewport: ${JSON.stringify(p)}`);
    return{x:p.x,y:p.y};
  };
  const click=async selector=>pointer(await target(selector));
  const resize=async(panel,edge,extent)=>{
    const g=await group(panel),b=g.resize_handles.find(h=>h.edge===edge).bounds,p={x:b.x+b.width/2,y:b.y+b.height/2};
    await pointer(p,{x:edge==='right'?g.bounds.x+extent:p.x,y:edge==='bottom'?g.bounds.y+extent:p.y});
  };
  const capture=async label=>{
    const geometry=await read(`({viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,scale:visualViewport.scale,groups:layerApp.app.layout(innerWidth,innerHeight).groups.map(g=>({id:g.id,panel:g.active,bounds:g.bounds,min_body_height:g.min_body_height})),previews:[...document.querySelectorAll('.navigator-overview,[data-scope-control="chart"]')].map(n=>({panel:n.closest('.dock-group')?.dataset.panel,bounds:n.getBoundingClientRect().toJSON()}))})`);
    reports.push({label,...geometry});
    if(label.endsWith('640x360')||/^illustrator-.+-1257x691$/.test(label)){const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(`${directory}/${label}.png`,Buffer.from(shot.data,'base64'));}
    return geometry;
  };
  const navigator=async label=>{
    const selector='.dock-group .navigator-overview';
    if(!await evaluate(`!!document.querySelector(${JSON.stringify(selector)})`))return;
    await send({type:'invoke',command:'fit_canvas'});
    const p=await target(selector),r=await read(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
    assert.ok(r.width>=35.5&&r.height>=35.5,`${label}: usable Navigator preview ${r.width} × ${r.height}`);
    const native=await read(`(()=>{const overview=document.querySelector(${JSON.stringify(selector)}),surface=overview.querySelector('.navigator-surface'),hole=overview.querySelector('.overview-hole'),b=surface.getBoundingClientRect(),expected=layerApp.app.navigator_geometry(${r.width},${r.height});return {pixels:[surface.width,surface.height],css:[b.width,b.height],dpr:devicePixelRatio,expected:expected?.image,hole:hole.hidden?null:Object.fromEntries(['left','top','width','height'].map(k=>[k,parseFloat(hole.style[k])]))};})()`);
    for(const axis of [0,1]) {
      assert.ok(Math.abs(native.pixels[axis]-native.css[axis]*native.dpr)<=1,`${label}: native Navigator pixels match its physical canvas`);
      assert.ok(native.pixels[axis]+1>=[r.width,r.height][axis]*native.dpr,`${label}: native Navigator canvas covers the current observed extent`);
    }
    assert.ok(native.expected&&native.hole,`${label}: current Navigator image geometry exists`);
    for(const [style,field] of [['left','x'],['top','y'],['width','width'],['height','height']])
      assert.ok(Math.abs(native.hole[style]-native.expected[field])<=.01,`${label}: cached Navigator ${field} matches the settled observed extent`);
    const camera=await read('layerApp.state().camera.translation');await pointer(p,{x:p.x+Math.min(20,r.width/4),y:p.y+Math.min(10,r.height/4)});
    assert.notDeepEqual(await read('layerApp.state().camera.translation'),camera,`${label}: Navigator pans the camera`);
    assert.doesNotMatch(await evaluate("document.querySelector('#status').textContent"),/Invalid Navigator size/);
    const zoom=await evaluate('layerApp.state().camera.zoom');await click('.dock-group [data-navigator-command="zoom_in"]');
    assert.ok(await evaluate('layerApp.state().camera.zoom')>zoom,`${label}: Navigator Zoom In action`);
  };
  const histogram=async label=>{
    const selector='[data-scope="histogram"] [data-scope-control="chart"]';
    if(!await evaluate(`!!document.querySelector(${JSON.stringify(selector)})`))return;
    const p=await target(selector,true),r=await read(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
    assert.ok(r.width>=35.5&&r.height>=35.5,`${label}: usable Histogram graph ${JSON.stringify({p,r})}`);
    for(const selector of ['[data-scope="histogram"] [data-scope-control="source"]','[data-scope="histogram"] [data-scope-control="channel"]'])await target(selector);
    const before=await evaluate('layerApp.state().histogram.logarithmic');
    await click('[data-scope="histogram"] [data-scope-control="log"]');
    assert.equal(await evaluate('layerApp.state().histogram.logarithmic'),!before,`${label}: Histogram Log action`);
    await click('[data-scope="histogram"] [data-scope-control="log"]');
  };
  await poll('layerApp.startupTimes.complete!==null');await idle();
  const original=await snapshot(),originalId=await evaluate('JSON.parse(layerApp.app.workspace_view()).id'),theme=await evaluate('layerApp.state().theme');
  const originalSettings=axes?await evaluate('JSON.stringify(layerApp.state().settings)'):null;
  const artwork=()=>read('({layers:layerApp.state().layers.map(l=>[String(l.id),String(l.paint_revision)]),modified:layerApp.state().document_file.modified})'),documentBefore=await artwork();
  const native=await read('({viewport:[innerWidth,innerHeight],dpr:devicePixelRatio,scale:visualViewport.scale,agent:navigator.userAgent})');
  const presets={};let temporary;
  try {
    await manager({type:'form',action:{type:'new'}});await manager({type:'submit',name:`Workspace constraints ${Date.now()}`});
    temporary=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
    for(const preset of ['painter','illustrator','photographer']) {
      const id=`builtin:workspace:${preset}`;await manager({type:'switch',id});
      const capture=()=>read('JSON.parse(layerApp.app.workspace_capture())'),normalize=value=>JSON.parse(JSON.stringify(value,(k,v)=>k==='timestamp_ms'?'date':v));
      const saved=normalize(await capture());
      await manager({type:'form',action:{type:'reset',value:id}});
      presets[preset]=await read('layerApp.state().workspace');
      await manager({type:'cancel'});
      assert.deepEqual(normalize(await capture()),saved,'Reading the starting layout preserves workspace history');
    }
    await manager({type:'switch',id:temporary});
    if(axes) {
      assert.ok(await evaluate('layerApp.app.bootstrap_view().shipped_tags.includes("de")'),'German is a registered long-label locale');
      await send({type:'restore_saved_settings',saved:await evaluate('JSON.stringify({...layerApp.state().settings,language:{Explicit:"de"}})')});
      await poll('!layerApp.app.language_pending()&&document.documentElement.lang==="de"');
      const key=async key=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code:key,windowsVirtualKeyCode:key==='Enter'?13:27,...(key==='Enter'&&type==='keyDown'?{text:'\r',unmodifiedText:'\r'}:{})});await idle();};
      for(const theme of ['light','dark'])for(const scale of [1,1.25,1.5]) {
        await send({type:'set_theme',theme});await send({type:'restore_workspace',workspace:presets.illustrator});
        const durable=await snapshot();
        await call('Emulation.setDeviceMetricsOverride',{width:1257,height:691,deviceScaleFactor:scale,mobile:false});await idle();
        await navigator(`de-${theme}-dpr${scale}`);assert.deepEqual(await snapshot(),durable,'Locale and DPR fitting preserve the saved layout');
        const ng=await group('navigator'),cg=await group('color');
        await send({type:'select_panel_tab',group:ng.id,panel:'proof'});await target('.proof-panel .proof-modes button');
        await send({type:'select_panel_tab',group:ng.id,panel:'navigator'});await navigator(`inactive-return-de-${theme}-dpr${scale}`);
        await send({type:'select_panel_tab',group:cg.id,panel:'palettes'});
        const root='.dock-group .palettes-panel';await evaluate(`document.querySelector('${root} .palette-scroll').scrollTop=1e6`);await settle();
        const last=`${root} .palette-swatches > .palette-tile[data-id]:nth-last-child(2)`;await click(last);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(last)}).classList.contains('selected')`),true,'Last swatch remains usable with German labels and fractional DPR');
        await target(`${root} .palette-add`);await send({type:'select_panel_tab',group:cg.id,panel:'color'});
        await send({type:'customize',action:{type:'set_column_collapsed',group:ng.id,collapsed:true}});
        const column=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).collapsed.find(c=>c.groups.some(g=>g.group===${ng.id})).id`);
        await send({type:'customize',action:{type:'set_column_drawers',column,drawers:true}});
        const button='.collapsed-column [data-panel="navigator"]';
        await target(button);await evaluate(`document.querySelector(${JSON.stringify(button)}).focus()`);await key('Enter');
        await poll('!!document.querySelector(".content-drawer .navigator-overview")');
        const before=await evaluate('layerApp.state().camera.zoom');await click('.content-drawer [data-navigator-command="zoom_in"]');
        assert.ok(await evaluate('layerApp.state().camera.zoom')>before,`Keyboard-opened drawer forwards Navigator controls at ${theme}/DPR${scale}`);
        await evaluate(`document.querySelector(${JSON.stringify(button)}).focus()`);await key('Enter');await poll('!document.querySelector(".content-drawer .navigator-overview")');
        await send({type:'restore_workspace',workspace:presets.illustrator});assert.deepEqual(await snapshot(),durable,'Closing and restoring the drawer preserves the saved layout');
        const label=`de-${theme}-dpr${scale}`;
        await send({type:'customize',action:{type:'set_panel_visible',panel:'histogram',visible:true}});
        await send({type:'move_panel',panel:'histogram',target:{kind:'tab',group:(await group('color')).id,index:null},viewport:[1257,691]});
        await send({type:'select_panel_tab',group:(await group('histogram')).id,panel:'histogram'});
        assert.ok(await evaluate(`!!document.querySelector('[data-scope="histogram"] [data-scope-control="chart"]')`),`${label}: Histogram controls are presented`);
        await histogram(label);
        await send({type:'restore_workspace',workspace:presets.illustrator});assert.deepEqual(await snapshot(),durable,'Histogram controls preserve the restored saved layout');
        await capture(label);
        const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(`${directory}/${label}.png`,Buffer.from(shot.data,'base64'));
      }
      console.log('PASS: German long labels, DPR1/1.25/1.5, both themes, native Navigator pixels/input, Histogram controls, inactive tabs, palette end control and keyboard drawer open/close');
      return;
    }
    for(const theme of ['light','dark'])for(const [preset,workspace]of Object.entries(presets)) {
      await size(1440,1000);await send({type:'restore_workspace',workspace});await send({type:'set_theme',theme});
      const durable=await snapshot();
      for(const[width,height]of viewports) {
        const label=`${preset}-${theme}-${width}x${height}`;await size(width,height);
        await navigator(label);await histogram(label);await capture(label);
        if(preset==='photographer') {
          await evaluate(`(()=>{const n=[...document.querySelectorAll('.properties-panel .property-controls select:not([disabled])')].at(-1);if(n)n.dataset.layoutLastField='true';})()`);
          if(await evaluate(`!!document.querySelector('[data-layout-last-field]')`))await target('[data-layout-last-field]');
        }
        assert.deepEqual(await snapshot(),durable,`${label}: fitting and camera actions preserve the saved layout`);
      }
      if(preset==='illustrator')for(const height of [680,685,690,695,700,710,720,740]) {
        await size(1257,height);await navigator(`boundary-${theme}-${height}`);await capture(`boundary-${theme}-${height}`);
        assert.deepEqual(await snapshot(),durable,'Neighboring heights preserve the saved layout');
      }
      await size(1440,1000);assert.deepEqual(await snapshot(),durable,'Growing restores the same saved layout');
      console.log(`Workspace constraints: ${preset}/${theme}, seven CSS viewports and saved layout passed`);
    }
    for(const theme of ['light','dark']) {
      await size(1440,1000);await send({type:'set_theme',theme});
      for(const panel of ['navigator','color','properties','adjustments','proof','palettes']) {
        await send({type:'restore_workspace',workspace:presets.illustrator});
        await send({type:'move_panel',panel,target:{kind:'float',position:[420,100]}});
        await resize(panel,'bottom',0);
        const g=await group(panel);assert.ok(g.bounds.height>=72,`${panel}: floating minimum keeps a usable body ${JSON.stringify(g.bounds)}`);
        if(panel==='navigator')await navigator(`floating-${theme}`);
        else await target(`.floating-panel[data-panel="${panel}"] .panel :is(button:not([disabled]),select:not([disabled]),input:not([disabled]))`);
        await capture(`floating-height-${panel}-${theme}`);
      }
      for(const panel of ['properties','adjustments','proof']) {
        await send({type:'restore_workspace',workspace:presets.illustrator});
        await send({type:'move_panel',panel,target:{kind:'float',position:[420,100]}});await resize(panel,'right',0);
        const selector=panel==='properties'?'.floating-panel .properties-panel .property-controls select':panel==='adjustments'?'.floating-panel .filter-picker-header input':'.floating-panel .proof-panel select';
        if(panel==='adjustments')await click('.floating-panel .filter-picker-header button');
        if(panel==='proof')await click('.floating-panel .proof-modes button[value="print"]');
        await click(selector);
        if(panel==='adjustments') {
          await call('Input.insertText',{text:'glass'});await idle();
          assert.match(await evaluate("document.querySelector('.floating-panel .filter-picker-list').textContent"),/Glass/i);
        } else {
          assert.ok(await evaluate('document.activeElement===window.workspaceConstraintTarget'),`${panel}: narrow dropdown accepts native focus`);
        }
        for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await evaluate('document.activeElement?.blur()');await poll('visualViewport.height>=innerHeight-1');await idle();
        await capture(`floating-width-${panel}-${theme}`);
      }
      for(const extra of [0,17]) {
        await send({type:'restore_workspace',workspace:presets.illustrator});await send({type:'move_panel',panel:'palettes',target:{kind:'float',position:[420,100]}});
        const measurement=await read("layerApp.app.panel_measurements().find(m=>m.panel==='palettes')");
        await resize('palettes','bottom',36+measurement.scroll.fixed_height+measurement.scroll.unit_height*2+extra);
        const root='.floating-panel .palettes-panel',scroll=`${root} .palette-scroll`,last=`${root} .palette-swatches > .palette-tile[data-id]:nth-last-child(2)`;
        await evaluate(`document.querySelector(${JSON.stringify(scroll)}).scrollTop=1e6`);await settle();
        await click(last);assert.equal(await evaluate(`document.querySelector(${JSON.stringify(last)}).classList.contains('selected')`),true,'Last swatch can be selected');
        await send({type:'set_color',rgba:[.1123,.3456,.7891,1]});
        const count=await evaluate('layerApp.app.palette_panel().swatches.length');
        await click(`${root} .palette-add`);assert.equal(await evaluate('layerApp.app.palette_panel().swatches.length'),count+1,'Trailing Add stores a swatch');
        const id=await evaluate('String(layerApp.app.palette_panel().swatches.at(-1).id)');
        await send({type:'color',action:{op:'library',action:{op:'remove',id:Number(id)}}});
        await capture(`palette-two-rows-${extra}-${theme}`);
      }
      console.log(`Workspace constraints: floating controls and palette end actions/${theme} passed`);
    }
    await size(1257,691);await send({type:'restore_workspace',workspace:presets.illustrator});
    await send({type:'move_panel',panel:'navigator',target:{kind:'float',position:[420,100]}});
    assert.deepEqual(await artwork(),documentBefore,'Panel and camera actions preserve the artwork');
    const durable=await snapshot();await idle();await reload();await poll('window.layerApp?.startupTimes.complete!==null&&!!window.layerApp');await idle();
    assert.deepEqual(await snapshot(),durable,'An intentional workspace change survives reload');await navigator('reload');
    console.log('PASS: workspace constraints, all presets and themes, seven CSS viewports, Navigator pan, Histogram controls, floating minima, palette end actions and reload');
  } catch(error) {
    reports.push({failure:String(error.stack||error)});
    const shot=await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:false});await writeFile(`${directory}/failure.png`,Buffer.from(shot.data,'base64'));
    throw error;
  } finally {
    await writeFile(`${directory}/constraints.json`,JSON.stringify({native,requestedViewports:viewports,axes:axes?{locale:'de',dpr:[1,1.25,1.5],limitations:'DPR emulation; browser zoom and OS text scaling unverified. No registered RTL locale.'}:null,reports},null,2));
    await call('Emulation.clearDeviceMetricsOverride');await idle();
    await manager({type:'cancel'});
    if(originalSettings){await send({type:'restore_saved_settings',saved:originalSettings});await poll('!layerApp.app.language_pending()');assert.equal(await evaluate('JSON.stringify(layerApp.state().settings)'),originalSettings,'Focused axes restore the original settings and language');}
    await manager({type:'switch',id:originalId});await send({type:'set_theme',theme});
    assert.deepEqual(await snapshot(),original,'The original workspace remains intact');
    if(temporary){await manager({type:'form',action:{type:'delete',value:temporary}});await manager({type:'submit',name:''});}
  }
}

// Observe actual panel widths on the browser's display clock. CDP uses native
// mouse/touch handling; the desktop runner can additionally feed OS input.
export async function checkWorkspaceResize({call,evaluate,settle,scenarios=["left","right","navigator"]}) {
  if(scenarios.some(s=>s.startsWith('color')))await call('Emulation.setDeviceMetricsOverride',{
    width:1440,height:1000,deviceScaleFactor:Number(process.env.LAYER_MOTION_SCALE||1),mobile:false,
  });
  const native=process.argv.includes("--native-input"),dir=process.env.LAYER_NATIVE_INPUT_DIR;
  let step=0,held=null,point;
  const perform=async events=>{
    const path=`${dir}/step-${step}.json`;
    const nativeEvents=nativeScreenEvents(events,await evaluate('({screenX,screenY,outerWidth,outerHeight,innerWidth,innerHeight})'));
    await writeFile(`${path}.tmp`,JSON.stringify(nativeEvents));await rename(`${path}.tmp`,path);
    const deadline=Date.now()+20000;
    while(!existsSync(`${dir}/done-${step}`)) {assert.ok(Date.now()<deadline,"native input timeout");await new Promise(r=>setTimeout(r,2));}
    step++;
  };
  const event=(phase,p)=>held==="touch"?{touch:phase,point:[p.x,p.y]}:phase==="up"?{down:false}:{point:[p.x,p.y],...(phase==="down"?{down:true}:{})};
  const input=(phase,p=point)=>{
    point=p;
    if(native)return perform([event(phase,p)]);
    if(held==="touch")return call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd",cancel:"touchCancel"}[phase],touchPoints:["up","cancel"].includes(phase)?[]:[{id:1,...p}]});
    return call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[phase],...p,button:"left",buttons:phase==="up"?0:1,clickCount:1});
  };
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,250))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const snapshot=()=>evaluate("layerApp.state().workspace");
  await evaluate("new Promise(resolve=>{const poll=()=>layerApp.startupTimes.complete?resolve():setTimeout(poll,50);poll();})");
  const saved=await snapshot(),fixture=structuredClone(saved),reports=[];
  const tabs=(id,panels)=>({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
  Object.assign(fixture.layout,{bands:[
    {id:40,edge:"left",extent:252,root:tabs(41,["brushes","sizes"])},
    {id:42,edge:"right",extent:310,root:tabs(43,["layers","properties","adjustments"])},
    {id:44,edge:"top",extent:36,root:tabs(45,["toolbar"])},
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],column_stacks:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  if(native)await writeFile(`${dir}/ready`,"ready");
  const stop=()=>evaluate(`(()=>{const p=window.resizeProbe;if(!p)return;p.running=false;p.observer.disconnect();p.allocationObserver?.disconnect();for(const[k,v]of Object.entries(p.original))layerApp.app[k]=v;Node.prototype.cloneNode=p.clone;})()`);
  try {
    for(const device of ["mouse","touch"])for(const scenario of scenarios) {
      const color=scenario.startsWith('color'),floating=scenario==='color-float';
      for(const theme of color?['light','dark']:['dark'])for(let repeat=0;repeat<(color?3:1);repeat++) {
        await send({type:'set_theme',theme});
        const workspace=structuredClone(fixture);
        if(scenario==="navigator") {workspace.layout.bands[1].root.panels.push("navigator");workspace.layout.bands[1].root.active="navigator";}
        if(color){
          if(floating)workspace.layout.floating.push({root:tabs(46,['color']),position:[380,140],width:272,height:430,default_width:null,toolbar_layout:'compact'});
          else workspace.layout.bands[0].root=tabs(41,['color','sizes']);
          workspace.layout.next_id=Math.max(47,workspace.layout.next_id);
        }
        await send({type:"restore_workspace",workspace});
        const left=scenario==='left'||color;
        const before=await snapshot(),group=floating?46:left?41:43,id=left?40:42;
        const handle=floating?`groups.find(g=>g.id===${group}).resize_handles.find(h=>h.edge==='right')`:`dividers.find(d=>d.band&&d.id===${id})`;
        const start=await evaluate(`(()=>{const b=layerApp.app.layout(innerWidth,innerHeight).${handle}.bounds;return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
        held=device;await input("down",start);
        const origin={x:start.x+(left?20:-20),y:start.y};
        await input("move",origin);await wait();
        await evaluate(`(()=>{
          const app=layerApp.app,p=window.resizeProbe={original:{},counts:{},cpu:{},frames:[],widths:[],running:true,clones:0,added:0,removed:0,allocations:0,scaledFrames:0};
          p.node=document.querySelector('.dock-group[data-group="${group}"]');p.content=p.node.querySelector('.panel');p.surface=p.node.querySelector('.navigator-surface');
          if(p.surface){p.allocationObserver=new MutationObserver(records=>{p.allocations+=records.length;});p.allocationObserver.observe(p.surface,{attributes:true,attributeFilter:["width","height"]});}
          for(const name of ["state","layout","layout_update","workspace_update","dispatch","frame","reflow_navigators","editor_models","panel_view","navigator_size","navigator_surface","color_field_pixels","color_field_request"]){
            if(typeof app[name]!=="function")continue;
            p.original[name]=app[name].bind(app);app[name]=(...args)=>{const t=performance.now(),v=p.original[name](...args);const key=name==="dispatch"?"dispatch:"+args[0].type:name;(p.cpu[key]??=[]).push(performance.now()-t);p.counts[key]=(p.counts[key]||0)+1;return v;};
          }
          p.clone=Node.prototype.cloneNode;Node.prototype.cloneNode=function(...args){p.clones++;return p.clone.apply(this,args);};
          p.observer=new MutationObserver(records=>{for(const r of records){p.added+=r.addedNodes.length;p.removed+=r.removedNodes.length;}});p.observer.observe(document.querySelector('#workspace'),{childList:true,subtree:true});
          const frame=t=>{if(!p.running)return;const width=p.node.getBoundingClientRect().width;if(width!==p.last){p.frames.push(t);p.widths.push(width);p.last=width;}if(p.surface){const r=p.surface.getBoundingClientRect();if(Math.abs(p.surface.width-r.width*devicePixelRatio)>1||Math.abs(p.surface.height-r.height*devicePixelRatio)>1)p.scaledFrames++;}requestAnimationFrame(frame);};requestAnimationFrame(frame);
        })()`);
        const pending=[],began=performance.now();
        for(let i=0;native?i<(color?1250:550):performance.now()-began<(color?5000:2200);i++) {
          const t=native?i*.004:(performance.now()-began)/1000,p=t*5%4,triangle=p<1?p:p<3?2-p:p-4;
          point={x:origin.x+triangle*65,y:origin.y};
          pending.push(native?event("move",point):input("move",point));
          if(!native)await new Promise(r=>setTimeout(r,4));
        }
        if(native)await perform(pending);else await Promise.all(pending);await settle();
        const probe=await evaluate(`(()=>{const p=resizeProbe,b=p.node.getBoundingClientRect(),expected=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.id===${group}).bounds;return{scale:devicePixelRatio,counts:p.counts,cpu:p.cpu,frames:p.frames,widths:p.widths,allocations:p.allocations,scaledFrames:p.scaledFrames,clones:p.clones,added:p.added,removed:p.removed,retained:p.node.isConnected&&p.content===p.node.querySelector('.panel'),error:Math.abs(b.width-expected.width),nativeSurface:!p.surface||(p.surface===p.node.querySelector('.navigator-surface')&&Math.abs(p.surface.width-p.surface.getBoundingClientRect().width*devicePixelRatio)<=1)};})()`);
        await stop();
        const stats=values=>{values.sort((a,b)=>a-b);return{count:values.length,p50:values[Math.floor(values.length*.5)],p95:values[Math.floor(values.length*.95)],p99:values[Math.floor(values.length*.99)],total:values.reduce((a,b)=>a+b,0)};};
        const n=probe.frames.length,result={device,scenario,theme,repeat,scale:probe.scale,input:native?"Wayland":"CDP",geometryHz:n>1?(n-1)*1000/(probe.frames.at(-1)-probe.frames[0]):0,gapP99Ms:stats(probe.frames.slice(1).map((t,i)=>t-probe.frames[i])).p99,changedFrames:n,widthRange:[Math.min(...probe.widths),Math.max(...probe.widths)],counts:probe.counts,bridgeMs:Object.fromEntries(Object.entries(probe.cpu).map(([k,v])=>[k,stats(v)])),allocations:probe.allocations,scaledFrames:probe.scaledFrames,clones:probe.clones,added:probe.added,removed:probe.removed,retained:probe.retained};
        assert.ok(n>20,"actual changing geometry");assert.ok(probe.error<=1,"native width matches Rust");assert.ok(probe.retained&&probe.nativeSurface,"retain native-resolution content/resources");
        if(color){assert.equal(result.counts.color_field_pixels||0,0,'no synchronous color raster work during resize');assert.ok(result.counts.color_field_request>0,'resize schedules physical-size fields on the worker');}
        if(process.env.LAYER_RESIZE_RETAINED){
          assert.equal(result.counts.state||0,0,"no full content refresh during steady resize");
          assert.equal(result.clones,0,"retain intrinsic measurement controls during steady resize");
          assert.equal(result.added+result.removed,0,"retain visible DOM during steady resize");
          assert.equal(result.scaledFrames,0,"every observed Navigator frame keeps native resolution");
          assert.ok(result.allocations<=2*(Math.ceil((result.widthRange[1]-result.widthRange[0])*probe.scale/64)+2),"retain GPU capacity while resizing");
        }
        if(process.env.LAYER_RESIZE_MIN_HZ)assert.ok(result.geometryHz>=Number(process.env.LAYER_RESIZE_MIN_HZ),`${device}/${scenario}: ${result.geometryHz} Hz`);
        reports.push(result);console.log(JSON.stringify(result));
        await input("up");held=null;await wait();
        const after=await snapshot();assert.notDeepEqual(after,before);
        await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),before,"one undo restores resize");
        await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snapshot(),after,"redo restores resized geometry");
        const cancelStart=await evaluate(`(()=>{const b=layerApp.app.layout(innerWidth,innerHeight).${handle}.bounds;return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
        held=device;await input("down",cancelStart);await input("move",{x:cancelStart.x+35,y:cancelStart.y});await wait();
        assert.notDeepEqual(await snapshot(),after);
        if(!native&&device==="touch")await input("cancel");
        else {
          // Exercise the host focus-loss cancellation path for mouse/native
          // input. CDP supplies a real pointercancel for the touch case above.
          await evaluate("window.dispatchEvent(new Event('blur'))");
          await input("up");
        }
        held=null;await wait();assert.deepEqual(await snapshot(),after,"cancellation restores the live layout");
      }
    }
    const output=process.env.LAYER_TEST_ARTIFACTS||"artifacts/workspace-resize";
    await mkdir(output,{recursive:true});await writeFile(`${output}/web.json`,JSON.stringify(reports,null,2));
    console.log("PASS: changing panel geometry, native-resolution resources, mouse/touch, undo/redo");
  } finally {
    await stop();if(held)await input("up");
    await send({type:"restore_workspace",workspace:saved});
    if(native)await writeFile(`${dir}/finished`,"finished");
  }
}
