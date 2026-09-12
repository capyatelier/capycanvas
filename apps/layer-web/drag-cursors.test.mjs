import assert from "node:assert/strict";

export async function checkDragCursors({ call, evaluate, settle }) {
  const wait = async () => {
    await settle(); await evaluate("new Promise(r=>setTimeout(r,180))");
    await evaluate("Promise.all([...document.querySelectorAll('.neighbor-tab-preview')].flatMap(n=>n.getAnimations()).map(a=>a.finished))");
  };
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
  const noSlide = async () => assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay, .dragged-tab-source').length"), 0);
  const clean = async () => {
    assert.equal(await evaluate("document.querySelector('#workspace').dataset.workspaceCursor ?? null"), null);
    await noSlide();
  };
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
      const original = await rect(selector), start = center(original);
      const neighbor = await rect('.dock-tab[data-panel="properties"]');
      await press(start); await move({ x: start.x + 2, y: start.y }); await clean();
      for (const dx of [17, 19, 19, 17, 50, 24, -16]) {
        await move({ x: start.x + dx, y: start.y + 8 });
        const preview = await rect('.dragged-tab-preview');
        assert.equal(preview.x, original.x + Math.max(0, dx), "Attached tab follows the pointer within its strip");
        assert.equal(preview.y, original.y, "Attached tab stays on its original row");
        assert.equal(preview.width, original.width);
        assert.deepEqual(await rect(selector), original, "The tab's insertion slot stays fixed");
        assert.deepEqual(await rect('.dock-tab[data-panel="properties"]'), neighbor);
        assert.equal((await rect('.neighbor-tab-preview')).x, neighbor.x - (dx >= neighbor.width / 2 ? original.width : 0),
          "The dragged edge crosses a frozen midpoint; the same boundary reverses the swap");
        assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0);
      }
      const clip = await rect('.tab-slide-overlay');
      for (const x of [clip.x - 40, clip.x + clip.width + 10]) {
        await move({ x, y: start.y });
        const preview = await rect('.dragged-tab-preview');
        assert.equal(preview.x, x < clip.x ? clip.x : clip.x + clip.width - preview.width);
        assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0,
          "Visual clamping does not change the undocking threshold");
      }
      await move(away);
      await noSlide();
      assert.equal(await cursor(), "grabbing");
      assert.equal(await cursor("#canvas"), "grabbing", "Canvas cannot hide the drag cursor");
      assert.equal(await cursor('.dock-tab[data-panel="brushes"]'), "grabbing");
      if (end === "cancel") await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");
      if (end === "capture") await evaluate("document.querySelector('#workspace').releasePointerCapture(1)");
      if (end === "blur") await evaluate("window.dispatchEvent(new Event('blur'))");
      await release(); await clean();
      assert.notEqual(await cursor("#canvas"), "grabbing");
    }
    // Attached slides clean up without disturbing normal insertion or cancellation.
    for (const end of ["release", "cancel", "capture", "blur"]) {
      await send({ type: "restore_workspace", workspace: fixture });
      const selector = '.dock-tab[data-panel="properties"]';
      const source = await rect(selector);
      const start = center(source);
      await press(start);
      const first = await rect('.dock-tab[data-panel="layers"]');
      // Release just past halfway, before the pointer reaches the old midpoint.
      const target = { x: start.x - first.width / 2 - 1, y: start.y };
      await move(target);
      assert.equal(await evaluate("document.querySelectorAll('.dragged-tab-preview').length"), 1);
      assert.equal((await rect('.neighbor-tab-preview')).x, first.x + source.width);
      await move(target);
      assert.equal((await rect('.neighbor-tab-preview')).x, first.x + source.width,
        "Moving neighbors cannot oscillate the insertion decision");
      if (end === "cancel") await evaluate("document.querySelector('#workspace').dispatchEvent(new PointerEvent('pointercancel',{pointerId:1,bubbles:true}))");
      if (end === "capture") await evaluate("document.querySelector('#workspace').releasePointerCapture(1)");
      if (end === "blur") await evaluate("window.dispatchEvent(new Event('blur'))");
      await release(); await clean();
      assert.deepEqual(await evaluate("layerApp.state().workspace.layout.bands.find(b=>b.id===42).root.panels"),
        end === "release" ? ["properties", "layers"] : ["layers", "properties"]);
      assert.equal(await evaluate("layerApp.state().workspace.layout.floating.length"), 0);
    }
    await send({ type: "restore_workspace", workspace: fixture });
    await send({ type: "customize", action: { type: "set_column_collapsed", group: 41, collapsed: true } });
    await press(center(await rect('.collapsed-column[data-column="41"] > .panel-grip')));
    await move(away);
    assert.equal(await cursor(), "grabbing");
    const target = await evaluate("layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.id===43).bounds");
    await move({ x: target.x - 20, y: target.y + target.height / 2 });
    assert.equal(await cursor(), "grabbing");
    await move(away); assert.equal(await cursor(), "grabbing");
    await release(); await clean();

    // Pointer capture keeps the resize cursor when the divider is rebuilt.
    await send({ type: "restore_workspace", workspace: fixture });
    const divider = await evaluate("layerApp.app.layout(innerWidth,innerHeight).dividers.find(d=>d.id===40)");
    const start = center(divider.bounds);
    await press(start); await move({ x: start.x + 40, y: start.y });
    assert.equal(await cursor(), "col-resize");
    assert.equal(await cursor("#canvas"), "col-resize");
    await release(); await clean();
    console.log("Web drag feedback: attached tab sliding, insertion, tear-off, cursors, resize, release, cancellation, capture loss, and blur passed.");
  } finally {
    if (held) await release();
    await send({ type: "restore_workspace", workspace: saved });
  }
}
