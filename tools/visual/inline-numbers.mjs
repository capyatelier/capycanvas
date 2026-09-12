import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Production inline numeric controls; Rust supplies all formatting and fills.
export async function captureInlineNumbers({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema,1);
  const reports=[];
  for(const fixture of manifest.fixtures) {
    assert.match(fixture.name,/^[a-z0-9-]+$/);
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    const metrics=await evaluate(`(async()=>{
      const f=${JSON.stringify(fixture)}, {createNumberField}=await import('/numeric.js');
      document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
      await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
      document.body.replaceChildren();document.body.dataset.theme=f.theme;
      document.documentElement.style.setProperty('--ui-text-size',f.text_size+'px');
      for(const [name,color] of Object.entries(f.palette))if(typeof color==='string')
        document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
      document.body.style.background=f.palette.panel;
      const panel=document.createElement('div');panel.className='panel';
      panel.style.cssText='position:absolute;left:6px;top:6px;width:'+(f.width-12)+'px;height:auto;padding:0;border:0';
      const root=createNumberField({control:f.control,label:'Layer opacity',inline:true,
        resolve:({operation,value})=>{
          if(operation.type!=='format')throw Error('Unexpected edit during inline capture');
          if(value===f.value)return f.formatted;if(value===f.control.min)return f.minimum;
          if(value===f.control.max)return f.maximum;throw Error('Missing Rust numeric format');
        },onChange(){throw Error('Unexpected edit during inline capture');},
        icon:()=>document.createElement('span')}); // Inline controls remove both step icons.
      root.update(f.value);root.setDisabled(!f.enabled);panel.append(root);document.body.append(panel);
      await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const frames={};
      for(const [part,selector] of Object.entries({root:null,track:'.number-slider',value:'.number-value'}))
        frames['layer-opacity:'+part]=(selector?root.querySelector(selector):root).getBoundingClientRect().toJSON();
      const value=root.querySelector('.number-value');
      if(value.textContent!==f.formatted.text||value.disabled===f.enabled)throw Error('Incorrect inline display/availability');
      const measure=root.querySelector('.number-measure');
      const result={frames,measure:{text:measure.textContent,bounds:measure.getBoundingClientRect().toJSON(),font:getComputedStyle(measure).font}};
      if(f.enabled){
        value.click();
        result.entry={bounds:root.entry.getBoundingClientRect().toJSON(),text:root.entry.value,size:root.entry.size};
        root.cancelEditing();root.entry.blur();
      }
      return result;
    })()`);
    const keys=Object.keys(metrics.frames), missing=keys.filter(key=>!fixture.frames[key]);
    const extra=Object.keys(fixture.frames).filter(key=>!metrics.frames[key]);
    const differences=keys.filter(key=>fixture.frames[key]).map(key=>({key,native:fixture.frames[key],web:metrics.frames[key],
      error:Math.max(...['x','y','width','height'].map(p=>Math.abs(metrics.frames[key][p]-fixture.frames[key][p])))}));
    const maximum=differences.length?Math.max(...differences.map(d=>d.error)):null;
    const geometry={missing,extra,differences,maximum_error_points:maximum,
      within_one_point:missing.length===0&&extra.length===0&&maximum!==null&&maximum<=1};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify({...metrics,geometry},null,2)+'\n');
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true,captureBeyondViewport:false});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(`${output}/web-${fixture.name}.png`,png);
    reports.push({name:fixture.name,...geometry});
  }
  await writeFile(`${output}/inline-number-geometry.json`,JSON.stringify(reports,null,2)+'\n');
  console.log(`Captured ${reports.length} browser inline numeric controls`);
}
