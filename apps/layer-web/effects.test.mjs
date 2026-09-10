import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";

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
