import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
export async function checkWorkspaceRendering({call,evaluate,settle}) {
 const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/workspace-motion';await mkdir(dir,{recursive:true});
 const send=async a=>{await evaluate(`layerApp.dispatch(${JSON.stringify(a)})`);await settle();await evaluate('new Promise(r=>setTimeout(r,250))');};
 const saved=await evaluate('layerApp.state().workspace'),fixture=structuredClone(saved);
 const transparency=['off','low','medium','high'].indexOf(await evaluate('layerApp.state().settings.transparency'));
 await send({type:'preferences',action:{type:'edit',id:'transparency',value:0}});
 const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'name'});
 Object.assign(fixture.layout,{bands:[{id:40,edge:'left',extent:252,root:tabs(41,['brushes','sizes'])},{id:42,edge:'right',extent:360,root:tabs(43,['layers','properties','adjustments'])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],column_stacks:[],next_id:Math.max(44,fixture.layout.next_id)});fixture.zen_mode=false;
 for(const dpr of [1,2]) {
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:dpr,mobile:false});
  await checkActiveTabShade({call,evaluate,send,fixture,dir,dpr});
  await send({type:'restore_workspace',workspace:fixture});
  for(const group of [41,43])await send({type:'customize',action:{type:'set_column_collapsed',group,collapsed:true}});
  const alignment=await evaluate(`Array.from(document.querySelectorAll('.collapsed-column .column-tab, .collapsed-column > .panel-grip'),n=>{const b=n.getBoundingClientRect(),i=n.querySelector('svg').getBoundingClientRect();return {kind:n.className,error:i.x+i.width/2-b.x-b.width/2,transform:getComputedStyle(n.querySelector('svg')).transform};})`);
  assert.ok(alignment.length>=7);assert.ok(alignment.every(a=>Math.abs(a.error)<.01),JSON.stringify(alignment));
  assert.ok(alignment.filter(a=>a.kind.includes('panel-grip')).every(a=>a.transform.startsWith('matrix(0, 1, -1, 0,')));
  const collapsed=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/web-collapsed-${dpr}x.png`,Buffer.from(collapsed.data,'base64'));
  await send({type:'restore_workspace',workspace:fixture});
  await send({type:'move_group',group:43,target:{kind:'float',position:[550,220]}});
  const rect=()=>evaluate(`(()=>{const b=document.querySelector('.dock-group[data-group="43"]').getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height};})()`);
  const crop=async()=>{const b=await rect();return (await call('Page.captureScreenshot',{format:'png',clip:{x:b.x+12,y:b.y+42,width:210,height:200,scale:1}})).data;};
  const initial=await rect(),before=await crop();
  const start=await evaluate(`(()=>{const b=document.querySelector('.dock-group[data-group="43"] .dock-tabs>.panel-grip').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
  await call('Input.dispatchMouseEvent',{type:'mousePressed',...start,button:'left',buttons:1,clickCount:1});
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:start.x+100,y:start.y+100,button:'left',buttons:1});await settle();
  const during=await crop();
  const difference=await evaluate(`(async()=>{const load=async s=>{const i=new Image();i.src='data:image/png;base64,'+s;await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);return x.getImageData(0,0,c.width,c.height).data;};const a=await load(${JSON.stringify(before)}),b=await load(${JSON.stringify(during)});let changed=0,max=0;for(let i=0;i<a.length;i++){if(a[i]!==b[i])changed++;max=Math.max(max,Math.abs(a[i]-b[i]));}return{channels:a.length,changed,max};})()`);
  await writeFile(`${dir}/web-before-${dpr}x.png`,Buffer.from(before,'base64'));await writeFile(`${dir}/web-drag-${dpr}x.png`,Buffer.from(during,'base64'));
  // Allow tiny compositor rounding differences, while rejecting resampling
  // that changes control edges across the image.
  assert.ok(difference.max<=8 && difference.changed/difference.channels<.001,`Native content must preserve sharpness at ${dpr}x: ${JSON.stringify(difference)}`);
  const presentation=await evaluate('JSON.parse(JSON.stringify(layerApp.app.workspace_update(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const moved=await rect();assert.equal(moved.x,presentation.drag.group.bounds.x);assert.equal(moved.y,presentation.drag.group.bounds.y);
  await send({type:'set_theme',theme:'light'});
  const rebased=await rect();assert.deepEqual(rebased,moved,'unrelated model refresh does not jump placement');
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:start.x+101,y:start.y+101,button:'left',buttons:1});
  await evaluate('window.dispatchEvent(new Event("blur"))');await settle();
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:start.x+101,y:start.y+101,button:'left',buttons:0,clickCount:1});
  await settle();const canceled=await rect();assert.equal(canceled.x,initial.x);assert.equal(canceled.y,initial.y);
  console.log(JSON.stringify({dpr,centeredIcons:alignment.length,pixelComparison:difference,concurrentRefresh:true,pendingCancellation:true}));
  await send({type:'set_theme',theme:'dark'});
  const nav=structuredClone(fixture);nav.layout.bands[1].root.panels.push('navigator');nav.layout.bands[1].root.active='navigator';
  await send({type:'restore_workspace',workspace:nav});
  await send({type:'move_group',group:43,target:{kind:'float',position:[550,220]}});
  await send({type:'invoke',command:'fit_canvas'});
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function ready(){if(layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null)resolve(true);else if(performance.now()>end)reject(Error('Navigator fixture canvas did not become ready'));else setTimeout(ready,50)}ready()})`);
  await evaluate('layerApp.app.wait_for_canvas()');
  const compare=async(a,b)=>evaluate(`(async()=>{const load=async s=>{const i=new Image();i.src='data:image/png;base64,'+s;await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);return x.getImageData(0,0,c.width,c.height).data;};const a=await load(${JSON.stringify(a)}),b=await load(${JSON.stringify(b)});return a.reduce((n,v,i)=>n+(v!==b[i]),0);})()`);
  const native=await evaluate(`(()=>{const c=document.querySelector('.dock-group[data-group="43"] .navigator-surface'),r=c.getBoundingClientRect();window.retainedNavigator=c;return{width:c.width,height:c.height,expected:[Math.round(r.width*devicePixelRatio),Math.round(r.height*devicePixelRatio)]};})()`);
  assert.deepEqual([native.width,native.height],native.expected,'Navigator GPU canvas uses native resolution');
  // Include child GPU surfaces in the compositor screenshot; decode comparisons
  // with the same CPU-readable canvas mode as the rest of the Web test harness.
  const navCrop=async()=>{
    const full=(await call('Page.captureScreenshot',{format:'png'})).data;
    return evaluate(`(async()=>{const r=document.querySelector('.dock-group[data-group="43"] .overview-hole').getBoundingClientRect(),i=new Image();i.src='data:image/png;base64,'+${JSON.stringify(full)};await i.decode();const s=i.width/innerWidth,c=document.createElement('canvas');c.width=Math.round((r.width-4)*s);c.height=Math.round((r.height-4)*s);c.getContext('2d',{willReadFrequently:true}).drawImage(i,Math.round((r.x+2)*s),Math.round((r.y+2)*s),c.width,c.height,0,0,c.width,c.height);return c.toDataURL('image/png').split(',')[1];})()`);
  };
  const navBefore=await navCrop();
  const grip=await evaluate(`(()=>{const b=document.querySelector('.dock-group[data-group="43"] .dock-tabs>.panel-grip').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2};})()`);
  await call('Input.dispatchMouseEvent',{type:'mousePressed',...grip,button:'left',buttons:1,clickCount:1});
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:grip.x+100,y:grip.y+100,button:'left',buttons:1});await settle();
  const navDuring=await navCrop();
  assert.equal(await compare(navBefore,navDuring),0,'Navigator pixels remain sharp and attached while dragging');
  const documentPixel=await evaluate(`(async()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),points=[150,500,900,1200].flatMap(x=>[200,500,750].map(y=>({x:r.x+(c.translation[0]+x*c.zoom)*r.width/c.viewport[0],y:r.y+(c.translation[1]+y*c.zoom)*r.height/c.viewport[1]}))),p=points.find(p=>document.elementFromPoint(p.x,p.y)===layerApp.canvas);if(!p)throw Error('All Navigator source paper samples are obscured');const im=new Image();im.src='data:image/png;base64,'+${JSON.stringify((await call('Page.captureScreenshot',{format:'png'})).data)};await im.decode();const scale=im.width/innerWidth,canvas=document.createElement('canvas');canvas.width=im.width;canvas.height=im.height;const context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(im,0,0);const pixels=context.getImageData(Math.floor(p.x*scale)-16,Math.floor(p.y*scale)-16,32,32).data;return [0,1,2].map(k=>Math.round(pixels.reduce((sum,v,i)=>sum+(i%4===k?v:0),0)/1024));})()`);
  assert.ok(await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,'+${JSON.stringify(navDuring)};await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);const a=x.getImageData(0,0,c.width,c.height).data,expected=${JSON.stringify(documentPixel)};let matching=0;for(let n=0;n<a.length;n+=4)if(expected.every((v,k)=>Math.abs(v-a[n+k])<=3))matching++;return matching>c.width*c.height*.9;})()`),'Native GPU Navigator pixels match the displayed document paper');
  assert.ok(await evaluate('retainedNavigator===document.querySelector(\'.dock-group[data-group="43"] .navigator-surface\')'),'retain the same Navigator canvas');
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:grip.x+100,y:grip.y+100,button:'left',buttons:0,clickCount:1});await settle();
  await send({type:'invoke',command:'fit_canvas'});const cameraBefore=await navCrop();for(let i=0;i<3;i++)await send({type:'invoke',command:'zoom_in'});
  assert.ok(await compare(cameraBefore,await navCrop())>0,'Navigator updates the camera outline');
  await writeFile(`${dir}/web-navigator-${dpr}x.png`,Buffer.from(navDuring,'base64'));
  console.log(JSON.stringify({dpr,nativeNavigator:native,retainedPixels:true,cameraUpdate:true}));
  await send({type:'move_group',group:43,target:{kind:'float',position:[1070,160]}});
  await send({type:'invoke',command:'fit_canvas'});
  await send({type:'select_brush',id:1});await send({type:'set_brush_size',value:180});
  const colors=async()=>{
    const data=await navCrop();
    return evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,'+${JSON.stringify(data)};await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);const a=x.getImageData(0,0,c.width,c.height).data;let red=0,blue=0;for(let n=0;n<a.length;n+=4){if(a[n]>150&&a[n+1]<100&&a[n+2]<100)red++;if(a[n+2]>150&&a[n]<100&&a[n+1]<100)blue++;}return{red,blue};})()`);
  };
  const awaitColors=async predicate=>{
    const end=Date.now()+5000;let actual;
    do {actual=await colors();if(predicate(actual))return actual;await evaluate('new Promise(r=>setTimeout(r,50))');}while(Date.now()<end);
    assert.fail(`Navigator artwork did not converge: ${JSON.stringify(actual)}`);
  };
  const stroke=async(y,rgba,key)=>{
    await send({type:'set_color',rgba});
    const p=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{x:r.x+(c.translation[0]+700*c.zoom)*r.width/c.viewport[0],y:r.y+(c.translation[1]+${y}*c.zoom)*r.height/c.viewport[1],dx:450*c.zoom*r.width/c.viewport[0]};})()`);
    for(const [type,dx] of [['mousePressed',0],['mouseMoved',p.dx/2]]){
      await call('Input.dispatchMouseEvent',{type,x:p.x+dx,y:p.y,button:'left',buttons:1,pointerType:'pen',force:1});await settle();
    }
    const held=await awaitColors(c=>c[key]>20);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+p.dx,y:p.y,button:'left',buttons:1,pointerType:'pen',force:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:p.x+p.dx,y:p.y,button:'left',buttons:0,pointerType:'pen',force:0});await settle();
    await awaitColors(c=>c[key]>held[key]*1.2);
  };
  await stroke(640,[.85,.12,.24,1],'red');await stroke(890,[.1,.2,.9,1],'blue');
  for(const theme of ['light','dark']){
    await send({type:'set_theme',theme});await awaitColors(c=>c.red>20&&c.blue>20);
    const artwork=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/web-navigator-artwork-${theme}-${dpr}x.png`,Buffer.from(artwork.data,'base64'));
    await send({type:'invoke',command:'undo'});await awaitColors(c=>c.red>20&&c.blue===0);
    await send({type:'invoke',command:'redo'});await awaitColors(c=>c.red>20&&c.blue>20);
    await send({type:'select_panel_tab',group:43,panel:'layers'});
    await send({type:'invoke',command:'undo'});
    await send({type:'select_panel_tab',group:43,panel:'navigator'});await awaitColors(c=>c.red>20&&c.blue===0);
    await send({type:'invoke',command:'redo'});await awaitColors(c=>c.red>20&&c.blue>20);
  }
  await evaluate('layerApp.restartGpu()');
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;function ready(){if(layerApp.app.brush_ready()&&layerApp.startupTimes.complete!==null)resolve(true);else if(performance.now()>end)reject(Error('GPU restart did not become ready'));else setTimeout(ready,50)}ready()})`);
  await awaitColors(c=>c.red>20&&c.blue>20);
  await send({type:'invoke',command:'undo'});await send({type:'invoke',command:'undo'});await awaitColors(c=>c.red===0&&c.blue===0);
  console.log(JSON.stringify({dpr,navigatorArtwork:true,brushUpConvergence:true,artworkUndoRedo:true,inactiveSourceChange:true,gpuRestart:true,themes:['light','dark']}));
  // Use real host measurements and all pointer types for the clipped-preview
  // path, alongside the existing retained-pixel and cancellation checks.
  for (const device of ["mouse", "touch", "pen"]) for (const scenario of [
    {panel:"color"}, {panel:"layers"}, {panel:"adjustments"},
    {panel:"adjustments", sourceHeight:140}, {panel:"adjustments", sourceHeight:370},
    {panel:"color", footer:true}, {panel:"layers", established:true},
  ]) {
    const {panel,footer,sourceHeight,established} = scenario;
    const dropped = structuredClone(fixture);
    dropped.layout.bands[1].root = tabs(43, [panel]);
    if (footer) dropped.layout.panels.find(p=>p.id===panel).hide_tab = true;
    if (sourceHeight) {
      dropped.layout.bands[1].root = {kind:"split",id:46,axis:"vertical",fraction:sourceHeight/946,
        first:tabs(43,[panel]),second:tabs(47,["navigator"])};
      dropped.layout.next_id = Math.max(48,dropped.layout.next_id);
    }
    if (established) {
      dropped.layout.floating = [{root:dropped.layout.bands.pop().root,position:[550,220],width:360,default_width:360,height:320,toolbar_layout:"compact"}];
    }
    await send({type:"restore_workspace", workspace:dropped});
    const savedLayout = await evaluate("layerApp.state().workspace");
    const source = await rect();
    const start = await evaluate(`(()=>{const r=document.querySelector('.dock-group[data-group="43"] .panel-grip').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    let point = start;
    const input = async (phase, p = point) => {
      point = p;
      if (device === "touch") await call("Input.dispatchTouchEvent", {type:{down:"touchStart",move:"touchMove",up:"touchEnd"}[phase],touchPoints:phase==="up"?[]:[{id:1,...p}]});
      else await call("Input.dispatchMouseEvent", {type:{down:"mousePressed",move:"mouseMoved",up:"mouseReleased"}[phase],...p,button:"left",buttons:phase==="up"?0:1,clickCount:1,pointerType:device});
      await settle();
    };
    const live = () => evaluate(`(()=>{const p=layerApp.app.workspace_update().drag.group,n=document.querySelector('.dock-group[data-group="'+p.id+'"]'),r=n.getBoundingClientRect();return {bounds:p.bounds,actual:{x:r.x,y:r.y,width:r.width,height:r.height},id:p.id}})()`);
    await input("down");
    await input("move", {x:800,y:650});
    const first = await live();
    assert.equal(first.bounds.width, source.width, `${device} ${panel}: source width`);
    assert.ok(Math.abs(first.bounds.height-source.height)<.02, `${device} ${panel}: source height`);
    for (const y of [980, 995, 650]) {
      await input("move", {x:800,y});
      const preview = await live();
      assert.ok(Math.abs(preview.bounds.y-(y - (start.y-source.y)))<.02);
      assert.ok(Math.abs(preview.bounds.height-source.height)<.02);
      assert.ok(["x","y","width","height"].every(k=>Math.abs(preview.actual[k]-preview.bounds[k])<=.51), `${device} ${panel}: retained native allocation at edge`);
    }
    const measurement = await evaluate(`layerApp.app.layout_update(innerWidth,innerHeight).panel_measurements.find(m=>m.panel===${JSON.stringify(panel)})`);
    assert.ok(measurement.content_height > 0, `${panel}: natural native height`);
    const chrome = footer ? 20 : 36;
    const minimum = measurement.scroll ? Math.min(measurement.content_height,measurement.scroll.fixed_height+4*(measurement.scroll.unit_height||36))+chrome : measurement.content_height+chrome;
    // Compact/short content moves inward whole; long content uses the available
    // space down to four rows, then moves inward to keep those rows reachable.
    const room = sourceHeight ? 800 : panel === "adjustments" ? minimum+15 : 10;
    await input("move", {x:800,y:footer ? 995 : 1000-6-room+(start.y-source.y)});
    if (!sourceHeight) assert.ok((await live()).actual.y + source.height > 1000, "preview extends beyond the clip");
    await input("up"); await send({type:"set_theme",theme:"dark"});
    const final = await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(panel)}))`);
    const natural = measurement.content_height+chrome;
    const budget = Math.max(400,minimum);
    const preferred = established ? source.height : measurement.scroll && natural>budget ?
      (source.height>=minimum && source.height<=budget ? source.height : budget) : natural;
    const expected = Math.min(preferred,Math.max(room,minimum));
    assert.ok(Math.abs(final.bounds.height-expected)<1, `${device} ${panel}: ${final.bounds.height} versus ${expected}, ${JSON.stringify(measurement)}`);
    assert.ok(final.bounds.y+final.bounds.height<=994.01);
    if (panel === "layers") assert.ok(final.bounds.height<300, "two layers do not produce a tall float");
    if (panel === "color") {
      const body = await evaluate(`(()=>{const r=document.querySelector('.floating-panel .color-wheel-square').getBoundingClientRect(),b=document.querySelector('.floating-panel').getBoundingClientRect();return {side:r.height,gap:b.bottom-r.bottom}})()`);
      assert.ok(body.gap<(footer ? 32 : 12) && body.side>300, `color fits its square: ${JSON.stringify(body)}`);
    }
    if (device === "mouse" && !sourceHeight && !established) {
      const capture = await call("Page.captureScreenshot",{format:"png"});
      await writeFile(`${dir}/web-drop-${panel}${footer ? "-footer" : ""}-${dpr}x.png`,Buffer.from(capture.data,"base64"));
    }
    const committed = await evaluate("layerApp.state().workspace");
    await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await evaluate("layerApp.state().workspace"),savedLayout);
    await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await evaluate("layerApp.state().workspace"),committed);
    console.log(JSON.stringify({dpr,device,...scenario,natural:measurement.content_height,settledHeight:final.bounds.height,clippedPreview:true}));
  }

 }
 await send({type:'restore_workspace',workspace:saved});await send({type:'preferences',action:{type:'edit',id:'transparency',value:transparency}});console.log('PASS: native pixels at 1x/2x, model rebase/cancel, mouse/touch/pen clipping, compact/short/long drops, squashed/preserved source heights, footer anchor, undo/redo');
}

async function checkActiveTabShade({call,evaluate,send,fixture,dir,dpr}) {
 const levels=['off','low','medium','high'],mismatches=[];let variation=0,compared=0,continuous=0;
 await send({type:'restore_workspace',workspace:fixture});await send({type:'move_group',group:43,target:{kind:'float',position:[1000,600]}});
 await send({type:'invoke',command:'fit_canvas'});await send({type:'select_brush',id:1});await send({type:'set_color',rgba:[0,0,0,1]});
 await send({type:'set_brush_size',value:20/await evaluate('layerApp.state().camera.zoom')});
 const stripes=Array.from({length:13},(_,i)=>380+i*40);
 for(const x of stripes)for(const [type,y] of [['mousePressed',150],['mouseMoved',320],['mouseMoved',500],['mouseReleased',500]])
  await call('Input.dispatchMouseEvent',{type,x,y,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1});
 await send({type:'move_group',group:43,target:{kind:'float',position:[550,220]}});
 const range=pixels=>Math.max(...[0,1,2].map(i=>Math.max(...pixels.map(p=>p[i]))-Math.min(...pixels.map(p=>p[i]))));
 const inspect=async(name,translucent)=>{
  const shot=(await call('Page.captureScreenshot',{format:'png'})).data;await writeFile(`${dir}/${name}.png`,Buffer.from(shot,'base64'));
  const groups=await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${shot}';await i.decode();const s=i.width/innerWidth,c=new OffscreenCanvas(i.width,i.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);
   const at=(px,py)=>Array.from(x.getImageData(Math.floor(px*s),Math.floor(py*s),1,1).data.slice(0,3)),across=(left,right)=>Array.from({length:Math.floor((right-left-8)/4)+1},(_,k)=>left+4+k*4);
   const slide=document.querySelector('.tab-slide-overlay>.dock-tab[aria-selected=true]')?.getBoundingClientRect();
   return [...document.querySelectorAll('.dock-group')].flatMap(n=>{const h=n.querySelector('.dock-tabs:has(>.tab-list)')?.getBoundingClientRect(),b=n.querySelector('.panel-preview>.panel-frame')?.getBoundingClientRect();if(!h||!b)return[];
    const t=slide&&Math.abs(slide.y-h.y)<1&&slide.x>=h.x&&slide.right<=h.right?slide:n.querySelector('.tab-list>.dock-tab[aria-selected=true]').getBoundingClientRect();
    return [{group:n.dataset.group,samples:across(t.x,t.right).map(px=>({x:px,tab:at(px,t.bottom-3),panel:at(px,b.y+2)})),
     strip:across(h.x,h.right).filter(px=>px<t.x-7||px>t.right+7).map(px=>at(px,t.bottom-3)),body:across(h.x,h.right).map(px=>at(px,b.y+2))}];});})()`);
  for(const {group,samples,strip,body} of groups){
   compared+=samples.length;
   if(range(body)<=1){continuous++;if(range(strip)>1)mismatches.push(`${name} group ${group}: tab strip ${JSON.stringify(strip)} is not one surface over a uniform backdrop`);}
   for(const {x,tab,panel} of samples)if(tab.some((v,i)=>Math.abs(v-panel[i])>1))mismatches.push(`${name} group ${group} x=${x.toFixed(1)}: tab ${tab} panel ${panel}`);
   if(translucent)variation=Math.max(variation,range(samples.map(s=>s.panel)));
  }
 };
 const slide=async(name,panel,dx)=>{
  const p=await evaluate(`(()=>{const b=document.querySelector('.dock-group[data-group="41"] .dock-tab[data-panel="${panel}"]').getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',buttons:1,clickCount:1});
  for(const step of [12,dx])await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:p.x+step,y:p.y,button:'left',buttons:1});
  await evaluate('new Promise(r=>setTimeout(r,300))');assert.ok(await evaluate("!!document.querySelector('.tab-slide-overlay')"),`${name}: the tab slides`);
  await inspect(name,true);
  await evaluate('window.dispatchEvent(new Event("blur"))');
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:p.x+dx,y:p.y,button:'left',buttons:0,clickCount:1});await evaluate('new Promise(r=>setTimeout(r,300))');
 };
 for(const theme of ['dark','light']){
  for(const level of levels){
   await send({type:'set_theme',theme});await send({type:'preferences',action:{type:'edit',id:'transparency',value:levels.indexOf(level)}});
   await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:800,y:500});
   await inspect(`web-active-tab-${theme}-${level}-${dpr}x`,level!=='off');
  }
  await slide(`web-active-tab-${theme}-dragged-${dpr}x`,'brushes',30);
  await slide(`web-active-tab-${theme}-neighbor-${dpr}x`,'sizes',-60);
 }
 for(const _ of stripes)await send({type:'invoke',command:'undo'});
 await send({type:'set_theme',theme:'dark'});await send({type:'preferences',action:{type:'edit',id:'transparency',value:0}});
 assert.ok(variation>2,`stripes vary the backdrop behind an active tab: ${variation}`);
 assert.ok(continuous>=12,`a tab strip over a uniform backdrop in every theme, level and slide: ${continuous}`);
 assert.deepEqual(mismatches,[],`active tabs differ from their panels:\n${mismatches.join('\n')}`);
 console.log(JSON.stringify({dpr,activeTabMatchesPanel:true,themes:['dark','light'],levels,slides:['dragged','neighbor'],compared,continuous,backdropVariation:variation}));
}
