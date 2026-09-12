// Actual live web header controls on the native component fixture's background.
// Mac OS menus/control reservations and the Apple fullscreen capability are
// explicit reference adaptations. Metal and UIKit remain separate gates.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureHeaderControls({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.fixtures.length, 96);
  const adapter = await evaluate(`(async()=>{const a=await navigator.gpu.requestAdapter();
    return {vendor:a.info.vendor,architecture:a.info.architecture,isFallbackAdapter:a.info.isFallbackAdapter};})()`);
  assert.equal(adapter.isFallbackAdapter, false);
  await evaluate(`new Promise((resolve,reject)=>{
    const start=performance.now();function check(){const view=JSON.parse(layerApp.app.workspace_view());
      if(view.ready&&!view.busy)resolve(true);else if(performance.now()-start>15000)reject(new Error('Workspace startup timed out'));else setTimeout(check,20);
    }check();
  })`);
  await evaluate(`(async()=>{
    const {createSystemStatus}=await import('/system-status.js');
    const battery=Object.assign(new EventTarget(),{level:0.85,charging:false});
    Object.defineProperty(navigator,'getBattery',{configurable:true,value:async()=>battery});
    const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
    const status=createSystemStatus({element,changed:()=>{}});
    document.getElementById('system-status').replaceWith(status.root);
    window.capyHeaderStatus=status;
    const background=element('div');background.id='header-component-background';
    background.style.cssText='position:absolute;inset:0 0 auto;height:48px;z-index:999;pointer-events:none';
    document.getElementById('workspace').append(background);
  })()`);
  const reports = [];
  for (const fixture of manifest.fixtures) {
    const [width, height] = fixture.viewport;
    await call('Emulation.setDeviceMetricsOverride', {width, height, deviceScaleFactor:fixture.scale, mobile:false});
    await evaluate(`(async()=>{
      const id=${JSON.stringify(fixture.active_workspace)}, view=()=>JSON.parse(layerApp.app.workspace_view());
      if(view().id!==id) [...document.querySelectorAll('.workspace-switcher button')].find(button=>button.dataset.workspaceId===id).click();
      await new Promise((resolve,reject)=>{const start=performance.now();function check(){const v=view();
        if(v.id===id&&v.ready&&!v.busy)resolve(true);else if(performance.now()-start>10000)reject(new Error('Workspace switch timed out'));else setTimeout(check,20);
      }check();});
    })()`);
    await evaluate(`{
      const fixture=${JSON.stringify(fixture)}, mac=fixture.platform===1, width=fixture.viewport[0];
      layerApp.dispatch({type:'set_theme',theme:fixture.theme});
      const header=document.getElementById('header');
      header.style.setProperty('--header-item-gap',mac?'6px':width<=850?'0px':'6px');
      for(const menu of document.querySelectorAll('.header-menu')){menu.style.display=mac?'none':'';menu.open=false;}
      document.querySelector('.header-menu-labels').style.display=mac?'none':'';
      document.querySelector('.zen-spacer').style.width=(36+fixture.header_leading_inset)+'px';
      document.getElementById('zen-button').style.left=(6+fixture.header_leading_inset)+'px';
      document.getElementById('fullscreen').style.display='none';
      const title=document.getElementById('document-title');
      title.style.display=mac?'block':'';
      title.textContent=fixture.title.title+' · '+fixture.title.width+' × '+fixture.title.height;
      capyHeaderStatus.setClockVisibility(fixture.clock_visible?'always':'never');
      document.getElementById('system-clock').textContent=fixture.clock;
      document.getElementById('header-component-background').style.background=fixture.surface;
    }`);
    await evaluate(`document.fonts.ready.then(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))))`);
    const metrics = await evaluate(`(()=>{
      const elements={}, rect=node=>{const b=node.getBoundingClientRect();return {x:b.x,y:b.y,width:b.width,height:b.height};};
      for(const menu of document.querySelectorAll('.header-menu')){
        const summary=menu.querySelector('summary');if(summary.getBoundingClientRect().width&&getComputedStyle(summary).visibility!=='hidden')
          elements[menu.dataset.menu==='all'?'application-menus':'menu-'+summary.textContent]=rect(summary);
      }
      elements['workspace-switcher']=rect(document.querySelector('.workspace-switcher'));
      for(const button of document.querySelectorAll('.workspace-switcher button')) elements['workspace-switch-'+button.dataset.workspaceId]=rect(button);
      for(const [id,selector] of Object.entries({'document-title':'#document-title','system-clock':'#system-clock',
        'system-battery':'#system-battery','settings-button':'#header-end [data-command="settings"]','zen-button':'#zen-button'})){
        const node=document.querySelector(selector);if(node&&node.getBoundingClientRect().width)elements[id]=rect(node);
      }
      return {elements,clock:document.getElementById('system-clock').textContent,title:document.getElementById('document-title').textContent,
        active_workspace:JSON.parse(layerApp.app.workspace_view()).id,scale:devicePixelRatio};
    })()`);
    if (fixture.clock_visible) assert.equal(metrics.clock, fixture.clock);
    assert.equal(metrics.title, `${fixture.title.title} · ${fixture.title.width} × ${fixture.title.height}`);
    assert.equal(metrics.active_workspace, fixture.active_workspace);
    assert.deepEqual(Object.keys(metrics.elements).sort(), Object.keys(fixture.elements).sort(), `Header controls differ in ${fixture.name}`);
    const shot = await call('Page.captureScreenshot', {format:'png', fromSurface:true, clip:{...fixture.clip,scale:1}});
    const png = Buffer.from(shot.data, 'base64');
    assert.equal(png.readUInt32BE(16), width * fixture.scale);
    assert.equal(png.readUInt32BE(20), fixture.clip.height * fixture.scale);
    await writeFile(`${output}/web-${fixture.name}.png`, png);
    const differences = [];
    for (const [id, native] of Object.entries(fixture.elements)) {
      const web = metrics.elements[id];
      assert.ok(web, `Missing Chrome counterpart for native ${id} in ${fixture.name}`);
      differences.push({id,native,web,maximum_error:Math.max(...['x','y','width','height'].map(key=>Math.abs(native[key]-web[key]))),
        maximum_edge_error:Math.max(Math.abs(native.x-web.x),Math.abs(native.y-web.y),
          Math.abs(native.x+native.width-web.x-web.width),Math.abs(native.y+native.height-web.y-web.height))});
    }
    const report = {name:fixture.name,metrics,differences,adaptations:{mac_os_menus:fixture.platform===1,
      native_window_control_reservation:fixture.header_leading_inset,fullscreen_button:'Apple capability differs; remains a separate feature gate'}};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify(report,null,2));
    reports.push(report);
  }
  await writeFile(`${output}/header-geometry.json`,JSON.stringify(reports,null,2));
  const maximum = key => Math.max(...reports.flatMap(report=>report.differences.map(difference=>difference[key])));
  await writeFile(`${output}/chrome-header-capture.json`,JSON.stringify({adapter,fixtures:reports.length,
    maximum_position_or_size_error:maximum('maximum_error'),maximum_edge_error:maximum('maximum_edge_error'),
    scope:manifest.scope},null,2));
  console.log(`Captured ${reports.length} live Chrome header references with explicit Apple adaptations`);
}
