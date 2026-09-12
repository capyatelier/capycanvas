import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
export async function checkWorkspaceRendering({call,evaluate,settle}) {
 const dir='artifacts/workspace-motion';await mkdir(dir,{recursive:true});
 const send=async a=>{await evaluate(`layerApp.dispatch(${JSON.stringify(a)})`);await settle();await evaluate('new Promise(r=>setTimeout(r,250))');};
 const saved=await evaluate('layerApp.state().workspace'),fixture=structuredClone(saved);
 const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'name'});
 Object.assign(fixture.layout,{bands:[{id:40,edge:'left',extent:252,root:tabs(41,['brushes','sizes'])},{id:42,edge:'right',extent:360,root:tabs(43,['layers','properties','adjustments'])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(44,fixture.layout.next_id)});fixture.zen_mode=false;
 for(const dpr of [1,2]) {
  await call('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:dpr,mobile:false});
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
  await evaluate('new Promise(resolve=>{const poll=()=>layerApp.startupTimes.complete?resolve():setTimeout(poll,50);poll();})');
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
  assert.ok(await evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,'+${JSON.stringify(navDuring)};await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);const a=x.getImageData(0,0,c.width,c.height).data;let white=0;for(let n=0;n<a.length;n+=4)if(a[n]>245&&a[n+1]>245&&a[n+2]>245)white++;return white>c.width*c.height*.9;})()`),'Native GPU canvas displays the white document against a dark panel');
  assert.ok(await evaluate('retainedNavigator===document.querySelector(\'.dock-group[data-group="43"] .navigator-surface\')'),'retain the same Navigator canvas');
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:grip.x+100,y:grip.y+100,button:'left',buttons:0,clickCount:1});await settle();
  await send({type:'invoke',command:'fit_canvas'});const cameraBefore=await navCrop();for(let i=0;i<3;i++)await send({type:'invoke',command:'zoom_in'});
  assert.ok(await compare(cameraBefore,await navCrop())>0,'Navigator updates the camera outline');
  await writeFile(`${dir}/web-navigator-${dpr}x.png`,Buffer.from(navDuring,'base64'));
  console.log(JSON.stringify({dpr,nativeNavigator:native,retainedPixels:true,cameraUpdate:true}));
 }
 await send({type:'restore_workspace',workspace:saved});console.log('PASS: collapsed icon centering/rotation, native pixels at 1x/2x, model rebase and queued cancellation');
}
