import assert from "node:assert/strict";
import { mkdir, writeFile, rename } from "node:fs/promises";
import { existsSync } from "node:fs";

// CDP input travels through Chromium's native pointer/capture machinery. No
// synthetic DOM pointer events or direct DragWorkspace dispatch in the probe.
export async function checkWorkspaceMotion({call, evaluate, settle}) {
  const native=process.argv.includes("--native-input"), inputDir=process.env.LAYER_NATIVE_INPUT_DIR;
  let nativeStep=0;
  const performNative=async events=>{
    const step=nativeStep++,path=`${inputDir}/step-${step}.json`;
    await writeFile(`${path}.tmp`,JSON.stringify(events));await rename(`${path}.tmp`,path);
    const deadline=Date.now()+15000;
    while(!existsSync(`${inputDir}/done-${step}`)){
      assert.ok(Date.now()<deadline,"compositor input timed out");await new Promise(r=>setTimeout(r,2));
    }
  };
  const saved = await evaluate("layerApp.state().workspace");
  const fixture = structuredClone(saved);
  const tabs = (id, panels) => ({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
  Object.assign(fixture.layout, {bands:[
    {id:40,edge:"left",extent:252,root:tabs(41,["brushes","sizes"])},
    {id:42,edge:"right",extent:310,root:tabs(43,["layers","properties","adjustments"])},
    {id:44,edge:"top",extent:36,root:tabs(45,["toolbar"])},
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,220))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  await evaluate("new Promise(resolve=>{const poll=()=>layerApp.startupTimes.complete?resolve():setTimeout(poll,50);poll();})");
  const results=[];
  let held=null, point;
  const nativeEvent=(type,p)=>held==="touch"?{touch:type,point:[p.x,p.y]}:
    type==="up"?{down:false}:{point:[p.x,p.y],...(type==="down"?{down:true}:{})};
  if(native)await writeFile(`${inputDir}/ready`,"ready");
  const input=async(type,p=point)=>{
    point=p;
    if(native)return performNative([nativeEvent(type,p)]);
    if(held==="touch") return call("Input.dispatchTouchEvent",{type:{down:"touchStart",move:"touchMove",up:"touchEnd",cancel:"touchCancel"}[type],touchPoints:["up","cancel"].includes(type)?[]:[{id:1,...p}]});
    return call("Input.dispatchMouseEvent",{type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[type],...p,button:"left",buttons:type==="up"?0:1,clickCount:1});
  };
  const snapshot=()=>evaluate("layerApp.state().workspace");
  try {
    for(const device of ["mouse","touch"]) for(const scenario of ["group","tab","tear-off","navigator"]) {
      const workspace=structuredClone(fixture);
      if(scenario==="navigator") {workspace.layout.bands[1].root.panels.push("navigator");workspace.layout.bands[1].root.active="navigator";}
      await send({type:"restore_workspace",workspace});
      if(["group","navigator"].includes(scenario))await send({type:"move_group",group:43,target:{kind:"float",position:[550,220]}});
      const before=await snapshot();
      const selector=["group","navigator"].includes(scenario)?'.dock-group[data-group="43"] .dock-tabs > .panel-grip':'.dock-group[data-group="43"] .dock-tab[data-panel="properties"]';
      const start=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
      held=device;await input("down",start);
      const origin=scenario==="tear-off"?{x:650,y:400}:scenario==="tab"?start:{x:start.x-14,y:start.y};
      if(scenario==="tab")await input("move",{x:start.x-14,y:start.y});
      await input("move",origin);await wait();
      await evaluate(`(()=>{
        const app=layerApp.app, original={};
        const probe=window.motionProbe={counts:{},cpu:[],frames:[],callbacks:[],placementCpu:[],moves:0,running:true,original,refreshStacks:[],raf:window.requestAnimationFrame};
        window.requestAnimationFrame=callback=>probe.raf.call(window,callback.name==="presentWorkspaceFrame"?now=>{const t=performance.now();callback(now);probe.callbacks.push(now);probe.placementCpu.push(performance.now()-t);}:callback);
        for(const name of ["state","layout","workspace_update","drop_hint","tab_drag_preview","dispatch","frame","navigator_surface","navigator_size"]){
          original[name]=app[name].bind(app);app[name]=(...args)=>{const t=performance.now();const value=original[name](...args);probe.counts[name]=(probe.counts[name]||0)+1;if(name==="frame"&&value.regions)probe.refreshStacks.push({regions:value.regions,revision:String(value.revision)});if(name==="state")probe.refreshStacks.push(new Error().stack);if(name==="dispatch"&&args[0].phase==="move"){probe.moves++;probe.cpu.push(performance.now()-t);}return value;};
        }
        const update=original.workspace_update(),id=update.drag.group?.id;
        probe.group=id;probe.node=id?document.querySelector('.dock-group[data-group="'+id+'"]'):document.querySelector('.tab-slide-overlay');
        probe.content=probe.node?.firstElementChild;probe.surface=probe.node?.querySelector(".navigator-surface");
        probe.revision=String(update.model_revision);
        const frame=t=>{if(!probe.running)return;const value=probe.node?.getAttribute('style')+Array.from(probe.node?.children||[],n=>n.getAttribute('style')).join();if(value!==probe.last){probe.frames.push(t);probe.last=value;}requestAnimationFrame(frame);};requestAnimationFrame(frame);
      })()`);
      const pending=[],began=performance.now();
      const triangle=t=>{const p=t%4;return p<1?p:p<3?2-p:p-4;};
      // Feed above the display rate; only publication may coalesce.
      for(let i=0;native?i<550:performance.now()-began<2200;i++){
        const t=native?i*.004:(performance.now()-began)/1000;
        const dx=scenario==="tab"?triangle(t*8)*40:triangle(t*4)*100;
        const dy=scenario==="tab"?0:triangle(t*3)*65;
        point={x:origin.x+dx,y:origin.y+dy};
        pending.push(native?nativeEvent("move",point):input("move",point));
        if(!native)await new Promise(r=>setTimeout(r,4));
      }
      if(native)await performNative(pending);else await Promise.all(pending);await settle();
      const probe=await evaluate(`(()=>{const p=motionProbe;p.running=false;const u=p.original.workspace_update();const b=p.node.getBoundingClientRect(),g=u.drag.group;const result={counts:p.counts,cpu:p.cpu,frames:p.callbacks,visualChanges:p.frames.length,placementCpu:p.placementCpu,moves:p.moves,retained:p.node.isConnected&&p.content===p.node.firstElementChild,revision:p.revision,finalRevision:String(u.model_revision),error:g?Math.max(Math.abs(b.x-g.bounds.x),Math.abs(b.y-g.bounds.y)):0};for(const [k,v]of Object.entries(p.original))layerApp.app[k]=v;window.requestAnimationFrame=p.raf;return result;})()`);
      assert.equal(probe.retained,true,`${device}/${scenario}: retain DOM and content`);
      if(probe.counts.state)console.log(await evaluate("motionProbe.refreshStacks"));
      assert.equal(probe.counts.state||0,0,`${device}/${scenario}: no full state serialization`);
      assert.equal(probe.counts.layout||0,0,`${device}/${scenario}: no full layout refresh`);
      assert.equal(probe.counts.drop_hint||0,0,"shared drop feedback");
      assert.equal(probe.counts.tab_drag_preview||0,0,"shared tab feedback");
      assert.equal(probe.revision,probe.finalRevision,"retain the Rust model revision");
      assert.ok(probe.error<=1,"native placement matches absolute shared geometry");
      assert.ok(probe.moves>100,"exercise sustained real input");
      if(scenario==="navigator") {
        assert.equal(probe.counts.navigator_size||0,0,"native overview needs no geometry publication during translation");
        assert.equal(probe.counts.navigator_surface||0,0,"retain the GPU surface");
        assert.equal(probe.counts.frame||0,0,"placement needs no GPU rendering");
        assert.ok(await evaluate(`(()=>{const p=motionProbe,c=p.node.querySelector('.navigator-surface'),b=c.getBoundingClientRect(),r=p.node.querySelector('.navigator-overview').getBoundingClientRect();return c===p.surface&&c.width===Math.round(b.width*devicePixelRatio)&&Math.abs(b.x-r.x)<1&&Math.abs(b.y-r.y)<1;})()`), "native-resolution GPU surface follows its panel");
      }
      const sorted=probe.cpu.sort((a,b)=>a-b),n=probe.frames.length;
      const placement=probe.placementCpu.sort((a,b)=>a-b);
      const result={device,scenario,input:native?"Wayland":"CDP",inputs:probe.moves,modelRefreshes:0,gpuFrames:probe.counts.frame||0,frames:n,visualChanges:probe.visualChanges,hz:n>1?(n-1)*1000/(probe.frames.at(-1)-probe.frames[0]):0,dispatchMs:{p50:sorted[Math.floor(sorted.length*.5)],p95:sorted[Math.floor(sorted.length*.95)]},placementMs:{p50:placement[Math.floor(placement.length*.5)],p95:placement[Math.floor(placement.length*.95)]}};
      results.push(result);console.log(JSON.stringify(result));
      if(process.env.LAYER_MOTION_MIN_HZ)assert.ok(result.hz>=Number(process.env.LAYER_MOTION_MIN_HZ), `${device}/${scenario}: ${result.hz.toFixed(1)} Hz`);
      if(scenario==="tab")await input("move",await evaluate("(()=>{const r=document.querySelector('.dock-group[data-group=\"43\"] .dock-tab[data-panel=\"layers\"]').getBoundingClientRect();return{x:r.x+r.width*.25,y:r.y+r.height/2}})()"));
      await input("up");held=null;await wait();
      const after=await snapshot();
      assert.notDeepEqual(after,before,"drop changes the workspace");
      await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snapshot(),before,"one undo restores the whole gesture");
      await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snapshot(),after,"redo restores the drop");
      assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay,.dragged-tab-source').length"),0);
    }
    await mkdir("artifacts/workspace-motion",{recursive:true});
    await writeFile("artifacts/workspace-motion/web.json",JSON.stringify(results,null,2));
    console.log("PASS: native mouse/touch motion, retained models, matching geometry, drop, undo and redo");
  } finally {
    await evaluate("if(window.motionProbe){motionProbe.running=false;for(const[k,v]of Object.entries(motionProbe.original))layerApp.app[k]=v;window.requestAnimationFrame=motionProbe.raf;}");
    if(held){await input("up");held=null;}
    await send({type:"restore_workspace",workspace:saved});
    if(native)await writeFile(`${inputDir}/finished`,"finished");
  }
}
