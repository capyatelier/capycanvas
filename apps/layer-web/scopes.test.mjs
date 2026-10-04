import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {scopesFixture} from './scopes-fixture.mjs';
import {histogramJourney} from './histogram-journey.mjs';

export async function checkScopes({call,evaluate,settle}) {
  const fixture=await scopesFixture({call,evaluate,settle});
  const {checkpoint}=fixture;
  const scope=histogramJourney({evaluate,settle});
  const targets={dockContainer:'.dock-group',floatContainer:'.floating-panel'};
  for(const name of ['histogram','waveform']){const root=`[data-scope="${name}"]`;targets[name]={root,tab:`.dock-tab[data-panel="${name}"]`,...Object.fromEntries(['source','channel','log','status','shadows','highlights','chart'].map(key=>[key,`${root} [data-scope-control="${key}"]`]))};}
  const graphPixels=selector=>evaluate(`(()=>{const c=document.querySelector(${JSON.stringify(selector)}),a=c.getContext('2d').getImageData(0,0,c.width,c.height).data;let colored=0;const dominant=[0,0,0];for(let i=0;i<a.length;i+=4){if(a[i+3]&&Math.max(a[i],a[i+1],a[i+2])-Math.min(a[i],a[i+1],a[i+2])>20){colored++;for(let j=0;j<3;j++)if(a[i+j]>a[i+(j+1)%3]&&a[i+j]>a[i+(j+2)%3])dominant[j]++;}}return{colored,dominant}})()`);
  try {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/scopes-web';await mkdir(directory,{recursive:true});
  const state=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state(),(_,v)=>typeof v==="bigint"?Number(v):v))');
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const poll=async condition=>{const end=Date.now()+120000;while(Date.now()<end){if(await evaluate(condition))return;await settle();await new Promise(r=>setTimeout(r,100));}throw Error(`Timed out: ${condition}\n${JSON.stringify(await state())}`);};
  const ready=async name=>{await poll(`layerApp.state().${name}.data!=null&&!layerApp.state().host_error`);await evaluate('layerApp.app.wait_for_canvas()');await poll(`layerApp.state().${name}.data!=null&&layerApp.app.brush_ready()`);};
  const point=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  const click=async selector=>{const p=await point(selector);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  const focus=async name=>{const active=await evaluate(`layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes(${JSON.stringify(name)}))?.active===${JSON.stringify(name)}`);if(!active)await click(targets[name].tab);if(await evaluate(`layerApp.state().customization.expanded===${JSON.stringify(name)}`))await click(targets[name].tab);await poll('layerApp.state().customization.expanded==null');await settle();};
  const choose=async(selector,index)=>{await click(selector);for(const key of ['Home',...Array(index).fill('ArrowDown'),'Enter'])for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,windowsVirtualKeyCode:{Home:36,ArrowDown:40,Enter:13}[key]});await settle();};
  const bounds=async(name,floating=false)=>{
    const t=targets[name];const result=await evaluate(`(()=>{const t=${JSON.stringify(t)},root=document.querySelector(t.root),container=root.closest(${JSON.stringify(floating?targets.floatContainer:'.floating-panel, .dock-group')}),b=n=>{const r=n.getBoundingClientRect();return{x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height}};return{container:b(container),root:b(root),items:Object.fromEntries(Object.entries(t).filter(([k])=>!['root','tab'].includes(k)).map(([k,s])=>[k,b(document.querySelector(s))])),status:document.querySelector(t.status).textContent,scrollWidth:document.querySelector(t.status).scrollWidth,clientWidth:document.querySelector(t.status).clientWidth}})()`);
    for(const [key,r] of Object.entries(result.items)){assert.ok(r.width>0&&r.height>0,`${name}/${key} visible`);assert.ok(r.x>=result.container.x-1&&r.right<=result.container.right+1&&r.y>=result.container.y-1&&r.bottom<=result.container.bottom+1,`${name}/${key} inside actual container: ${JSON.stringify(result)}`);}
    assert.ok(result.clientWidth>=result.scrollWidth,'Full Exact/Preview status is readable');
    if(name==='waveform')assert.ok(result.items.chart.width>=result.root.width-16,'Waveform uses available horizontal width');
    return result;
  };
  const shot=async name=>{const png=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(png.data,'base64'));};
  await poll('layerApp.startupTimes.complete!==null&&!layerApp.documents.busy()');
  const photoId=await evaluate('Number(layerApp.state().layer_tools.editing_layer.id)');
  await fixture.send({type:'layer',action:{op:'reference',id:photoId}});await fixture.invoke('select_all');
  await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:photographer'}));null`);await fixture.poll(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:photographer'&&!JSON.parse(layerApp.app.workspace_view()).busy`);
  const workspace=await evaluate('layerApp.state().workspace');const original=await checkpoint();
  for(const width of process.env.LAYER_SCOPES_WIDTH?[Number(process.env.LAYER_SCOPES_WIDTH)]:[640,1100])for(const theme of process.env.LAYER_SCOPES_THEME?[process.env.LAYER_SCOPES_THEME]:['light','dark']){
    await call('Emulation.setDeviceMetricsOverride',{width,height:800,deviceScaleFactor:1,mobile:false});await send({type:'set_theme',theme});await send({type:'restore_workspace',workspace});
    for(const name of ['histogram','waveform']){
      await send({type:'restore_workspace',workspace});await fixture.invoke('fit_canvas');
      await send({type:'customize',action:{type:'set_panel_visible',panel:name,visible:true}});for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await focus(name);await ready(name);
      const initialPixels=await fixture.visibleSamples();assert.ok(initialPixels.some(p=>Math.max(...p.slice(0,3))-Math.min(...p.slice(0,3))>10),'Actual photo baseline is nonconstant RGB');
      for(let channel=0;channel<5;channel++){await choose(targets[name].channel,channel);if((await state())[name].channel!==channel){await shot(`${name}-channel-failed-${width}-${theme}`);await writeFile(`${directory}/${name}-channel-failed-${width}-${theme}.json`,JSON.stringify({expected:channel,state:(await state())[name],dom:await evaluate(`Array.from(document.querySelectorAll('[data-scope] [data-scope-control=channel]')).map(n=>{const r=n.getBoundingClientRect();return{kind:n.closest('[data-scope]').dataset.scope,value:n.value,visible:n.checkVisibility(),focus:document.activeElement===n,bounds:{x:r.x,y:r.y,width:r.width,height:r.height},parent:n.closest('.dock-group')?.outerHTML.slice(0,500)}})`)}));}assert.equal((await state())[name].channel,channel);await ready(name);}
      const before=(await state())[name].logarithmic;await click(targets[name].log);assert.equal((await state())[name].logarithmic,!before);await click(targets[name].log);
      await choose(targets[name].channel,0);await scope.exact();await focus(name);await ready(name);const geometry=await bounds(name);await shot(`${name}-${width}-${theme}`);await writeFile(`${directory}/${name}-${width}-${theme}.json`,JSON.stringify(geometry,null,2));
      const plot=await graphPixels(targets[name].chart);assert.ok(plot.colored>0,'Actual rendered scope contains colored plot pixels');assert.ok(plot.dominant.every(n=>n>0),'RGB scope renders all three channel colors');
      const pixels=await fixture.visibleSamples();await writeFile(`${directory}/${name}-artwork-${width}-${theme}.json`,JSON.stringify({pixels,initialPixels}));assert.deepEqual(pixels,initialPixels,'Scope controls preserve actual color-managed framebuffer samples');
      if(name==='histogram'){
        for(let source=0;source<4;source++){console.log(`Scope source ${source} at ${width}/${theme}`);await choose(targets[name].source,source);await ready(name);assert.equal((await state()).histogram.source,source);}
        await fixture.invoke('deselect');await poll('layerApp.state().histogram.data==null');await fixture.idle();
        assert.equal((await graphPixels(targets[name].chart)).colored,0,'Empty Selection clears previous scope pixels');
        await fixture.invoke('select_all');await ready(name);await choose(targets[name].source,2);
        await fixture.send({type:'layer',action:{op:'reference',id:photoId}});await ready(name);
        assert.equal((await state()).histogram.data.pixels,0,'Reference without membership has no opaque source pixels');
        await fixture.send({type:'layer',action:{op:'reference',id:photoId}});await ready(name);await choose(targets[name].source,0);
      }
      const start=await point(`.dock-group:has(.dock-tab[data-panel="${name}"]) .panel-grip`),end={x:width*.3,y:450};await call('Input.dispatchMouseEvent',{type:'mousePressed',...start,button:'left',buttons:1,clickCount:1});for(let step=1;step<=6;step++){await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:start.x+(end.x-start.x)*step/6,y:start.y+(end.y-start.y)*step/6,button:'left',buttons:1});await settle();}await call('Input.dispatchMouseEvent',{type:'mouseReleased',...end,button:'left',buttons:0,clickCount:1});await settle();await shot(`${name}-floating-${width}-${theme}`);await bounds(name,true);
      await scope.hide();await poll(`layerApp.state().${name}.data==null`);
      await send({type:'customize',action:{type:'set_panel_visible',panel:name,visible:true}});for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await focus(name);await ready(name);
      const reopened=await bounds(name);assert.ok(reopened.container.width>=253,'Reopened scope retains its real minimum width');
      await choose(targets[name].channel,4);assert.equal((await state())[name].channel,4);await ready(name);await choose(targets[name].channel,0);await ready(name);await shot(`${name}-reopened-${width}-${theme}`);
    }
    await send({type:'restore_workspace',workspace});await scope.exact();
    for(const key of ['shadows','highlights']){const before=(await state()).histogram[key];await click(targets.histogram[key]);assert.equal((await state()).histogram[key],!before);await click(targets.histogram[key]);}
    await send({type:'move_panel',panel:'histogram',target:{kind:'edge',edge:'right',outer:true}});await scope.exact();
    const column=await evaluate("Number(layerApp.state().workspace.layout.bands.find(b=>b.edge==='right').root.id)");
    await send({type:'customize',action:{type:'set_column_drawers',column,drawers:true}});
    await send({type:'customize',action:{type:'set_column_collapsed',group:column,collapsed:true}});
    await click('.column-tab[data-panel="histogram"]');await ready('histogram');
    if(!await evaluate('layerApp.state().customization.column_drawers.length>0')){await shot(`drawer-failed-${width}-${theme}`);await writeFile(`${directory}/drawer-failed-${width}-${theme}.json`,JSON.stringify({column,state:await state(),dom:await evaluate(`Array.from(document.querySelectorAll('.column-tab')).map(n=>({text:n.textContent,panel:n.dataset.panel,bounds:n.getBoundingClientRect().toJSON(),visible:n.checkVisibility()}))`)}));}
    assert.ok(await evaluate('layerApp.state().customization.column_drawers.length>0'),'Collapsed scopes open as a real drawer');
    await shot(`histogram-drawer-${width}-${theme}`);
    await send({type:'customize',action:{type:'set_column_collapsed',group:column,collapsed:false}});
    await fixture.reopen();await fixture.recover();
    assert.deepEqual(await checkpoint(),original,'Monitor/source/channel/clipping/layout changes preserve document history and source');
  }
  } finally {await fixture.dispose();}
  console.log('PASS: scopes sources/channels/log/clipping, full-width plots, readable precision, floating bounds and cancellation preserve source/history');
}

export async function checkScopesSmoke({call,evaluate,settle}) {
  await call('Emulation.setDeviceMetricsOverride',{width:640,height:800,deviceScaleFactor:1,mobile:false});

  const fixture=await scopesFixture({call,evaluate,settle}),scope=histogramJourney({evaluate,settle});
  try {
    const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/scopes-web';await mkdir(directory,{recursive:true});
    const capture=async name=>{const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(data,'base64'));};
    await evaluate(`layerApp.app.workspace_input(JSON.stringify({type:'switch',id:'builtin:workspace:photographer'}));null`);await fixture.poll(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:photographer'&&!JSON.parse(layerApp.app.workspace_view()).busy`);await fixture.invoke('fit_canvas');await fixture.idle();
    const before=await fixture.checkpoint();const histogram=await scope.exact();await capture('histogram-smoke');
    assert.equal(histogram.pixels,256*256);assert.equal(histogram.transparent,0);
    for(const channel of histogram.channels)assert.equal(channel.bins.reduce((sum,n)=>sum+n,0),histogram.pixels);
    await fixture.send({type:'customize',action:{type:'set_panel_visible',panel:'waveform',visible:true}});
    const group=await evaluate("Number(layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('waveform')).id)");
    if(await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('waveform')).active!=='waveform'"))await fixture.send({type:'select_panel_tab',group,panel:'waveform'});
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();
    await fixture.poll('layerApp.state().waveform.data!=null');await fixture.idle();
    await fixture.poll(`document.querySelector('[data-scope="waveform"] [data-scope-control="status"]').textContent==='Exact'`);
    const pixels=await evaluate(`(()=>{const c=document.querySelector('[data-scope="waveform"] [data-scope-control="chart"]'),a=c.getContext('2d').getImageData(0,0,c.width,c.height).data;return{width:c.width,height:c.height,colored:Array.from(a).filter((n,i)=>i%4!==3&&n>0).length}})()`);
    await capture('waveform-smoke');
    await writeFile(`${directory}/waveform-smoke.json`,JSON.stringify({pixels},null,2));
    assert.ok(pixels.width>150&&pixels.height>100&&pixels.colored>0,`Shared Waveform RGBA is drawn on the real canvas: ${JSON.stringify(pixels)}`);
    assert.deepEqual(await fixture.checkpoint(),before);
    console.log('PASS: real shared Histogram counts and Waveform RGBA publication render without document edits');
  } finally {await fixture.dispose();}
}
