import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

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
        const start=source==="grip"?center(g):source==="empty"?{x:g.x-12,y:g.y+g.height/2}:center(await rect(`${drawer} .drawer-tabs .dock-tab[data-panel="${source==="active"?"brushes":"sizes"}"]`));
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
