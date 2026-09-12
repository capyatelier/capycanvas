import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

export async function checkResizeRendering({call,evaluate,settle}) {
  const saved=await evaluate("layerApp.state().workspace"),output=process.env.LAYER_TEST_ARTIFACTS||"artifacts/workspace-resize";
  await mkdir(output,{recursive:true});
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,250))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const snapshot=()=>evaluate("layerApp.state().workspace");
  let device,point,held=false;
  const input=async(phase,p=point)=>{
    point=p;
    if(device==="touch")await call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd"}[phase],touchPoints:phase==="up"?[]:[{id:1,...p}]});
    else await call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[phase],...p,button:"left",buttons:phase==="up"?0:1,clickCount:1});
    held=phase!=="up";await wait();
  };
  const divider=()=>evaluate("(()=>{const b=layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.band&&d.id===40).bounds;return{x:b.x+b.width/2,y:b.y+b.height/2};})()");
  const rows=()=>evaluate("new Set(Array.from(document.querySelectorAll('.dock-group[data-group=\"41\"] .size-grid .size-button'),n=>Math.round(n.getBoundingClientRect().y))).size");
  try {
    for(const scale of [1,2]) {
      await call("Emulation.setDeviceMetricsOverride",{width:1440,height:1000,deviceScaleFactor:scale,mobile:false});
      for(device of ["mouse","touch"]) {
        const workspace=structuredClone(saved),tabs=(id,panels)=>({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
        Object.assign(workspace.layout,{bands:[{id:40,edge:"left",extent:310,root:tabs(41,["sizes"])},{id:42,edge:"right",extent:310,root:tabs(43,["navigator","layers"])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(workspace.layout.next_id,44)});
        workspace.zen_mode=false;await send({type:"restore_workspace",workspace});
        const before=await snapshot(),start=await divider(),wideRows=await rows();
        await evaluate("window.resizeControl=document.querySelector('.dock-group[data-group=\"41\"] .size-grid .size-button')");
        await input("down",start);await input("move",{x:start.x-140,y:start.y});
        assert.ok(await rows()>wideRows,"controls wrap live while the contact is held");
        assert.equal(await evaluate("resizeControl===document.querySelector('.dock-group[data-group=\"41\"] .size-grid .size-button')"),true);
        const clip=await evaluate("(()=>{const b=document.querySelector('.dock-group[data-group=\"41\"]').getBoundingClientRect();return{x:b.x+8,y:b.y+28,width:b.width-16,height:Math.min(500,b.height-40),scale:1};})()");
        const live=(await call("Page.captureScreenshot",{format:"png",clip})).data;
        await input("up");
        const finished=(await call("Page.captureScreenshot",{format:"png",clip})).data;
        const pixels=await evaluate(`(async()=>{const read=async data=>{const i=new Image();i.src='data:image/png;base64,'+data;await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const ctx=c.getContext('2d',{willReadFrequently:true});ctx.drawImage(i,0,0);return ctx.getImageData(0,0,c.width,c.height).data;};const a=await read(${JSON.stringify(live)}),b=await read(${JSON.stringify(finished)});let changed=0,max=0;for(let i=0;i<a.length;i++){const d=Math.abs(a[i]-b[i]);if(d>8)changed++;max=Math.max(max,d);}return{changed,max,channels:a.length};})()`);
        assert.ok(pixels.changed/pixels.channels<.001,"live reflow stays as sharp as a full refresh");
        await writeFile(`${output}/web-resize-${device}-${scale}x-live.png`,Buffer.from(live,"base64"));
        await writeFile(`${output}/web-resize-${device}-${scale}x-finished.png`,Buffer.from(finished,"base64"));
        const after=await snapshot();await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),before);await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snapshot(),after);

        // Cross the collapse boundary and reverse it with the same contact.
        const collapseStart=await divider(),expanded=await snapshot();
        await input("down",collapseStart);await input("move",{x:24,y:collapseStart.y});
        assert.ok((await snapshot()).layout.collapsed.some(c=>c.root===41),"live collapse");
        await input("move",{x:350,y:collapseStart.y});
        assert.equal((await snapshot()).layout.collapsed.some(c=>c.root===41),false,"same-contact expansion");
        await input("up");const reversed=await snapshot();
        await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),expanded);await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snapshot(),reversed);
        assert.equal(await evaluate("!!document.querySelector('.collapsed-column[data-column=\"41\"]')"),false);
        console.log(JSON.stringify({device,scale,pixels,liveReflow:true,collapseReverse:true}));
      }
    }
  } finally {
    if(held)await input("up");
    await call("Emulation.setDeviceMetricsOverride",{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
    await send({type:"restore_workspace",workspace:saved});
  }
  console.log("PASS: mouse/touch live wrapping, retained controls, sharp 1x/2x rendering, collapse/reverse, undo/redo");
}
