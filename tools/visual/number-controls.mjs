import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// Production DOM numeric controls. All displayed values/fills come from the
// same Rust resolver as the native fixture; this does not emulate numeric math.
export async function captureNumberControls({fixture, width, height, scale, output, theme, evaluate, call}) {
  assert.equal(fixture.schema, 1);
  assert.equal(fixture.width, width); assert.equal(fixture.height, height);
  assert.equal(fixture.scale, scale); assert.equal(fixture.theme, theme);
  const frames = await evaluate(`(async()=>{
    const fixture=${JSON.stringify(fixture)};
    const {createNumberField}=await import('/numeric.js');
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
    document.documentElement.style.setProperty('--ui-text-size',fixture.text_size+'px');
    for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
      document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
    document.body.style.background=fixture.palette.panel;
    const icons={};
    for(const name of ['minus','plus']) {
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');
      if(!response.ok)throw Error('Missing numeric icon');icons[name]=await response.text();
    }
    const controls=[];
    let x=6;
    for(let column=0;column<fixture.column_widths.length;column++) {
      for(let row=0;row<fixture.rows.length;row++) {
        const item=fixture.rows[row], panel=document.createElement('div'); panel.className='panel';
        panel.style.cssText='position:absolute;left:'+x+'px;top:'+(6+row*64)+'px;width:'+fixture.column_widths[column]+'px;height:auto;padding:0;border:0';
        const root=createNumberField({control:item.control,label:item.label,
          resolve:({operation,value})=>{if(operation.type!=='format')throw Error('Unexpected operation in capture');
            if(value===item.value)return item.formatted;if(value===item.control.min)return item.minimum;
            throw Error('Missing Rust format result');},
          onChange(){throw Error('Unexpected edit in capture');},
          icon:name=>{const template=document.createElement('template');template.innerHTML=icons[name];return template.content.firstElementChild;}});
        root.update(item.value);root.setDisabled(!item.enabled);panel.append(root);document.body.append(panel);
        controls.push({key:column+'-'+row,root});
      }
      x+=fixture.column_widths[column]+8;
    }
    await document.fonts.ready;
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    const frames={};
    for(const {key,root} of controls) {
      const item=fixture.rows[Number(key.split('-')[1])], steps=root.querySelectorAll('.number-step');
      if(steps[0].disabled!==(!item.enabled||item.value<=item.control.min)||
         steps[1].disabled!==(!item.enabled||item.value>=item.control.max))throw Error('Incorrect step availability');
      const display=root.querySelector(item.control.kind==='slider'?'.number-value':'.number-entry');
      if((item.control.kind==='slider'?display.textContent:display.value)!==item.formatted.text)throw Error('Incorrect formatted display');
      for(const [part,selector] of Object.entries({root:null,header:'.number-header',label:'.number-labels',value:'.number-value',entry:'.number-entry',minus:'.number-step:first-of-type',plus:'.number-step:last-of-type',track:'.number-slider'})) {
        let node=selector?root.querySelector(selector):root;
        if(part==='minus'||part==='plus')node=root.querySelectorAll('.number-step')[part==='minus'?0:1];
        if(node&&node.getBoundingClientRect().height)frames[key+':'+part]=node.getBoundingClientRect().toJSON();
      }
    }
    return frames;
  })()`);
  assert.equal(Object.keys(frames).filter(k=>k.endsWith(':root')).length,30);
  const missing=Object.keys(fixture.frames).filter(k=>!frames[k]);
  const extra=Object.keys(frames).filter(k=>!fixture.frames[k]);
  const differences=Object.entries(frames).filter(([key])=>fixture.frames[key]).map(([key,bounds])=>({key,
    error:Math.max(...['x','y','width','height'].map(p=>Math.abs(bounds[p]-fixture.frames[key][p])))}));
  const maximum=Math.max(...differences.map(v=>v.error));
  const geometry={maximum_error_points:maximum,missing,extra,differences,
    within_one_point:maximum<=1&&missing.length===0&&extra.length===0};
  await writeFile(output+'/web-'+fixture.name+'.json',JSON.stringify({scale,frames,geometry},null,2));
  const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
  const png=Buffer.from(shot.data,'base64');
  assert.equal(png.readUInt32BE(16),width*scale);assert.equal(png.readUInt32BE(20),height*scale);
  await writeFile(output+'/web-'+fixture.name+'.png',png);
  if(fixture.require_geometry_parity)assert(geometry.within_one_point,
    'Numeric geometry differs by more than one logical pixel: '+JSON.stringify(geometry));
  console.log('Captured 30 production browser numeric controls: '+fixture.name);
}
