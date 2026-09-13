import assert from 'node:assert/strict';
import {readFile, writeFile} from 'node:fs/promises';
import {basename, dirname, join} from 'node:path';

const selectors={panel:'.color-wheel-square',wheel:'.color-wheel',readout:'.color-readout',
  foreground:'[data-color-slot="foreground"]',background:'[data-color-slot="background"]',
  transparent:'[data-color-slot="transparent"]',swap:'.color-swap',
  'shape-0':'.color-shape','shape-1':'.color-shape'};

// Both native manifests feed one production DOM renderer. Apple captures each
// complete panel; Windows captures a grid of panels with prefixed frame names.
function captureItems(fixture,grid) {
  const items=grid?fixture.items:[{x:0,y:0,size:fixture.width,model:fixture.model,
    ...fixture.resources,field_file:fixture.field_file,field_side:fixture.field_side}];
  assert.ok(items.length>0);
  return items.map(item=>({...item,frame_specs:[
    ...Object.entries(selectors).map(([id,selector])=>({
      key:grid?item.key+'/color-'+id:id,selector,
      // Shape buttons share a parent with the other controls, so their index is
      // within the shape collection, not CSS's collection of all button siblings.
      ...(id.startsWith('shape-')?{selector:'.color-shape',index:Number(id.slice(-1))}:{}),
    })),
    ...(!grid?['foreground','background','transparent'].map(id=>({key:'paint-'+id,selector:selectors[id]+' span'})):[]),
  ]}));
}

// Exact Rust models, layouts, stops and field bytes; no copied color policy.
export async function captureColorPanels({manifest,fixturePath,output,evaluate,call}) {
  assert.equal(manifest.schema,2);
  assert.ok(manifest.fixtures.length>0);
  const grid=Array.isArray(manifest.fixtures[0].items);
  assert.ok(manifest.fixtures.every(f=>Array.isArray(f.items)===grid),'Mixed fixture formats');
  if(!grid)assert.equal(manifest.fixtures.length,manifest.interaction_states===true?18:216);
  else assert.notEqual(manifest.interaction_states,true);
  await evaluate(`(async()=>{
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    window.colorCaptureIcons={};
    for(const name of ['color-circle','color-square','color-triangle','color-swap']){
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');
      if(!response.ok)throw Error('Missing color icon');colorCaptureIcons[name]=await response.text();
    }
  })()`);
  const reports=[];
  for(const source of manifest.fixtures){
    assert.match(source.name,grid?/^(dark|light)-(128|160|226|360)$/:/^[a-z0-9-]+$/);
    const fixture={...source,items:captureItems(source,grid)};
    for(const item of fixture.items)if(item.field_file){
      assert.equal(basename(item.field_file),item.field_file);
      if(!grid)assert.equal(item.field_file,'field-'+fixture.name+'.rgba');
      const bytes=await readFile(join(dirname(fixturePath),item.field_file));
      assert.equal(bytes.length,item.field_side**2*4);
      item.field_base64=bytes.toString('base64');
    }
    await evaluate('window.colorCaptureRoots?.forEach(root=>root.disposeEditor()); window.colorCaptureRoots=[]; document.body.replaceChildren()');
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    // Place the pointer outside all controls before constructing them, avoiding
    // stale hover state when a preceding fixture had a different size or state.
    const outside={x:fixture.width-2,y:2};
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...outside});
    const frames=await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)};
      const {createEditorPanels}=await import('/editor-panels.js');
      document.body.dataset.theme=fixture.theme;
      document.body.style.cssText='margin:0;padding:0;overflow:hidden;background:'+fixture.palette.panel;
      if(fixture.catalog)document.documentElement.style.setProperty('--ui-text-size',(fixture.catalog.text_size_pt*96/72)+'px');
      else document.documentElement.style.removeProperty('--ui-text-size');
      for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
        document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
      const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
      const button=(label,action,css)=>{const e=element('button',css,label);e.onclick=action;return e;};
      const icon=name=>{const template=document.createElement('template');template.innerHTML=colorCaptureIcons[name];return template.content.firstElementChild;};
      for(const item of fixture.items){
        const bytes=item.field_base64?Uint8Array.from(atob(item.field_base64),c=>c.charCodeAt(0)):null;
        const app={color_panel:()=>item.model,color_panel_layout:size=>{
          if(size!==item.size)throw Error('Unexpected panel allocation');return item.layout;
        },color_hue_stops:()=>item.hue_stops,color_field_pixels:side=>{
          if(side!==item.field_side)throw Error('Unexpected field raster size');return bytes;
        }};
        const panels=createEditorPanels({app,state:()=>({}),element,button,icon,
          dispatch(){throw Error('Unexpected edit during capture');}});
        const root=panels.control('color_wheel');
        root.style.cssText='position:absolute;left:'+item.x+'px;top:'+item.y+'px;width:'+item.size+'px;height:'+item.size+'px';
        document.body.append(root);colorCaptureRoots.push(root);
      }
      await document.fonts.ready;
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const frames={};
      fixture.items.forEach((item,i)=>{
        const root=colorCaptureRoots[i];
        for(const spec of item.frame_specs)
          frames[spec.key]=root.querySelectorAll(spec.selector)[spec.index||0].getBoundingClientRect().toJSON();
        for(const node of root.querySelectorAll('[data-color-slot]')){
          const swatch=item.model.swatches.find(s=>s.slot===node.dataset.colorSlot);
          if(node.getAttribute('aria-pressed')!==String(swatch.selected))throw Error('Incorrect paint selection');
        }
      });
      return frames;
    })()`);
    assert.deepEqual(Object.keys(frames).sort(),Object.keys(fixture.frames).sort());
    const differences=Object.entries(frames).map(([key,web])=>({key,native:fixture.frames[key],web,
      error:Math.max(...['x','y','width','height'].map(p=>Math.abs(web[p]-fixture.frames[key][p])))}));
    const maximum=Math.max(...differences.map(d=>d.error));
    const report={name:fixture.name,frames,differences,maximum_error_points:maximum,within_one_point:maximum<=1};
    await writeFile(join(output,'geometry-'+fixture.name+'.json'),JSON.stringify(report,null,2));
    let target='',pressed=false,focus=false;
    if(manifest.interaction_states===true){
      ({target,pressed}=fixture.interaction);
      assert(['','foreground','background','transparent','shape-0','swap'].includes(target));
      assert.equal(typeof pressed,'boolean');
      assert.ok(target||!pressed);
    }else if(grid&&(fixture.capture_state||'default')!=='default'){
      assert(['color-readout','color-shape-0','color-background','color-swap'].includes(fixture.capture_target));
      target=fixture.capture_target.slice('color-'.length);
      focus=fixture.capture_state.endsWith('-focus');
      assert.ok(focus||fixture.capture_state.endsWith('-hover'),'Unknown color capture state');
    }
    if(target){
      const spec=fixture.items[0].frame_specs.find(s=>s.key===(grid?fixture.items[0].key+'/color-'+target:target));
      const node=`colorCaptureRoots[0].querySelectorAll(${JSON.stringify(spec.selector)})[${spec.index||0}]`;
      if(focus){
        await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
        await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
        await evaluate(`${node}.focus()`);
        assert.equal(await evaluate(`${node}.matches(':focus-visible')`),true);
      }else{
        const frame=frames[spec.key],point={
          x:!grid&&target==='background'?frame.x+frame.width-3:frame.x+frame.width/2,y:frame.y+frame.height/2};
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});
        assert.equal(await evaluate(`${node}.matches(':hover')`),true);
        if(pressed){
          await call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
          assert.equal(await evaluate(`${node}.matches(':active')`),true);
        }
      }
      await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
    }
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(join(output,'web-'+fixture.name+'.png'),png);
    if(pressed){
      // Cancel outside the button so the reference fixture never dispatches an edit.
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',...outside,button:'left',buttons:1});
      await call('Input.dispatchMouseEvent',{type:'mouseReleased',...outside,button:'left',buttons:0,clickCount:1});
    }
    reports.push({name:fixture.name,maximum_error_points:maximum,within_one_point:maximum<=1});
  }
  await evaluate('window.colorCaptureRoots?.forEach(root=>root.disposeEditor())');
  await writeFile(join(output,'color-panel-geometry.json'),JSON.stringify(reports,null,2));
  console.log('Captured '+reports.length+' production compact Color fixtures');
}
