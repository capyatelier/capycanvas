import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkColumnStacks({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace');
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon'});
  const split=(id,first,second)=>({kind:'split',id,axis:'vertical',fraction:.5,first,second});
  const wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(180);};
  const edit=action=>send({type:'customize',action});
  const snapshot=()=>evaluate('layerApp.state().workspace');
  const resolved=()=>evaluate('layerApp.app.layout(innerWidth,innerHeight)');
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const center=b=>({x:b.x+b.width/2,y:b.y+b.height/2});
  const tile=panel=>`.collapsed-column .column-tab[data-panel="${panel}"]`;
  const click=async selector=>{await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();await wait(180);};
  let device='mouse',point,down=false;
  const input=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...p}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
    down=type!=='up';await settle();
  };
  const escape=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();await wait(180);};
  const fixture=structuredClone(saved);
  Object.assign(fixture.layout,{bands:[
    {id:40,edge:'left',extent:42,root:split(41,split(42,tabs(43,['brushes','sizes']),tabs(44,['navigator'])),tabs(45,['color']))},
    {id:46,edge:'right',extent:252,root:split(47,tabs(48,['layers','properties','adjustments']),tabs(49,['toolbar']))},
  ],floating:[],collapsed:[41,42,45].map(root=>({root,expanded_width:246})),column_stacks:[{column:41,members:[42,45],drawers:false,auto_hide:false}],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],next_id:Math.max(50,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const reset=()=>send({type:'restore_workspace',workspace:fixture});
  try {
    await call('Page.bringToFront');await reset();
    let layout=await resolved();
    assert.equal(layout.collapsed.length,2);
    assert.equal(layout.collapsed[1].bounds.y-layout.collapsed[0].bounds.y-layout.collapsed[0].bounds.height,6);
    assert.equal(await evaluate('document.querySelector(".collapsed-content").firstElementChild.classList.contains("column-tab")'),true);
    assert.ok(layout.dividers.find(d=>d.id===40).fixed);
    assert.equal(await evaluate('document.querySelectorAll(".divider.horizontal").length'),1,'Closed stack has no resize handle');
    await evaluate(`window.__stackTile=document.querySelector(${JSON.stringify(tile('brushes'))})`);
    await click(tile('brushes'));layout=await resolved();
    const opened=layout.collapsed[0].open;
    assert.ok(opened);assert.equal(layout.groups.filter(g=>[43,44].includes(g.id)).length,2,'Uses ordinary groups');
    assert.equal(opened.bounds.y,layout.collapsed[0].bounds.y);
    assert.equal(opened.bounds.y+opened.bounds.height,layout.collapsed[1].bounds.y+layout.collapsed[1].bounds.height);
    assert.equal(await evaluate('window.__stackTile===document.querySelector(".column-tab[data-panel=brushes]")'),true,'Open retains sidebar tile');
    assert.equal(await evaluate('document.querySelectorAll(".column-tab[aria-selected=true]").length'),2);
    assert.equal(await evaluate('document.querySelectorAll(".column-connection").length'),2);
    const beforeResize=await snapshot(),edge=center(layout.dividers.find(d=>d.id===40).bounds);
    await input('down',edge);await input('move',{x:edge.x+80,y:edge.y});await wait(100);
    const wider=(await resolved()).collapsed[0].open;
    assert.ok(wider.bounds.width>opened.bounds.width+60,'Open member width resizes');
    assert.equal(await evaluate('window.__stackTile===document.querySelector(".column-tab[data-panel=brushes]")'),true);
    const connection=wider.connections[0][1].bounds,domConnection=await rect('.column-connection');
    assert.ok(Math.abs(domConnection.x-connection.x)<1&&Math.abs(domConnection.width-connection.width)<1,'Connector follows resize before release');
    await input('up');await wait(180);
    await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),beforeResize);
    await click(tile('brushes'));
    await click(tile('sizes'));assert.equal((await resolved()).groups.find(g=>g.id===43).active,'sizes');
    await click(tile('color'));layout=await resolved();
    assert.equal(layout.collapsed.filter(c=>c.open).length,1);assert.ok(layout.collapsed.find(c=>c.id===45).open);
    await escape();assert.ok((await resolved()).collapsed.every(c=>!c.open));
    await edit({type:'set_column_drawers',column:42,drawers:true});await click(tile('brushes'));
    assert.equal(await evaluate('layerApp.state().customization.column_drawers.length'),1);
    assert.ok((await resolved()).collapsed.every(c=>!c.open));await escape();
    await edit({type:'set_column_drawers',column:42,drawers:false});await edit({type:'set_column_auto_hide',column:42,auto_hide:true});await click(tile('brushes'));
    await input('down',center((await resolved()).work_area));await input('up');await wait(180);
    assert.ok((await resolved()).collapsed.every(c=>!c.open),'Auto-hide closes on outside contact');

    // Each real source and pointer type reaches both sides of the footer boundary.
    const sources=[
      ['panel','.dock-group[data-group="48"] .dock-tab[data-panel="layers"]',['layers'],false],
      ['group','.dock-group[data-group="48"] .panel-grip',['layers','properties','adjustments'],false],
      ['toolbar','.dock-group[data-group="49"] .panel-grip',['toolbar'],false],
      ['tile',tile('color'),['color'],true],
    ];
    for(device of ['mouse','touch','pen'])for(const[name,selector,panels,hold]of sources)for(const mode of ['append','member','cancel']) {
      await reset();const before=await snapshot();
      const column=(await resolved()).collapsed.find(c=>c.id===42),last=column.groups.at(-1);
      const destination=mode==='append'?{x:column.bounds.x+18,y:last.bounds.y+last.bounds.height+3}:center(column.grip);
      await input('down',center(await rect(selector)));if(hold)await wait(650);
      await input('move',{x:720,y:550});await input('move',destination);await wait(100);
      assert.equal(await evaluate('document.querySelector(".drop-indicator").hidden'),false,`${device} ${name} ${mode}: drop preview`);
      if(mode==='append')assert.ok(Math.abs(center(await rect('.drop-indicator')).y-last.bounds.y-last.bounds.height)<1,'Append preview at last tile boundary');
      if(mode==='cancel')await escape();
      await input('up');await wait(220);
      if(mode==='cancel'){assert.deepEqual(await snapshot(),before);continue;}
      layout=await resolved();const after=await snapshot();
      assert.equal(after.layout.floating.length,0);
      const member=layout.collapsed.find(c=>c.groups.some(g=>g.icons.some(i=>i.panel===panels[0])));
      const moved=member.groups.find(g=>g.icons.some(i=>i.panel===panels[0]));
      assert.deepEqual(moved.icons.map(i=>i.panel),panels,`${device} ${name} ${mode}: preserves dropped content`);
      const stack=after.layout.column_stacks.find(s=>s.members.includes(member.id));
      assert.equal(stack.members.length,(hold?1:2)+(mode==='member'?1:0),`${device} ${name} ${mode}: member count`);
      assert.equal(member.groups.length,mode==='append'?3:1);
      assert.equal(stack.drawers,false);assert.equal(stack.auto_hide,false);
      await click(tile(panels[0]));assert.ok((await resolved()).collapsed.find(c=>c.id===member.id).open);
      if(device==='mouse'&&name==='group'&&mode==='append') {
        const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/column-stacks/web';await mkdir(dir,{recursive:true});
        const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/opened-stack.png`,Buffer.from(shot.data,'base64'));
      }
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),before,'One undo restores the complete move');
      await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),after);
    }
    console.log('PASS: Web column stacks, ordinary full-height opening, active connections, preferences, fixed width, mouse/touch/pen panel/group/toolbar/tile drops, footer append, cancellation and undo/redo');
  } finally {if(down)await input('up');await send({type:'restore_workspace',workspace:saved});}
}
