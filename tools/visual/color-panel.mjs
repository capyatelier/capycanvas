import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {dirname,join} from 'node:path';

// Capture actual production DOM controls using shared Rust models and CPU pixels.
export async function captureColorPanels({manifest,fixturePath,output,evaluate,call}) {
  assert.equal(manifest.schema,2);
  assert.ok(manifest.fixtures.length>0);
  await evaluate(`(async()=>{
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    window.colorCaptureIcons={};
    for(const name of ['color-square','color-circle','color-triangle','color-swap']){
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');
      if(!response.ok)throw Error('Missing shared color icon');
      colorCaptureIcons[name]=await response.text();
    }
  })()`);
  const reports=[];
  for(const source of manifest.fixtures) {
    assert.match(source.name,/^(dark|light)-(128|160|226|360)$/);
    const fixture=structuredClone(source);
    for(const item of fixture.items)if(item.field_file)
      item.field_base64=(await readFile(join(dirname(fixturePath),item.field_file))).toString('base64');
    await evaluate('window.colorCaptureRoots?.forEach(root=>root.disposeEditor())');
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    const frames=await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)};
      const {createEditorPanels}=await import('/editor-panels.js');
      window.colorCaptureRoots?.forEach(root=>root.disposeEditor());window.colorCaptureRoots=[];
      document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
      document.body.style.cssText='margin:0;padding:0;overflow:hidden;background:'+fixture.palette.panel;
      document.documentElement.style.setProperty('--ui-text-size',(fixture.catalog.text_size_pt*96/72)+'px');
      for(const [name,color]of Object.entries(fixture.palette))if(typeof color==='string')
        document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
      const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
      const button=(label,action,css)=>{const e=element('button',css,label);e.onclick=action;return e;};
      const icon=name=>{const template=document.createElement('template');template.innerHTML=colorCaptureIcons[name];return template.content.firstElementChild;};
      const roots=[];
      for(const item of fixture.items){
        const bytes=item.field_base64?Uint8Array.from(atob(item.field_base64),c=>c.charCodeAt(0)):null;
        const app={color_panel:()=>item.model,color_panel_layout:size=>{if(size!==item.size)throw Error('Unexpected layout size');return item.layout;},
          color_hue_stops:()=>item.hue_stops,color_field_pixels:side=>{if(side!==item.field_side)throw Error('Unexpected field resolution');return bytes;}};
        const panels=createEditorPanels({app,state:()=>({}),element,button,icon,dispatch(){throw Error('Unexpected capture edit');}});
        const root=panels.control('color_wheel');
        root.style.cssText='position:absolute;left:'+item.x+'px;top:'+item.y+'px;width:'+item.size+'px;height:'+item.size+'px';
        document.body.append(root);colorCaptureRoots.push(root);roots.push({item,root});
      }
      await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const frames={},selectors={'color-panel':'.color-wheel-square','color-wheel':'.color-wheel',
        'color-background':'[data-color-slot="background"]','color-foreground':'[data-color-slot="foreground"]',
        'color-transparent':'[data-color-slot="transparent"]','color-swap':'.color-swap','color-readout':'.color-readout'};
      for(const {item,root}of roots){
        for(const [id,selector]of Object.entries(selectors))frames[item.key+'/'+id]=root.querySelector(selector).getBoundingClientRect().toJSON();
        root.querySelectorAll('.color-shape').forEach((node,i)=>frames[item.key+'/color-shape-'+i]=node.getBoundingClientRect().toJSON());
        for(const node of root.querySelectorAll('[data-color-slot]')){
          const swatch=item.model.swatches.find(s=>s.slot===node.dataset.colorSlot);
          if(node.getAttribute('aria-pressed')!==String(swatch.selected))throw Error('Wrong selected paint');
        }
      }
      return frames;
    })()`);
    assert.deepEqual(Object.keys(frames).sort(),Object.keys(fixture.frames).sort());
    const differences=Object.entries(frames).map(([key,web])=>({key,native:fixture.frames[key],web,
      error:Math.max(...['x','y','width','height'].map(p=>Math.abs(web[p]-fixture.frames[key][p])))}));
    const maximum=Math.max(...differences.map(d=>d.error));
    await writeFile(join(output,'geometry-'+fixture.name+'.json'),JSON.stringify({differences,maximum_error_points:maximum},null,2));
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(join(output,'web-'+fixture.name+'.png'),png);
    reports.push({name:fixture.name,maximum_error_points:maximum});
  }
  await evaluate('window.colorCaptureRoots?.forEach(root=>root.disposeEditor())');
  await writeFile(join(output,'color-panel-geometry.json'),JSON.stringify(reports,null,2));
  console.log('Captured '+reports.length+' production compact Color fixtures');
}
