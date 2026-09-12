// Real DOM widget factories with explicit selected/disabled combinations.
// Placement is deterministic; these are component pixels, not editor workflows.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureControlColors({fixture, width, height, scale, output, theme, evaluate, call}) {
  assert.equal(fixture.schema, 1);
  assert.equal(fixture.width, width); assert.equal(fixture.height, height);
  assert.equal(fixture.scale, scale); assert.equal(fixture.theme, theme);
  const metrics = await evaluate(`(async()=>{
    const fixture=${JSON.stringify(fixture)};
    const {createEditorPanels}=await import('/editor-panels.js');
    const {createWorkspaceChrome}=await import('/workspace-chrome.js');
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
    document.documentElement.style.setProperty('--ui-text-size',fixture.text_size+'px');
    for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
      document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
    document.body.style.background=fixture.palette.panel;
    const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
    const button=(text,action,css)=>{const b=element('button',css,text);b.onclick=action;return b;};
    const response=await fetch('/icons/layer-'+fixture.tile.icon+'-symbolic.svg');
    if(!response.ok)throw Error('Missing icon '+fixture.tile.icon);
    const svg=new DOMParser().parseFromString(await response.text(),'image/svg+xml').documentElement;
    svg.setAttribute('aria-hidden','true');
    const icon=()=>svg.cloneNode(true);
    const place=(node,b)=>{for(const [key,value] of Object.entries({left:b.x,top:b.y,width:b.width,height:b.height}))node.style[key]=value+'px';};
    const workspace=element('div','zen-hidden');workspace.style.cssText='position:relative;width:100vw;height:100vh';
    document.body.append(workspace);
    const captured=[];
    for(const row of fixture.rows.filter(row=>row.kind==='icon')) {
      const ids=['zoom_out','zoom_in','rotate_left','rotate_right','flip_horizontal','flip_vertical'];
      const commands=ids.map((id,i)=>({...fixture.tile,...row.cells[i%row.cells.length],id}));
      const panels=createEditorPanels({app:{navigator_surface(){},remove_navigator_surface(){},reflow_navigators(){return false;}},
        state:()=>({commands}),element,button,icon,dispatch(){},wake(){}});
      const root=panels.control('navigator');root.disposeEditor();
      const controls=root.querySelector('.navigator-buttons');
      while(controls.children.length>row.cells.length)controls.lastChild.remove();
      root.replaceChildren(controls);root.classList.add('panel');
      root.style.cssText='position:absolute;padding:0;margin:0;border:0;box-shadow:none;background:transparent;';
      place(root,{x:row.cells[0].bounds.x,y:row.cells[0].bounds.y,width:156,height:36});
      controls.style.cssText='gap:4px;flex:0 0 36px;height:36px;';
      workspace.append(root);captured.push([...controls.children]);
    }
    const rows=fixture.rows.filter(row=>row.kind==='toolbar');
    const views=rows.map(row=>({...fixture.panel,tiles:row.cells.map((cell,id)=>({...fixture.tile,...cell,id}))}));
    const sections=rows.map((row,index)=>({panel:String(index),edge:'top',style:fixture.panel.tile_style,
      bounds:{x:row.cells[0].bounds.x,y:row.cells[0].bounds.y,width:156,height:36},
      tiles:row.cells.map((cell,id)=>[id,{x:id*40,y:0,width:36,height:36}])}));
    const chrome=createWorkspaceChrome({app:{workspace_projection:()=>[true,{sections}]},
      state:()=>({customization:{column_drawers:[]},workspace:{layout:{panels:[]}}}),workspace,element,button,icon,place,
      dispatch(){},customization:{view:id=>views[Number(id)],target(){}},editor:{queuePositions(){}},draggable:node=>node});
    chrome.arrange({collapsed:[]});
    for(const row of workspace.querySelectorAll('.zen-toolbar'))captured.push([...row.querySelectorAll('.tool-tile button')]);
    await document.fonts.ready;
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    return captured.map(row=>row.map(node=>({bounds:node.getBoundingClientRect().toJSON(),
      icon:node.querySelector('svg').getBoundingClientRect().toJSON(),
      selected:node.getAttribute('aria-pressed')==='true',enabled:!node.disabled,
      background:getComputedStyle(node).backgroundColor,opacity:getComputedStyle(node).opacity})));
  })()`);
  await writeFile(output+'/web-'+theme+'-control-colors.json', JSON.stringify(metrics, null, 2));
  assert.equal(metrics.length, fixture.rows.length);
  for (const [r, row] of metrics.entries()) {
    assert.equal(row.length, fixture.rows[r].cells.length);
    for (const [c, cell] of row.entries()) {
      const expected = fixture.rows[r].cells[c];
      for (const key of ['x', 'y', 'width', 'height']) assert.equal(cell.bounds[key], expected.bounds[key]);
      assert.equal(cell.selected, expected.selected, `row ${r} cell ${c} selection`);
      assert.equal(cell.enabled, expected.enabled, `row ${r} cell ${c} enabled`);
      assert.equal(cell.icon.width, 16); assert.equal(cell.icon.height, 16);
      assert.equal(cell.opacity, expected.enabled ? '1' : '0.36');
    }
  }
  const shot = await call('Page.captureScreenshot', {format:'png', fromSurface:true});
  const png = Buffer.from(shot.data, 'base64');
  assert.equal(png.readUInt32BE(16), width * scale); assert.equal(png.readUInt32BE(20), height * scale);
  await writeFile(output+'/web-'+theme+'-control-colors.png', png);
  console.log('Captured 24 real browser icon/toolbar controls; geometry and states passed');
}
