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
    await call('Page.bringToFront');
    for(const theme of ['light','dark']) for(device of ['mouse','pen','touch']) for(const right of [false,true]) {
      const workspace=structuredClone(saved),minimum=right?226:242;
      Object.assign(workspace.layout,{bands:[{id:40,edge:right?'right':'left',extent:320,root:tabs(41,right?['layers']:['brushes','tool_settings'])}],floating:[],collapsed:[],column_stacks:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],next_id:Math.max(42,workspace.layout.next_id)});
      workspace.zen_mode=false;
      await send({type:'set_theme',theme});await send({type:'restore_workspace',workspace});
      const before=await snapshot(),layout=await resolved(),divider=layout.dividers.find(d=>d.id===40),start=center(divider.bounds);
      const origin=right?divider.parent.x+divider.parent.width-3:divider.parent.x+3;
      const at=width=>({x:origin+(right?-width:width),y:start.y});
      await input('down',start);
      for(const width of [minimum,minimum-72,minimum-107]) {
        await input('move',at(width));await wait(100);
        assert.equal((await snapshot()).layout.collapsed.length,0,`${theme}/${device}: ${minimum-width}px overshoot stays expanded`);
      }
      await input('move',at(minimum-109));await wait(100);
      assert.equal((await snapshot()).layout.collapsed[0].root,41);
      await input('move',at(minimum-107));await wait(100);
      assert.equal((await snapshot()).layout.collapsed.length,0,'Reversing crosses the same collapse boundary');
      await input('move',at(minimum-109));await input('up');await wait(100);
      const collapsed=await snapshot();
      assert.equal(collapsed.layout.collapsed[0].expanded_width,layout.groups.find(g=>g.id===41).bounds.width);
      await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await snapshot(),before);
      await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await snapshot(),collapsed);
    }
    let layout;
    for(const theme of ['light','dark']) for(device of ['mouse','touch','pen']) {
    await send({type:'set_theme',theme});await reset();
    layout=await resolved();
    assert.equal(layout.collapsed.length,2);
    assert.equal(layout.collapsed[1].bounds.y-layout.collapsed[0].bounds.y-layout.collapsed[0].bounds.height,6);
    assert.equal(await evaluate('document.querySelector(".collapsed-content").firstElementChild.classList.contains("column-tab")'),true);
    const stackEdge=layout.dividers.find(d=>d.id===40),member=layout.collapsed[1];
    assert.ok(!stackEdge.fixed,'A closed stack edge opens a member');
    const closedStack=await snapshot(),from={x:stackEdge.bounds.x+stackEdge.bounds.width/2,y:member.bounds.y+20};
    await input('down',from);await input('move',{x:from.x+90,y:from.y});await wait(100);await input('up');await wait(180);
    assert.ok((await resolved()).collapsed.find(c=>c.id===member.id).open,'Dragging a closed stack edge opens the member beside the contact');
    assert.deepEqual(await snapshot(),closedStack,'Opening a member is transient');
    await escape();layout=await resolved();
    const fittedCorner=await evaluate(`(() => {const icon=document.querySelector(${JSON.stringify(tile('brushes'))});icon.style.setProperty('--corner-fit','.54');const value=parseFloat(getComputedStyle(icon).borderTopLeftRadius);icon.style.removeProperty('--corner-fit');return value;})()`);
    const authoredCorner=layout.collapsed[0].groups[0].icons[0].corner_radii[0];
    assert.ok(Math.abs(fittedCorner-authoredCorner*.54)<.001,'Column icon corners preserve the native fallback fit');
    await evaluate(`window.__stackTile=document.querySelector(${JSON.stringify(tile('brushes'))})`);
    await click(tile('brushes'));layout=await resolved();
    const opened=layout.collapsed[0].open;
    assert.ok(opened);assert.equal(layout.groups.filter(g=>[43,44].includes(g.id)).length,2,'Uses ordinary groups');
    assert.equal(opened.bounds.y,layout.collapsed[0].bounds.y);
    assert.equal(opened.bounds.y+opened.bounds.height,layout.collapsed[1].bounds.y+layout.collapsed[1].bounds.height);
    assert.equal(await evaluate('window.__stackTile===document.querySelector(".column-tab[data-panel=brushes]")'),true,'Open retains sidebar tile');
    assert.equal(await evaluate('document.querySelectorAll(".column-tab[aria-selected=true]").length'),2);
    const surface=async()=>{
      for(let attempt=0;attempt<100;attempt++) {
        const ready=await evaluate(`(() => {
          const node=document.querySelector('.column-surface');
          return node?.querySelector('defs > path')?.getAttribute('d')?.includes('C')&&document.querySelectorAll('.vector-panel').length===2;
        })()`);
        if(ready)return;
        await wait(20);await settle();
      }
      assert.fail('Prepared combined SVG reaches both ordinary receivers');
    };
    await surface();
    assert.equal(await evaluate('document.querySelectorAll(".column-surface").length'),1,'One combined SVG owns the opened column');
    assert.equal(await evaluate('document.querySelectorAll(".column-connection").length'),0,'Ordinary receivers use the combined surface');
    const originalOutline=await evaluate('document.querySelector(".column-surface defs>path").getAttribute("d")');
    await edit({type:'show_all_controls',panel:'brushes'});
    assert.equal(await evaluate('document.querySelectorAll(".expanded-panel").length'),1);
    assert.equal(await evaluate('document.querySelectorAll(".column-surface").length'),0,'Configuration retires the ordinary combined surface');
    await evaluate('layerApp.dispatch({type:"customize",action:{type:"close_expanded"}})');
    await wait(500);
    const restored=await evaluate(`(() => {
      const surface=document.querySelector('.column-surface');
      return {expanded:document.querySelectorAll('.expanded-panel').length,count:document.querySelectorAll('.column-surface').length,
        outline:surface?.querySelector('defs>path')?.getAttribute('d'),receivers:[...document.querySelectorAll('.vector-panel')].map(n=>({id:Number(n.dataset.group),bounds:n.getBoundingClientRect().toJSON(),clip:n.querySelector('.panel-preview').style.clipPath}))};
    })()`);
    assert.equal(restored.expanded,0,'Configuration close animation finished');
    assert.equal(restored.count,0,'Configured receivers leave the source column');
    assert.equal(restored.receivers.length,0,'Configuration close leaves no vector receiver ghosts');
    const restoredLayout=await resolved();
    assert.ok(restoredLayout.collapsed.every(c=>!c.open),'Configuration reveals an ordinary expanded column');
    const ordinary=await rect('.dock-group[data-group="43"]'),ordinaryBounds=restoredLayout.groups.find(g=>g.id===43).bounds;
    for(const key of ['x','y','width','height'])assert.ok(Math.abs(ordinary[key]-ordinaryBounds[key])<1,`Ordinary receiver ${key} matches shared bounds after configuration`);
    assert.equal(await evaluate(`document.querySelector('.dock-group[data-group="43"] .panel-preview').style.clipPath`),'','Configured receiver releases its vector clip');
    await reset();await click(tile('brushes'));await surface();layout=await resolved();
    assert.equal(await evaluate('document.querySelector(".column-surface defs>path").getAttribute("d")'),originalOutline,'Reopening restores the matching ordinary contour');
    await edit({type:'show_all_controls',panel:'brushes'});
    const immediate=await evaluate(`(()=>{layerApp.dispatch({type:'customize',action:{type:'close_expanded'}});layerApp.dispatch({type:'restore_workspace',workspace:${JSON.stringify(fixture)}});layerApp.dispatch({type:'customize',action:{type:'toggle_column_drawer',group:43,panel:'brushes'}});return document.querySelectorAll('.expanded-panel').length;})()`);
    await wait(500);
    const rapidLayout=await resolved(),rapid=await evaluate(`(()=>({expanded:document.querySelectorAll('.expanded-panel').length,surfaces:document.querySelectorAll('.column-surface').length,receivers:[...document.querySelectorAll('.vector-panel')].map(n=>({id:Number(n.dataset.group),bounds:n.getBoundingClientRect().toJSON(),clip:n.querySelector('.panel-preview').style.clipPath}))}))()`);
    const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/column-stacks/web';await mkdir(dir,{recursive:true});
    await writeFile(`${dir}/rapid-configuration-${theme}-${device}.json`,JSON.stringify({immediate,rapid,layout:rapidLayout},null,2));
    assert.equal(rapid.expanded,0,'Rapid restore finishes the configuration close animation');
    assert.equal(rapid.surfaces,1,'Rapid reopen restores the combined surface without further input');
    assert.equal(rapid.receivers.length,2);
    for(const receiver of rapid.receivers) {
      const b=rapidLayout.groups.find(g=>g.id===receiver.id).bounds;
      assert.ok(receiver.clip.startsWith('url('),'Rapid reopen reattaches receiver clips');
      for(const key of ['x','y','width','height'])assert.ok(Math.abs(receiver.bounds[key]-b[key])<1,`Rapid restored receiver ${receiver.id} ${key} matches the latest shared bounds`);
    }
    layout=rapidLayout;
    await evaluate(`window.__stackTile=document.querySelector(${JSON.stringify(tile('brushes'))})`);
    const beforeResize=await snapshot(),edge=center(layout.dividers.find(d=>d.id===40).bounds);
    await input('down',edge);await input('move',{x:edge.x+80,y:edge.y});await wait(100);
    const wider=(await resolved()).collapsed[0].open;
    if(wider.bounds.width<=opened.bounds.width+60) {
      const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/column-stacks/web';await mkdir(dir,{recursive:true});
      await writeFile(`${dir}/resize-failure.json`,JSON.stringify({theme,device,opened,wider,edge,layout:await resolved(),viewport:await evaluate('({width:innerWidth,height:innerHeight,dpr:devicePixelRatio})')},null,2));
    }
    assert.ok(wider.bounds.width>opened.bounds.width+60,'Open member width resizes');
    assert.equal(await evaluate('window.__stackTile===document.querySelector(".column-tab[data-panel=brushes]")'),true);
    await surface();
    const domSurface=await rect('.column-surface');
    assert.ok(Math.abs(domSurface.x-layout.collapsed[0].bounds.x)<1,'Combined surface begins at the source column');
    assert.ok(Math.abs(domSurface.x+domSurface.width-wider.bounds.x-wider.bounds.width)<1,'Combined surface follows resize before release');
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

    assert.equal(await evaluate('document.querySelectorAll(".column-surface,.vector-panel,.vector-source").length'),0,'Closing retires combined geometry and source clips');
    }

    // Each real source and pointer type reaches both sides of the footer boundary.
    const sources=[
      ['panel','.dock-group[data-group="48"] .dock-tab[data-panel="layers"]',['layers'],false],
      ['group','.dock-group[data-group="48"] .panel-grip',['layers','properties','adjustments'],false],
      ['toolbar','.dock-group[data-group="49"] .panel-grip',['toolbar'],false],
      ['tile',tile('color'),['color'],true],
    ];
    for(device of ['mouse','touch','pen'])for(const[name,selector,panels,hold]of sources)for(const mode of ['prepend','append','member','cancel']) {
      await reset();const before=await snapshot();
      const column=(await resolved()).collapsed.find(c=>c.id===42),first=column.groups[0],last=column.groups.at(-1),splitDrop=['prepend','append'].includes(mode);
      assert.equal(first.bounds.y,column.bounds.y,'First group begins at the column top without padding');
      const destination=mode==='prepend'?{x:column.bounds.x+18,y:first.bounds.y-3}:mode==='append'?{x:column.bounds.x+18,y:last.bounds.y+last.bounds.height+3}:center(column.grip);
      await input('down',center(await rect(selector)));if(hold)await wait(650);
      await input('move',{x:720,y:550});await input('move',destination);await wait(100);
      assert.equal(await evaluate('document.querySelector(".drop-indicator").hidden'),false,`${device} ${name} ${mode}: drop preview`);
      if(mode==='prepend') {
        const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/column-stacks/web';await mkdir(dir,{recursive:true});
        await writeFile(`${dir}/prepend-${device}-${name}.json`,JSON.stringify({first,column,destination,hint:await rect('.drop-indicator')},null,2));
      }
      if(mode==='prepend')assert.ok(Math.abs((await rect('.drop-indicator')).y-first.bounds.y)<1,'First drop preview begins at the first tile boundary');
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
      assert.equal(member.groups.length,splitDrop?3:1);
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
