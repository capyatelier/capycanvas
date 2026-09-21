import assert from "node:assert/strict";

// Shared by the focused pen and packaged-app suites. Inspect presented pixels:
// model-only cursor assertions would miss a broken GPU overlay.
export async function checkPenRendering({ call, evaluate, settle }) {
  const inPage = (fn, ...args) => evaluate(`(${fn})(...${JSON.stringify(args)})`);
  // Keep recovery copies from earlier runs on the dedicated device test origin.
  await inPage(() => new Promise((resolve, reject) => {
    const deadline = performance.now() + 10000;
    let quiet = performance.now();
    function check() {
      const button = [...document.querySelectorAll("dialog[open] button")]
        .find(node => node.textContent === "Keep for Later");
      if (button) { button.click(); quiet = performance.now(); }
      if (performance.now() - quiet > 400) resolve();
      else if (performance.now() > deadline) reject(Error("Recovery prompts did not settle"));
      else setTimeout(check, 50);
    }
    check();
  }));
  await settle();
  assert.equal(await evaluate("document.querySelectorAll('dialog[open]').length"), 0,
    "Pen fixture must have an unobstructed canvas");
  const action = async value => {
    await inPage(value => layerApp.dispatch(value), value);
    await settle();
  };
  const saved = await evaluate("layerApp.state().settings");
  const brush = await evaluate("layerApp.state().brush");
  await action({ type: "select_brush", id: 1 });
  await action({ type: "set_brush_size", value: 18 });
  // Keep predicted ink from changing underneath cursor pixel comparisons.
  await action({ type: "restore_settings", settings: { ...saved, cursor: "brush_size", hide_cursor_while_drawing: true, feedback: false } });
  await action({ type: "open_settings", page: "input" });
  assert.equal(await evaluate("document.querySelector('#setting-cursor').closest('[data-page]').dataset.page"), "input");
  assert.equal(await evaluate("document.querySelector('#setting-hide-cursor-while-drawing').checked"), true);
  await inPage(() => document.querySelector('#setting-hide-cursor-while-drawing').click());
  await settle();
  assert.equal(await evaluate("layerApp.state().settings.hide_cursor_while_drawing"), false);
  await action({ type: "preferences", action: { type: "reset", id: "hide_cursor_while_drawing" } });
  assert.equal(await evaluate("document.querySelector('#setting-hide-cursor-while-drawing').checked"), true);
  await action({ type: "close_settings" });
  const point = await inPage(() => {
    const camera = layerApp.app.camera(), rect = layerApp.canvas.getBoundingClientRect(), area = camera.work_area;
    return { x: rect.x + (area[0] + area[2] / 2) * rect.width / camera.viewport[0],
      y: rect.y + (area[1] + area[3] / 2) * rect.height / camera.viewport[1] };
  });
  const send = async event => {
    await call("Input.dispatchMouseEvent", event);
    await settle();
    // CDP verifies requested CSS policy, not the physical tablet's system cursor.
    assert.equal(await evaluate("getComputedStyle(layerApp.canvas).cursor"), "none");
  };
  const away = pointerType => send({ type: "mouseMoved", pointerType, x: 1, y: 1, buttons: 0 });
  const pixels = async () => {
    const shot = await call("Page.captureScreenshot", { format: "png",
      clip: { x: point.x - 50, y: point.y - 35, width: 100, height: 70, scale: 1 } });
    return inPage(async data => {
      const image = new Image();
      image.src = "data:image/png;base64," + data;
      await image.decode();
      const canvas = document.createElement("canvas");
      canvas.width = image.width; canvas.height = image.height;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      context.drawImage(image, 0, 0);
      return Array.from(context.getImageData(0, 0, canvas.width, canvas.height).data);
    }, shot.data);
  };
  const difference = (a, b) => a.reduce((count, value, i) => count + (Math.abs(value - b[i]) > 8 ? 1 : 0), 0);
  await away("mouse");
  const before = await pixels();
  try {
    await inPage(() => {
      const proto = Object.getPrototypeOf(layerApp.canvas.getContext("webgpu").getConfiguration().device.features);
      const original = proto.has;
      window.penFeatures = { proto, original, calls: 0 };
      proto.has = function(...args) { penFeatures.calls++; return original.apply(this, args); };
    });
    for (const pointerType of ["mouse", "pen"]) {
      await send({ type: "mouseMoved", pointerType, ...point, buttons: 0 });
      assert.ok(difference(before, await pixels()) > 4, `${pointerType} cursor reaches GPU pixels`);
      await away(pointerType);
      assert.deepEqual(await pixels(), before, `${pointerType} leaving clears the GPU cursor`);
      await send({ type: "mousePressed", pointerType,
        x: point.x - 30, y: point.y, button: "left", buttons: 1, clickCount: 1, force: .3 });
      for (let i = 1; i <= 6; i++)
        await send({ type: "mouseMoved", pointerType,
          x: point.x - 30 + i * 10, y: point.y, button: "left", buttons: 1, force: .3 + i * .1 });
      const hidden = await pixels();
      const preference = (id, value) => action({ type: "preferences", action: { type: "edit", id, value } });
      await preference("hide_cursor_while_drawing", false);
      assert.ok(difference(hidden, await pixels()) > 4, `${pointerType}: disabling hiding shows the live outline`);
      await preference("hide_cursor_while_drawing", true);
      assert.equal(difference(await pixels(), hidden), 0, `${pointerType}: enabling hiding clears the retained GPU cursor`);
      await preference("cursor", 0);
      assert.equal(difference(await pixels(), hidden), 0, `${pointerType}: hidden drawing matches No cursor pixels`);
      await action({ type: "preferences", action: { type: "reset", id: "cursor" } });
      await send({ type: "mouseReleased", pointerType,
        x: point.x + 30, y: point.y, button: "left", buttons: 0, clickCount: 1, force: 0 });
      const released = await pixels();
      await away(pointerType);
      const painted = await pixels();
      assert.ok(difference(released, painted) > 4, `${pointerType}: release restores the hover cursor`);
      assert.ok(difference(before, painted) > 20, `${pointerType}: 18 px G Pen commits GPU ink`);
      await action({ type: "invoke", command: "undo" });
      assert.deepEqual(await pixels(), before, "one undo removes exactly this contact");
      await action({ type: "invoke", command: "redo" });
      assert.equal(difference(await pixels(), painted), 0, "redo restores the committed stroke");
      await action({ type: "invoke", command: "undo" });
    }
    assert.equal(await evaluate("penFeatures.calls"), 0, "drawing never remaps immutable GPUDevice features");
  } finally {
    await inPage(() => {
      if (!window.penFeatures) return;
      penFeatures.proto.has = penFeatures.original;
      delete window.penFeatures;
    });
    await action({ type: "restore_settings", settings: saved });
    await action({ type: "select_brush", id: brush.preset });
    await action({ type: "set_brush_size", value: brush.diameter });
  }
  console.log("GPU mouse/pen cursors, hide while drawing toggle/reset, G Pen 18 px, undo/redo and cached device features passed");
}
