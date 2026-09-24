// Exercise the actual DOM listeners and packed Wasm records without a GPU.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");

test("cursor hover preserves mouse, pen, and eraser device kinds", () => {
  const canvas = {}, samples = [];
  const context = {
    gpuReady: true, canvas, lastPenEvent: null,
    document: { elementFromPoint: () => canvas },
    position: () => [20, 30], wake() {},
    app: { cursor_input: sample => samples.push(Array.from(sample)) },
  };
  runInNewContext(source.slice(source.indexOf("let canvasCursorActive"), source.indexOf("function wake()")), context);
  for (const [pointerType, buttons, kind] of [["mouse", 0, 1], ["pen", 0, 0], ["pen", 32, 2]]) {
    context.cursorInput({ pointerType, buttons, pointerId: 1, target: canvas, clientX: 20,
      clientY: 30, timeStamp: 12, pressure: .5 });
    assert.equal(samples.at(-1).length, 11);
    assert.equal(samples.at(-1)[9], kind);
  }
  context.cursorInput({ pointerType: "touch" });
  assert.deepEqual(samples.at(-1), []);
});
function harness({ raw = false, prediction = false, paint: allowPaint = true } = {}) {
  const listeners = new Map(), records = [], phases = [], cursors = [];
  let contact = null;
  const context = {
    canvas: {
      width: 800, height: 600,
      getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }),
      focus() {}, setPointerCapture() {},
      addEventListener: (name, fn) => listeners.set(name, fn),
    },
    lastPenEvent: null, pending: [], state: { camera: { revision: 1 }, settings: { feedback: prediction, platform_prediction: prediction } },
    app: { pen(batch) { records.push(...batch); return batch.length / 11; } },
    input(event) {
      phases.push(event);
      if (event.phase === "down" && contact === null && event.kind !== "touch")
        contact = { id: event.id, paint: allowPaint && event.button === "primary" };
      const paint = contact?.id === event.id && contact.paint;
      const handled = contact?.id === event.id;
      if (contact?.id === event.id && ["up", "cancel"].includes(event.phase)) contact = null;
      return { paint, handled };
    },
    cursorInput(event) { cursors.push(event?.type ?? null); }, wake() {},
  };
  if (raw) context.onpointerrawupdate = null;
  runInNewContext(source.slice(source.indexOf("function position("),
    source.indexOf('canvas.addEventListener("contextmenu"')), context);
  let timeStamp = 0;
  return {
    send(type, overrides = {}) {
      listeners.get(type)({ type, pointerId: -7, pointerType: "pen", button: 0,
        buttons: 1, pressure: 0.6, clientX: 40, clientY: 50,
        timeStamp: ++timeStamp, cancelable: type !== "pointerrawupdate", preventDefault() {}, ...overrides });
    },
    samples: () => Array.from({ length: records.length / 11 }, (_, i) => records.slice(i * 11, i * 11 + 11)),
    phases, cursors,
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
  assert.equal(h.cursors.at(-1), null);
});

test("normal mouse and pen release preserves hover after capture ends", () => {
  for (const pointerType of ["mouse", "pen"]) {
    const h = harness();
    h.send("pointerdown", { pointerType });
    h.send("pointerup", { pointerType, pressure: 0, buttons: 0 });
    h.send("lostpointercapture", { pointerType, pressure: 0, buttons: 0 });
    assert.equal(h.cursors.at(-1), "pointerup");
  }
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
  for (const overrides of [{ pointerType: "touch" }, { pointerType: "mouse", button: 2, buttons: 2 }]) {
    const h = harness();
    h.send("pointerdown", overrides);
    h.send("pointercancel", overrides);
    assert.deepEqual(h.samples(), []);
    assert.equal(h.phases.at(-1).phase, "cancel");
  }
});

for (const raw of [false, true]) {
  for (const [button, mask] of [[1, 4], [2, 2]]) {
    test(`pen side button ${button} preserves the contact (raw=${raw})`, () => {
      const h = harness({ raw });
      const move = (overrides) => {
        if (raw) h.send("pointerrawupdate", overrides);
        h.send("pointermove", overrides);
      };
      h.send("pointerdown");
      move({ button, buttons: 1 | mask, clientX: 80 });
      move({ button: -1, buttons: 1 | mask, clientX: 100 });
      move({ button, buttons: 1, clientX: 120 });
      h.send("pointerup", { buttons: 0, pressure: 0 });
      assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 2, 2, 3]);
      assert.ok(h.phases.every(p => p.button === "primary"));
    });

    test(`pen tip can draw and lift while side button ${button} stays held (raw=${raw})`, () => {
      const h = harness({ raw });
      h.send("pointerdown", { button, buttons: mask, pressure: 0 });
      assert.equal(h.phases.length, 0, "hover button must not start navigation");
      for (const tip of [1, 32]) {
        h.send("pointermove", { button: tip === 1 ? 0 : 5, buttons: tip | mask });
        if (raw) h.send("pointerrawupdate", { button: -1, buttons: tip | mask, clientX: 100 });
        h.send("pointermove", { button: -1, buttons: tip | mask, clientX: 100 });
        if (raw) h.send("pointerrawupdate", { buttons: mask, pressure: 0 });
        h.send("pointermove", { button: tip === 1 ? 0 : 5, buttons: mask, pressure: 0 });
      }
      h.send("pointerup", { button, buttons: 0, pressure: 0 });
      assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 2, 3]);
      assert.deepEqual(h.samples().map(s => s[10]), [0, 0, 0, 2, 2, 2]);
    });
  }
}

test("tip lift wins over fallback pressure while a barrel button stays active", () => {
  const h = harness();
  h.send("pointerdown", { buttons: 3 });
  h.send("pointermove", { button: -1, buttons: 3, clientX: 100 });
  h.send("pointermove", { button: 0, buttons: 2, pressure: .5 });
  h.send("pointermove", { button: -1, buttons: 2, pressure: .5 });
  h.send("pointerup", { button: 2, buttons: 0, pressure: 0 });
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});

test("pen navigation also follows tip boundaries with a held barrel button", () => {
  const h = harness({ paint: false });
  h.send("pointerdown", { button: 2, buttons: 2, pressure: 0 });
  h.send("pointermove", { button: 0, buttons: 3 });
  h.send("pointermove", { button: -1, buttons: 3 });
  h.send("pointermove", { button: 0, buttons: 2, pressure: 0 });
  assert.deepEqual(h.phases.map(p => p.phase), ["down", "move", "up"]);
  assert.deepEqual(h.samples(), []);
});

for (const ending of ["pointerup", "pointercancel", "lostpointercapture", "pointerrawupdate", "pointermove"]) {
  test(`raw pen contact is not duplicated and survives ${ending}`, () => {
    const h = harness({ raw: true });
    h.send("pointerdown");
    h.send("pointerrawupdate", { clientX: 100 });
    h.send("pointermove", { clientX: 100 });
    h.send(ending, { clientX: 700, clientY: 500, buttons: 0, pressure: 0 });
    h.send("lostpointercapture", { buttons: 0, pressure: 0 });
    h.send("pointermove", { buttons: 0, pressure: 0 });
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
    h.send("pointerdown");
    // A contact without actual raw delivery still uses pointermove.
    h.send("pointermove", { clientX: 200 });
    h.send("pointerup");
    assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3, 1, 2, 3]);
  });
}
test("raw coalesced samples arrive once, with later native predictions from pointermove", () => {
  const h = harness({ raw: true, prediction: true });
  const sample = (x, timeStamp) => ({ pointerId: -7, pointerType: "pen", buttons: 1,
    clientX: x, clientY: 50, pressure: .6, timeStamp });
  h.send("pointerdown");
  const history = [sample(80, 2), sample(100, 3)];
  h.send("pointerrawupdate", { timeStamp: 3, clientX: 100, getCoalescedEvents: () => history });
  h.send("pointermove", { timeStamp: 3, clientX: 100, getCoalescedEvents: () => history,
    getPredictedEvents: () => [sample(90, 2), sample(120, 4)] });
  h.send("pointerup");
  const records = h.samples();
  assert.deepEqual(records.map(s => s[9]), [2, 2, 2, 3, 2]);
  assert.deepEqual(records.slice(1, 4).map(s => s[2]), [80, 100, 120]);
});
test("raw hover, foreign pointers, touch and mouse do not enter the pen stream", () => {
  const h = harness({ raw: true });
  h.send("pointerrawupdate");
  h.send("pointerdown");
  h.send("pointerrawupdate", { pointerId: 9 });
  h.send("pointerrawupdate", { pointerType: "touch" });
  h.send("pointerrawupdate", { pointerType: "mouse" });
  h.send("pointermove");
  h.send("pointerup");
  assert.deepEqual(h.samples().map(s => s[1]), [1, 2, 3]);
});
