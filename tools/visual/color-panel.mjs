import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Capture production browser controls using the same Rust color/numeric models
// as the native fixture. No color conversion or numeric policy is duplicated.
export async function captureColorPanels({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema,1); assert.equal(manifest.fixtures.length,48);
  await evaluate(`(async()=>{
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    window.colorCaptureIcons={};
    for(const name of ['minus','plus']){
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');
      if(!response.ok)throw Error('Missing numeric icon');colorCaptureIcons[name]=await response.text();
    }
  })()`);
  const reports=[];
  for(const fixture of manifest.fixtures){
    assert.match(fixture.name,/^[a-z0-9-]+$/);
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    const frames=await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)};
      const {createEditorPanels}=await import('/editor-panels.js');
      const {createNumberField}=await import('/numeric.js');
      window.colorCaptureRoot?.disposeEditor();
      document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
      document.documentElement.style.setProperty('--ui-text-size',fixture.text_size+'px');
      for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
        document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
      document.body.style.background=fixture.palette.panel;
      const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
      const button=(label,action,css)=>{const e=element('button',css,label);e.onclick=action;return e;};
      const icon=name=>{const template=document.createElement('template');template.innerHTML=colorCaptureIcons[name];return template.content.firstElementChild;};
      const numberField=(control,label,onChange)=>{
        const index=fixture.model.components.findIndex(c=>c.name===label),component=fixture.model.components[index];
        return createNumberField({control,label,onChange,icon,resolve:({operation,value})=>{
          if(operation.type!=='format')throw Error('Unexpected operation in capture');
          if(value===component.value)return fixture.formatted[index].value;
          if(value===control.min)return fixture.formatted[index].minimum;
          throw Error('Missing Rust numeric result');
        }});
      };
      const panels=createEditorPanels({app:{color_panel:()=>fixture.model},state:()=>({}),element,button,icon,numberField,
        dispatch(){throw Error('Unexpected edit during capture');}});
      const panel=element('div','panel');panel.style.cssText='width:100%;height:auto;padding:0;overflow:visible';
      const root=panels.control('color_wheel');window.colorCaptureRoot=root;panel.append(root);document.body.append(panel);
      await document.fonts.ready;
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const rect=n=>n.getBoundingClientRect().toJSON();
      const frames={panel:rect(root),wheel:rect(root.querySelector('.color-wheel')),space:rect(root.querySelector('.color-space'))};
      for(const swatch of root.querySelectorAll('[data-color-slot]')){
        const item=fixture.model.swatches.find(s=>s.slot===swatch.dataset.colorSlot);
        if(swatch.getAttribute('aria-pressed')!==String(item.selected))throw Error('Incorrect paint selection');
        frames[item.slot]=rect(swatch);
        frames['paint-'+item.slot]=rect(swatch.querySelector('span'));
      }
      frames.swap=rect(root.querySelector('.color-swatches > button:last-child'));
      for(const [index,node] of [...root.querySelectorAll('.number-control')].entries()){
        if(node.querySelector('.number-value').textContent!==fixture.formatted[index].value.text)throw Error('Incorrect numeric display');
        const key='color-'+index;
        for(const [part,selector] of Object.entries({root:null,header:'.number-header',label:'.number-labels',value:'.number-value',track:'.number-slider'}))
          frames[key+':'+part]=rect(selector?node.querySelector(selector):node);
        const steps=node.querySelectorAll('.number-step');frames[key+':minus']=rect(steps[0]);frames[key+':plus']=rect(steps[1]);
      }
      return frames;
    })()`);
    assert.deepEqual(Object.keys(frames).sort(),Object.keys(fixture.frames).sort());
    const differences=Object.entries(frames).map(([key,web])=>({key,native:fixture.frames[key],web,
      error:Math.max(...['x','y','width','height'].map(p=>Math.abs(web[p]-fixture.frames[key][p])))}));
    const maximum=Math.max(...differences.map(d=>d.error));
    const report={name:fixture.name,frames,differences,maximum_error_points:maximum,within_one_point:maximum<=1};
    await writeFile(output+'/geometry-'+fixture.name+'.json',JSON.stringify(report,null,2));
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(output+'/web-'+fixture.name+'.png',png);
    reports.push({name:fixture.name,maximum_error_points:maximum,within_one_point:maximum<=1});
  }
  await evaluate('window.colorCaptureRoot?.disposeEditor()');
  await writeFile(output+'/color-panel-geometry.json',JSON.stringify(reports,null,2));
  console.log('Captured '+reports.length+' production browser Color panels');
}
