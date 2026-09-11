import assert from "node:assert/strict";

export async function checkColumnSizing({ call, evaluate, settle }) {
  const send = async action => {
    await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);
    await settle();
  };
  const snapshot = () => evaluate("layerApp.state().workspace");
  const resolved = () => evaluate("layerApp.app.layout(innerWidth,innerHeight)");
  const mouse = (type, point, extra = {}) => call("Input.dispatchMouseEvent", { type, ...point, ...extra });
  const doubleClick = async point => {
    await mouse("mouseMoved", point);
    for (const clickCount of [1, 2]) {
      await mouse("mousePressed", point, { button: "left", clickCount });
      await mouse("mouseReleased", point, { button: "left", clickCount });
    }
    await settle();
  };
  const center = bounds => ({ x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 });
  const initial = await snapshot();
  const tabs = (id, panels) => ({ kind: "tabs", id, panels, active: panels[0], tab_style: "automatic" });
  const split = (id, axis, fraction, first, second) => ({ kind: "split", id, axis, fraction, first, second });
  const nested = edge => {
    const workspace = structuredClone(initial);
    workspace.layout.bands = [{ id: 40, edge, extent: 700,
      root: split(41, "vertical", .4, tabs(42, ["sizes", "layers"]),
        split(43, "horizontal", .8, tabs(44, ["brushes"]),
          split(45, "vertical", .3, tabs(46, ["navigator"]), tabs(47, ["tool_settings"])))) }];
    workspace.layout.next_id = 48;
    return workspace;
  };
  try {
    for (const edge of ["left", "right"]) {
      for (const mode of ["expanded", "collapsed", "nested"]) {
        const workspace = mode === "nested" ? nested(edge) : structuredClone(initial);
        const band = workspace.layout.bands.find(b => b.edge === edge && b.root.kind === "split");
        assert.ok(band, `${edge} content column`);
        band.extent = 700;
        await send({ type: "restore_workspace", workspace });
        if (mode === "collapsed") await send({ type: "customize",
          action: { type: "set_column_collapsed", group: band.root.id, collapsed: true } });
        const before = await snapshot();
        const divider = (await resolved()).dividers.find(d => d.id === band.id && d.band);
        const point = center(divider.bounds);
        await doubleClick(point);
        const after = await snapshot();
        const width = mode === "nested" ? 502 : edge === "left" ? 242 : 254;
        const resized = after.layout.bands.find(b => b.id === band.id);
        assert.equal(resized.extent, width + 6, `${edge} ${mode}: normal starting width`);
        assert.equal(after.layout.collapsed.some(c => c.root === band.root.id), false);
        if (mode === "nested") {
          assert.equal(resized.root.fraction, before.layout.bands[0].root.fraction);
          assert.ok(Math.abs(resized.root.second.fraction - 242 / 496) < 1e-6);
          const groups = (await resolved()).groups;
          for (const [id, expected] of [[42, 502], [44, 242], [46, 254], [47, 254]])
            assert.ok(Math.abs(groups.find(g => g.id === id).bounds.width - expected) < .01);
        }
        // Release after the double-click must leave no pending resize.
        await mouse("mouseMoved", { x: point.x + 15, y: point.y });
        await settle();
        assert.deepEqual(await snapshot(), after);
        await send({ type: "invoke", command: "undo_workspace" });
        assert.deepEqual(await snapshot(), before);
        await send({ type: "invoke", command: "redo_workspace" });
        assert.deepEqual(await snapshot(), after);

        // The same divider remains draggable after a reset.
        const start = center((await resolved()).dividers.find(d => d.id === band.id && d.band).bounds);
        const end = { x: start.x + (edge === "left" ? 40 : -40), y: start.y };
        await mouse("mouseMoved", start);
        await mouse("mousePressed", start, { button: "left", clickCount: 1 });
        await mouse("mouseMoved", end, { button: "left", buttons: 1 });
        await mouse("mouseReleased", end, { button: "left", clickCount: 1 });
        await settle();
        assert.ok(Math.abs((await snapshot()).layout.bands.find(b => b.id === band.id).extent - (width + 46)) < .01);
        await send({ type: "invoke", command: "undo_workspace" });
        assert.deepEqual(await snapshot(), after);

        if (mode === "nested") {
          // Internal column and stacked-row dividers keep their existing behavior.
          for (const id of [41, 43, 45]) {
            await doubleClick(center((await resolved()).dividers.find(d => d.id === id).bounds));
            assert.deepEqual(await snapshot(), after);
          }
        }
      }
    }
    await send({ type: "restore_workspace", workspace: initial });
    const row = (await resolved()).dividers.find(d => d.band && d.axis === "vertical");
    await doubleClick(center(row.bounds));
    assert.deepEqual(await snapshot(), initial, "Top/bottom band dividers do not reset column widths");
    console.log("Web column sizing: real double-clicks on both sides, collapsed and recursive groups, undo/redo, and subsequent resizing passed.");
  } finally {
    await send({ type: "restore_workspace", workspace: initial });
  }
}
