// Exercise the actual DOM listeners and packed Wasm records without a GPU.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");
function harness() {
  const listeners = new Map(), records = [], phases = [];
  let contact = null;
  const context = {
    canvas: {
      width: 800, height: 600,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
      focus() {}, setPointerCapture() {},
      addEventListener: (name, fn) => listeners.set(name, fn),
    },
    lastPenEvent: null, pending: [], state: { camera: { revision: 1 } },
    app: { pen(batch) { records.push(...batch); return batch.length / 11; } },
    input(event) {
      phases.push(event);
      if (event.phase === "down" && contact === null && event.kind !== "touch")
        contact = { id: event.id, paint: event.button === "primary" };
      const paint = contact?.id === event.id && contact.paint;
      if (contact?.id === event.id && ["up", "cancel"].includes(event.phase)) contact = null;
      return { paint };
    },
    cursorInput() {}, wake() {},
  };
  runInNewContext(source.slice(source.indexOf("function position(e)"),
    source.indexOf('canvas.addEventListener("contextmenu"')), context);
  let timeStamp = 0;
  return {
    send(type, overrides = {}) {
      listeners.get(type)({ type, pointerId: -7, pointerType: "pen", button: 0,
        buttons: 1, pressure: 0.6, clientX: 40, clientY: 50,
        timeStamp: ++timeStamp, preventDefault() {}, ...overrides });
    },
    samples: () => Array.from({ length: records.length / 11 }, (_, i) => records.slice(i * 11, i * 11 + 11)),
    phases,
  };
}

for (const ending of ["pointerup", "pointercancel", "lostpointercapture", "pointermove"]) {
  test(`pen stroke survives ${ending} and finishes exactly once`, () => {
    const h = harness();
    h.send("pointerdown");
    h.send("pointermove", { clientX: 100 });
    h.send(ending, { clientX: 700, clientY: 500, buttons: 0, pressure: 0 });
    h.send("lostpointercapture", { pointerType: "", buttons: 0 });
    h.send("pointerup", { buttons: 0, pressure: 0 });
    h.send("pointermove", { buttons: 0, pressure: 0 });
    const samples = h.samples();
    assert.deepEqual(samples.map(s => s[1]), [1, 2, 3]);
    assert.equal(samples[2][0], -7 >>> 0);
    if (ending !== "pointerup") {
      assert.deepEqual(samples[2].slice(2, 5), [100, 50, 0.6]);
    }
    // A second stroke must remain usable after interruption.
    h.send("pointerdown");
    h.send("pointerup", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 3]);
  });
}

test("capture loss with missing pointer type still finishes the active pen", () => {
  const h = harness();
  h.send("pointerdown");
  h.send("lostpointercapture", { pointerType: "", pressure: 0, buttons: 0 });
  assert.deepEqual(h.samples().map(s => s[1]), [1, 3]);
});

test("foreign pointers cannot end the active stroke", () => {
  const h = harness();
  h.send("pointerdown");
  h.send("pointercancel", { pointerId: 9 });
  h.send("lostpointercapture", { pointerId: 9 });
  h.send("pointermove");
  h.send("pointerup");
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});

test("zero pressure with tip or eraser contact is not a lift", () => {
  for (const buttons of [1, 32]) {
    const h = harness();
    h.send("pointerdown", { buttons });
    h.send("pointermove", { buttons, pressure: 0 });
    h.send("pointerup", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
  }
});

test("mouse cancellation and touch/pan routing retain their semantics", () => {
  const mouse = harness();
  mouse.send("pointerdown", { pointerType: "mouse" });
  mouse.send("pointercancel", { pointerType: "mouse" });
  assert.deepEqual(mouse.samples().map(s => s[1]), [1, 4]);
  for (const overrides of [{ pointerType: "touch" }, { button: 2, buttons: 2 }]) {
    const h = harness();
    h.send("pointerdown", overrides);
    h.send("pointercancel", overrides);
    assert.deepEqual(h.samples(), []);
    assert.equal(h.phases.at(-1).phase, "cancel");
  }
});
