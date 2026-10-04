import {histogramJourney} from './histogram-journey.mjs';
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkSpatialFilterWindows({call,evaluate,settle,canvasPixels}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/spatial-filter-windows-web';
  await mkdir(directory,{recursive:true});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,50);}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const radius=()=>evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="sigma")');
  const editRadius=async(text,commit=true)=>{
    const at=await evaluate(`(()=>{const field=document.querySelector('[data-property-key="sigma"]'),entry=field.querySelector('.number-entry'),n=entry.hidden?field.querySelector('.number-value'):entry;n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...at,button:'left',buttons:1,clickCount:1});await call('Input.dispatchMouseEvent',{type:'mouseReleased',...at,button:'left',buttons:0,clickCount:1});await settle();
    await evaluate(`(()=>{const n=document.querySelector('[data-property-key="sigma"] .number-entry');n.value=${JSON.stringify(text)};n.dispatchEvent(new Event('input',{bubbles:true}))})()`);
    const key=commit?'Enter':'Escape',code=commit?13:27;
    await call('Input.dispatchKeyEvent',{type:'keyDown',key,code:key,windowsVirtualKeyCode:code});await call('Input.dispatchKeyEvent',{type:'keyUp',key,code:key,windowsVirtualKeyCode:code});await settle();await evaluate('layerApp.app.wait_for_canvas()');
  };
  const histogram=histogramJourney({evaluate,settle}).exact;
  await wait('layerApp.startupTimes.complete!==null && !layerApp.documents.busy()');
  await send({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
  await evaluate(`(async()=>{
    window.spatialPicker=window.showOpenFilePicker;
    const canvas=new OffscreenCanvas(6000,4000),context=canvas.getContext('2d',{willReadFrequently:true});
    const gradient=context.createLinearGradient(0,0,6000,4000);gradient.addColorStop(0,'rgb(220,40,70)');gradient.addColorStop(1,'rgb(30,160,230)');
    context.fillStyle=gradient;context.fillRect(0,0,6000,4000);context.fillStyle='white';
    for(let x=0;x<6000;x+=256)context.fillRect(x,0,12,4000);
    const blob=await canvas.convertToBlob({type:'image/png'});
    window.showOpenFilePicker=async()=>[{name:'spatial-window.png',getFile:async()=>new File([blob],'spatial-window.png',{type:'image/png'})}];
    layerApp.dispatch({type:'invoke',command:'open_document'});
  })()`);
  try {
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
    await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.state().tabs.some(t=>t.width===6000&&t.height===4000) && layerApp.app.brush_ready()');
    const source=await histogram();
    assert.equal(source.pixels,24000000);
    assert.equal(source.transparent,0,'The imported photo has rendered opaque GPU pixels');
    for(const sigma of [9,21,13]) {
      await send({type:'effect',action:{op:'insert',effect:'gaussian_blur'}});
      const layer=await evaluate('Number(layerApp.state().layer_properties.layer)');
      await send({type:'effect',action:{op:'set',layer,key:'sigma',value:{kind:'number',value:sigma}}});
    }
    const layer=await evaluate('Number(layerApp.state().layer_properties.layer)');
    await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();
    const width=await evaluate('innerWidth');
    for(const theme of ['dark','light']) {
      await send({type:'set_theme',theme});await send({type:'set_zoom',zoom:.5});
      assert.equal(await evaluate('layerApp.state().camera.zoom'),.5);
      const before=(await radius()).value.value;assert.equal((await radius()).kind.numeric.max,85);
      await editRadius('85',false);assert.equal((await radius()).value.value,before,'Cancelling the generic Radius draft preserves its value');
      await editRadius('85');assert.equal((await radius()).value.value,85);
      await send({type:'invoke',command:'undo'});assert.equal((await radius()).value.value,before,'Radius85 is one undo step');
      await send({type:'invoke',command:'redo'});assert.equal((await radius()).value.value,85);
      await editRadius('86');assert.equal((await radius()).value.value,85,'Generic numeric input respects the shared Radius85 bound');
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      const radiusShot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/radius85-${width}-${theme}.png`,Buffer.from(radiusShot.data,'base64'));

      for(const [step,[center,sigma]] of [[[2400,1600],85],[[3300,2100],85],[[2400,1600],120],[[2400,1600],0],[[2400,1600],7]].entries()) {
        await send({type:'effect',action:{op:'set',layer,key:'sigma',value:{kind:'number',value:sigma}}});
        await evaluate(`(()=>{const c=layerApp.app.camera(),p=${JSON.stringify(center)},m=c.document_to_surface??[c.zoom,0,0,c.zoom,...c.translation];
          layerApp.app.gesture(m[0]*p[0]+m[2]*p[1]+m[4],m[1]*p[0]+m[3]*p[1]+m[5],c.viewport[0]/2,c.viewport[1]/2,1,0);layerApp.wake();})()`);
        await settle();await evaluate('layerApp.app.wait_for_canvas()');
        await wait('layerApp.app.brush_ready()');
        assert.equal(await evaluate('layerApp.state().host_error??null'),null);
        assert.equal(await evaluate('layerApp.state().layer_properties.controls.find(c=>c.key==="sigma").value.value'),sigma);
        const presented=await canvasPixels();
        assert.ok(presented.colored>presented.total*.2,'The presented canvas contains the filtered photo');
        const shot=await call('Page.captureScreenshot',{format:'png'});
        await writeFile(`${directory}/${width}-${theme}-${step}.png`,Buffer.from(shot.data,'base64'));
      }
    }
    console.log('PASS: generic Radius85 draft/cancel/bounds/undo, saved sigma120, 24 MP chained spatial filters at 50% zoom, pan round trips and support changes, light and dark themes');
  } finally {
    await evaluate('window.showOpenFilePicker=spatialPicker;delete window.spatialPicker');
  }
}

export async function checkCurveEndpoint({call,evaluate,settle}) {
  const count=()=>evaluate("document.querySelector('.curve-field:not([hidden]) .curve-editor').querySelectorAll('circle').length");
  const before=await count();assert.equal(before,2);
  const point=await evaluate(`(()=>{
    const graph=document.querySelector('.curve-field:not([hidden]) .curve-editor'),endpoint=graph.querySelector('circle:last-child');
    const p=new DOMPoint(endpoint.cx.baseVal.value,endpoint.cy.baseVal.value).matrixTransform(graph.getScreenCTM());
    // Press the visible inner part; the panel scrollbar overlaps the outer edge.
    return {x:p.x-3,y:p.y+1};
  })()`);
  await call("Input.dispatchMouseEvent",{type:"mousePressed",...point,button:"left",buttons:1,clickCount:1});
  const target={x:point.x,y:point.y+5};
  await call("Input.dispatchMouseEvent",{type:"mouseMoved",...target,button:"left",buttons:1});
  await call("Input.dispatchMouseEvent",{type:"mouseReleased",...target,button:"left",buttons:0,clickCount:1});
  await settle();
  assert.equal(await count(),before,"Dragging a visible curve endpoint must edit it without inserting another point");
  const moved=await evaluate("document.querySelector('.curve-field:not([hidden]) .curve-editor circle:last-child').cy.baseVal.value");
  assert.ok(Math.abs(moved-6)<0.01,"The endpoint must follow the drag; a missed hit is not a successful pickup");
}

export async function checkCurveEditing({call,evaluate,settle}) {
  const visible=".curve-field:not([hidden])";
  const count=()=>evaluate(`document.querySelector('${visible} .curve-editor').querySelectorAll('circle').length`);
  const resetHidden=()=>evaluate(`document.querySelector('${visible} [data-action=curve-reset]').hidden`);
  const box=await evaluate(`(()=>{const b=document.querySelector('${visible} .curve-editor').getBoundingClientRect();return {x:b.left,y:b.top,width:b.width,height:b.height};})()`);
  const mouse=(type,p,clickCount=1)=>call("Input.dispatchMouseEvent",{type,...p,button:"left",buttons:type==="mouseReleased"?0:1,clickCount});
  const click=async(p,clickCount=1)=>{await mouse("mousePressed",p,clickCount);await mouse("mouseReleased",p,clickCount);await settle();};
  assert.equal(await resetHidden(),false,"An edited curve offers reset on the chart");
  await evaluate(`document.querySelector('${visible} [data-action=curve-reset]').click()`);await settle();
  assert.equal(await count(),2,"Reset removes every added point");
  assert.equal(await resetHidden(),true,"An unedited curve hides reset");
  const point={x:box.x+.4*box.width,y:box.y+.4*box.height};
  await click(point);assert.equal(await count(),3,"Click adds a point");
  await mouse("mousePressed",point,1);await mouse("mouseReleased",point,1);
  await mouse("mousePressed",point,2);await mouse("mouseReleased",point,2);await settle();
  assert.equal(await count(),2,"Double-click removes a point");
  await click(point);assert.equal(await count(),3);
  await mouse("mousePressed",point);
  await mouse("mouseMoved",{x:point.x,y:box.y+box.height+60});await settle();
  assert.equal(await count(),2,"Dragging a point off the graph removes it while dragging");
  await mouse("mouseMoved",point);await settle();
  assert.equal(await count(),3,"Returning before release restores the point");
  await mouse("mouseMoved",{x:point.x,y:box.y+box.height+60});
  await mouse("mouseReleased",{x:point.x,y:box.y+box.height+60});await settle();
  assert.equal(await count(),2,"Releasing off the graph keeps the point removed");
}

export async function checkAdjustments({call,evaluate,settle}) {
  const directory="artifacts/ui/adjustments-web";
  await mkdir(directory,{recursive:true});
  const send=action=>evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const capture=async name=>{await wait('layerApp.app.brush_ready()');await settle();const shot=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,"base64"));};
  const wait=async(condition,timeout=120000)=>{const end=Date.now()+timeout;while(!await evaluate(condition))if(Date.now()>end)throw Error(`Timed out: ${condition}`);else await new Promise(resolve=>setTimeout(resolve,50));};
  const histogram=histogramJourney({evaluate,settle}).exact;
  await send({type:"set_theme",theme:"dark"});
  // Inserting above a selected clipping base must preserve the whole stack.
  await send({type:"layer",action:{op:"new",group:false,clipped:true}});
  const clip=await evaluate("Number(layerApp.state().layer_tools.editing_layer.id)");
  await send({type:"select_layer",id:1});
  await send({type:"effect",action:{op:"insert",effect:"heat_haze"}});
  assert.deepEqual(await evaluate("layerApp.state().layers.slice(1,3).map(l=>Number(l.id))"),[clip,1]);
  await send({type:"layer",action:{op:"delete_selected"}});
  await send({type:"select_layer",id:clip});
  await send({type:"layer",action:{op:"delete_selected"}});
  // Paint a colorful opaque fixture through the real input/renderer path.
  await wait('layerApp.app.brush_ready()');
  const blank=await histogram();
  await send({type:"set_brush_size",value:260});
  for(const [i,color] of [[.9,.12,.08,1],[.08,.7,.15,1],[.1,.2,.9,1]].entries()) {
    await send({type:"set_color",rgba:color});
    const x=570+i*160,y=460;
    await call("Input.dispatchMouseEvent",{type:"mousePressed",x,y,button:"left",buttons:1,clickCount:1});
    await call("Input.dispatchMouseEvent",{type:"mouseMoved",x:x+40,y:y+180,button:"left",buttons:1});
    await call("Input.dispatchMouseEvent",{type:"mouseReleased",x:x+40,y:y+180,button:"left",buttons:0,clickCount:1});
    await settle();
  }
  assert.notDeepEqual(await histogram(),blank,'The filter fixture contains rendered paint');
  await wait("layerApp.startupTimes.complete!==null");
  await evaluate("document.querySelector('.dock-tab[data-panel=adjustments]').click()");
  await evaluate(`new Promise((resolve,reject)=>{
    const deadline=performance.now()+20000;
    const ready=()=>{
      const canvas=document.querySelector('.filter-row canvas');
      if(canvas?.getContext('2d').getImageData(0,0,200,40).data.some((v,i)=>i%4===3&&v>0))resolve(true);
      else if(performance.now()>deadline)reject(Error('GPU filter preview timed out'));
      else setTimeout(ready,100);
    };ready();
  })`);
  await capture("01-adjustments");
  assert.ok(await evaluate("document.querySelector('.filter-row').getBoundingClientRect().width>160"));
  assert.equal(await evaluate("document.querySelector('.filter-row canvas').getBoundingClientRect().height"),40);
  assert.ok(await evaluate("document.querySelector('.filter-row canvas').getContext('2d').getImageData(0,0,200,40).data.some((v,i)=>i%4===3&&v>0)"),"GPU preview pixels reach the visible row");
  await send({type:"filter_picker",action:{op:"category",category:"distort"}});
  await send({type:"filter_picker",action:{op:"toggle_search"}});
  await evaluate("(()=>{const input=document.querySelector('.filter-picker-header input');input.value='glass';input.dispatchEvent(new Event('input',{bubbles:true}));})()");
  assert.deepEqual(await evaluate("layerApp.state().adjustments.map(c=>c.id)"),["glass","rainy_glass"]);
  assert.equal(await evaluate("document.querySelector('[data-effect=glass] .filter-animation')!==null"),false);
  assert.equal(await evaluate("document.querySelector('[data-effect=rainy_glass] .filter-animation')!==null"),true);
  assert.ok(await evaluate("(()=>{const label=document.querySelector('[data-effect=rainy_glass] > span'),mark=label.querySelector('svg').getBoundingClientRect(),range=document.createRange();range.selectNodeContents(label.lastChild);const text=range.getBoundingClientRect();return Math.abs(mark.top+mark.height/2-text.top-text.height/2)<4&&mark.right<text.left;})()"),"animation marker precedes the name on the same line");
  await evaluate("new Promise(resolve=>setTimeout(resolve,600))");await capture("filtered-glass");
  await send({type:"filter_picker",action:{op:"toggle_search"}});
  await send({type:"filter_picker",action:{op:"category",category:null}});
  const choices=await evaluate("layerApp.state().adjustments.map(x=>x.id)");
  assert.equal(choices.length,42);
  assert.deepEqual(choices.filter(id=>["solid_color","gradient_fill"].includes(id)),["solid_color","gradient_fill"],"The picker lists both fill generators");
  for(const [index,effect] of choices.entries()) {
    await evaluate("document.querySelector('.dock-tab[data-panel=adjustments]').click()");
    await evaluate(`document.querySelector('[data-effect="${effect}"]').click()`);await settle();
    const view=await evaluate("JSON.parse(JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v==='bigint'?Number(v):v))");
    assert.ok(view.controls.length>0);
    if(effect==="color_balance")assert.deepEqual(await evaluate("Array.from(document.querySelectorAll('.property-section'),e=>e.textContent)"),["Shadows","Midtones","Highlights"]);
    assert.equal(await evaluate("document.querySelector('.effect-properties').getClientRects().length>0"),true);
    for(const color of view.controls.filter(c=>c.kind.kind==="color")) {
      assert.ok(color.color_action,`${effect}: ${color.key} offers the current colour`);
      assert.equal(await evaluate(`document.querySelector('.effect-properties [data-action="${color.key.replaceAll("_","-")}-bucket"]')?.closest('.property-row').firstChild.textContent`),color.label,`${effect}: ${color.key} keeps its labelled row`);
    }
    if(["solid_color","gradient_fill"].includes(effect)) {
      const layer=await evaluate(`JSON.parse(JSON.stringify(layerApp.state().layers.find(l=>Number(l.id)===${view.layer}),(_,v)=>typeof v==='bigint'?Number(v):v))`);
      assert.equal(layer.has_mask,true,`${effect}: a fill layer starts with a reveal-all mask`);
    }
    if(effect==="solid_color")assert.deepEqual(view.controls[0].value.value,await evaluate("JSON.parse(JSON.stringify(layerApp.state().colors.foreground))"),"Solid Color starts in the current colour");
    const curve=view.controls.find(c=>c.kind.kind==="curve");
    const number=view.controls.find(c=>c.kind.kind==="number");
    const gradient=view.controls.find(c=>c.kind.kind==="gradient");
    if(curve) {
      await checkCurveEndpoint({call,evaluate,settle});
      await checkCurveEditing({call,evaluate,settle});
      await send({type:"effect",action:{op:"curve_point",layer:view.layer,key:curve.key,index:null,point:[.45,.65],remove:false}});
    }
    else if(number) {
      const value=()=>evaluate(`layerApp.state().layer_properties.controls.find(c=>c.key===${JSON.stringify(number.key)}).value.value`);
      await send({type:"effect",action:{op:"set",layer:view.layer,key:number.key,value:{kind:"number",value:number.kind.numeric.min}}});
      assert.equal(await value(),number.kind.numeric.min,`${effect}: numeric minimum`);
      await send({type:"effect",action:{op:"reset",layer:view.layer,key:number.key}});
      assert.equal(await value(),number.value.value,`${effect}: numeric default`);
    }
    if(gradient) {
      await evaluate("document.querySelector('.gradient-ramp').click()");
      const target=await evaluate(`layerApp.state().layer_properties.controls.find(c=>c.key===${JSON.stringify(gradient.key)}).gradient.destination`);
      await send({type:"effect",action:{op:"gradient",target,edit:{kind:"stop",index:null,position:.5,color:{space:"Srgb",rgba:[.8,.2,.1,1]},remove:false}}});
      assert.equal(await evaluate("document.querySelectorAll('.gradient-stops button').length"),3);
      if(view.controls.some(c=>c.key==="amount"))await send({type:"effect",action:{op:"reset",layer:view.layer,key:"amount"}});
    }
    await capture(`${String(index+2).padStart(2,"0")}-${effect}`);
    await send({type:"set_layer_visibility",id:view.layer,visible:false});
  }
  await send({type:"customize",action:{type:"set_panel_visible",panel:"stats",visible:true}});
  await send({type:"move_panel",panel:"stats",viewport:[1440,1000],target:{kind:"float",position:[720,120]}});
  const editing=await evaluate("Number(layerApp.state().layer_properties.layer)");
  await send({type:"set_layer_visibility",id:editing,visible:true});
  for(let i=0;i<20;i++){await send({type:"set_layer_opacity",id:editing,opacity:.8+i*.005});await settle();}
  await evaluate(`new Promise((resolve,reject)=>{
    const deadline=performance.now()+10000;
    const ready=()=>{
      const panel=document.querySelector('.renderer-stats');
      if(panel.getBoundingClientRect().height>0 && panel.children.length===layerApp.app.renderer_stats().rows.length+2)resolve(true);
      else if(performance.now()>deadline)reject(Error('Diagnostics rows and chart did not render'));
      else setTimeout(ready,100);
    };ready();
  })`);
  assert.equal(await evaluate("document.querySelector('.renderer-chart').getBoundingClientRect().height"),46);
  const stats=await evaluate("JSON.parse(JSON.stringify(layerApp.app.renderer_stats(),(_,v)=>typeof v==='bigint'?Number(v):v))");assert.ok(stats.samples.length>0,JSON.stringify({stats,layout:await evaluate("JSON.stringify(layerApp.app.layout(innerWidth,innerHeight),(_,v)=>typeof v==='bigint'?Number(v):v)"),error:await evaluate("document.querySelector('#status')?.textContent")}));
  const order=stats.rows.map(row=>row.label);order.splice(Number(stats.chart_after_rows),0,"chart");order.push("stroke-recording");
  assert.deepEqual(await evaluate("Array.from(document.querySelector('.renderer-stats').children,child=>child.matches('.renderer-chart')?'chart':child.dataset.control??child.firstChild.textContent)"),order);
  await capture("stats-dark");await send({type:"set_theme",theme:"light"});await capture("stats-light");
  console.log(`PASS: ${choices.length} categorized filters, GPU previews, search, insertion/properties, controls, GPU rendering and live telemetry`);
}

export async function checkCurves({call,evaluate,settle}) {
  const directory=process.env.LAYER_IMAGE_CAPTURE_DIR??'artifacts/ui/curves-web';
  await mkdir(directory,{recursive:true});
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+25000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+": "+document.body.innerText.slice(-1200)));else setTimeout(poll,40)}poll()})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const properties=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v=== "bigint"?Number(v):v))');
  const invoke=async command=>{await wait(`layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)})?.enabled`);await send({type:'invoke',command});};
  const pointer=(type,p,pointerType='mouse')=>call('Input.dispatchMouseEvent',{type,...p,pointerType,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,force:type==='mouseReleased'?0:.65});
  const activateNumber=async axis=>{
    const point=await evaluate(`(()=>{const root=document.querySelector('[data-curve-axis="${axis}"]'),entry=root.querySelector('.number-entry'),button=root.querySelector('.number-value'),n=button&&!button.hidden?button:entry;const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await pointer('mousePressed',point);await pointer('mouseReleased',point);await settle();
  };
  const graphPoint=async index=>evaluate(`(()=>{const g=document.querySelector('.curve-editor'),c=g.querySelectorAll('circle')[${index}],p=new DOMPoint(c.cx.baseVal.value,c.cy.baseVal.value).matrixTransform(g.getScreenCTM());return{x:p.x,y:p.y}})()`);
  const curves=view=>view.controls.filter(c=>c.curve).map(c=>({key:c.key,value:c.value}));
  await wait('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  await send({type:'effect',action:{op:'insert',effect:'curves'}});
  try {
    for(const width of [1100,640]) {
      await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});
      await evaluate(`for(const {id} of layerApp.state().workspace.layout.panels)layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:id,visible:['toolbar','commands','properties'].includes(id)}})`);await settle();
      for(const theme of ['light','dark']) {
        await send({type:'set_theme',theme});
        await wait('document.querySelector(".curve-editor")&&document.querySelector("[data-properties-page]")');
        let view=await properties();
        assert.ok(view.pages.length>=4,'Curves exposes shared channel pages');
        const page=view.pages.at(-1).id;
        await evaluate(`(()=>{const n=document.querySelector('[data-properties-page]');n.value=${JSON.stringify(page)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);await settle();
        assert.equal((await properties()).page,page);
        view=await properties();const control=view.controls.find(c=>c.curve);
        const empty=curves(view);
        const emptyAt=await evaluate(`(()=>{const g=document.querySelector('.curve-editor'),p=new DOMPoint(100,100).matrixTransform(g.getScreenCTM());return{x:p.x,y:p.y}})()`);
        for(const clickCount of [1,2]) {
          await call('Input.dispatchMouseEvent',{type:'mousePressed',...emptyAt,button:'left',buttons:1,clickCount});
          await call('Input.dispatchMouseEvent',{type:'mouseReleased',...emptyAt,button:'left',buttons:0,clickCount});
        }
        await settle();
        assert.equal((await properties()).controls.find(c=>c.key===control.key).value.value.length,3,'Double-click on empty graph inserts one point instead of removing its unpublished insert');
        await invoke('undo');assert.deepEqual(curves(await properties()),empty,'Empty double-click insertion is one undo step');
        await send({type:'effect',action:{op:'curve_point',layer:view.layer,key:control.key,index:null,point:[.123456789,.4],remove:false}});
        view=await properties();const current=view.controls.find(c=>c.key===control.key);
        const index=current.value.value.findIndex(p=>p[0]>0&&p[0]<1);
        await send({type:'effect',action:{op:'curve_select_point',layer:view.layer,key:control.key,epoch:view.epoch,index}});
        const before=curves(await properties());
        const point=await graphPoint(index);
        await pointer('mousePressed',{x:point.x+3,y:point.y+2});await pointer('mouseReleased',{x:point.x+3,y:point.y+2});await settle();
        assert.deepEqual(curves(await properties()),before,'A nearby knot click preserves exact stored coordinates');
        for(const axis of ['input','output']) {
          const selector=`[data-curve-axis="${axis}"] .number-entry`;
          await wait(`document.querySelector(${JSON.stringify(selector)})`);
          await activateNumber(axis);
          await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
          await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
          assert.deepEqual(curves(await properties()),before,'Committing presented numeric text preserves point bits');
        }
        for(const step of [false,true]) {
          const old=curves(await properties());
          await activateNumber('output');
          await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});
          await activateNumber('input');
          await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});await settle();
          const changed=curves(await properties());
          assert.notDeepEqual(changed,old,'Held Output arrow commits when a native pointer enters another numeric control');
          if(step) {
            for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});await settle();
            const stepped=curves(await properties());
            assert.notEqual(stepped.find(c=>c.key===control.key).value.value[index][0],old.find(c=>c.key===control.key).value.value[index][0],'Focused Input keyboard step starts its own edit after prior Output key release');
            await invoke('undo');
          }
          await invoke('undo');assert.deepEqual(curves(await properties()),old,'Cross-control pointer transition preserves separate atomic gestures');
        }
        for(const pointerType of ['mouse','pen']) {
          const start=await graphPoint(index),end={x:start.x,y:start.y-12};
          await pointer('mousePressed',start,pointerType);await pointer('mouseMoved',end,pointerType);await pointer('mouseReleased',end,pointerType);await settle();
          const moved=curves(await properties());assert.notDeepEqual(moved,before,'Native drag changes the shared curve');
          await invoke('undo');assert.deepEqual(curves(await properties()),before,'A complete contact creates one undo step');
          await invoke('redo');assert.deepEqual(curves(await properties()),moved);
          await invoke('undo');
        }
        await evaluate('document.querySelector(".curve-editor").focus()');
        for(const repeat of [false,true,true])await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38,autoRepeat:repeat});
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});await settle();
        assert.notDeepEqual(curves(await properties()),before,'Native held arrow updates selected point');
        await invoke('undo');assert.deepEqual(curves(await properties()),before,'Held arrow repeats create one undo step');
        for(const method of ['Delete','right','double']) {
          const at=await graphPoint(index);
          if(method==='Delete') {
            await evaluate('document.querySelector(".curve-editor").focus()');
            await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Delete',code:'Delete',windowsVirtualKeyCode:46});
            await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Delete',code:'Delete',windowsVirtualKeyCode:46});
          } else if(method==='right') {
            await call('Input.dispatchMouseEvent',{type:'mousePressed',...at,button:'right',buttons:2,clickCount:1});
            await call('Input.dispatchMouseEvent',{type:'mouseReleased',...at,button:'right',buttons:0,clickCount:1});
          } else {
            for(const clickCount of [1,2]) {
              await call('Input.dispatchMouseEvent',{type:'mousePressed',...at,button:'left',buttons:1,clickCount});
              await call('Input.dispatchMouseEvent',{type:'mouseReleased',...at,button:'left',buttons:0,clickCount});
            }
          }
          await settle();assert.equal((await properties()).controls.find(c=>c.key===control.key).value.value.length,2,`${method} removes an interior marker`);
          await invoke('undo');assert.deepEqual(curves(await properties()),before,`${method} removal is one undo step`);
        }
        const canceledTouch=await graphPoint(index);
        await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...canceledTouch,id:91,radiusX:1,radiusY:1,force:.65}]});
        await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:canceledTouch.x,y:canceledTouch.y-10,id:91,radiusX:1,radiusY:1,force:.65}]});
        await call('Input.dispatchTouchEvent',{type:'touchCancel',touchPoints:[]});await settle();
        assert.deepEqual(curves(await properties()),before,'Native touch cancellation restores the captured curve');
        const touchStart=await graphPoint(index),touchEnd={x:touchStart.x,y:touchStart.y-10};
        await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{...touchStart,id:91,radiusX:1,radiusY:1,force:.65}]});
        await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...touchEnd,id:91,radiusX:1,radiusY:1,force:.65}]});
        await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
        assert.notDeepEqual(curves(await properties()),before,'Native touch drag changes the curve');
        await invoke('undo');assert.deepEqual(curves(await properties()),before,'Touch contact creates one undo step');
        const start=await graphPoint(index);
        await pointer('mousePressed',start);await pointer('mouseMoved',{x:start.x+8,y:start.y-8});
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
        await pointer('mouseReleased',start);await settle();
        assert.deepEqual(curves(await properties()),before,'Escape cancels the held curve contact');
        assert.equal((await properties()).page,page,'Publishing curve edits retains the selected page');
        const textWidth=await evaluate(`(()=>{const n=document.querySelector('[data-curve-axis="output"] .number-entry'),s=getComputedStyle(n),c=document.createElement('canvas').getContext('2d');c.font=[s.fontStyle,s.fontWeight,s.fontSize,s.fontFamily].join(' ');return{text:n.value,content:n.getBoundingClientRect().width-parseFloat(s.paddingLeft)-parseFloat(s.paddingRight)-parseFloat(s.borderLeftWidth)-parseFloat(s.borderRightWidth),needed:c.measureText(n.value).width}})()`);
        assert.equal(textWidth.text,'102.000','The precise Output fixture includes all seven displayed characters');
        assert.ok(textWidth.content>=textWidth.needed,`Full Output text fits at ${width}px: ${JSON.stringify(textWidth)}`);
        console.log('Curves precise Output width',theme,width,textWidth);
        const shot=await call('Page.captureScreenshot',{format:'png'});
        await writeFile(`${directory}/${theme}-${width}.png`,Buffer.from(shot.data,'base64'));
        await send({type:'effect',action:{op:'curve_point',layer:view.layer,key:control.key,index,point:current.value.value[index],remove:true}});
      }
    }
    await call('Emulation.setDeviceMetricsOverride',{width:1100,height:800,deviceScaleFactor:1,mobile:false});
    await invoke('new_document');
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===layerApp.app.editor_models(innerWidth,innerHeight).document_options.discard_label)?.click()`);
    await wait('document.querySelector("[data-document-field=depth]")');
    await evaluate(`(()=>{const n=document.querySelector('[data-document-field=depth]');n.value='F32';n.dispatchEvent(new Event('change',{bubbles:true}));document.querySelector('[data-document-action=create]').click()})()`);
    await wait('!layerApp.state().document_file.busy&&layerApp.app.document_color().depth==="F32"&&layerApp.app.brush_ready()');
    await send({type:'effect',action:{op:'insert',effect:'curves'}});
    let hdr=await properties();const hdrControl=hdr.controls.find(c=>c.curve);
    assert.ok(hdrControl,`Curves insert publishes its shared field: ${JSON.stringify(hdr)} ${await evaluate('document.body.innerText.slice(-800)')}`);
    assert.equal(hdrControl.curve.domain.kind,'log_hdr');
    await send({type:'effect',action:{op:'curve_point',layer:hdr.layer,key:hdrControl.key,index:null,point:[.2,.2],remove:false}});
    hdr=await properties();
    await send({type:'effect',action:{op:'curve_select_point',layer:hdr.layer,key:hdrControl.key,epoch:hdr.epoch,index:1}});
    const input='[data-curve-axis="output"] .number-entry';
    await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(input)});n.focus();n.value='1e-20';n.dispatchEvent(new Event('input',{bubbles:true}))})()`);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    const tiny=(await properties()).controls.find(c=>c.key===hdrControl.key);
    assert.ok(tiny.curve.output.value>0&&tiny.curve.output.value<1e-19,`Native expression commit preserves positive tiny HDR output: ${JSON.stringify(tiny.curve)}`);
    const exactTiny=curves(await properties());
    await evaluate(`document.querySelector(${JSON.stringify(input)}).focus()`);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});await settle();
    assert.deepEqual(curves(await properties()),exactTiny,'Escape cancels numeric held-key edits through native release');
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    assert.deepEqual(curves(await properties()),exactTiny,'Unchanged tiny HDR text does not quantize stored knot');
    console.log('Curves: HDR numeric cancel passed');
    const domain='[data-property-key="domain"] select';
    await wait(`document.querySelector(${JSON.stringify(domain)})`);
    for(const choice of ['0','1']) {
      await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(domain)});n.value=${JSON.stringify(choice)};n.dispatchEvent(new Event('change',{bubbles:true}))})()`);await settle();
      assert.equal((await properties()).controls.find(c=>c.curve).curve.domain.kind,choice==='0'?'encoded':'log_hdr','Native domain choice forwards shared policy');
      assert.deepEqual(curves(await properties()),exactTiny,'Changing display domain preserves curve graph knots');
    }
    console.log('Curves: native domain choices passed');
    await evaluate(`window.curvesFiles={open:window.showOpenFilePicker,save:window.showSaveFilePicker};
      window.showSaveFilePicker=async o=>({name:o.suggestedName,async createWritable(){return{async write(v){curvesFiles.bytes=new Uint8Array(v instanceof Blob?await v.arrayBuffer():v)},async close(){},async abort(){}}}});
      window.showOpenFilePicker=async()=>[{name:'curves.capy',async getFile(){return new File([curvesFiles.bytes],'curves.capy')}}];`);
    try {
      await invoke('save_document_as');await wait('!layerApp.state().document_file.busy&&!layerApp.state().document_file.modified');
      await invoke('open_document');await wait('!layerApp.state().document_file.busy&&layerApp.app.brush_ready()');
      await wait('layerApp.state().layer_properties.controls.some(c=>c.curve)');
      assert.deepEqual(curves(await properties()),exactTiny,'Native archive reopen preserves precise HDR knots');
    } finally {await evaluate('window.showOpenFilePicker=curvesFiles.open;window.showSaveFilePicker=curvesFiles.save;delete window.curvesFiles');}
    const hdrSlider='[data-property-key="hdr_stops"] .number-slider';
    await wait(`document.querySelector(${JSON.stringify(hdrSlider)})`);
    const stopsBefore=(await properties()).controls.find(c=>c.key==='hdr_stops').value;
    hdr=await properties();
    await send({type:'effect',action:{op:'curve_select_point',layer:hdr.layer,key:hdrControl.key,epoch:hdr.epoch,index:1}});
    await evaluate(`document.querySelector('[data-curve-axis="output"] .number-entry').focus()`);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});
    const heldCurve=curves(await properties());
    const otherSlider=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(hdrSlider)}).getBoundingClientRect();return{x:r.left+r.width*.6,y:r.top+r.height*.5}})()`);
    await pointer('mousePressed',otherSlider);await pointer('mouseReleased',otherSlider);
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'ArrowUp',code:'ArrowUp',windowsVirtualKeyCode:38});await settle();
    assert.notDeepEqual((await properties()).controls.find(c=>c.key==='hdr_stops').value,stopsBefore,'Held coordinate key yields native ownership to ordinary slider');
    await invoke('undo');assert.deepEqual((await properties()).controls.find(c=>c.key==='hdr_stops').value,stopsBefore);
    assert.deepEqual(curves(await properties()),heldCurve,'Slider Undo preserves the prior held coordinate edit');
    await invoke('undo');assert.deepEqual(curves(await properties()),exactTiny,'Prior held key has its own Undo');
    const stopsPosition=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(hdrSlider)}),r=n.getBoundingClientRect();window.curvesStopsSlider=n;return{x:r.x+Number(n.value)*r.width,y:r.y+r.height/2,end:r.x+r.width*.65}})()`);
    await pointer('mousePressed',{x:stopsPosition.x,y:stopsPosition.y});
    for(const x of [stopsPosition.x+(stopsPosition.end-stopsPosition.x)*.5,stopsPosition.end]) {
      await pointer('mouseMoved',{x,y:stopsPosition.y});await settle();
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(hdrSlider)})===window.curvesStopsSlider`),true,'HDR domain publication preserves captured ordinary slider DOM identity');
    }
    await pointer('mouseReleased',{x:stopsPosition.end,y:stopsPosition.y});await settle();
    assert.notDeepEqual((await properties()).controls.find(c=>c.key==='hdr_stops').value,stopsBefore,'Native HDR stops drag changes its shared parameter');
    await invoke('undo');assert.deepEqual((await properties()).controls.find(c=>c.key==='hdr_stops').value,stopsBefore,'HDR stops drag commits one undo step');
    assert.deepEqual(curves(await properties()),exactTiny,'HDR stops presentation preserves stored curve knots');
    await evaluate('delete window.curvesStopsSlider');
    hdr=await properties();
    await send({type:'effect',action:{op:'curve_select_point',layer:hdr.layer,key:hdrControl.key,epoch:hdr.epoch,index:0}});
    assert.equal(await evaluate(`document.querySelector('[data-curve-axis="input"] .number-entry').disabled`),true,'Endpoint Input is readonly in native control');
    hdr=await properties();
    await send({type:'effect',action:{op:'curve_select_point',layer:hdr.layer,key:hdrControl.key,epoch:hdr.epoch,index:1}});
    await evaluate(`(()=>{const n=document.querySelector('[data-curve-axis="output"] .number-entry');n.focus();n.value='1/0';n.dispatchEvent(new Event('input',{bubbles:true}))})()`);
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});await settle();
    assert.equal(await evaluate(`document.querySelector('[data-curve-axis="output"] .number-entry').getAttribute('aria-invalid')`),'true','Invalid expression remains editable with native refusal');
    assert.deepEqual(curves(await properties()),exactTiny,'Invalid expression does not mutate stored knots');
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();

    await send({type:'effect',action:{op:'insert',effect:'brightness_contrast'}});
    const numberView=await properties(),numberControl=numberView.controls.find(c=>c.kind.kind==='number'&&c.kind.numeric.kind==='slider');
    assert.ok(numberControl,'Generic Properties fixture has an ordinary shared slider');
    const numberSelector=`[data-property-key="${numberControl.key}"] .number-slider`;
    console.log('Curves: generic number control',numberControl.key);
    await wait(`document.querySelector(${JSON.stringify(numberSelector)})`);
    const numberValue=async()=>(await properties()).controls.find(c=>c.key===numberControl.key).value;
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});
      console.log('Curves: generic theme',theme,await evaluate(`JSON.stringify({keys:layerApp.state().layer_properties.controls.map(c=>c.key),dom:[...document.querySelectorAll('[data-property-key]')].map(n=>n.dataset.propertyKey)})`));
      await wait(`document.querySelector(${JSON.stringify(numberSelector)})`);
      const original=await numberValue();
      const slider=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(numberSelector)}),r=n.getBoundingClientRect();return{x:r.x+Number(n.value)*r.width,y:r.y+r.height/2,end:r.x+r.width*.8}})()`);
      await pointer('mousePressed',{x:slider.x,y:slider.y});
      for(const x of [slider.x+(slider.end-slider.x)*.4,slider.end])await pointer('mouseMoved',{x,y:slider.y});
      await pointer('mouseReleased',{x:slider.end,y:slider.y});await settle();
      const edited=await numberValue();assert.notDeepEqual(edited,original,'Native generic slider changes the shared value');
      await invoke('undo');assert.deepEqual(await numberValue(),original,'Generic slider drag makes exactly one undo step');
      await invoke('redo');assert.deepEqual(await numberValue(),edited);await invoke('undo');
      await pointer('mousePressed',{x:slider.x,y:slider.y});await pointer('mouseMoved',{x:slider.end,y:slider.y});
      await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
      await pointer('mouseReleased',{x:slider.end,y:slider.y});await settle();
      assert.deepEqual(await numberValue(),original,'Escape cancels generic numeric pointer gesture through release');
      assert.equal(await evaluate(`document.getElementById('workspace').classList.contains('zen-hidden')`),false,'Numeric Escape cancels only its gesture and keeps ordinary panels visible');
      await invoke('redo');assert.deepEqual(await numberValue(),edited,'Canceled generic slider preserves redo');await invoke('undo');
    }
    await send({type:'effect',action:{op:'insert',effect:'levels'}});
    assert.equal((await properties()).pages.length,1,'Current Levels has only its implemented master page');
    assert.equal(await evaluate(`document.querySelector('[data-properties-page]').hidden`),true,'A singleton Properties page has no redundant chooser');
    console.log('PASS: focused Curves pages, exact unchanged numbers, native mouse/pen/touch contacts, one-step undo/redo, Escape, tiny HDR and native archive reopen in both themes at wide and narrow sizes');
  } finally {await call('Emulation.clearDeviceMetricsOverride');}
}
