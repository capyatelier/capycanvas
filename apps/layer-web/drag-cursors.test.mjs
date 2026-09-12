import assert from "node:assert/strict";

export async function checkDragCursors({ call, evaluate, settle }) {
  const wait = async () => { await settle(); await evaluate("new Promise(r=>setTimeout(r,180))"); };
  const send = async action => { await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`); await wait(); };
  const saved = await evaluate("layerApp.state().workspace");
  const fixture = structuredClone(saved);
  const tabs = (id, panels) => ({ kind: "tabs", id, panels, active: panels[0], tab_style: "icon" });
  Object.assign(fixture.layout, { bands: [
    { id: 40, edge: "left", extent: 252, root: tabs(41, ["brushes", "sizes"]) },
    { id: 42, edge: "right", extent: 252, root: tabs(43, ["layers", "properties"]) },
  ], floating: [], collapsed: [], column_scroll: [], fit_tab_groups: [], next_id: Math.max(44, fixture.layout.next_id) });
  fixture.zen_mode = false;
  const center = b => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });
  const rect = selector => evaluate(`(()=>{const b=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height}})()`);
  const cursor = (selector = "#workspace") => evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(selector)})).cursor`);
  const clean = async () => assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor ?? null"), null);
  const away = await evaluate("({x:innerWidth/2,y:innerHeight/2})");
  let held = false, last = away;
  const move = async p => { last = p; await call("Input.dispatchMouseEvent", { type: "mouseMoved", ...p, button: held ? "left" : "none", buttons: held ? 1 : 0 }); await wait(); };
  const press = async p => { await move(p); held = true; await call("Input.dispatchMouseEvent", { type: "mousePressed", ...p, button: "left", buttons: 1, clickCount: 1 }); };
  const release = async () => { await call("Input.dispatchMouseEvent", { type: "mouseReleased", ...last, button: "left", buttons: 0, clickCount: 1 }); held = false; await wait(); };
  try {
    for (const end of ["release", "cancel", "capture", "blur"]) {
      await send({ type: "restore_workspace", workspace: fixture });
      const selector = '.dock-tab[data-panel="layers"]';
      assert.equal(await cursor(selector), "grab");
      const start = center(await rect(selector));
      await press(start); await move({ x: start.x + 2, y: start.y }); await clean();
      await move(away);
      assert.equal(await cursor(), "grabbing");
      assert.equal(await cursor("#canvas"), "grabbing", "Canvas cannot hide the drag cursor");
      assert.equal(await cursor('.dock-tab[data-panel="brushes"]'), "grabbing");
      if (end === "cancel") await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");
      if (end === "capture") await evaluate("document.querySelector('#workspace').releasePointerCapture(1)");
      if (end === "blur") await evaluate("window.dispatchEvent(new Event('blur'))");
      await release(); await clean();
      assert.notEqual(await cursor("#canvas"), "grabbing");
    }
    await send({ type: "restore_workspace", workspace: fixture });
    await send({ type: "customize", action: { type: "set_column_collapsed", group: 41, collapsed: true } });
    await press(center(await rect('.collapsed-column[data-column="41"] > .panel-grip')));
    await move(away);
    assert.equal(await cursor(), "no-drop");
    const target = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.id===43).bounds");
    await move({ x: target.x - 20, y: target.y + target.height / 2 });
    assert.equal(await cursor(), "grabbing");
    await move(away); assert.equal(await cursor(), "no-drop");
    await release(); await clean();

    // Pointer capture keeps the resize cursor when the divider is rebuilt.
    await send({ type: "restore_workspace", workspace: fixture });
    const divider = await evaluate("layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.id===40)");
    const start = center(divider.bounds);
    await press(start); await move({ x: start.x + 40, y: start.y });
    assert.equal(await cursor(), "col-resize");
    assert.equal(await cursor("#canvas"), "col-resize");
    await release(); await clean();
    console.log("Web drag cursors: hover, tear-off, collapsed-column validity, resize, release, cancellation, capture loss, and blur passed.");
  } finally {
    if (held) await release();
    await send({ type: "restore_workspace", workspace: saved });
  }
}
