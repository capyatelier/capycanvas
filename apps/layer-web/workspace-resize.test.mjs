import assert from "node:assert/strict";
import { mkdir, writeFile, rename } from "node:fs/promises";
import { existsSync } from "node:fs";

// Observe actual panel widths on the browser's display clock. CDP uses native
// mouse/touch handling; the desktop runner can additionally feed OS input.
export async function checkWorkspaceResize({call,evaluate,settle}) {
  const native=process.argv.includes("--native-input"),dir=process.env.LAYER_NATIVE_INPUT_DIR;
  let step=0,held=null,point;
  const perform=async events=>{
    const path=`${dir}/step-${step}.json`;
    await writeFile(`${path}.tmp`,JSON.stringify(events));await rename(`${path}.tmp`,path);
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
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  if(native)await writeFile(`${dir}/ready`,"ready");
  const stop=()=>evaluate(`(()=>{const p=window.resizeProbe;if(!p)return;p.running=false;p.observer.disconnect();p.allocationObserver?.disconnect();for(const[k,v]of Object.entries(p.original))layerApp.app[k]=v;Node.prototype.cloneNode=p.clone;})()`);
  try {
    for(const device of ["mouse","touch"])for(const scenario of ["left","right","navigator"]) {
      const workspace=structuredClone(fixture);
      if(scenario==="navigator") {workspace.layout.bands[1].root.panels.push("navigator");workspace.layout.bands[1].root.active="navigator";}
      await send({type:"restore_workspace",workspace});
      const before=await snapshot(),group=scenario==="left"?41:43,id=scenario==="left"?40:42;
      const start=await evaluate(`(()=>{const b=layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.band&&d.id===${id}).bounds;return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
      held=device;await input("down",start);
      const origin={x:start.x+(scenario==="left"?20:-20),y:start.y};
      await input("move",origin);await wait();
      await evaluate(`(()=>{
        const app=layerApp.app,p=window.resizeProbe={original:{},counts:{},cpu:{},frames:[],widths:[],running:true,clones:0,added:0,removed:0,allocations:0,scaledFrames:0};
        p.node=document.querySelector('.dock-group[data-group="${group}"]');p.content=p.node.querySelector('.panel');p.surface=p.node.querySelector('.navigator-surface');
        if(p.surface){p.allocationObserver=new MutationObserver(records=>{p.allocations+=records.length;});p.allocationObserver.observe(p.surface,{attributes:true,attributeFilter:["width","height"]});}
        for(const name of ["state","layout","layout_update","workspace_update","dispatch","frame","reflow_navigators","editor_models","panel_view","workspace_projection","navigator_size","navigator_surface"]){
          if(typeof app[name]!=="function")continue;
          p.original[name]=app[name].bind(app);app[name]=(...args)=>{const t=performance.now(),v=p.original[name](...args);const key=name==="dispatch"?"dispatch:"+args[0].type:name;(p.cpu[key]??=[]).push(performance.now()-t);p.counts[key]=(p.counts[key]||0)+1;return v;};
        }
        p.clone=Node.prototype.cloneNode;Node.prototype.cloneNode=function(...args){p.clones++;return p.clone.apply(this,args);};
        p.observer=new MutationObserver(records=>{for(const r of records){p.added+=r.addedNodes.length;p.removed+=r.removedNodes.length;}});p.observer.observe(document.querySelector('#workspace'),{childList:true,subtree:true});
        const frame=t=>{if(!p.running)return;const width=p.node.getBoundingClientRect().width;if(width!==p.last){p.frames.push(t);p.widths.push(width);p.last=width;}if(p.surface){const r=p.surface.getBoundingClientRect();if(Math.abs(p.surface.width-r.width*devicePixelRatio)>1||Math.abs(p.surface.height-r.height*devicePixelRatio)>1)p.scaledFrames++;}requestAnimationFrame(frame);};requestAnimationFrame(frame);
      })()`);
      const pending=[],began=performance.now();
      for(let i=0;native?i<550:performance.now()-began<2200;i++) {
        const t=native?i*.004:(performance.now()-began)/1000,p=t*5%4,triangle=p<1?p:p<3?2-p:p-4;
        point={x:origin.x+triangle*65,y:origin.y};
        pending.push(native?event("move",point):input("move",point));
        if(!native)await new Promise(r=>setTimeout(r,4));
      }
      if(native)await perform(pending);else await Promise.all(pending);await settle();
      const probe=await evaluate(`(()=>{const p=resizeProbe,b=p.node.getBoundingClientRect(),expected=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.id===${group}).bounds;return{scale:devicePixelRatio,counts:p.counts,cpu:p.cpu,frames:p.frames,widths:p.widths,allocations:p.allocations,scaledFrames:p.scaledFrames,clones:p.clones,added:p.added,removed:p.removed,retained:p.node.isConnected&&p.content===p.node.querySelector('.panel'),error:Math.abs(b.width-expected.width),nativeSurface:!p.surface||(p.surface===p.node.querySelector('.navigator-surface')&&Math.abs(p.surface.width-p.surface.getBoundingClientRect().width*devicePixelRatio)<=1)};})()`);
      await stop();
      const stats=values=>{values.sort((a,b)=>a-b);return{count:values.length,p50:values[Math.floor(values.length*.5)],p95:values[Math.floor(values.length*.95)],total:values.reduce((a,b)=>a+b,0)};};
      const n=probe.frames.length,result={device,scenario,scale:probe.scale,input:native?"Wayland":"CDP",geometryHz:n>1?(n-1)*1000/(probe.frames.at(-1)-probe.frames[0]):0,changedFrames:n,widthRange:[Math.min(...probe.widths),Math.max(...probe.widths)],counts:probe.counts,bridgeMs:Object.fromEntries(Object.entries(probe.cpu).map(([k,v])=>[k,stats(v)])),allocations:probe.allocations,scaledFrames:probe.scaledFrames,clones:probe.clones,added:probe.added,removed:probe.removed,retained:probe.retained};
      assert.ok(n>20,"actual changing geometry");assert.ok(probe.error<=1,"native width matches Rust");assert.ok(probe.retained&&probe.nativeSurface,"retain native-resolution content/resources");
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
      const cancelStart=await evaluate(`(()=>{const b=layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.band&&d.id===${id}).bounds;return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
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
    const output=process.env.LAYER_TEST_ARTIFACTS||"artifacts/workspace-resize";
    await mkdir(output,{recursive:true});await writeFile(`${output}/web.json`,JSON.stringify(reports,null,2));
    console.log("PASS: changing panel geometry, native-resolution resources, mouse/touch, undo/redo");
  } finally {
    await stop();if(held)await input("up");
    await send({type:"restore_workspace",workspace:saved});
    if(native)await writeFile(`${dir}/finished`,"finished");
  }
}
