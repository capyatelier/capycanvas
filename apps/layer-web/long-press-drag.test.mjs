import assert from "node:assert/strict";

export async function checkLongPressDragging({call, evaluate, settle}) {
  const saved = await evaluate("layerApp.state().workspace");
  const fixture = structuredClone(saved);
  const tabs = (id, panels) => ({kind:"tabs", id, panels, active:panels[0], tab_style:"icon"});
  Object.assign(fixture.layout, {bands:[
    {id:40, edge:"left", extent:252, root:tabs(41,["brushes","sizes"])},
    {id:42, edge:"right", extent:252, root:tabs(43,["layers","properties","adjustments"])},
    {id:44, edge:"top", extent:36, root:tabs(45,["toolbar"])},
  ], floating:[], collapsed:[], column_scroll:[], fit_tab_groups:[], next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode = false;
  const wait = async () => { await settle(); await evaluate("new Promise(r=>setTimeout(r,200))"); };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await wait(); };
  const snapshot = () => evaluate("layerApp.state().workspace");
  const menu = () => evaluate("document.querySelector('.panel-context-menu').matches(':popover-open')");
  const rect = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});if(!n)throw Error('Missing '+${JSON.stringify(selector)});const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const center = b => ({x:b.x+b.width/2,y:b.y+b.height/2});
  let held = false, last, pen = false;
  const press = async point => {
    held = true; last = point;
    await call(pen ? "Input.dispatchMouseEvent" : "Input.dispatchTouchEvent", pen
      ? {type:"mousePressed",...point,button:"left",buttons:1,clickCount:1,pointerType:"pen"}
      : {type:"touchStart",touchPoints:[{id:1,...point}]});
  };
  const move = async point => {
    last = point;
    await call(pen ? "Input.dispatchMouseEvent" : "Input.dispatchTouchEvent", pen
      ? {type:"mouseMoved",...point,button:"left",buttons:1,pointerType:"pen"}
      : {type:"touchMove",touchPoints:[{id:1,...point}]});
    await wait();
  };
  const release = async () => {
    await call(pen ? "Input.dispatchMouseEvent" : "Input.dispatchTouchEvent", pen
      ? {type:"mouseReleased",...last,button:"left",buttons:0,clickCount:1,pointerType:"pen"}
      : {type:"touchEnd",touchPoints:[]});
    held = false; await wait();
    // Android Chrome in desktop-site mode delays the synthesized touch click.
    await evaluate("new Promise(r=>setTimeout(r,350))");
  };
  const clean = async () => {
    assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay,.dragged-tab-source').length"),0);
    assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor ?? null"),null);
  };
  const contextEvent = selector => evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});const r=n.getBoundingClientRect();n.dispatchEvent(new PointerEvent('contextmenu',{bubbles:true,cancelable:true,pointerType:'pen',clientX:r.x+5,clientY:r.y+5}));})()`);
  try {
    for (const mode of ["release", "reorder", "float", "cancel", "grip", "drawer", "pen", "floating-tab", "floating-group", "toolbar-grip", "tool", "tool-cancel", "tool-pen"]) {
      await send({type:"restore_workspace",workspace:fixture});
      await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
      const tile = mode.startsWith("tool") && mode !== "toolbar-grip";
      if (mode.startsWith("floating")) await send({type:"move_group",group:43,target:{kind:"float",position:[600,250]},viewport:await evaluate("[innerWidth,innerHeight]")});
      const drawer = mode === "drawer";
      if (drawer) {
        await send({type:"customize",action:{type:"set_column_collapsed",group:43,collapsed:true}});
        await send({type:"customize",action:{type:"toggle_column_drawer",group:43,panel:"layers"}});
      }
      const scope = drawer ? '.content-drawer[data-drawer="43"]' : '.dock-group[data-group="43"]';
      const selector = tile ? '.toolbar-controls [draggable="true"]' : mode === "toolbar-grip" ? '.toolbar-controls > .panel-grip' : ["grip","floating-group"].includes(mode) ? `${scope} .dock-tabs > .panel-grip` : `${scope} .dock-tab[data-panel="properties"]`;
      const start = center(await rect(selector)), before = await snapshot();
      pen = mode === "pen" || mode === "tool-pen";
      await press(start);
      if (pen) await contextEvent(selector);
      else await evaluate("new Promise(r=>setTimeout(r,650))");
      assert.equal(await menu(),true,`${mode}: hold opens the menu`);
      assert.deepEqual(await snapshot(),before,`${mode}: hold does not move or select the tab`);
      await move({x:start.x+2,y:start.y});
      assert.equal(await menu(),true,"Small movements keep the menu open");
      if (mode === "release") {
        await release(); await clean();
        assert.equal(await menu(),true,"Releasing a hold keeps its menu available");
        assert.deepEqual(await snapshot(),before,"Releasing a hold must not select the inactive tab");
        const button = await evaluate("(()=>{const n=[...document.querySelectorAll('.panel-context-menu button')].find(n=>n.textContent.includes('Hide Properties'));const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()");
        await press(button); await release();
        assert.equal(await menu(),false,"A subsequent contact can choose a menu action");
        assert.deepEqual((await snapshot()).layout.bands[1].root.panels,["layers","adjustments"]);
        continue;
      }
      const away = await evaluate("({x:innerWidth*.5,y:innerHeight*.55})");
      const point = tile ? await evaluate("(()=>{const r=document.querySelectorAll('.toolbar-controls [draggable=true]')[2].getBoundingClientRect();return{x:r.x+r.width*.8,y:r.y+r.height*.8}})()") : ["reorder","drawer","pen"].includes(mode) ? {x:start.x-20,y:start.y} : away;
      await move(point);
      assert.equal(await menu(),false,`${mode}: moving after the hold dismisses the menu`);
      if (!tile) assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor"),"grabbing");
      else assert.equal(await evaluate("document.querySelector('.drop-indicator').hidden"),false);
      if (pen) {
        await contextEvent(selector);
        assert.equal(await menu(),false,"A late native context event cannot reopen the menu during dragging");
      }
      if (mode === "cancel" || mode === "tool-cancel") {
        await call("Input.dispatchTouchEvent",{type:"touchCancel",touchPoints:[]}); held=false; await wait();
        assert.deepEqual(await snapshot(),before,"Cancel restores the source layout");
      } else {
        await release();
        const after = await snapshot();
        if (tile || mode.startsWith("floating") || mode === "toolbar-grip") assert.notDeepEqual(after,before,`${mode}: dropping moves the element`);
        else if (["reorder","drawer","pen"].includes(mode))
          assert.deepEqual(after.layout.bands[1].root.panels,["properties","layers","adjustments"]);
        else {
          assert.equal(after.layout.floating.length,1);
          assert.equal(after.layout.floating[0].root.panels.length,mode === "grip" ? 3 : 1);
        }
        await send({type:"invoke",command:"undo_workspace"});
        assert.deepEqual(await snapshot(),before,"The continued drag is a single undo step");
      }
      await clean();
    }
    await send({type:"restore_workspace",workspace:fixture});
    for (let i=0;i<2;i++) await send({type:"layer",action:{op:"new",group:false,clipped:false}});
    const order = () => evaluate("layerApp.state().layers.map(l=>String(l.id))");
    const beforeLayers = await order();
    pen = false;
    for (const cancel of [true,false]) {
      const rows = await evaluate("[...document.querySelectorAll('#layer-rows .layer-row')].map(n=>n.dataset.layer)");
      const source = `#layer-rows .layer-row[data-layer="${rows[0]}"] .layer-grip`;
      await press(center(await rect(source)));
      await evaluate("new Promise(r=>setTimeout(r,650))");
      assert.equal(await menu(),true,"Layer grip hold opens its menu");
      const target = await rect(`#layer-rows .layer-row[data-layer="${rows[1]}"]`);
      await move({x:target.x+target.width*.5,y:target.y+target.height-3});
      assert.equal(await menu(),false,"Layer grip movement closes its menu");
      assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),1);
      if (cancel) { await call("Input.dispatchTouchEvent",{type:"touchCancel",touchPoints:[]});held=false;await wait();assert.deepEqual(await order(),beforeLayers); }
      else { await release();assert.notDeepEqual(await order(),beforeLayers);await send({type:"invoke",command:"undo"});assert.deepEqual(await order(),beforeLayers); }
      assert.equal(await evaluate("document.querySelectorAll('.layer-drag-preview').length"),0);
    }
    for (let i=0;i<2;i++) await send({type:"invoke",command:"undo"});
    console.log("PASS: long-press menu release/action, tab reorder, tear-off, docked/floating groups, drawers, toolbar grips, tools, layer grips, cancellation, undo, and native pen context events");
  } finally {
    if (held) await release();
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await send({type:"restore_workspace",workspace:saved});
  }
}
