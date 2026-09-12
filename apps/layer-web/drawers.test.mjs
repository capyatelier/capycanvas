import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

export async function checkToolbarDrawerSwitching({call,evaluate,settle}) {
  const saved=await evaluate('(()=>{const s=layerApp.state();return{workspace:s.workspace,brush:s.brush,commands:s.commands}})()'),fixture=structuredClone(saved.workspace);
  const wait=async()=>{await settle();await evaluate('new Promise(r=>setTimeout(r,300))');};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon_name'});
  Object.assign(fixture.layout,{bands:[
    {id:40,edge:'left',extent:252,root:tabs(41,['sizes','properties'])},
    {id:42,edge:'top',extent:42,root:tabs(43,['toolbar'])},
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(44,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const ids=[fixture.layout.next_tile_id++,fixture.layout.next_tile_id++,fixture.layout.next_tile_id++];
  fixture.layout.panels.find(p=>p.id==='toolbar').content.tiles=ids.map((id,i)=>({id,control:i===2?{kind:'color'}:{kind:'command',command:i===0?'brush':'eraser'}}));
  const selector=i=>`.toolbar-controls[data-panel="toolbar"] [data-tile="${ids[i]}"] > button`;
  const model=()=>evaluate('layerApp.state().customization.drawer');
  const click=async(i,previous)=>{
    await call('Page.bringToFront');
    const p=await evaluate(`(()=>{const b=document.querySelector(${JSON.stringify(selector(i))}).getBoundingClientRect();return{x:b.x+b.width/2,y:b.y+b.height/2}})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0});await settle();
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',buttons:1,clickCount:1});
    try {if(previous!=null)assert.equal((await model())?.anchor.tile,ids[previous],'Press keeps the old drawer until release');}
    finally {await call('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'left',buttons:0,clickCount:1});}
    await wait();
  };
  const check=async(i)=>{
    const drawer=await model();assert.equal(drawer?.anchor.tile,ids[i],'One click moves the drawer to the new tool');
    assert.deepEqual(drawer.columns,i===2?[['color']]:[['brushes'],['tool_settings']]);
    assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector(i))}).dataset.drawerFacing`),'Connector corners follow the new opener');
    if(i!==2) {
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector(i))}).getAttribute('aria-pressed')`),'true','New tool activates immediately');
      assert.equal(await evaluate('layerApp.state().brush.tool'),i===0?'brush':'eraser');
    }
    assert.equal(await evaluate('document.querySelectorAll(\'.content-drawer[data-drawer="tool"] > .drawer-column\').length'),i===2?1:2);
  };
  const toolCommands=['pen','pencil','brush','eraser','airbrush','decoration','blend','liquify','lasso','move','hand','eyedropper','gradient','figure','ruler','auto_select','fill'];
  const originalTool=saved.commands.find(c=>c.selected&&toolCommands.includes(c.id))?.id;
  try {
    await send({type:'restore_workspace',workspace:fixture});
    for(const edge of ['top','bottom','left','right']) {
      await send({type:'move_panel',panel:'toolbar',target:{kind:'edge',edge,outer:true}});
      await send({type:'invoke',command:'pen'});
      await click(0);assert.equal(await evaluate('layerApp.state().brush.tool'),'brush',`${edge}: first click selects Brush`);
      assert.equal(await model(),undefined,'Without an open drawer the first click only selects');
      await click(0);await check(0);
      await click(1,0);await check(1);
      await click(2,1);await check(2);
      await click(0,2);await check(0);
      await click(0);assert.equal(await model(),undefined,'Current opener still closes its drawer');
    }
    await send({type:'move_panel',panel:'toolbar',target:{kind:'tab',group:41,index:null}});
    await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});
    await send({type:'customize',action:{type:'toggle_column_drawer',group:41,panel:'toolbar'}});
    await click(2);await check(2);await click(1,2);await check(1);await click(0,1);await check(0);
    console.log('PASS: first-click tool and drawer switching on all toolbar edges and inside a collapsed drawer');
  } finally {
    await send({type:'restore_workspace',workspace:saved.workspace});
    await send({type:'select_brush',id:saved.brush.preset});
    if(originalTool)await send({type:'invoke',command:originalTool});
  }
}

export async function checkDrawerStyling({call,evaluate,settle}) {
  await call('Page.bringToFront');
  await call('DOM.enable');await call('CSS.enable');
  const saved=await evaluate('({workspace:layerApp.state().workspace,settings:layerApp.state().settings})');
  const dir=process.env.LAYER_TEST_ARTIFACTS||'artifacts/ui/drawer-style/web';
  await mkdir(dir,{recursive:true});
  const wait=async()=>{await settle();await evaluate('new Promise(r=>setTimeout(r,350))');};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const customize=action=>send({type:'customize',action});
  const tabs=(id,panels)=>({kind:'tabs',id,panels,active:panels[0],tab_style:'icon_name'});
  const fixture=structuredClone(saved.workspace);
  Object.assign(fixture.layout,{bands:[
    {id:40,edge:'left',extent:252,root:tabs(41,['brushes','sizes'])},
    {id:42,edge:'right',extent:252,root:tabs(43,['layers','properties'])},
    {id:44,edge:'top',extent:42,root:tabs(45,['toolbar'])},
  ],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const toolbar=fixture.layout.panels.find(p=>p.id==='toolbar');
  const tile=toolbar.content.tiles[0].id;
  const alternateTile=fixture.layout.next_tile_id++;
  toolbar.content.tiles=[{id:tile,control:{kind:'panel',panel:'color'}},{id:alternateTile,control:{kind:'panel',panel:'sizes'}}];
  const tool=`.toolbar-controls[data-panel="toolbar"] [data-tile="${tile}"] > button`;
  const alternateTool=`.toolbar-controls[data-panel="toolbar"] [data-tile="${alternateTile}"] > button`;
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const click=async selector=>{
    await call('Page.bringToFront');
    const b=await rect(selector),p={x:b.x+b.width/2,y:b.y+b.height/2};
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0});await settle();
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...p,button:'left',buttons:1,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...p,button:'left',buttons:0,clickCount:1});await wait();
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:500,y:450,buttons:0});await wait();
  };
  const style=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),s=getComputedStyle(n);return{background:s.backgroundColor,selected:n.getAttribute('aria-selected'),facing:n.dataset.drawerFacing,radii:[s.borderTopLeftRadius,s.borderTopRightRadius,s.borderBottomRightRadius,s.borderBottomLeftRadius]}})()`);
  const sample=async(data,points)=>evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const g=c.getContext('2d',{willReadFrequently:true});g.drawImage(i,0,0);const scale=i.width/innerWidth;return ${JSON.stringify(points)}.map(([x,y])=>[...g.getImageData(Math.floor(x*scale),Math.floor(y*scale),1,1).data]);})()`);
  const check=async(selector,id,name)=>{
    const s=await style(selector),facing={top:[0,1],right:[1,2],bottom:[2,3],left:[0,3]}[s.facing];
    assert.ok(facing,`${name}: source faces drawer`);
    assert.deepEqual(s.radii.map((r,i)=>r=== (facing.includes(i)?'0px':'6px')),[true,true,true,true],`${name}: joined corners`);
    const b=await rect(selector),d=await rect(`.content-drawer[data-drawer="${id}"]`);
    const points=[[b.x+2,b.y+b.height/2],[b.right-2,b.y+b.height/2],[b.x+b.width/2,b.y+2],[b.x+b.width/2,b.bottom-2]];
    if(s.facing==='right')points.push([(b.right+d.x)/2,b.y+b.height/2]);
    if(s.facing==='left')points.push([(b.x+d.right)/2,b.y+b.height/2]);
    if(s.facing==='bottom')points.push([b.x+b.width/2,(b.bottom+d.y)/2]);
    if(s.facing==='top')points.push([b.x+b.width/2,(b.y+d.bottom)/2]);
    const corners=[[b.left+1,b.top+1],[b.right-1,b.top+1],[b.right-1,b.bottom-1],[b.left+1,b.bottom-1]];
    points.push(...facing.map(i=>corners[i]));
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile(`${dir}/${name}.png`,Buffer.from(shot.data,'base64'));
    const pixels=await sample(shot.data,points),fill=pixels[{left:0,right:1,top:2,bottom:3}[s.facing]];
    assert.ok(fill[2]-fill[0]>12&&fill[2]-fill[1]>6,`${name}: the open drawer tile is active blue: ${fill}`);
    for(const color of pixels.slice(-2))assert.ok(color.every((v,i)=>Math.abs(v-fill[i])<=1),`${name}: ancestors preserve square source corners`);
    // Compare actual pixels with shadows disabled; catches alpha blending and stacking contexts.
    await evaluate(`(()=>{const s=document.createElement('style');s.id='drawer-shadow-check';s.textContent='.drawer-shadow,.content-drawer{box-shadow:none!important}';document.head.append(s)})()`);await wait();
    try {
      const flat=await call('Page.captureScreenshot',{format:'png'});
      assert.deepEqual(pixels,await sample(flat.data,points),`${name}: shadow leaves source and connector unchanged`);
    } finally {await evaluate("document.querySelector('#drawer-shadow-check').remove()");}
  };
  try {
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});await send({type:'restore_workspace',workspace:fixture});
      for(const [group,panel] of [[41,'brushes'],[43,'layers']]) {
        await customize({type:'set_column_collapsed',group,collapsed:true});
        const expandSelector=`.collapsed-column[data-column="${group}"] .column-expand`;
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(expandSelector)}).querySelector('svg').dataset.asset`),
          group===41?'chevron-double-right':'chevron-double-left','Both chevrons point toward the canvas');
        const expandBounds=await rect(expandSelector),glyphBounds=await rect(`${expandSelector} svg`);
        assert.ok(Math.abs(glyphBounds.x+glyphBounds.width/2-expandBounds.x-expandBounds.width/2)<.01,'Expand icon is horizontally centered');
        assert.ok(Math.abs(glyphBounds.y+glyphBounds.height/2-expandBounds.y-expandBounds.height/2)<.01,'Expand icon is vertically centered');
        const selector=`.collapsed-column [data-panel="${panel}"]`;
        assert.equal((await style(selector)).background,'rgba(0, 0, 0, 0)','Closed buttons have no selection tint');
        const b=await rect(selector);
        await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:b.x+b.width/2,y:b.y+b.height/2,buttons:0});await wait();
        // Hold the CSS state while checking it; desktop compositor pointer focus can change independently of CDP.
        const {root}=await call('DOM.getDocument'),{nodeId}=await call('DOM.querySelector',{nodeId:root.nodeId,selector});
        await call('CSS.forcePseudoState',{nodeId,forcedPseudoClasses:['hover']});
        const hover=await style(selector);assert.notEqual(hover.background,'rgba(0, 0, 0, 0)','Hover has grey feedback');
        await call('CSS.forcePseudoState',{nodeId,forcedPseudoClasses:[]});
        await click(selector);
        assert.equal((await style(selector)).selected,'true',`${theme} ${panel}: sidebar opens its drawer`);assert.notEqual((await style(selector)).background,hover.background,'Open changes grey to blue');
        await check(selector,group,`${theme}-column-${group}`);
        const alternate=group===41?'sizes':'properties',alternateSelector=`.column-tab[data-panel="${alternate}"]`;
        await click(`.content-drawer[data-drawer="${group}"] .dock-tab[data-panel="${alternate}"]`);
        assert.equal((await style(selector)).selected,'false','Previous sidebar tab is no longer active');
        assert.equal((await style(alternateSelector)).selected,'true','Sidebar selection follows the drawer tab');
        await check(alternateSelector,group,`${theme}-switched-column-${group}`);
        await click(alternateSelector);assert.equal((await style(alternateSelector)).background,'rgba(0, 0, 0, 0)');
        await click(expandSelector);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(expandSelector)})`),null,'Expand button still opens the column');
        await customize({type:'set_column_collapsed',group,collapsed:true});
      }
      for(const edge of ['top','bottom','left','right']) {
        await send({type:'move_panel',panel:'toolbar',target:{kind:'edge',edge,outer:true}});
        await click(tool);await check(tool,'tool',`${theme}-toolbar-${edge}`);
        await click(alternateTool);await check(alternateTool,'tool',`${theme}-toolbar-${edge}-switched`);
        assert.equal((await style(tool)).facing,undefined,'Previous toolbar source returns to rounded corners');
        assert.equal((await style(tool)).background,'rgba(0, 0, 0, 0)','Previous panel opener loses its active blue');
        await click(tool);await check(tool,'tool',`${theme}-toolbar-${edge}-switched-back`);await click(tool);
      }
      await send({type:'move_panel',panel:'toolbar',target:{kind:'tab',group:41,index:null}});
      await click('.collapsed-column [data-panel="toolbar"]');
      // Start with the outer button, leaving the other opener exposed beside its drawer.
      await click(alternateTool);await check(alternateTool,'tool',`${theme}-nested-toolbar`);
      await evaluate(`window.__drawerSwitchFrames=[];window.__drawerQuery=layerApp.app.drawer.bind(layerApp.app);layerApp.app.drawer=q=>{const r=window.__drawerQuery(q);if(q.column==null&&!q.closing&&r)window.__drawerSwitchFrames.push({progress:q.progress,...r});return r}`);
      try {
        await click(tool);
        const frames=await evaluate('window.__drawerSwitchFrames');
        assert.ok(frames.length>0&&frames.every(f=>f.connection),`Switching keeps the connector throughout the transition: ${JSON.stringify(frames.filter(f=>!f.connection))}`);
      } finally {await evaluate('layerApp.app.drawer=window.__drawerQuery;delete window.__drawerQuery;delete window.__drawerSwitchFrames');}
      await check(tool,'tool',`${theme}-nested-toolbar-switched`);
    }
    const divided=structuredClone(fixture),ids=[tile,divided.layout.next_tile_id++,divided.layout.next_tile_id++];
    divided.layout.panels.find(p=>p.id==='toolbar').content.tiles=ids.map((id,i)=>({id,control:i===1?{kind:'divider'}:{kind:'panel',panel:'color'}}));
    divided.layout.bands[0].root={kind:'split',id:46,axis:'vertical',fraction:.5,first:tabs(41,['brushes']),second:tabs(47,['sizes'])};
    divided.layout.next_id=Math.max(48,divided.layout.next_id);
    for(const theme of ['light','dark']) {
      await send({type:'set_theme',theme});await send({type:'restore_workspace',workspace:divided});
      await customize({type:'set_column_collapsed',group:46,collapsed:true});
      const a=await rect('.column-tab[data-panel="brushes"]'),b=await rect('.column-tab[data-panel="sizes"]');
      const expand=await rect('.collapsed-column[data-column="46"] .column-expand');
      assert.ok(Math.abs(a.top-expand.bottom-12)<.01,'Leading divider uses toolbar spacing below the expand button');
      assert.ok(Math.abs(b.top-a.bottom-12)<.01,'Collapsed groups use the toolbar divider plus its two gaps');
      const line=async(selector,horizontal,name)=>{
        const r=await rect(selector),p=[r.x+r.width/2,r.y+r.height/2];
        assert.ok(Math.abs((horizontal?r.height:r.width)-8)<.01,'Divider keeps its 8px slot');
        const shot=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${dir}/${theme}-${name}.png`,Buffer.from(shot.data,'base64'));
        const colors=await sample(shot.data,[p,horizontal?[p[0],p[1]+2]:[p[0]+2,p[1]]]);
        assert.notDeepEqual(colors[0],colors[1],`${theme} ${name}: separator line is visible`);
      };
      await line('.column-divider',true,'column-divider');
      await line('.column-divider ~ .column-divider',true,'column-group-divider');
      await line('.toolbar-controls .tile-divider',false,'toolbar-divider-horizontal');
      await send({type:'move_panel',panel:'toolbar',target:{kind:'edge',edge:'right',outer:true}});
      await line('.toolbar-controls .tile-divider',true,'toolbar-divider-vertical');
      await send({type:'move_panel',panel:'toolbar',target:{kind:'tab',group:41,index:null}});
      await click('.column-tab[data-panel="toolbar"]');
      await line('.content-drawer .tile-divider',true,'toolbar-divider-in-drawer');
    }
    console.log('PASS: drawer corners, closed/hover/open colors, shadow-free source/connector pixels, and visible dividers with matching spacing in both themes and drawer/toolbar orientations');
  } finally {
    await send({type:'restore_workspace',workspace:saved.workspace});
    await send({type:'restore_settings',settings:saved.settings});
  }
}

export async function checkDrawerDragging({call,evaluate,settle}) {
  const saved=await evaluate("layerApp.state().workspace");
  const dir=process.env.LAYER_TEST_ARTIFACTS||"artifacts/ui/drawer-drag/web";
  await mkdir(dir,{recursive:true});
  const wait=async()=>{await settle();await evaluate("new Promise(r=>setTimeout(r,280))");};
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await wait();};
  const customize=action=>send({type:"customize",action});
  const snap=()=>evaluate("layerApp.state().workspace");
  const rect=selector=>evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});const b=n.getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height}})()`);
  const center=r=>({x:r.x+r.width/2,y:r.y+r.height/2});
  const tabs=(id,panels)=>({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
  const fixture=structuredClone(saved);
  Object.assign(fixture.layout,{bands:[{id:40,edge:"left",extent:252,root:tabs(41,["brushes","sizes","tool_settings"])},{id:42,edge:"right",extent:252,root:tabs(43,["layers","properties","adjustments"])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(44,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const drawer='.content-drawer[data-drawer="41"]';
  const group=panel=>evaluate(`(()=>{const l=layerApp.state().workspace.layout;function find(n){if(n.kind==='tabs')return n.panels.includes(${JSON.stringify(panel)})?n:null;return find(n.first)||find(n.second);}return [...l.bands.map(b=>b.root),...l.floating.map(f=>f.root)].map(find).find(Boolean)})()`);
  let held=false,pointer="mouse",last;
  const press=async p=>{last=p;held=true;await call(pointer==="mouse"?"Input.dispatchMouseEvent":"Input.dispatchTouchEvent",pointer==="mouse"?{type:"mousePressed",...p,button:"left",buttons:1,clickCount:1}:{type:"touchStart",touchPoints:[{id:1,...p}]});};
  const move=async p=>{last=p;await call(pointer==="mouse"?"Input.dispatchMouseEvent":"Input.dispatchTouchEvent",pointer==="mouse"?{type:"mouseMoved",...p,button:held?"left":"none",buttons:held?1:0}:{type:"touchMove",touchPoints:[{id:1,...p}]});await wait();};
  const release=async()=>{await call(pointer==="mouse"?"Input.dispatchMouseEvent":"Input.dispatchTouchEvent",pointer==="mouse"?{type:"mouseReleased",...last,button:"left",buttons:0,clickCount:1}:{type:"touchEnd",touchPoints:[]});held=false;await wait();};
  const click=async p=>{await press(p);await release();};
  const cancel=async()=>{
    if(pointer==="touch")await call("Input.dispatchTouchEvent",{type:"touchCancel",touchPoints:[]});
    else {await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");await release();}
    held=false;await wait();
  };
  const open=async()=>{await customize({type:"set_column_collapsed",group:41,collapsed:true});await click(center(await rect('.collapsed-column [data-panel="brushes"]')));await wait();assert.ok(await evaluate(`!!document.querySelector('${drawer} .drawer-tabs')`));};
  const history=async before=>{const after=await snap();assert.notDeepEqual(after,before);await send({type:"invoke",command:"undo_workspace"});assert.deepEqual(await snap(),before);await send({type:"invoke",command:"redo_workspace"});assert.deepEqual(await snap(),after);};
  await evaluate(`window.__drawerActions=[];window.__drawerDispatch=layerApp.app.dispatch.bind(layerApp.app);layerApp.app.dispatch=a=>{if(a.type==='measure_column_drawers'||a.type==='drag_workspace')window.__drawerActions.push(structuredClone(a));return window.__drawerDispatch(a)};`);
  try {
    for(pointer of ["mouse","touch"]) {
      // Lifting and redocking a singleton preserve its visible or hidden header.
      for(const hidden of [false,true]) {
        const singleton=structuredClone(fixture);
        Object.assign(singleton.layout.bands[0].root,{panels:["sizes"],active:"sizes"});
        singleton.layout.panels.find(p=>p.id==="sizes").hide_tab=hidden;
        await send({type:"restore_workspace",workspace:singleton});
        const grip=()=>rect('.dock-group[data-panel="sizes"] .panel-grip');
        const placed=()=>evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('sizes'))");
        const before=await snap(),away=await evaluate("({x:innerWidth*.5,y:innerHeight*.55})");
        await press(center(await grip()));await move(away);
        assert.equal((await placed()).floating,true);assert.equal((await placed()).tabs_visible,!hidden);
        assert.deepEqual((await snap()).layout.panels,before.layout.panels);
        await cancel();assert.deepEqual(await snap(),before);
        await press(center(await grip()));await move(away);await release();await history(before);
        const floated=await snap();
        await press(center(await grip()));await move(await evaluate("({x:innerWidth-2,y:innerHeight*.5})"));await release();
        assert.equal((await placed()).floating,false);assert.equal((await placed()).tabs_visible,!hidden);
        assert.deepEqual((await snap()).layout.panels,before.layout.panels);await history(floated);
      }
      // Tabs move one panel; the grip and unused header move the complete group.
      for(const source of ["active","inactive","grip","empty"]) {
        console.log(`Checking ${pointer} drawer ${source}`);
        await send({type:"restore_workspace",workspace:fixture});await open();
        const b=await rect(drawer),g=await rect(`${drawer} .column-drawer-grip`);
        assert.ok(Math.abs(g.x+g.width-b.x-b.width)<1,"Grip stays at the right edge");
        const tab=["active","inactive"].includes(source)?await rect(`${drawer} .drawer-tabs .dock-tab[data-panel="${source==="active"?"brushes":"sizes"}"]`):null;
        const start=source==="grip"?center(g):source==="empty"?{x:g.x-12,y:g.y+g.height/2}:center(tab);
        const before=await snap(),away=await evaluate("({x:innerWidth*.5,y:innerHeight*.55})");
        await press(start);await move({x:start.x+10,y:start.y});assert.deepEqual(await snap(),before,`${pointer} ${source}: Moving inside drawer keeps source docked`);
        assert.equal(await evaluate("document.querySelectorAll('.dragged-tab-preview').length"),["active","inactive"].includes(source)?1:0);
        if(["active","inactive"].includes(source)) {
          const preview=await rect('.dragged-tab-preview');
          assert.ok(Math.abs(preview.x+preview.width/2-start.x-10)<1);
          assert.ok(Math.abs(preview.y+preview.height/2-start.y)<1);
        }
        const measured=await evaluate("window.__drawerActions.findLast(a=>a.type==='measure_column_drawers').measurements.find(m=>m.group===41).bounds");
        assert.deepEqual(measured,b,"Rust receives displayed drawer bounds");
        await move(away);assert.equal((await snap()).layout.floating.length,1,`${pointer} ${source} live tear-off: ${JSON.stringify(await evaluate("({status:document.querySelector('#status').textContent,drawer:layerApp.state().customization.column_drawers})"))}`);
        assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay, .dragged-tab-source').length"),0);
        if (["active", "inactive"].includes(source)) {
          const panel = source === "inactive" ? "sizes" : "brushes";
          const floated = await rect(`.dock-group .dock-tab[data-panel="${panel}"]`);
          assert.ok(Math.abs(floated.x - (away.x - tab.width / 2)) < 1,
            "Detachment preserves the grab point within the original tab");
        }
        await release();
        assert.deepEqual((await snap()).layout.panels,before.layout.panels,"Tear-off preserves tab preferences");
        const moved=await group(source==="inactive"?"sizes":"brushes");
        assert.equal(moved.panels.length,["grip","empty"].includes(source)?3:1,`${pointer} ${source}`);
        await history(before);
      }
      await send({type:"restore_workspace",workspace:fixture});await open();
      let before=await snap();
      await press(center(await rect(`${drawer} .drawer-tabs .dock-tab[data-panel="tool_settings"]`)));
      let b=await rect(drawer);await move({x:b.x+3,y:b.y+18});await release();
      assert.deepEqual((await group("brushes")).panels,["tool_settings","brushes","sizes"]);
      assert.equal((await snap()).layout.floating.length,0);await history(before);
      // A cancelled gesture restores the source group and its open drawer.
      await send({type:"restore_workspace",workspace:fixture});await open();before=await snap();
      await press(center(await rect(`${drawer} .drawer-tabs .dock-tab[data-panel="brushes"]`)));await move(await evaluate("({x:innerWidth*.5,y:innerHeight*.55})"));
      await cancel();assert.deepEqual(await snap(),before);assert.ok(await evaluate(`!!document.querySelector('${drawer}:not([inert])')`));
      // Scrolling clips hit rectangles, while the group grip stays fixed.
      const overflow=structuredClone(fixture);
      Object.assign(overflow.layout.bands[0].root,{panels:["brushes","sizes","tool_settings","navigator","stats"],tab_style:"icon_name"});
      await send({type:"restore_workspace",workspace:overflow});await open();
      const gripBefore=await rect(`${drawer} .column-drawer-grip`);
      await evaluate(`(()=>{const strip=document.querySelector('${drawer} .drawer-tab-strip');strip.scrollLeft=strip.children[0].offsetWidth+strip.children[1].offsetWidth*.75})()`);await wait();
      assert.deepEqual(await rect(`${drawer} .column-drawer-grip`),gripBefore);
      const clip=await rect(`${drawer} .drawer-tab-strip`);
      before=await snap();await press(center(await rect('.dock-group .dock-tab[data-panel="layers"]')));
      await move(await evaluate("({x:innerWidth*.5,y:innerHeight*.55})"));
      await move({x:clip.x+2,y:clip.y+clip.height/2});
      const sent=await evaluate("window.__drawerActions.findLast(a=>a.type==='drag_workspace')");
      const hits=sent.tabs.filter(t=>t.group===41);
      assert.equal(hits[0].index,1,"Hidden tab is excluded and indices are preserved");
      for(const hit of hits)assert.ok(hit.bounds.x>=clip.x&&hit.bounds.x+hit.bounds.width<=clip.x+clip.width+.01,"Tab hit is clipped to the scroll viewport");
      await release();assert.deepEqual((await group("layers")).panels,["brushes","layers","sizes","tool_settings","navigator","stats"]);await history(before);
      // Drops onto an open drawer use shared tab, merge and split targets.
      for(const zone of ["tab","merge","top","bottom"]) {
        console.log(`Checking ${pointer} drawer drop ${zone}`);
        await send({type:"restore_workspace",workspace:fixture});await open();before=await snap();
        // Group DOM identity uses data-group; select the header grip explicitly.
        const sourcePoint=zone==="merge"?await evaluate("(()=>{const n=[...document.querySelectorAll('.dock-group')].find(n=>n.dataset.group==='43'||n.querySelector('.dock-tab[data-panel=layers]'));const r=n.querySelector('.dock-tabs > .panel-grip').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()"):center(await rect('.dock-group .dock-tab[data-panel="layers"]'));
        await press(sourcePoint);await move(await evaluate("({x:innerWidth*.5,y:innerHeight*.6})"));
        b=await rect(drawer);
        const destination=zone==="tab"?{x:b.x+3,y:b.y+18}:zone==="top"?{x:b.x+b.width/2,y:b.y+40}:zone==="bottom"?{x:b.x+b.width/2,y:b.y+b.height-4}:center(b);
        await move(destination);
        const hint=await evaluate("(()=>{const n=document.querySelector('.drop-indicator');return{hidden:n.hidden,status:document.querySelector('#status').textContent}})()");
        assert.equal(hint.hidden,false,`${pointer} ${zone}: ${JSON.stringify(hint)}`);
        await release();const target=await group("layers");
        assert.deepEqual((await snap()).layout.panels,before.layout.panels,"Dropping preserves tab preferences");
        if(["top","bottom"].includes(zone))assert.notEqual(target.id,41);else assert.equal(target.id,41);
        if(zone==="tab")assert.equal(target.panels[0],"layers");
        if(zone==="merge")assert.equal(target.panels.length,6);
        assert.equal((await snap()).layout.floating.length,0);await history(before);
      }
      const shot=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${dir}/${pointer}.png`,Buffer.from(shot.data,"base64"));
      assert.equal(await evaluate("document.querySelector('#status').textContent"),"");
      console.log(`PASS: ${pointer} drawer panel/group drag, tab reorder, insertion, merge, top/bottom splits, clipped tab hits, preserved tab visibility, cancel, undo/redo`);
    }
  } finally {await evaluate("layerApp.app.dispatch=window.__drawerDispatch;delete window.__drawerDispatch;delete window.__drawerActions");if(held)await release();await send({type:"restore_workspace",workspace:saved});}
}
