// Production browser tool-action factory at matching component dimensions.
import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export async function captureToolActions({fixture, width, height, scale, output, theme, evaluate, call}) {
  assert.equal(fixture.schema, 1);
  assert.equal(fixture.width, width); assert.equal(fixture.height, height);
  assert.equal(fixture.scale, scale); assert.equal(fixture.theme, theme);
  const frames = await evaluate(`(async()=>{
    const fixture=${JSON.stringify(fixture)};
    const {createEditorPanels}=await import('/editor-panels.js');
    document.head.innerHTML='<link rel="stylesheet" href="/style.css">';
    await new Promise((resolve,reject)=>{const link=document.head.querySelector('link');link.onload=resolve;link.onerror=reject;});
    document.body.replaceChildren();document.body.dataset.theme=fixture.theme;
    document.documentElement.style.setProperty('--ui-text-size',fixture.text_size+'px');
    for(const [name,color] of Object.entries(fixture.palette))if(typeof color==='string')
      document.body.style.setProperty('--'+name.replaceAll('_','-'),name==='button'?color+'0d':color);
    document.body.style.background=fixture.palette.panel;
    const element=(tag,css,text)=>{const e=document.createElement(tag);if(css)e.className=css;if(text)e.textContent=text;return e;};
    const button=(text,action,css)=>{const b=element('button',css,text);b.onclick=action;return b;};
    const controls=[];
    for(let column=0;column<4;column++) {
      const commands=fixture.actions.map(item=>({...item.command,selected:column%2===1,enabled:column<2}));
      const tool_actions=fixture.actions.map(item=>({checkable:item.checkable,command:item.command.id}));
      const panels=createEditorPanels({state:()=>({commands,tool_actions,tool_settings:[]}),element,button,dispatch(){}});
      const root=panels.control('tool_settings');root.disposeEditor();
      root.style.cssText='position:absolute;top:6px;left:'+(6+column*(fixture.column_width+8))+'px;width:'+fixture.column_width+'px';
      document.body.append(root);controls.push([...root.children]);
    }
    await document.fonts.ready;
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    return controls.flatMap((nodes,column)=>nodes.map((node,index)=>({key:column+'-'+index,
      bounds:node.getBoundingClientRect().toJSON(),enabled:!node.disabled,selected:node.getAttribute('aria-pressed'),
      label:node.textContent,opacity:getComputedStyle(node).opacity,background:getComputedStyle(node).backgroundColor})));
  })()`);
  assert.equal(frames.length, 4*fixture.actions.length);
  for (const row of frames) {
    const [column,index]=row.key.split('-').map(Number), item=fixture.actions[index];
    assert.equal(row.enabled,column<2);
    assert.equal(row.selected,item.checkable?String(column%2===1):null);
    assert.equal(row.label,item.command.label);
    assert.equal(row.opacity,column<2?'1':'0.36');
    assert.equal(row.bounds.width,fixture.column_width);
  }
  const name='web-'+theme+'-'+fixture.column_width;
  await writeFile(output+'/'+name+'.json',JSON.stringify({scale,frames:Object.fromEntries(frames.map(r=>[r.key,r.bounds])),controls:frames},null,2));
  const shot=await call('Page.captureScreenshot',{format:'png',fromSurface:true});
  const png=Buffer.from(shot.data,'base64');
  assert.equal(png.readUInt32BE(16),width*scale);assert.equal(png.readUInt32BE(20),height*scale);
  await writeFile(output+'/'+name+'.png',png);
  console.log('Captured '+frames.length+' production browser tool-action buttons');
}
