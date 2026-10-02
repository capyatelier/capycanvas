import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

export async function checkSpatialFilterWindows({call,evaluate,settle,canvasPixels}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/spatial-filter-windows-web';
  await mkdir(directory,{recursive:true});
  const wait=expression=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,50);}check();})`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const histogram=()=>evaluate(`(async()=>{const control=layerApp.app.capture_control();try{return JSON.parse(JSON.stringify((await layerApp.app.histogram(control)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v));}finally{control.free();}})()`);
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
    for(const theme of ['dark','light']) {
      await send({type:'set_theme',theme});await send({type:'set_zoom',zoom:.5});
      assert.equal(await evaluate('layerApp.state().camera.zoom'),.5);
      for(const [step,[center,sigma]] of [[[2400,1600],13],[[3300,2100],21],[[2400,1600],0],[[2400,1600],7]].entries()) {
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
        await writeFile(`${directory}/${theme}-${step}.png`,Buffer.from(shot.data,'base64'));
      }
    }
    console.log('PASS: 24 MP chained spatial filters at 50% zoom, pan round trips, support changes, light and dark themes');
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
  const histogram=()=>evaluate(`(async()=>{const control=layerApp.app.capture_control();try{
    return JSON.parse(JSON.stringify((await layerApp.app.histogram(control)).histogram,(_,v)=>typeof v==='bigint'?Number(v):v));
  }finally{control.free();}})()`);
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
      await send({type:"effect",action:{op:"gradient_stop",layer:view.layer,key:gradient.key,index:null,position:.5,color:{space:"Srgb",rgba:[.8,.2,.1,1]},remove:false}});
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
