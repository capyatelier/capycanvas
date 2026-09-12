import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Production HTML select primitives and CSS ancestors used by effects/layers.
// Rust supplies option names; this isolates closed controls from popup routing.
export async function captureChoices({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema,1);
  const reports=[];
  for (const fixture of manifest.fixtures) {
    assert.match(fixture.name,/^[a-z0-9-]+$/);
    await call('Emulation.setDeviceMetricsOverride',{width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    const metrics=await evaluate(`(async()=>{
      const f=${JSON.stringify(fixture)};
      document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
      await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
      document.body.replaceChildren();document.body.dataset.theme=f.theme;
      document.documentElement.style.setProperty('--ui-text-size',f.text_size+'px');
      for(const [key,value] of Object.entries(f.palette))if(typeof value==='string')
        document.body.style.setProperty('--'+key.replaceAll('_','-'),value);
      document.body.style.background=f.palette.panel;
      const make=(tag,css)=>{const n=document.createElement(tag);n.className=css||'';return n;};
      const select=()=>{const n=make('select');for(const [i,label] of f.options.entries()){
        const o=make('option');o.value=i;o.textContent=label;n.append(o);
      }n.value=f.selected;n.disabled=!f.enabled;return n;};
      const property=make('div','property-controls'+(f.enabled?'':' disabled'));
      property.style.cssText='position:absolute;left:6px;top:6px;width:'+(f.width-12)+'px';
      const row=make('label','property-row'),label=make('span');label.textContent='Blend mode';
      const choices={property:select(),wide:select(),compact:select()};row.append(label,choices.property);property.append(row);
      const wide=make('div','property-controls'+(f.enabled?'':' disabled'));
      wide.style.cssText='position:absolute;left:6px;top:56px;width:'+(f.width-12)+'px';wide.append(choices.wide);
      const compact=make('div','panel');compact.style.cssText='position:absolute;padding:0;left:6px;top:106px;width:'+(f.width-12)+'px';
      const grid=make('div','layer-options');grid.append(choices.compact,make('div'));compact.append(grid);
      document.body.append(property,wide,compact);
      await document.fonts.ready;await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      const output={};const canvas=document.createElement('canvas'),context=canvas.getContext('2d');
      for(const [name,node] of Object.entries(choices)){
        const style=getComputedStyle(node);context.font=style.font;
        output[name]={bounds:node.getBoundingClientRect().toJSON(),font:style.font,color:style.color,
          padding:[style.paddingTop,style.paddingRight,style.paddingBottom,style.paddingLeft],
          radius:style.borderRadius,appearance:style.appearance,
          optionWidths:f.options.map(text=>context.measureText(text).width)};
      }
      context.font=getComputedStyle(label).font;
      output.label={bounds:label.getBoundingClientRect().toJSON(),font:context.font,naturalWidth:context.measureText(label.textContent).width};
      return output;
    })()`);
    const differences=['property','wide','compact'].map(key=>{
      assert.ok(fixture.frames[key],`Missing native control: ${key}`);
      const native=fixture.frames[key],web=metrics[key].bounds;
      return {key,native,web,error:Math.max(...['x','y','width','height'].map(p=>Math.abs(web[p]-native[p])))};
    });
    const maximum=Math.max(...differences.map(d=>d.error));
    const geometry={differences,maximum_error_points:maximum,within_one_point:maximum<=1};
    await writeFile(`${output}/geometry-${fixture.name}.json`,JSON.stringify({...metrics,geometry},null,2)+'\n');
    const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true,captureBeyondViewport:false});
    const png=Buffer.from(shot.data,'base64');
    assert.equal(png.readUInt32BE(16),fixture.width*fixture.scale);
    assert.equal(png.readUInt32BE(20),fixture.height*fixture.scale);
    await writeFile(`${output}/web-${fixture.name}.png`,png);
    reports.push({name:fixture.name,maximum_error_points:maximum,within_one_point:maximum<=1});
  }
  await writeFile(`${output}/choice-geometry.json`,JSON.stringify(reports,null,2)+'\n');
  console.log(`Captured ${reports.length} browser choice fixtures`);
}
