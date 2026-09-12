import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

// The production web app loads and clones these same canonical inline SVGs.
// This isolates glyph paint/compositing from editor geometry and font differences.
export async function captureIcons({manifest, output, evaluate, call}) {
  assert.equal(manifest.schema, 1);
  for (const fixture of manifest.fixtures) {
    assert.match(fixture.name, /^[a-z0-9-]+$/);
    await call('Emulation.setDeviceMetricsOverride', {width:fixture.width,height:fixture.height,deviceScaleFactor:fixture.scale,mobile:false});
    const frames = await evaluate(`(async()=>{
      const fixture=${JSON.stringify(fixture)};
      document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
      await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
      document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
      document.body.style.background=fixture.background;document.body.style.color=fixture.foreground;
      const frames=[];
      for (const [index,name] of fixture.icons.entries()) {
        const response=await fetch('/icons/'+name+'.svg');if(!response.ok)throw Error('Missing '+name);
        const svg=new DOMParser().parseFromString(await response.text(),'image/svg+xml').documentElement;
        svg.style.cssText='position:absolute;width:'+fixture.size+'px;height:'+fixture.size+'px;left:'+
          (index%12*48+24-fixture.size/2)+'px;top:'+(Math.floor(index/12)*48+24-fixture.size/2)+'px;opacity:'+fixture.opacity;
        document.body.append(svg);frames.push({name,bounds:svg.getBoundingClientRect().toJSON()});
      }
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      return frames;
    })()`);
    await writeFile(`${output}/geometry-${fixture.name}.json`, JSON.stringify(frames,null,2)+'\n');
    const image=await call('Page.captureScreenshot',{format:'png',fromSurface:true,captureBeyondViewport:false});
    await writeFile(`${output}/web-${fixture.name}.png`,Buffer.from(image.data,'base64'));
  }
}
