import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkToolVariations({call,evaluate,settle}) {
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await pause(180);};
  const wait=async expression=>{for(let i=0;i<200;i++){if(await evaluate(expression))return;await pause(100);}throw Error(expression);};
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error(${JSON.stringify(selector)});const r=n.getBoundingClientRect();if(!r.width||!r.height)throw Error('Hidden '+${JSON.stringify(selector)});const p={x:r.x+r.width/2,y:r.y+r.height/2},hit=document.elementFromPoint(p.x,p.y);if(!n.contains(hit))throw Error('Covered '+${JSON.stringify(selector)}+' by '+hit?.outerHTML);return p})()`);
  let device='mouse',point;
  const pointer=async(type,p=point)=>{
    point=p;
    if(device==='touch')await call('Input.dispatchTouchEvent',{type:{down:'touchStart',move:'touchMove',up:'touchEnd'}[type],touchPoints:type==='up'?[]:[{id:1,...point}]});
    else await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...point,button:'left',buttons:type==='up'?0:1,clickCount:1,pointerType:device});
  };
  const clickPoint=async p=>{await pointer('down',p);await pointer('up');await settle();await pause(180);};
  const click=async selector=>clickPoint(await rect(selector));
  const key=async(key,modifiers=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code:key,windowsVirtualKeyCode:{F10:121,ArrowDown:40,Enter:13,Escape:27}[key],modifiers,...(type==='keyDown'&&key==='Enter'?{text:'\r'}:{})});await settle();};
  const resize=async width=>{await call('Emulation.setDeviceMetricsOverride',{width,height:1000,deviceScaleFactor:1,mobile:false});await settle();await pause(180);};
  const settleDrawer=()=>wait(`(()=>{const root=document.querySelector('.content-drawer[data-drawer="tool"]');if(!root||root.inert)return false;const viewport=document.querySelector('#workspace'),heights=[...root.querySelectorAll(':scope > .drawer-column')].map(column=>column.scrollHeight),target=layerApp.app.drawer({viewport:[viewport.clientWidth,viewport.clientHeight],column:null,heights,progress:1,from:null,closing:false})?.placement.bounds;if(!target)return false;const actual=root.getBoundingClientRect(),offset=viewport.getBoundingClientRect();return Math.abs(actual.x-offset.x-target.x)<.5&&Math.abs(actual.y-offset.y-target.y)<.5&&Math.abs(actual.width-target.width)<.5&&Math.abs(actual.height-target.height)<.5})()`);
  const menuOpen=()=>evaluate("!!document.querySelector('.panel-context-menu:popover-open')");
  const closeMenu=()=>evaluate("document.querySelector('.panel-context-menu').hidePopover()");
  const model=target=>evaluate(`layerApp.app.context_menu(${JSON.stringify(target)})`);
  const choose=async label=>{
    await evaluate(`(()=>{document.querySelector('[data-variation-choice]')?.removeAttribute('data-variation-choice');const b=[...document.querySelectorAll('.panel-context-menu button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)});if(!b)throw Error(${JSON.stringify(label)});b.dataset.variationChoice='true';})()`);
    await click('[data-variation-choice]');
  };
  const groupView=anchor=>anchor.kind==='header'?`layerApp.app.header_view().items.find(i=>i.id===${anchor.id})`:`layerApp.app.panel_view('${anchor.panel}').tiles.find(t=>t.id===${anchor.tile})`;
  const groupBody=(anchor,selector)=>selector.includes('data-header-overflow-item')?selector:anchor.kind==='header'?`${selector} .header-tool`:`${selector} > button:first-child`;
  const markerPoint=async(anchor,selector)=>{
    const geometry=await evaluate(`(()=>{const hit=document.querySelector(${JSON.stringify(selector+' .tool-variations')}),owner=document.querySelector(${JSON.stringify(groupBody(anchor,selector))}),shape=hit.querySelector('svg path'),box=shape.getBBox(),matrix=shape.getScreenCTM(),stroke=getComputedStyle(shape).stroke==='none'?0:parseFloat(getComputedStyle(shape).strokeWidth)/2,transform=(x,y)=>new DOMPoint(x,y).matrixTransform(matrix),corners=[[box.x-stroke,box.y-stroke],[box.x+box.width+stroke,box.y-stroke],[box.x-stroke,box.y+box.height+stroke],[box.x+box.width+stroke,box.y+box.height+stroke]].map(([x,y])=>transform(x,y)),ink={left:Math.min(...corners.map(p=>p.x)),right:Math.max(...corners.map(p=>p.x)),top:Math.min(...corners.map(p=>p.y)),bottom:Math.max(...corners.map(p=>p.y))},bounds=owner.getBoundingClientRect(),target=owner.getBoundingClientRect(),style=getComputedStyle(owner),radii=style.borderBottomRightRadius.split(' '),radius=(value,length)=>Math.min(length/2,parseFloat(value)*(value.endsWith('%')?length/100:1)),rx=radius(radii[0],bounds.width),ry=radius(radii[1]||radii[0],bounds.height),corner=style.getPropertyValue('corner-bottom-right-shape')||style.getPropertyValue('corner-shape'),power=corner==='squircle'||corner==='superellipse(2)'?4:2,insideCorner=corners.every(p=>p.x<=bounds.right-rx||p.y<=bounds.bottom-ry||((p.x-(bounds.right-rx))/rx)**power+((p.y-(bounds.bottom-ry))/ry)**power<=1);let point;for(let y=1;y<8&&!point;y++)for(let x=1;x<8&&!point;x++){const p=new DOMPoint(box.x+box.width*x/8,box.y+box.height*y/8);if(shape.isPointInFill(p))point=transform(p.x,p.y);}if(!point)throw Error('Marker has no painted interior');return {clearance:[ink.left-bounds.left,bounds.right-ink.right,ink.top-bounds.top,bounds.bottom-ink.bottom],insideTarget:ink.left>=target.left&&ink.right<=target.right&&ink.top>=target.top&&ink.bottom<=target.bottom,insideCorner,paintedHit:owner.contains(document.elementFromPoint(point.x,point.y)),labelClear:!owner.matches('[data-header-overflow-item]')||owner.querySelector('.header-overflow-label').getBoundingClientRect().right<=ink.left,decorative:hit.tagName==='SPAN'&&hit.getAttribute('aria-hidden')==='true'&&!hit.hasAttribute('tabindex')&&!hit.hasAttribute('aria-haspopup')&&!hit.hasAttribute('data-context'),point:{x:point.x,y:point.y}}})()`);
    assert.ok(geometry.clearance.every(gap=>gap>=6-.01),`marker ink including stroke clears owning button by 6px: ${JSON.stringify(geometry)}`);
    assert.ok(geometry.insideTarget,'painted marker stays inside its native hit target');assert.ok(geometry.insideCorner,`painted marker stays inside rendered button corner: ${JSON.stringify(geometry)}`);assert.ok(geometry.paintedHit,'painted marker input belongs to its tool button');assert.ok(geometry.decorative,'marker has no independent focus, accessibility or context target');assert.ok(geometry.labelClear,'overflow label leaves room for marker ink');
    return geometry.point;
  };
  const markerActivation=async(anchor,selector)=>{
    const view=groupView(anchor);await send({type:'customize',action:{type:'close_expanded'}});await send({type:'invoke',command:'hand'});await wait(`!${view}.selected`);
    await clickPoint(await markerPoint(anchor,selector));await wait(`${view}.selected`);
    assert.equal(await menuOpen(),false,`${device}: inactive marker click selects without a context menu`);assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,'inactive marker click does not open drawer');
    await clickPoint(await markerPoint(anchor,selector));assert.equal(await menuOpen(),false,'active marker click opens full drawer without context menu');assert.deepEqual(await evaluate('layerApp.state().customization.drawer.anchor'),anchor);
    await clickPoint(await markerPoint(anchor,selector));assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,'active marker click closes full drawer');
  };
  const openChoices=async(anchor,selector)=>{
    const p=await markerPoint(anchor,selector),drawer=await evaluate('layerApp.state().customization.drawer?.anchor??null');
    if(device==='mouse')for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});
    else {await pointer('down',p);await pause(620);assert.ok(await menuOpen(),`${device}: marker hold opens context menu`);await pointer('up');}
    await settle();assert.ok(await menuOpen(),`${device}: context menu from tool button`);assert.deepEqual(await evaluate('layerApp.state().customization.drawer?.anchor??null'),drawer,'context gesture does not activate tool');
    const rows=(await model(anchor)).sections.flat();assert.deepEqual(await evaluate("[...document.querySelectorAll('.panel-context-menu .menu-label')].map(n=>n.textContent)"),rows.map(row=>row.label));
  };
  const checkCommandGroup=async(anchor,selector,icons=[])=>{
    const view=groupView(anchor),body=groupBody(anchor,selector),rows=(await model({kind:'tool_variants',anchor})).sections.flat();
    assert.ok(rows.length,'existing command group has choices');assert.equal(await evaluate(`${view}.has_variants`),true);
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector+' .tool-variations')}).hidden`),false,'command group exposes its decorative corner');
    await markerActivation(anchor,selector);
    await evaluate(`window.retainedCommandGroup=document.querySelector(${JSON.stringify(selector)})`);
    for(const icon of icons.length?icons:[rows.find(r=>!r.selected)?.icon??rows[0].icon]) {
      const choice=rows.find(row=>row.icon===icon);assert.ok(choice,`group exposes ${icon}`);
      await openChoices(anchor,selector);
      await choose(choice.label);await wait(`${view}.selected`);
      assert.equal(await evaluate(`${view}.icon`),icon);
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(body+' > svg')}).dataset.asset`),icon,'native group icon follows chosen medium');
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)})===retainedCommandGroup`),true,'existing group retains its native control');
    }
    await send({type:'customize',action:{type:'close_expanded'}});await click(body);
    assert.deepEqual(await evaluate('layerApp.state().customization.drawer.anchor'),anchor,'active group opens its original drawer');
    return {rows,body,view};
  };
  const directory=process.env.LAYER_TEST_ARTIFACTS||'artifacts/ui/tool-variations';await mkdir(directory,{recursive:true});
  const shot=async name=>{const capture=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}.png`,Buffer.from(capture.data,'base64'));};
  const original=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  await call('Emulation.setDeviceMetricsOverride',{width:1400,height:1000,deviceScaleFactor:1,mobile:false});await settle();
  try {
    for(const theme of ['light','dark'])for(const [workspace,count,slot] of [['photographer',15,'lasso'],['illustrator',17,'manual_selection']]) {
      console.log(`Tool variations: ${workspace}/${theme}`);
      device='mouse';await click(`.workspace-switcher button[data-workspace-id="builtin:workspace:${workspace}"]`);
      await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');await send({type:'set_theme',theme});
      const fixture=await evaluate('layerApp.state().workspace');
      const tiles=fixture.layout.panels.find(p=>p.id==='toolbar').content.tiles;
      assert.equal(tiles.filter(t=>!['divider','color'].includes(t.control.kind)).length,count,`${theme}/${workspace}: compact tool count`);
      const tile=tiles.find(t=>t.control.slot===slot);assert.ok(tile,slot);
      const selector=`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tile.id}"]`,anchor={kind:'tile',panel:'toolbar',tile:tile.id};
      const variants=await model({kind:'tool_variants',anchor}),rows=variants.sections.flat();assert.ok(rows.length>1);
      await markerPoint(anchor,selector);
      assert.ok(rows.every(r=>r.action&&r.icon));
      const target={kind:'tile',panel:'toolbar',tile:tile.id},full=await model(target);
      assert.deepEqual(full.sections[0].map(r=>r.label),rows.map(r=>r.label));assert.ok(full.sections.length>1,'secondary menu keeps customization');
      for(device of ['mouse','touch','pen']) {
        await evaluate(`window.retainedVariationTile=document.querySelector(${JSON.stringify(selector)})`);
        await markerActivation(anchor,selector);await openChoices(anchor,selector);
        const alternate=(await model({kind:'tool_variants',anchor})).sections.flat().find(r=>!r.selected);await choose(alternate.label);
        assert.equal(await menuOpen(),false);
        const view=await evaluate(`layerApp.app.panel_view('toolbar').tiles.find(t=>t.id===${tile.id})`);
        assert.equal(view.label,alternate.label);assert.equal(view.icon,alternate.icon);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)})===retainedVariationTile`),true,'variant retains pressed tile');
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector+' > button:first-child > svg')}).dataset.asset`),alternate.icon);
        await send({type:'customize',action:{type:'close_expanded'}});
        await click(`${selector} > button:first-child`);
        assert.ok(await evaluate('layerApp.state().customization.drawer!=null'),`${device}: active click opens drawer`);
        const drawer=await evaluate('layerApp.state().customization.drawer');
        assert.deepEqual(drawer.anchor,anchor);assert.deepEqual(drawer.columns,[['brushes'],['tool_settings']]);
        assert.deepEqual(await evaluate("[...document.querySelectorAll('.content-drawer .brushes-control .tool-groups .tool-choice-name')].map(n=>n.textContent)"),rows.map(r=>r.label));
        await evaluate("window.retainedSlotDrawer=document.querySelector('.content-drawer .brushes-control')");
        for(let choice=0;choice<3;choice++) {
          await settleDrawer();
          const sibling=await evaluate('layerApp.state().customization.drawer.tool_set.groups.find(r=>!r.selected&&r.enabled)');
          await click(`.content-drawer .brushes-control .tool-groups [data-tool-choice="${sibling.label}"]`);
          assert.deepEqual(await evaluate('layerApp.state().customization.drawer.anchor'),anchor,'drawer sibling keeps its origin');
          assert.equal(await evaluate("document.querySelector('.content-drawer .brushes-control')===retainedSlotDrawer"),true,'drawer siblings retain native controls');
          assert.equal(await evaluate(`layerApp.app.panel_view('toolbar').tiles.find(t=>t.id===${tile.id}).icon`),sibling.icon,`${device}: full drawer choice updates tile`);
        }
        await click(`${selector} > button:first-child`);assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,`${device}: active click closes drawer`);
        await pointer('down',await rect(`${selector} > button:first-child`));await pause(620);
        assert.equal(await menuOpen(),device!=='mouse',`${device}: hold menu ownership`);
        await pointer('up');await settle();assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true,'hold release suppresses activation');await closeMenu();
        const before=await evaluate('layerApp.state().workspace');
        const start=await rect(`${selector} > button:first-child`),destination=await rect(`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tiles.at(-1).id}"]`);
        await pointer('down',start);await pointer('move',destination);await pause(620);await pointer('up');await settle();
        assert.deepEqual(await evaluate('layerApp.state().workspace'),before,`${device}: motion before hold does not reorder`);
        await pointer('down',start);await pause(620);await pointer('move',destination);await settle();assert.equal(await menuOpen(),false,'drag dismisses menu');await pointer('up');await settle();
        const after=await evaluate('layerApp.state().workspace');assert.notDeepEqual(after,before,`${device}: held same contact reorders`);
        await send({type:'invoke',command:'undo_workspace'});assert.deepEqual(await evaluate('layerApp.state().workspace'),before,'one layout undo');
        await send({type:'invoke',command:'redo_workspace'});assert.deepEqual(await evaluate('layerApp.state().workspace'),after,'one layout redo');
        await send({type:'invoke',command:'undo_workspace'});
      }
      device='mouse';const p=await rect(`${selector} > button:first-child`);
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'right',buttons:type==='mousePressed'?2:0,clickCount:1});await settle();
      assert.ok(await menuOpen());assert.ok(await evaluate("document.querySelectorAll('.panel-context-menu .menu-label').length")>rows.length);await closeMenu();
      await evaluate(`document.querySelector(${JSON.stringify(groupBody(anchor,selector))}).focus()`);
      await key('F10',8);assert.ok(await menuOpen(),'keyboard context action keeps tool choices available');
      assert.equal(await evaluate("!!document.activeElement.closest('.panel-context-menu')"),true,'keyboard menu owns focus');
      await key('ArrowDown');const choice=await evaluate('document.activeElement.menuItem');assert.ok(rows.some(row=>row.label===choice.label),'arrow key reaches a tool variant');
      await key('Enter');assert.equal(await menuOpen(),false,'Enter activates focused variant');assert.equal(await evaluate(`${groupView(anchor)}.icon`),choice.icon);
      await evaluate(`document.querySelector(${JSON.stringify(groupBody(anchor,selector))}).focus()`);await key('F10',8);await key('Escape');assert.equal(await menuOpen(),false);assert.equal(await evaluate(`document.activeElement===document.querySelector(${JSON.stringify(groupBody(anchor,selector))})`),true,'Escape returns focus to tool button');
      await shot(`${workspace}-${theme}`);
      await send({type:'restore_workspace',workspace:fixture});
    }
    for(const theme of ['light','dark']) {
      device='mouse';await click('.workspace-switcher button[data-workspace-id="builtin:workspace:illustrator"]');
      await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');await send({type:'set_theme',theme});
      const paint=await evaluate('layerApp.state().workspace'),tiles=paint.layout.panels.find(p=>p.id==='toolbar').content.tiles;
      for(const [index,[command,icons]] of [['pen',['marker']],['pencil',['pastel']],['brush',['watercolor','oil-paint']],['airbrush',['spray']],['eraser',[]],['decoration',[]],['liquify',[]]].entries()) {
        device=['mouse','touch','pen'][index%3];
        const tile=tiles.find(t=>t.control.command===command);assert.ok(tile,`Paint has existing ${command} group`);console.log(`Paint group: ${command}/${theme}/${device}`);
        const anchor={kind:'tile',panel:'toolbar',tile:tile.id},selector=`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tile.id}"]`;
        await checkCommandGroup(anchor,selector,icons);
        assert.deepEqual(await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='toolbar').content.tiles.find(t=>t.id===${tile.id}).control`),tile.control,'medium choices preserve existing command configuration');
        await send({type:'customize',action:{type:'close_expanded'}});
      }
      await shot(`paint-media-${theme}`);
      const selection={};
      for(const slot of ['manual_selection','automatic_selection']) {
        device=slot==='manual_selection'?'touch':'pen';
        const tile=tiles.find(t=>t.control.slot===slot);assert.ok(tile);
        const anchor={kind:'tile',panel:'toolbar',tile:tile.id},selector=`.toolbar-controls[data-panel="toolbar"] > [data-tile="${tile.id}"]`,view=groupView(anchor);
        const rows=(await model({kind:'tool_variants',anchor})).sections.flat();
        assert.ok(rows.some(row=>row.icon===(slot==='manual_selection'?'lasso':'auto-select')));
        await openChoices(anchor,selector);await choose(rows[0].label);await wait(`${view}.selected`);
        const dock='.dock-group .brushes-control .tool-subtools';
        const dockLabels=await evaluate(`[...document.querySelectorAll('${dock} .tool-choice-name')].map(n=>n.textContent)`);
        assert.deepEqual([...dockLabels].sort(),rows.map(row=>row.label).sort(),'docked Tool Set uses the same scoped choices as the menu');
        const alternate=rows.find(row=>row.label!==rows[0].label)||rows[0];
        await click(`${dock} [data-tool-choice="${alternate.label}"]`);assert.equal(await evaluate(`${view}.icon`),alternate.icon,'docked selection choice updates its group icon');
        await send({type:'customize',action:{type:'close_expanded'}});await click(groupBody(anchor,selector));await settleDrawer();
        const drawerLabels=await evaluate("[...document.querySelectorAll('.content-drawer .brushes-control .tool-groups .tool-choice-name')].map(n=>n.textContent)");
        assert.deepEqual(drawerLabels,rows.map(row=>row.label),'full selection drawer keeps the scoped choices');
        selection[slot]={menu:rows.map(row=>row.label),dock:dockLabels,drawer:drawerLabels};
        await shot(`${slot}-${theme}`);await send({type:'customize',action:{type:'close_expanded'}});
      }
      for(const surface of ['menu','dock','drawer'])assert.deepEqual(selection.manual_selection[surface].filter(label=>selection.automatic_selection[surface].includes(label)),[],`${surface}: manual and automatic selection groups do not overlap`);
      await send({type:'restore_workspace',workspace:paint});
      device='mouse';await click('.workspace-switcher button[data-workspace-id="builtin:workspace:painter"]');await wait('JSON.parse(layerApp.app.workspace_view()).ready&&!JSON.parse(layerApp.app.workspace_view()).busy');await send({type:'set_theme',theme});
      const sketch=await evaluate('layerApp.state().workspace');
      for(const [index,[command,icons,columns]] of [['drawing_brush',['marker','pastel','watercolor','oil-paint','spray'],3],['sculpt',['liquify'],3],['select',['polygon-select'],2]].entries()) {
        device=['mouse','pen','touch'][index];
        const entry=sketch.layout.header.zones.flat().find(item=>item.item.control?.command===command);assert.ok(entry);
        const anchor={kind:'header',id:entry.id},selector=`[data-header-item="${entry.id}"]`;
        const group=await checkCommandGroup(anchor,selector,icons);
        if(command==='select')assert.equal(group.rows.length,8,'Sketch keeps its single broad selection group');
        assert.equal(await evaluate('layerApp.state().customization.drawer.columns.length'),columns,'Sketch retains its full class drawer');
        assert.deepEqual(await evaluate(`layerApp.state().workspace.layout.header.zones.flat().find(e=>e.id===${entry.id}).item.control`),entry.item.control);
        await settleDrawer();await shot(`sketch-${command}-${theme}`);await send({type:'customize',action:{type:'close_expanded'}});
      }
      await send({type:'restore_workspace',workspace:sketch});
    }
    device='mouse';await click('.workspace-switcher button[data-workspace-id="builtin:workspace:painter"]');await wait('!JSON.parse(layerApp.app.workspace_view()).busy');
    const saved=await evaluate('layerApp.state().workspace'),fixture=structuredClone(saved);
    const entry=fixture.layout.header.zones.flat().find(e=>e.item.control?.command==='drawing_brush');assert.ok(entry);
    entry.item.control={kind:'tool_slot',slot:'drawing'};
    for(const theme of ['light','dark'])for(const size of ['small','medium','large']) {
      fixture.layout.header.size=size;
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      const anchor={kind:'header',id:entry.id},rows=(await model({kind:'tool_variants',anchor})).sections.flat();
      const selector=`[data-header-item="${entry.id}"]`;
      for(device of ['mouse','touch','pen']){await markerActivation(anchor,selector);await openChoices(anchor,selector);await choose(rows.find(r=>!r.selected).label);}
      const view=await evaluate(`layerApp.app.header_view().items.find(i=>i.id===${entry.id})`);
      assert.equal(await evaluate(`document.querySelector('[data-header-item="${entry.id}"] .header-tool > svg').dataset.asset`),view.icon);
      assert.equal(await evaluate(`document.querySelector('[data-header-item="${entry.id}"] .header-tool').getAttribute('aria-label')`),view.label);
      await markerPoint(anchor,selector);await shot(`header-marker-${size}-${theme}`);
      const zone=fixture.layout.header.zones.findIndex(items=>items.some(item=>item.id===entry.id)),overflow=`#header-overflow-${zone}`,row=`[data-header-overflow-item="${entry.id}"]`,openOverflow=async()=>{if(!await evaluate(`document.querySelector('${overflow}').open`))await click(`${overflow} > summary`);};
      await resize(200);await wait(`document.querySelector('${selector}').hidden`);
      for(device of ['mouse','touch','pen']) {
        await send({type:'invoke',command:'hand'});await wait(`!${groupView(anchor)}.selected`);await openOverflow();if(device==='mouse')await shot(`header-overflow-marker-${size}-${theme}`);await clickPoint(await markerPoint(anchor,row));await wait(`${groupView(anchor)}.selected`);
        assert.equal(await menuOpen(),false,'overflow marker selects through its tool row');assert.equal(await evaluate('layerApp.state().customization.drawer==null'),true);
        await openOverflow();await clickPoint(await markerPoint(anchor,row));assert.deepEqual(await evaluate('layerApp.state().customization.drawer.anchor'),anchor,'active overflow marker opens full drawer');await send({type:'customize',action:{type:'close_expanded'}});
        await openOverflow();await openChoices(anchor,row);await choose(rows.find(r=>!r.selected).label);
      }
      await resize(1400);
    }
    for(const theme of ['light','dark']) {
      fixture.layout.header.size='medium';entry.item.control={kind:'tool_slot',slot:'figure'};await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});await send({type:'invoke',command:'hand'});await send({type:'invoke',command:'quick_mask'});
      const anchor={kind:'header',id:entry.id},selector=`[data-header-item="${entry.id}"]`,view=groupView(anchor),zone=fixture.layout.header.zones.findIndex(items=>items.some(item=>item.id===entry.id)),overflow=`#header-overflow-${zone}`,row=`[data-header-overflow-item="${entry.id}"]`;
      await wait(`!${view}.enabled`);assert.equal(await evaluate(`document.querySelector('${selector} .header-tool').disabled`),true);
      for(device of ['mouse','touch','pen']){await clickPoint(await markerPoint(anchor,selector));assert.equal(await evaluate(`${view}.selected`),false,'disabled marker cannot activate its tool');assert.equal(await menuOpen(),false);await openChoices(anchor,selector);await closeMenu();}
      await resize(200);await wait(`document.querySelector('${selector}').hidden`);await click(`${overflow} > summary`);assert.equal(await evaluate(`document.querySelector('${row}').disabled`),true);
      for(device of ['mouse','touch','pen']){await clickPoint(await markerPoint(anchor,row));assert.equal(await evaluate(`${view}.selected`),false,'disabled overflow marker cannot activate its tool');assert.equal(await menuOpen(),false);await openChoices(anchor,row);await closeMenu();}
      await resize(1400);await send({type:'invoke',command:'return_to_artwork'});
    }
    await send({type:'restore_workspace',workspace:saved});
    console.log('PASS: Photo 15/Paint 17 tools, existing Paint/Sketch command groups, medium icons, disjoint selection menus/Tool Set/drawers, mouse/touch/pen hold/reorder, inset decorative toolbar/header/overflow markers, Small/Medium/Large painted-corner selection/drawers, disabled group rightclick/holds and keyboard menu focus/navigation/activation/Escape in both themes');
  } finally {device='mouse';await closeMenu();await resize(1400);await click(`.workspace-switcher button[data-workspace-id="${original}"]`);}
}
