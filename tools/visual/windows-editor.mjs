import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Full-editor evidence. Only reserve native caption-button space; do not copy
// native panel measurements, override controls, hide pixels or align the camera
// artificially. Each host fits the same document through the shared command.
export async function captureWindowsEditor({manifest,output,evaluate,call}) {
  assert.equal(manifest.schema,1);
  assert.equal(manifest.platform,'windows');
  assert.ok(manifest.fixtures.length);
  const reports=[];
  await call('DOM.enable');
  await call('CSS.enable');
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
      layerApp.dispatch({type:'set_theme',theme:fixture.theme});
      layerApp.dispatch({type:'measure_titlebar',insets:fixture.titlebar_insets});
      const header=document.getElementById('header');
      header.style.left=fixture.titlebar_insets[0]+'px';
      header.style.right=fixture.titlebar_insets[1]+'px';
      document.getElementById('zen-button').style.left=(6+fixture.titlebar_insets[0])+'px';
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
        if(n.matches('.workspace-switcher button'))return 'workspace-switch-'+n.dataset.workspaceId.split(':').pop();
        if(n.matches('.header-menu > summary'))return n.parentElement.dataset.menu==='all'?'application-menus':'application-menu-'+n.parentElement.dataset.menu;
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
    assert.ok(fixture.tool_set?.length,'Native Tool Set measurements are required');
    const toolSet=fixture.tool_set.map(native=>{
      const web=metrics.elements.find(e=>e.id===native.id);
      assert.ok(web,`Missing reference control: ${native.id}`);
      assert.equal(web.name,native.name);
      const a=native.bounds,b=web.bounds;
      const sizePosition=Math.max(...['x','y','width','height'].map(key=>Math.abs(a[key]-b[key])));
      const edges=Math.max(Math.abs(a.x-b.x),Math.abs(a.y-b.y),Math.abs(a.x+a.width-b.x-b.width),Math.abs(a.y+a.height-b.y-b.height));
      return {id:native.id,native:a,web:b,maximum_error_pixels:Math.max(sizePosition,edges)*scale};
    });
    const toolSetMaximum=Math.max(...toolSet.map(e=>e.maximum_error_pixels));
    // Native layout rounds to physical pixels. This bounds geometry only;
    // retain the complete raster differences for fonts, colors and shadows.
    assert.ok(toolSetMaximum<=1.01,`Tool Set geometry differs by ${toolSetMaximum} physical pixels`);
    const dom=await call('DOM.getDocument');
    const workspaceNode=await call('DOM.querySelector',{nodeId:dom.root.nodeId,selector:'.workspace-switcher button'});
    const headerFonts=await call('CSS.getPlatformFontsForNode',{nodeId:workspaceNode.nodeId});
    assert.ok(fixture.header?.length,'Native header measurements are required');
    const isHeader=id=>/^(application-menu[s-]|workspace-switch|document-title$|zen-button$|fullscreen$|settings-button$)/.test(id);
    assert.deepEqual(fixture.header.map(e=>e.id).sort(),metrics.elements.filter(e=>isHeader(e.id)).map(e=>e.id).sort(),
      'Native and reference headers expose different controls');
    const header=fixture.header.map(native=>{
      const web=metrics.elements.find(e=>e.id===native.id);
      assert.ok(web,`Missing visible reference header control: ${native.id}`);
      const a=native.bounds,b=web.bounds;
      return {id:native.id,native:a,web:b,maximum_error_pixels:scale*Math.max(...['x','y','width','height'].map(k=>Math.abs(a[k]-b[k])),
        Math.abs(a.x+a.width-b.x-b.width),Math.abs(a.y+a.height-b.y-b.height))};
    });
    const headerMaximum=Math.max(0,...header.map(e=>e.maximum_error_pixels));
    // UI Automation quantizes both origin and size to physical pixels. Bound
    // their accumulated edge error; this does not accept glyph/raster parity.
    assert.ok(headerMaximum<=2.01,`Header geometry differs by ${headerMaximum} physical pixels`);
    const layerId=id=>/^layer-(row-[0-9]+|[0-9]+-(content|mask|thumbnail|mask-thumbnail|visibility|selection|label|meta|drag)|controls|options|flags|footer|blend)$/.test(id);
    const layers=(fixture.layers||[]).map(native=>{
      const web=metrics.elements.find(e=>e.id===native.id);
      assert.ok(web,`Missing visible reference Layers control: ${native.id}`);
      const a=native.bounds,b=web.bounds;
      return {id:native.id,native:a,web:b,maximum_error_pixels:scale*Math.max(...['x','y','width','height'].map(k=>Math.abs(a[k]-b[k])),
        Math.abs(a.x+a.width-b.x-b.width),Math.abs(a.y+a.height-b.y-b.height))};
    });
    const layersMaximum=Math.max(0,...layers.map(e=>e.maximum_error_pixels));
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),Math.round(width*scale));
    assert.equal(png.readUInt32BE(20),Math.round(height*scale));
    await writeFile(`${output}/web-${fixture.name}.png`,png);
    const report={name:fixture.name,metrics,layers,layers_maximum_error_pixels:layersMaximum,tool_set:toolSet,tool_set_maximum_error_pixels:toolSetMaximum,header,header_fonts:headerFonts.fonts,header_maximum_error_pixels:headerMaximum,native:{camera:fixture.camera,layout:fixture.layout},
      adaptations:{layer_geometry_source:fixture.layer_geometry_source,native_surface_offset_pixels:fixture.surface_offset_pixels,native_full_client:fixture.full_client,native_caption_button_reservation:fixture.titlebar_insets,system_caption_buttons:'Windows owns these; Chrome leaves the reserved pixels visible'},
      scope:'Full editor; differences remain unaccepted until reviewed. No masks, cropping, resampling or native-measurement substitution.'};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify(report,null,2));
    if(fixture.layers){
      assert.ok(fixture.layers.some(e=>e.id.startsWith('layer-row-')),'Native Layers row measurements are required');
      assert.deepEqual(fixture.layers.map(e=>e.id).sort(),metrics.elements.filter(e=>layerId(e.id)).map(e=>e.id).sort(),
        'Native and reference Layers expose different measured controls');
      assert.ok(layersMaximum<=2.01,`Layers geometry differs by ${layersMaximum} physical pixels`);
    }
    reports.push({name:fixture.name,viewport:metrics.viewport,scale,adapter:metrics.adapter,layers_maximum_error_pixels:layersMaximum,tool_set_maximum_error_pixels:toolSetMaximum,header_maximum_error_pixels:headerMaximum});
  }
  await writeFile(`${output}/windows-editor.json`,JSON.stringify({captures:reports},null,2));
  console.log(JSON.stringify({captures:reports},null,2));
}
