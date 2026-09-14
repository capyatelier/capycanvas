// Live Web title-bar references for the Apple component manifest. Keep actual
// host styling; report differences rather than changing reference pixels.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureHeaderControls({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema, 2);
  assert.equal(manifest.fixtures.length, 144);
  const adapter = await evaluate(`(async()=>{const a=await navigator.gpu.requestAdapter();
    return {vendor:a.info.vendor,architecture:a.info.architecture,isFallbackAdapter:a.info.isFallbackAdapter};})()`);
  assert.equal(adapter.isFallbackAdapter, false);
  await evaluate(`{
    const original=layerApp.app.header_geometry.bind(layerApp.app);
    window.capyHeaderInsets=[0,0];
    layerApp.app.header_geometry=(width,insets,metrics)=>original(width,capyHeaderInsets,metrics);
    Object.defineProperty(document,'fullscreenElement',{configurable:true,get:()=>document.documentElement});
    document.dispatchEvent(new Event('fullscreenchange'));
    const background=document.createElement('div');background.id='header-component-background';
    background.style.cssText='position:absolute;inset:0 0 auto;z-index:999;pointer-events:none';
    document.getElementById('workspace').append(background);
  }`);
  const reports=[];
  for (const fixture of manifest.fixtures) {
    const [width,height]=fixture.viewport;
    await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:fixture.scale,mobile:false});
    await evaluate(`(async()=>{
      const id=${JSON.stringify(fixture.active_workspace)},view=()=>JSON.parse(layerApp.app.workspace_view());
      if(view().id!==id) [...document.querySelectorAll('.workspace-switcher button')].find(b=>b.dataset.workspaceId===id).click();
      await new Promise((resolve,reject)=>{const start=performance.now();function check(){const v=view();
        if(v.id===id&&v.ready&&!v.busy)resolve(true);else if(performance.now()-start>15000)reject(new Error('Workspace switch timed out'));else setTimeout(check,20);
      }check();});
    })()`);
    await evaluate(`{
      const fixture=${JSON.stringify(fixture)},workspace=structuredClone(fixture.workspace);
      // Mac's OS menus and each native host's window controls stay host owned.
      workspace.layout.header=fixture.header.model;
      capyHeaderInsets=[fixture.header_leading_inset,0];
      layerApp.dispatch({type:'restore_workspace',workspace});
      layerApp.dispatch({type:'set_theme',theme:fixture.theme});
      const title=document.getElementById('document-title');
      title.textContent=fixture.title.title+' · '+fixture.title.width+' × '+fixture.title.height;
      document.getElementById('system-clock').textContent=fixture.clock;
      Object.assign(document.getElementById('header-component-background').style,
        {height:fixture.clip.height+'px',background:fixture.surface});
      window.dispatchEvent(new Event('resize'));
    }`);
    await evaluate(`document.fonts.ready.then(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))))`);
    const metrics=await evaluate(`(()=>{
      const elements={},rect=node=>{const b=node.getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height};};
      for(const node of document.querySelectorAll('#header [data-header-item]'))if(!node.hidden)elements[node.id]=rect(node);
      return{elements,active_workspace:JSON.parse(layerApp.app.workspace_view()).id,scale:devicePixelRatio,
        fonts:[...document.querySelectorAll('.workspace-switcher button')].map(node=>({text:node.textContent,font:getComputedStyle(node).font}))};
    })()`);
    assert.equal(metrics.active_workspace,fixture.active_workspace);
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true,clip:{...fixture.clip,scale:1}});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),width*fixture.scale);
    assert.equal(png.readUInt32BE(20),fixture.clip.height*fixture.scale);
    await writeFile(`${output}/web-${fixture.name}.png`,png);
    const differences=[],missing=[],additional=[];
    const nativeItems=Object.entries(fixture.elements).filter(([id])=>id.startsWith('header-item-'));
    for(const [id,native] of nativeItems) {
      const web=metrics.elements[id];
      if(!web){missing.push(id);continue;}
      differences.push({id,native,web,maximum_error:Math.max(...['x','y','width','height'].map(k=>Math.abs(native[k]-web[k]))),
        maximum_edge_error:Math.max(Math.abs(native.x-web.x),Math.abs(native.y-web.y),Math.abs(native.x+native.width-web.x-web.width),Math.abs(native.y+native.height-web.y-web.height))});
    }
    for(const id of Object.keys(metrics.elements))if(!fixture.elements[id])additional.push(id);
    const report={name:fixture.name,metrics,differences,missing,additional,adaptations:{projected_apple_model:true,
      native_window_control_reservation:fixture.header_leading_inset,deterministic_fullscreen_status:true}};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify(report,null,2));reports.push(report);
  }
  await writeFile(`${output}/header-geometry.json`,JSON.stringify(reports,null,2));
  const maximum=key=>Math.max(...reports.flatMap(r=>r.differences.map(d=>d[key])));
  await writeFile(`${output}/chrome-header-capture.json`,JSON.stringify({adapter,fixtures:reports.length,
    maximum_position_or_size_error:maximum('maximum_error'),maximum_edge_error:maximum('maximum_edge_error'),
    unmatched_items:reports.reduce((n,r)=>n+r.missing.length+r.additional.length,0),scope:manifest.scope},null,2));
  console.log(`Captured ${reports.length} live Web title bars; inspect reported host differences at normal size`);
}
