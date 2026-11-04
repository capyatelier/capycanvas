import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { createNotice, TIMEOUT_MS } from "./notice.js";

class FakeElement extends SharedElement {
  constructor(tag, className = "") {
    super();
    this.tagName = tag.toUpperCase(); this.className = className; this.children = []; this.parentNode = null;
    this.attributes = new Map(); this.style = { setProperty: (name, value) => { this.style[name] = value; } }; this.hidden = false; this.listeners = {}; this.textContent = ""; this.width = 0;
    const names = new Set();
    this.classList = { toggle: (name, on) => on ? names.add(name) : names.delete(name), remove: name => names.delete(name), contains: name => names.has(name) };
  }
  getBoundingClientRect() { return { width: this.width }; }
  dispatch(type) {
    const event = { type, target: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; } };
    for (let node = this; node; node = node.parentNode) for (const listener of node.listeners[type] ?? []) listener(event);
    return event;
  }
  click() { this.dispatch("click"); }
}

const area = { x: 60, y: 48, width: 1000, height: 800 }, status = { x: 60, y: 820, width: 1000, height: 28 }, floor = status.y;
const placement = { x: 560, y: 808, max_width: 960, min_text_width: 960, window: false, bar_floor: floor - 72 };
const tiles = { gap: 2, size: [36, 36] };
const layout = { work_area: area, status, notice: placement };
function harness({ bar = null, arrangement = () => ({ beside: true }), above = () => placement } = {}) {
  const workspace = new FakeElement("main"), answers = [], timers = [], glass = { queued: 0, queue() { this.queued++; } };
  let clock = 0, barBounds = bar;
  const element = (tag, className, text) => { const node = new FakeElement(tag, className); if (text != null) node.textContent = text; return node; };
  const button = (text, click, className = "") => { const node = element("button", className, text); node.addEventListener("click", click); return node; };
  const notice = createNotice({
    arrangement, above,
    workspace, element, button, answer: (id, accept, action) => answers.push(action === undefined ? [id, accept] : [id, accept, action]),
    layout: () => layout, bar: () => barBounds, tiles, glass,
    setTimer: (callback, ms) => { const timer = { id: timers.length + 1, callback, at: clock + ms }; timers.push(timer); return timer.id; },
    clearTimer: id => { const timer = timers.find(t => t.id === id); if (timer) timer.cleared = true; },
  });
  const advance = ms => {
    clock += ms;
    for (const timer of timers.filter(t => !t.cleared && !t.fired && t.at <= clock)) { timer.fired = true; timer.callback(); }
  };
  const [text, actions] = notice.root.children;
  return { notice, workspace, answers, advance, text, actions, glass, get action() { return actions.children[0] ?? { hidden: true, click() {}, dispatch: () => ({}) }; },
    setBar: b => { barBounds = b; }, pending: () => timers.filter(t => !t.cleared && !t.fired) };
}
const refusal = id => ({ id, name: "Notice", text: "The active layer is locked", actions: [] });
const offer = id => ({ id, text: "This tool samples reference layers, and none is marked", actions: [{ id: "use_reference", label: "Use Photo as Reference", enabled: true, reason: null }] });
const objects = id => ({ id, text: "“Images” holds placed images, not paint.", actions: [
  { id: "add_mask", label: "Add Mask", enabled: true, reason: null },
  { id: "new_paint_layer", label: "New Paint Layer", enabled: true, reason: null },
  { id: "rasterize_layer", label: "Rasterize Layer", enabled: false, reason: "The active layer is locked" },
] });

test("a notice shows its text over the canvas without an action or a popover", () => {
  const h = harness();
  assert.equal(h.notice.root.parentNode, h.workspace);
  assert.equal(h.notice.root.hidden, true);
  assert.equal(h.notice.root.getAttribute("role"), "status");
  assert.equal(h.notice.root.popover, undefined, "never counts as an open popup");
  h.notice.publish(null);
  assert.equal(h.notice.root.hidden, true);
  h.notice.publish(refusal(1n));
  assert.equal(h.notice.root.hidden, false);
  assert.equal(h.text.textContent, "The active layer is locked");
  assert.equal(h.action.hidden, true);
  assert.equal(h.notice.root.style.transform, `translate(560px, ${placement.y}px) translate(-50%, -100%)`, "at the shared anchor");
  assert.equal(h.notice.root.style.maxWidth, `${placement.max_width}px`);
  assert.equal(h.notice.root.getAttribute("aria-label"), "Notice", "named by the shared notice name");
  assert.deepEqual(h.answers, []);
});

test("the action accepts its notice once, without taking focus", () => {
  const h = harness();
  h.notice.publish(offer(3n));
  assert.equal(h.actions.hidden, false);
  assert.equal(h.action.textContent, "Use Photo as Reference");
  assert.notEqual(h.action.tabIndex, -1, "Tab reaches the action");
  assert.equal(h.action.dispatch("mousedown").defaultPrevented, true, "a press does not move focus from the canvas");
  h.action.click();
  assert.equal(h.notice.root.hidden, true);
  assert.deepEqual(h.answers, [[3n, true, "use_reference"]]);
  h.action.click();
  h.advance(TIMEOUT_MS);
  assert.deepEqual(h.answers, [[3n, true, "use_reference"]], "neither a second tap nor the timeout answers again");
  h.notice.publish(offer(3n));
  assert.equal(h.notice.root.hidden, true, "the core clears an accepted notice; a republished id is not shown again");
});

test("a relocalized notice updates its text in place without restarting it", () => {
  const h = harness();
  h.notice.publish(offer(4n));
  const timers = h.pending().length;
  h.notice.publish({ id: 4n, text: "Dieses Werkzeug nutzt Referenzebenen", actions: [{ id: "use_reference", label: "Foto als Referenz verwenden", enabled: true, reason: null }] });
  assert.equal(h.text.textContent, "Dieses Werkzeug nutzt Referenzebenen");
  assert.equal(h.action.textContent, "Foto als Referenz verwenden");
  assert.equal(h.pending().length, timers, "the same notice keeps its timeout");
  assert.deepEqual(h.answers, []);
});

test("the timeout hides the notice and declines it", () => {
  const h = harness();
  h.notice.publish(offer(5n));
  h.advance(TIMEOUT_MS - 1);
  assert.equal(h.notice.root.hidden, false);
  assert.deepEqual(h.answers, []);
  h.advance(1);
  assert.equal(h.notice.root.hidden, true);
  assert.deepEqual(h.answers, [[5n, false]]);
  h.notice.publish(offer(5n));
  assert.equal(h.notice.root.hidden, true, "each id shows once");
  h.notice.publish(null);
  h.notice.publish(refusal(6n));
  assert.equal(h.notice.root.hidden, false, "a repeated refusal has a new id and shows again");
});

test("a canvas contact hides the notice without answering it", () => {
  const h = harness();
  h.notice.publish(offer(7n));
  h.notice.hide();
  assert.equal(h.notice.root.hidden, true);
  assert.equal(h.pending().length, 0);
  h.advance(TIMEOUT_MS * 2);
  assert.deepEqual(h.answers, [], "the core dismisses the notice at the same contact");
  h.notice.publish(offer(7n));
  assert.equal(h.notice.root.hidden, true, "a contact that raised nothing new keeps it hidden");
  h.notice.publish(refusal(8n));
  assert.equal(h.notice.root.hidden, false, "a notice raised by the contact shows");
});

test("a replacement notice restarts the timeout, and only the current id is answered", () => {
  const h = harness();
  h.notice.publish(offer(10n));
  h.advance(TIMEOUT_MS - 100);
  h.notice.publish(refusal(11n));
  assert.equal(h.text.textContent, "The active layer is locked");
  assert.equal(h.actions.hidden, true, "the replacement has no action");
  assert.equal(h.pending().length, 1);
  h.advance(TIMEOUT_MS - 1);
  assert.equal(h.notice.root.hidden, false);
  h.advance(1);
  assert.deepEqual(h.answers, [[11n, false]], "the replaced notice is never answered");
  h.notice.publish(offer(12n));
  h.notice.publish(offer(13n));
  h.action.click();
  assert.deepEqual(h.answers, [[11n, false], [13n, true, "use_reference"]]);
});

test("ordered actions keep their tokens, and a disabled choice shows its reason", () => {
  const h = harness();
  h.notice.publish(objects(20n));
  const [mask, paint, rasterize] = h.actions.children;
  assert.deepEqual([mask.textContent, paint.textContent, rasterize.textContent], ["Add Mask", "New Paint Layer", "Rasterize Layer"]);
  assert.equal(rasterize.disabled, true);
  assert.equal(rasterize.title, "The active layer is locked");
  assert.notEqual(paint.tabIndex, -1);
  paint.click();
  assert.deepEqual(h.answers, [[20n, true, "new_paint_layer"]]);
});

test("the notice forwards its shared placement and follows an updated canvas action bar", () => {
  const edge = { x: 400, y: 760, width: 300, height: 48 }, requests = [];
  const h = harness({ above: (notice, bar) => {
    requests.push([notice, bar]);
    return bar ? { ...placement, y: 748 } : placement;
  } });
  h.notice.publish(refusal(1n));
  assert.deepEqual(requests.at(-1), [placement, null]);
  h.setBar(edge);
  h.notice.place();
  assert.deepEqual(requests.at(-1), [placement, edge]);
  assert.equal(h.notice.root.style.transform, 'translate(560px, 748px) translate(-50%, -100%)');
  h.notice.hide();
  h.setBar(null);
  h.notice.place();
  assert.equal(requests.length, 2, "a hidden notice does not request placement");
  assert.equal(h.notice.root.style.transform, 'translate(560px, 748px) translate(-50%, -100%)');
});

test("text shares a row with the actions only on one line, and otherwise the actions stack below it", () => {
  const requests = [], replies = [true, true, false, true];
  const h = harness({ arrangement: request => { requests.push(request); return { beside: replies.shift() }; } });
  h.text.width = 300; h.actions.width = 240;
  h.notice.publish(objects(30n));
  assert.equal(h.notice.root.classList.contains("stacked"), false, "short text and actions share a row");
  h.text.width = placement.max_width - tiles.gap - h.actions.width;
  h.notice.place();
  assert.equal(h.notice.root.classList.contains("stacked"), false, "one line that fills the shared width beside the actions keeps the row");
  h.text.width += 1;
  h.notice.place();
  assert.equal(h.notice.root.classList.contains("stacked"), true, "text that would wrap beside the actions puts them under it");
  assert.equal(h.notice.root.style.maxWidth, `${placement.max_width}px`);
  h.text.width = 2000;
  h.notice.publish(refusal(31n));
  assert.equal(h.notice.root.classList.contains("stacked"), false, "text without actions wraps in its row");
  assert.deepEqual(requests.map(({ text, actions }) => ({ text, actions })), [{ text: 300, actions: 240 }, { text: 718, actions: 240 }, { text: 719, actions: 240 }, { text: 2000, actions: 0 }]);
  assert.ok(requests.every(request => request.type === 'notice_layout' && request.content === 960 && request.gap === 2 && request.min_text_width === 960));
  assert.deepEqual(h.answers, []);
});

test("the first layout of a new notice measures its actions", () => {
  const h = harness({ arrangement: () => ({ beside: false }) });
  h.text.width = placement.max_width - 100; h.actions.width = 240;
  h.notice.publish(objects(32n));
  assert.equal(h.notice.root.classList.contains("stacked"), true);
});

test("the notice is a Small canvas bar strip whose first action is the suggestion", () => {
  const h = harness();
  assert.equal(h.notice.root.style["--bar-tile"], "36px");
  assert.equal(h.notice.root.style["--bar-gap"], "2px");
  h.notice.publish(objects(40n));
  assert.deepEqual(h.actions.children.map(node => node.className), ["canvas-notice-action", "canvas-notice-action", "canvas-notice-action"]);
  h.notice.publish({ ...objects(41n), actions: objects(41n).actions.map((action, index) => ({ ...action, accent: index === 0 })) });
  assert.deepEqual(h.actions.children.map(node => node.className), ["canvas-notice-action suggested-action", "canvas-notice-action", "canvas-notice-action"]);
  assert.equal(h.notice.root.classList.contains("text-only"), false);
  h.notice.publish(refusal(42n));
  assert.equal(h.notice.root.classList.contains("text-only"), true, "a caption without actions is padded on both sides");
});

test("showing, moving and hiding the notice refreshes its glass", () => {
  const h = harness({ above: (notice, bar) => bar ? { ...notice, y: 748 } : notice });
  h.text.width = 300;
  h.notice.publish(refusal(50n));
  const shown = h.glass.queued;
  assert.ok(shown > 0, "a shown notice is measured");
  h.notice.place();
  assert.equal(h.glass.queued, shown, "an unchanged notice is not measured again");
  h.text.width = 400;
  h.notice.place();
  assert.equal(h.glass.queued, shown + 1, "a resized notice is measured again");
  h.setBar({ x: 400, y: 760, width: 300, height: 48 });
  h.notice.place();
  assert.equal(h.glass.queued, shown + 2, "a moved notice is measured again");
  h.notice.hide();
  assert.equal(h.glass.queued, shown + 3, "a hidden notice leaves the glass");
  h.notice.publish(refusal(51n));
  assert.equal(h.glass.queued, shown + 4, "the same notice shown again is measured again");
});

test("native notice measurements use the shared arrangement answer", () => {
  const requests = [];
  const h = harness({ arrangement: request => { requests.push(request); return { beside: false }; } });
  h.text.width = 100; h.actions.width = 80;
  h.notice.publish(objects(60n));
  assert.deepEqual(requests.at(-1), { type: "notice_layout", text: 100, content: 960, actions: 80, gap: 2, min_text_width: 960 });
  assert.equal(h.notice.root.classList.contains("stacked"), true);
  assert.equal(requests.length, 1);
  h.notice.publish(objects(61n));
  assert.equal(requests.length, 2, "a visible replacement makes one shared query");
  h.notice.publish(objects(61n));
  assert.equal(requests.length, 3, "an existing notice makes one shared query");
  h.notice.hide(); h.notice.place();
  assert.equal(requests.length, 3, "a hidden notice makes no shared query");
  assert.deepEqual(h.answers, []);
});
