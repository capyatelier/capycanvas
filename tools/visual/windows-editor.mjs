import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Full-editor evidence: use the same editable title-bar arrangement and reserve
// native caption-button space. Each host measures its own controls and panels,
// and fits the same document through the shared command.
export async function captureWindowsEditor({manifest,output,evaluate,call}) {
  assert.equal(manifest.schema,1);
  assert.equal(manifest.platform,'windows');
  assert.ok(manifest.fixtures.length);
  const reports=[],failures=[];
  await call('DOM.enable');
  await call('CSS.enable');
  await evaluate(`{
    const resolve=layerApp.app.header_geometry.bind(layerApp.app);
    window.capyWindowsCaptionInsets=[0,0];
    layerApp.app.header_geometry=(width,insets,metrics)=>resolve(width,
      insets.map((inset,i)=>inset+capyWindowsCaptionInsets[i]),metrics);
  }`);
  for (const fixture of manifest.fixtures) {
    const [width,height]=fixture.viewport,scale=fixture.scale;
    assert.match(fixture.name,/^[a-z0-9-]+$/);
    assert.ok(['initial','canvas-under-header'].includes(fixture.scenario));
    await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:scale,mobile:false});
    await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)}, view=()=>JSON.parse(layerApp.app.workspace_view());
      async function until(check,message){const start=performance.now();while(!check()){
        if(performance.now()-start>25000)throw new Error(message);await new Promise(r=>setTimeout(r,50));
      }}
      await until(()=>view()?.ready&&!view().busy,'Workspace startup timed out');
      if(view().id!==fixture.workspace){
        const button=[...document.querySelectorAll('.workspace-switcher button')].find(b=>b.dataset.workspaceId===fixture.workspace);
        if(!button)throw new Error('Reference workspace is unavailable');button.click();
        await until(()=>view().id===fixture.workspace&&view().ready&&!view().busy,'Workspace switch did not finish');
      }
      if(fixture.header_model){
        const openPanels=layerApp.app.layout(innerWidth,innerHeight).collapsed
          .flatMap(column=>column.open?column.open.connections.slice(0,1).map(([panel])=>panel):[]);
        const workspace=structuredClone(layerApp.state().workspace);
        workspace.layout.header=fixture.header_model;
        layerApp.dispatch({type:'restore_workspace',workspace});
        // Restoring a saved arrangement closes transient columns. Reopen the
        // reference's previous columns through their normal icon controls.
        await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
        for(const panel of openPanels){
          const icon=[...document.querySelectorAll('.collapsed-column .column-tab')].find(n=>n.dataset.panel===panel);
          if(!icon)throw new Error('Reference column icon is unavailable: '+panel);
          icon.click();
        }
      }
      layerApp.dispatch({type:'set_theme',theme:fixture.theme});
      layerApp.dispatch({type:'measure_titlebar',insets:fixture.titlebar_insets});
      window.capyWindowsCaptionInsets=fixture.titlebar_insets.slice(0,2);
      window.dispatchEvent(new Event('resize'));
      await document.fonts.ready;
      await until(()=>layerApp.startupTimes.complete!==null,'Staged GPU startup did not finish');
      await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
      layerApp.dispatch({type:'invoke',command:'fit_canvas'});
      if(fixture.scenario==='canvas-under-header')for(let i=0;i<4;i++)layerApp.dispatch({type:'invoke',command:'zoom_in'});
      await Promise.all([...document.images].map(i=>i.decode()));
      await until(()=>{
        const previews=[...document.querySelectorAll('.layer-thumbnail canvas')].filter(c=>{
          const b=c.getBoundingClientRect();return b.width>0&&b.height>0&&b.bottom>0&&b.top<innerHeight;
        });
        return previews.length>0&&previews.every(c=>c.getContext('2d').getImageData(0,0,c.width,c.height).data.every((v,i)=>i%4!==3||v===255));
      },'Visible layer previews did not settle');
      layerApp.canvas.focus();
      let previous,stable=0;
      await until(()=>{
        const state=layerApp.state();
        const value=JSON.stringify([layerApp.app.layout(innerWidth,innerHeight),state.workspace.layout.measurements,state.camera],(_,v)=>typeof v==='bigint'?v.toString():v);
        stable=value===previous?stable+1:0;previous=value;return stable>=3;
      },'Editor layout did not settle');
      await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
    })()`);
    const metrics=await evaluate(`(async()=>{
      const workspaceKeys=Object.fromEntries(${JSON.stringify(fixture.workspace_switcher||[])}.map(item=>[item.id,item.key]));
      const adapter=await navigator.gpu.requestAdapter();
      const rect=node=>{const b=node.getBoundingClientRect();return {x:b.x,y:b.y,width:b.width,height:b.height}};
      const layout=layerApp.app.layout(innerWidth,innerHeight);
      const elementId=n=>{
        const row=n.closest('.layer-row');
        if(row){
          const prefix='layer-'+row.dataset.layer+'-';
          if(n===row)return 'layer-row-'+row.dataset.layer;
          if(n===row.children[0])return prefix+'visibility';
          if(n===row.children[1])return prefix+'selection';
          if(n.matches('.layer-name'))return prefix+'label';
          if(n.matches('.layer-meta'))return prefix+'meta';
          if(n.matches('.layer-grip'))return prefix+'drag';
          const thumbnail=n.closest('.layer-thumbnail');
          if(thumbnail){
            const mask=[...row.querySelectorAll('.layer-thumbnail')].indexOf(thumbnail)===1;
            return prefix+(n.matches('canvas')?(mask?'mask-thumbnail':'thumbnail'):(mask?'mask':'content'));
          }
        }
        for(const [selector,id] of [['.layer-header','controls'],['.layer-options','options'],['.layer-flags','flags'],['.layer-footer','footer'],['.layer-options select','blend']])
          if(n.matches(selector))return 'layer-'+id;
        if(n.matches('.workspace-switcher'))return 'workspace-switcher';
        if(n.matches('.workspace-switcher button'))return 'workspace-switch-'+workspaceKeys[n.dataset.workspaceId];
        if(n.matches('#header-workspace-selector > summary'))return 'header-workspace-menu';
        if(n.matches('.header-overflow > summary'))return n.parentElement.id;
        if(n.matches('.header-menu > summary'))return n.parentElement.dataset.menu&&n.parentElement.dataset.menu!=='all'?'application-menu-'+n.parentElement.dataset.menu:'application-menus';
        if(n.dataset.command==='settings')return 'settings-button';
        return n.id;
      };
      const elements=[...document.querySelectorAll('button,summary,.panel,.panel-group,.group-tabs,.tool-choice-button,.brush-preview,#header,#document-title,.workspace-switcher,.layer-row,.layer-name,.layer-meta,.layer-grip,.layer-thumbnail canvas,.layer-header,.layer-options,.layer-flags,.layer-footer,.layer-options select')]
        .filter(n=>{const b=n.getBoundingClientRect();return getComputedStyle(n).visibility==='visible'&&b.width>0&&b.height>0&&b.bottom>0&&b.top<innerHeight})
        .map(n=>({id:n.matches('.tool-choice-button')?'tool-'+(n.parentElement.classList.contains('tool-groups')?'group':'subtool')+'-'+[...n.parentElement.children].indexOf(n):elementId(n),classes:n.className,name:n.getAttribute('aria-label')||n.textContent.trim(),command:n.dataset.command,
          bounds:rect(n),style:Object.fromEntries(['fontFamily','fontSize','fontWeight','lineHeight','padding','gap'].map(k=>[k,getComputedStyle(n)[k]]))}));
      return JSON.parse(JSON.stringify({viewport:[innerWidth,innerHeight],scale:devicePixelRatio,canvas:[layerApp.canvas.width,layerApp.canvas.height],
        adapter:{vendor:adapter.info.vendor,architecture:adapter.info.architecture,isFallbackAdapter:adapter.info.isFallbackAdapter},
        theme:layerApp.state().theme,document:layerApp.state().tabs[0],camera:layerApp.state().camera,layout,elements},(_,v)=>typeof v==='bigint'?v.toString():v));
    })()`);
    assert.equal(metrics.adapter.isFallbackAdapter,false);
    assert.deepEqual(metrics.viewport,fixture.viewport);
    assert.equal(metrics.scale,scale);
    assert.deepEqual(metrics.canvas,[Math.round(width*scale),Math.round(height*scale)]);
    assert.equal(metrics.theme,fixture.theme);
    for(const key of ['width','height','title'])assert.equal(metrics.document[key],fixture.document[key]);
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),Math.round(width*scale));
    assert.equal(png.readUInt32BE(20),Math.round(height*scale));
    await writeFile(`${output}/web-${fixture.name}.png`,png);
    await writeFile(`${output}/metrics-${fixture.name}.json`,JSON.stringify(metrics,null,2));
    // Keep every capture and report even when one area differs. A failed
    // comparison still fails the command after the remaining pairs are saved.
    const issues=[];
    function compare(label,expected,actual,limit,checkNames=false){
      assert.ok(expected?.length,`Native ${label} measurements are required`);
      const expectedIds=expected.map(e=>e.id).sort(),actualIds=actual.map(e=>e.id).sort();
      if(JSON.stringify(expectedIds)!==JSON.stringify(actualIds))issues.push(label+' exposes different controls');
      const entries=expected.map(native=>{
        const web=actual.find(e=>e.id===native.id),a=native.bounds,b=web?.bounds;
        if(!web)return{id:native.id,native:a,web:null,maximum_error_pixels:null};
        if(checkNames&&web.name!==native.name)issues.push(label+' label differs: '+native.id);
        return{id:native.id,native:a,web:b,maximum_error_pixels:scale*Math.max(
          ...['x','y','width','height'].map(k=>Math.abs(a[k]-b[k])),
          Math.abs(a.x+a.width-b.x-b.width),Math.abs(a.y+a.height-b.y-b.height))};
      });
      const maximum=Math.max(0,...entries.filter(e=>e.web).map(e=>e.maximum_error_pixels));
      if(!Number.isFinite(maximum)||maximum>limit)issues.push(`${label} geometry differs by ${maximum} physical pixels (limit ${limit})`);
      return{entries,maximum};
    }
    const toolSet=compare('Tool Set',fixture.tool_set,metrics.elements.filter(e=>/^tool-(group|subtool)-/.test(e.id)),1.01,true);
    const isHeader=id=>/^(application-menu[s-]|workspace-switch|header-workspace-menu$|header-overflow-[0-2]$|document-title$|zen-button$|fullscreen$|settings-button$)/.test(id);
    const header=compare('Header',fixture.header,metrics.elements.filter(e=>isHeader(e.id)),2.01);
    const layerId=id=>/^layer-(row-[0-9]+|[0-9]+-(content|mask|thumbnail|mask-thumbnail|visibility|selection|label|meta|drag)|controls|options|flags|footer|blend)$/.test(id);
    assert.ok(fixture.layers?.some(e=>e.id.startsWith('layer-row-')),'Native Layers row measurements are required');
    const layers=compare('Layers',fixture.layers,metrics.elements.filter(e=>layerId(e.id)),2.01);
    const dom=await call('DOM.getDocument');
    const workspaceNode=await call('DOM.querySelector',{nodeId:dom.root.nodeId,selector:'.workspace-switcher button'});
    const headerFonts=workspaceNode.nodeId?await call('CSS.getPlatformFontsForNode',{nodeId:workspaceNode.nodeId}):{fonts:[]};
    const report={name:fixture.name,metrics,layers:layers.entries,layers_maximum_error_pixels:layers.maximum,
      tool_set:toolSet.entries,tool_set_maximum_error_pixels:toolSet.maximum,header:header.entries,
      header_fonts:headerFonts.fonts,header_maximum_error_pixels:header.maximum,issues,native:{camera:fixture.camera,layout:fixture.layout},
      adaptations:{shared_header_arrangement:fixture.header_model,layer_geometry_source:fixture.layer_geometry_source,native_surface_offset_pixels:fixture.surface_offset_pixels,native_full_client:fixture.full_client,native_caption_button_reservation:fixture.titlebar_insets,system_caption_buttons:'Windows owns these; Chrome leaves the reserved pixels visible'},
      scope:'Full editor; differences remain unaccepted until reviewed. No masks, cropping, resampling or native-measurement substitution.'};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify(report,null,2));
    reports.push({name:fixture.name,viewport:metrics.viewport,scale,adapter:metrics.adapter,issues,
      layers_maximum_error_pixels:layers.maximum,tool_set_maximum_error_pixels:toolSet.maximum,header_maximum_error_pixels:header.maximum});
    failures.push(...issues.map(issue=>fixture.name+': '+issue));
  }
  await writeFile(`${output}/windows-editor.json`,JSON.stringify({captures:reports},null,2));
  console.log(JSON.stringify({captures:reports},null,2));
  assert.deepEqual(failures,[],'Matched editor differences remain; see the saved images and geometry reports');
}
