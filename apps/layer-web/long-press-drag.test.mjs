import assert from "node:assert/strict";

export async function checkLongPressDragging({call, evaluate, settle}) {
  const saved = await evaluate("layerApp.state().workspace");
  const fixture = structuredClone(saved);
  const tabs = (id, panels) => ({kind:"tabs", id, panels, active:panels[0], tab_style:"icon"});
  Object.assign(fixture.layout, {bands:[
    {id:40, edge:"left", extent:252, root:tabs(41,["brushes","sizes"])},
    {id:42, edge:"right", extent:252, root:tabs(43,["layers","properties","adjustments"])},
  ], floating:[], collapsed:[], column_scroll:[], fit_tab_groups:[], next_id:Math.max(44,fixture.layout.next_id)});
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
    for (const mode of ["release", "reorder", "float", "cancel", "grip", "drawer", "pen"]) {
      await send({type:"restore_workspace",workspace:fixture});
      await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
      const drawer = mode === "drawer";
      if (drawer) {
        await send({type:"customize",action:{type:"set_column_collapsed",group:43,collapsed:true}});
        await send({type:"customize",action:{type:"toggle_column_drawer",group:43,panel:"layers"}});
      }
      const scope = drawer ? '.content-drawer[data-drawer="43"]' : '.dock-group[data-group="43"]';
      const selector = mode === "grip" ? `${scope} .dock-tabs > .panel-grip` : `${scope} .dock-tab[data-panel="properties"]`;
      const start = center(await rect(selector)), before = await snapshot();
      pen = mode === "pen";
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
      const point = ["reorder","drawer","pen"].includes(mode) ? {x:start.x-20,y:start.y} : away;
      await move(point);
      assert.equal(await menu(),false,`${mode}: moving after the hold dismisses the menu`);
      assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor"),"grabbing");
      if (pen) {
        await contextEvent(selector);
        assert.equal(await menu(),false,"A late native context event cannot reopen the menu during dragging");
      }
      if (mode === "cancel") {
        await call("Input.dispatchTouchEvent",{type:"touchCancel",touchPoints:[]}); held=false; await wait();
        assert.deepEqual(await snapshot(),before,"Cancel restores the source layout");
      } else {
        await release();
        const after = await snapshot();
        if (["reorder","drawer","pen"].includes(mode))
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
    console.log("PASS: long-press menu release/action, tab reorder, tear-off, group and drawer dragging, cancellation, undo, and native pen context events");
  } finally {
    if (held) await release();
    await evaluate("document.querySelector('.panel-context-menu').hidePopover()");
    await send({type:"restore_workspace",workspace:saved});
  }
}
