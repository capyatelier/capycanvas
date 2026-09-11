// Component-only fixture: actual browser toolbar factory, CSS and SVG assets,
// with Rust-generated views and deterministic placement outside the full editor.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureToolbarFixture({fixture, width, height, scale, output, theme, evaluate, call}) {
  assert.equal(fixture.schema, 1); assert.equal(fixture.width, width); assert.equal(fixture.height, height);
  assert.equal(fixture.theme, theme); assert.equal(fixture.rows.length, 5);
  const metrics = await evaluate(`(async()=>{
    const fixture=${JSON.stringify(fixture)};
    const {createWorkspaceChrome}=await import('/workspace-chrome.js');
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
    document.documentElement.style.setProperty('--ui-text-size',fixture.text_size+'px');
    for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
      document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
    document.body.style.background=fixture.palette.panel;
    const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
    const workspace=element('div','zen-hidden');workspace.style.cssText='position:relative;width:100vw;height:100vh';
    workspace.style.setProperty('--paint-color','rgb('+fixture.color.slice(0,3).map(v=>Math.round(v*255)).join(',')+')');
    document.body.append(workspace);
    const icons=new Map();
    for(const name of new Set(fixture.rows.flatMap(r=>r.panel.tiles.map(t=>t.icon)))) {
      const response=await fetch('/icons/layer-'+name+'-symbolic.svg');if(!response.ok)throw Error('Missing icon '+name);
      const svg=new DOMParser().parseFromString(await response.text(),'image/svg+xml').documentElement;
      svg.dataset.asset=name;svg.setAttribute('aria-hidden','true');icons.set(name,svg);
    }
    let y=6;
    const sections=fixture.rows.map((row,index)=>{
      const [w,h]=row.size;const result={panel:String(index),edge:'top',style:row.panel.tile_style,
        bounds:{x:6,y,width:row.panel.tiles.length*(w+2)-2,height:h},
        tiles:row.panel.tiles.map((t,i)=>[t.id,{x:i*(w+2),y:0,width:w,height:h}])};y+=h+6;return result;
    });
    const chrome=createWorkspaceChrome({app:{workspace_projection:()=>[true,{sections}]},
      state:()=>({customization:{column_drawers:[]},workspace:{layout:{panels:[]}}}),workspace,element,
      button:(text,action)=>{const b=element('button',null,text);b.onclick=action;return b;},
      icon:name=>icons.get(name).cloneNode(true),
      place:(node,b)=>{for(const [key,value] of Object.entries({left:b.x,top:b.y,width:b.width,height:b.height}))node.style[key]=value+'px';},
      dispatch:()=>{},customization:{view:panel=>fixture.rows[Number(panel)].panel,target:()=>{}},
      editor:{queuePositions:()=>{}},draggable:node=>node});
    chrome.arrange({collapsed:[]});
    await document.fonts.ready;
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    return [...workspace.querySelectorAll('.zen-toolbar')].map((r,index)=>({style:fixture.rows[index].panel.tile_style,
      tiles:[...r.querySelectorAll('.tool-tile')].map(t=>({bounds:t.getBoundingClientRect().toJSON(),
        icon:t.querySelector('svg').getBoundingClientRect().toJSON(),
        label:t.querySelector('.tile-label')?.getBoundingClientRect().toJSON()??null,
        labelWeight:t.querySelector('.tile-label')?getComputedStyle(t.querySelector('.tile-label')).fontWeight:null}))}));
  })()`);
  assert.equal(metrics.length,fixture.rows.length);
  for(const [index,row] of metrics.entries()) {
    const source=fixture.rows[index];assert.equal(row.tiles.length,source.panel.tiles.length);
    for(const tile of row.tiles) {
      assert.equal(tile.bounds.width,source.size[0]);assert.equal(tile.bounds.height,source.size[1]);
      assert.equal(tile.icon.width,source.panel.tile_icon_size);assert.equal(tile.icon.height,source.panel.tile_icon_size);
      assert.equal(!!tile.label,source.panel.tile_label_lines>0);
      if(tile.label) {assert.equal(tile.label.x-tile.bounds.x,36);assert.equal(tile.label.width,72);}
    }
  }
  const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
  const png=Buffer.from(shot.data,'base64');assert.equal(png.readUInt32BE(16),width*scale);assert.equal(png.readUInt32BE(20),height*scale);
  await writeFile(output+'/web-'+theme+'-toolbar-tiles.png',png);
  await writeFile(output+'/web-'+theme+'-toolbar-metrics.json',JSON.stringify(metrics,null,2));
  console.log('Captured five shared toolbar styles; component geometry passed');
}
