import assert from "node:assert/strict";
import { mkdir, writeFile, readFile } from "node:fs/promises";

export async function checkRuntimeFilters({call,evaluate,settle,host}) {
  const wait=()=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){const s=layerApp.state();if(!s.filter_load.pending&&s.filter_catalog_revision>0n)resolve(s.filter_load.error??null);else if(performance.now()-start>15000)reject(Error('Filter validation timed out'));else setTimeout(poll,20);}poll();})`);
  assert.equal(await wait(),null);
  assert.equal(await evaluate("layerApp.state().adjustments.length"),40);
  await evaluate("layerApp.loadFilters('./runtime-filter/manifest.json')");
  assert.equal(await wait(),null);
  assert.equal(await evaluate("layerApp.state().adjustments.length"),41);
  await evaluate(`(()=>{const pixels=new Uint8Array(1024*768*4);for(let i=0;i<1024*768;i++)pixels.set((Math.floor(i%1024/32)+Math.floor(i/1024/32))%2?[30,160,220,255]:[230,50,80,255],i*4);layerApp.app.import_layer_image('Runtime checker',1024,768,pixels);layerApp.wake();})()`);
  await settle();
  await evaluate("layerApp.dispatch({type:'filter_picker',action:{op:'category',category:'examples'}});layerApp.dispatch({type:'select_panel_tab',group:8,panel:'adjustments'})");
  await settle();
  assert.equal(await evaluate("document.querySelector('.filter-picker select').selectedOptions[0].textContent"),"Examples");
  await evaluate("document.querySelector('[data-effect=\"example:tent_blur\"]').click()");
  await settle();
  assert.equal(await evaluate("layerApp.state().layer_properties.controls[0].label"),"Radius");
  await evaluate("layerApp.dispatch({type:'effect',action:{op:'set',layer:layerApp.state().layer_properties.layer,key:'radius',value:{kind:'number',value:9}}})");
  const manifestPath=`${host.filterFixture}/manifest.json`,preparePath=`${host.filterFixture}/prepare.wgsl`;
  const manifest=JSON.parse(await readFile(manifestPath,"utf8"));
  manifest.filters[0].program.parameters[0].label="Runtime radius";
  const original=await readFile(preparePath,"utf8");
  // Replace the actual served algorithm and metadata, not Wasm/application code.
  await writeFile(preparePath,original.replace("max(width-f32(i),0.)/(width*width)","select(0.,1./(2.*width-1.),i<=radius)"));
  await writeFile(manifestPath,JSON.stringify(manifest));
  await evaluate("layerApp.loadFilters('./runtime-filter/manifest.json','replace')");
  assert.equal(await wait(),null);
  await settle();
  assert.equal(await evaluate("layerApp.state().layer_properties.controls[0].label"),"Runtime radius");
  assert.equal(await evaluate("layerApp.state().layer_properties.controls[0].value.value"),9);
  // A resource catalog can contain both an existing definition and a new ID,
  // even when neither was part of the executable's embedded fallback.
  const added=structuredClone(manifest.filters[0]);
  added.program.id="example:second_tent";added.program.label="Second Tent Blur";
  manifest.filters.push(added);
  await writeFile(manifestPath,JSON.stringify(manifest));
  await evaluate("layerApp.loadFilters('./runtime-filter/manifest.json','merge')");
  assert.equal(await wait(),null);
  assert.ok(await evaluate("layerApp.state().adjustments.some(f=>f.id==='example:second_tent')"));
  assert.equal(await evaluate("layerApp.state().layer_properties.controls[0].value.value"),9);
  const revision=await evaluate("String(layerApp.state().filter_catalog_revision)");
  await writeFile(preparePath,"invalid preparation WGSL");
  await evaluate("layerApp.loadFilters('./runtime-filter/manifest.json','replace')");
  assert.match(await wait(),/WGSL|expected|invalid/i);
  assert.equal(await evaluate("String(layerApp.state().filter_catalog_revision)"),revision);
  await mkdir("artifacts/ui/runtime-filters-web",{recursive:true});
  const shot=await call("Page.captureScreenshot",{format:"png"});
  await writeFile("artifacts/ui/runtime-filters-web/runtime-properties.png",Buffer.from(shot.data,"base64"));
}

// The ordinary Wasm renderer API, driven once per browser frame. No benchmark
// GPU path or production instrumentation; GPU timings are the existing Stats.
export async function benchmarkFilters({evaluate}) {
  const choices=await evaluate("layerApp.app.state().adjustments.map(c=>c.id)");
  const expensive=["motion_blur","gaussian_blur","domain_warp","painterly","denoise"];
  const prepared=["pencil","soft_focus","bloom","gaussian_blur","unsharp_mask"];
  const cases=[["Baseline",[]],...choices.map(id=>[id,[id]]),["Five expensive",expensive],["Prepared edits",["unsharp_mask"]],["Five prepared edits",prepared]];
  const label=process.env.CAPY_FILTER_BENCHMARK_LABEL??"";
  if(label&&!/^[a-z0-9_-]+$/.test(label))throw new Error("Invalid benchmark label");
  const output=`artifacts/benchmarks/filter-web${label?`-${label}`:""}.json`;
  const report=[];
  for(const [label,filters] of cases) {
    const modes=label.endsWith("edits")?["relevant","unrelated"]:label==="Five expensive"?["local","full","animation"]:["local"];
    for(const mode of modes) {
      const result=await evaluate(`(${async function(filters,mode){
        const app=layerApp.app,send=a=>app.dispatch(a),effect=a=>send({type:"effect",action:a}),ids=[];
        send({type:"customize",action:{type:"set_panel_visible",panel:"stats",visible:true}});
        send({type:"select_layer",id:1});
        send({type:"set_brush_size",value:24});send({type:"set_color",rgba:[.2,.5,.7,1]});
        for(const id of filters){
          effect({op:"insert",effect:id});const view=app.state().layer_properties;ids.push(view.layer);
          if(view.controls.some(c=>c.key==="animate"))effect({op:"set",layer:view.layer,key:"animate",value:{kind:"toggle",value:mode==="animation"}});
          // Exercise non-neutral pointwise defaults as well as neighborhood work.
          if(id==="curves")effect({op:"curve_point",layer:view.layer,key:"curve_0",index:null,point:[.45,.65],remove:false});
          else {const c=view.controls.find(c=>c.kind.kind==="number"&&c.value.value===0&&c.key!=="time");
            if(c)effect({op:"set",layer:view.layer,key:c.key,value:{kind:"number",value:c.kind.numeric.max*.25}});}
        }
        send({type:"select_layer",id:1});
        const revision=app.state().camera.revision,frame=()=>new Promise(requestAnimationFrame),wall=[];
        const before=app.renderer_stats().rows.find(r=>r.label==="Frames").value;
        for(let i=0;i<180;i++){
          const now=await frame();
          const start=performance.now();
          if(mode==="local")app.pen(new Float64Array([777,i===0?1:i===179?3:2,600+i%80,500+Math.sin(i*.1)*35,.8,0,0,0,now,2,0]),revision);
          if(mode==="full")send({type:"set_layer_opacity",id:1,opacity:.7+(i%20)*.01});
          if(mode==="relevant"||mode==="unrelated")effect({op:"set",layer:ids[ids.length-1],key:mode==="relevant"?"sigma":"amount",value:{kind:"number",value:mode==="relevant"?2+(i%20)*.5:50+(i%20)*5}});
          app.frame(now,now+1000/120);if(i>=60)wall.push(performance.now()-start);
        }
        await frame();const stats=app.renderer_stats();
        for(const id of ids.reverse()){send({type:"select_layer",id});send({type:"layer",action:{op:"delete_selected"}});}
        send({type:"select_layer",id:1});
        wall.sort((a,b)=>a-b);
        return {stats,frames:Number(stats.rows.find(r=>r.label==="Frames").value)-Number(before),frame_cpu: [.5,.95,.99].map(q=>wall[Math.round((wall.length-1)*q)])};
      }})(${JSON.stringify(filters)},${JSON.stringify(mode)})`);
      assert.ok(result.frames>=150,`${label}: insufficient rendered updates (${result.frames})`);
      report.push({filter:label,mode,...result});
      console.log(`${label} ${mode}: ${result.stats.rows.slice(0,2).map(r=>r.label+" "+r.value).join("; ")}`);
      await mkdir("artifacts/benchmarks",{recursive:true});
      await writeFile(output,JSON.stringify(report,null,2));
    }
  }
}

export async function checkAdjustments({call,evaluate,settle}) {
  const directory="artifacts/ui/adjustments-web";
  await mkdir(directory,{recursive:true});
  const send=action=>evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
  const capture=async name=>{await settle();const shot=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${directory}/${name}.png`,Buffer.from(shot.data,"base64"));};
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
  await send({type:"set_brush_size",value:260});
  for(const [i,color] of [[.9,.12,.08,1],[.08,.7,.15,1],[.1,.2,.9,1]].entries()) {
    await send({type:"set_color",rgba:color});
    const x=570+i*160,y=460;
    await call("Input.dispatchMouseEvent",{type:"mousePressed",x,y,button:"left",buttons:1,clickCount:1});
    await call("Input.dispatchMouseEvent",{type:"mouseMoved",x:x+40,y:y+180,button:"left",buttons:1});
    await call("Input.dispatchMouseEvent",{type:"mouseReleased",x:x+40,y:y+180,button:"left",buttons:0,clickCount:1});
    await settle();
  }
  await evaluate("document.querySelector('.dock-tab[data-panel=adjustments]').click()");
  await evaluate("new Promise(resolve=>setTimeout(resolve,1500))");
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
  assert.equal(choices.length,40);
  for(const [index,effect] of choices.entries()) {
    await evaluate("document.querySelector('.dock-tab[data-panel=adjustments]').click()");
    await evaluate(`document.querySelector('[data-effect="${effect}"]').click()`);await settle();
    const view=await evaluate("JSON.parse(JSON.stringify(layerApp.state().layer_properties,(_,v)=>typeof v==='bigint'?Number(v):v))");
    assert.ok(view.controls.length>0);
    if(effect==="color_balance")assert.deepEqual(await evaluate("Array.from(document.querySelectorAll('.property-section'),e=>e.textContent)"),["Shadows","Midtones","Highlights"]);
    assert.equal(await evaluate("document.querySelector('.effect-properties').getClientRects().length>0"),true);
    const curve=view.controls.find(c=>c.kind.kind==="curve");
    const number=view.controls.find(c=>c.kind.kind==="number");
    const gradient=view.controls.find(c=>c.kind.kind==="gradient");
    if(curve) await send({type:"effect",action:{op:"curve_point",layer:view.layer,key:curve.key,index:null,point:[.45,.65],remove:false}});
    else if(number) await send({type:"effect",action:{op:"set",layer:view.layer,key:number.key,value:{kind:"number",value:number.kind.numeric.min}}});
    if(gradient) {
      await evaluate("document.querySelector('.gradient-ramp').click()");
      await send({type:"effect",action:{op:"gradient_stop",layer:view.layer,key:gradient.key,index:null,position:.5,color:[.8,.2,.1,1],remove:false}});
      assert.equal(await evaluate("document.querySelectorAll('.gradient-stops button').length"),3);
      await send({type:"effect",action:{op:"reset",layer:view.layer,key:"amount"}});
    }
    await capture(`${String(index+2).padStart(2,"0")}-${effect}`);
    await send({type:"set_layer_visibility",id:view.layer,visible:false});
  }
  await send({type:"customize",action:{type:"set_panel_visible",panel:"stats",visible:true}});
  await send({type:"move_panel",panel:"stats",viewport:[1440,1000],target:{kind:"float",position:[720,120]}});
  const editing=await evaluate("Number(layerApp.state().layer_properties.layer)");
  await send({type:"set_layer_visibility",id:editing,visible:true});
  for(let i=0;i<20;i++){await send({type:"set_layer_opacity",id:editing,opacity:.8+i*.005});await settle();}
  await evaluate("new Promise(resolve=>setTimeout(resolve,250))");
  assert.ok(await evaluate("document.querySelector('.renderer-stats').getBoundingClientRect().height>190"));
  const stats=await evaluate("layerApp.app.renderer_stats()");assert.ok(stats.samples.length>0,JSON.stringify({stats,layout:await evaluate("JSON.stringify(layerApp.app.layout(innerWidth,innerHeight),(_,v)=>typeof v==='bigint'?Number(v):v)"),error:await evaluate("document.querySelector('#status')?.textContent")}));
  await capture("stats-dark");await send({type:"set_theme",theme:"light"});await capture("stats-light");
  console.log(`PASS: ${choices.length} categorized filters, GPU previews, search, insertion/properties, controls, GPU rendering and live telemetry`);
}
