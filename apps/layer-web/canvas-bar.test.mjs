import assert from "node:assert/strict";
import test from "node:test";
import { createCanvasBar, GAP, PADDING } from "./canvas-bar.js";

class FakeElement {
  constructor(tag, className = "") {
    this.tagName = tag.toUpperCase(); this.className = className; this.children = []; this.parentNode = null;
    this.attributes = new Map(); this.dataset = {}; this.style = {}; this.hidden = false; this.disabled = false;
    this.listeners = {}; this.textContent = ""; this.title = ""; this.reads = 0; this.open = false;
  }
  get classList() {
    const names = () => new Set(this.className.split(/\s+/).filter(Boolean));
    const write = set => { this.className = [...set].join(" "); };
    return {
      add: (...values) => { const set = names(); values.forEach(v => set.add(v)); write(set); },
      contains: value => names().has(value),
      toggle: (value, force) => { const set = names(); const on = force ?? !set.has(value); if (on) set.add(value); else set.delete(value); write(set); return on; },
    };
  }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(type, listener) { (this.listeners[type] ??= []).push(listener); }
  dispatch(type, event = {}) { for (const listener of this.listeners[type] ?? []) listener({ type, target: this, preventDefault() { this.defaultPrevented = true; }, ...event }); }
  append(...nodes) { for (const node of nodes) { node.remove?.(); node.parentNode = this; this.children.push(node); } }
  before(...nodes) {
    for (const node of nodes) {
      node.remove(); node.parentNode = this.parentNode;
      this.parentNode.children.splice(this.parentNode.children.indexOf(this), 0, node);
    }
  }
  remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); this.parentNode = null; }
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  querySelectorAll(selector) {
    const tag = selector.toUpperCase(), found = [];
    const walk = node => { for (const child of node.children) { if (child.tagName === tag) found.push(child); walk(child); } };
    walk(this); return found;
  }
  closest() { return null; }
  matches(selector) { return selector === ":popover-open" && this.open; }
  hidePopover() { this.open = false; }
  click() { this.dispatch("click"); }
  getBoundingClientRect() {
    this.reads++;
    const width = this.tagName === "SPAN" ? 8 * this.textContent.length
      : this.className.includes("canvas-action-bar-more") ? 40
        : this.children.reduce((sum, child) => sum + child.width(), 0);
    return { width, height: this.className.includes("toolbar-segments") ? 40 : 36 };
  }
  width() {
    if (this.tagName === "SPAN") return 8 * this.textContent.length;
    if (this.tagName === "SVG") return 20;
    if (this.tagName === "BUTTON") return 20 + this.children.reduce((sum, child) => sum + child.width(), 0);
    return this.children.reduce((sum, child) => sum + child.width(), 0);
  }
}

function action(id, label, { enabled = true, selected = false, checkable = false } = {}) {
  return { option: { Action: { state: { id, icon: id, label, tooltip: `${label} tooltip`, enabled, selected }, checkable } }, label };
}
function view({ kind = "transform", generation = 1n, items, completion, placement = "near_object", label = null } = {}) {
  return {
    context: { generation, kind }, label, placement, anchor: [0, 0, 100, 100],
    items: items ?? [action("transform_aspect", "Uniform", { checkable: true }), action("transform_flip_horizontal", "Flip H"), action("reset_transform", "Reset")],
    completion: completion ?? [action("cancel_transform", "Cancel"), action("apply_transform", "Apply")],
  };
}
function harness({ layout = measure => ({ bounds: { x: 100.2, y: 50, width: 300, height: measure.height }, items: measure.items.length, side: "below" }) } = {}) {
  const workspace = new FakeElement("main"), dispatched = [], measures = [], menus = [], timers = [];
  const menu = new FakeElement("div", "panel-context-menu");
  let glassQueued = 0, clock = 0;
  const element = (tag, className, text) => { const node = new FakeElement(tag, className); if (text != null) node.textContent = text; return node; };
  const button = (text, click, className = "") => { const node = element("button", className, text); node.addEventListener("click", click); return node; };
  const app = {
    canvas_bar_reappear_ms: () => 180,
    command_disabled_reason: id => `${id} is unavailable`,
    canvas_bar_layout: measure => { measures.push(structuredClone(measure)); return layout(measure); },
    canvas_bar_menu: (context, shown) => { menus.push({ context, shown }); return { title: "More", sections: [] }; },
  };
  const bar = createCanvasBar({
    app, workspace, element, button, icon: name => element("svg", name), dispatch: action => dispatched.push(action),
    glass: { queue: () => glassQueued++ },
    openMenu: node => { menu.menuOwner = node; menu.open = true; return menu; },
    setTimer: (callback, ms) => { const timer = { id: timers.length + 1, callback, at: clock + ms, cleared: false }; timers.push(timer); return timer.id; },
    clearTimer: id => { const timer = timers.find(t => t.id === id); if (timer) timer.cleared = true; },
  });
  const advance = ms => {
    clock += ms;
    for (const timer of timers.filter(t => !t.cleared && !t.fired && t.at <= clock)) { timer.fired = true; timer.callback(); }
  };
  const pending = () => timers.filter(t => !t.cleared && !t.fired);
  const find = command => bar.root.querySelectorAll("button").find(b => b.dataset.command === command);
  return { bar, workspace, dispatched, measures, menus, menu, advance, pending, find, glass: () => glassQueued };
}

test("the bar is a toolbar in the workspace that stays hidden without a view", () => {
  const h = harness();
  assert.equal(h.bar.root.parentNode, h.workspace);
  assert.equal(h.bar.root.getAttribute("role"), "toolbar");
  assert.equal(h.bar.root.getAttribute("aria-label"), "Canvas actions");
  assert.ok(!h.bar.root.classList.contains("chrome"), "Zen must keep the bar");
  h.bar.refresh(null);
  assert.equal(h.bar.root.hidden, true);
  assert.equal(h.bar.bounds(), null);
  assert.equal(h.measures.length, 0);
});

test("natural control sizes reach the shared fitter, with completion after More", () => {
  const h = harness(), v = view({ label: "2 images" });
  h.bar.refresh(v);
  const [measure] = h.measures;
  assert.deepEqual(measure.context, v.context);
  assert.equal(measure.label, 8 * "2 images".length);
  assert.deepEqual(measure.items, [20 + 20 + 8 * 7, 20 + 20 + 8 * 6, 20 + 20 + 8 * 5]);
  assert.deepEqual(measure.completion, [20 + 20 + 8 * 6, 20 + 20 + 8 * 5]);
  assert.equal(measure.more, 40);
  assert.deepEqual([measure.gap, measure.padding, measure.height], [GAP, PADDING, 36 + 2 * PADDING]);
  const order = h.bar.root.children.filter(n => !n.hidden).map(n => n.className.includes("more") ? "more" : n.children[0]?.dataset?.command ?? "label");
  assert.deepEqual(order, ["label", "transform_aspect", "transform_flip_horizontal", "reset_transform", "more", "cancel_transform", "apply_transform"]);
  assert.ok(h.find("apply_transform").classList.contains("suggested-action"));
  assert.ok(!h.find("cancel_transform").classList.contains("suggested-action"));
  assert.equal(h.bar.root.style.transform, "translate(100px, 50px)", "Moves align to device pixels");
  assert.deepEqual(h.bar.bounds(), { x: 100.2, y: 50, width: 300, height: 48 });
  assert.ok(h.glass() > 0, "Showing the bar republishes glass");
});

test("overflowed items hide, completion items never do, and placement reuses measurements", () => {
  let shown = 1;
  const h = harness({ layout: measure => ({ bounds: { x: 10, y: 20, width: 200, height: measure.height }, items: shown, side: "bottom_edge" }) });
  h.bar.refresh(view());
  const rows = h.bar.root.children.filter(n => n.classList.contains("canvas-action-bar-item"));
  assert.deepEqual(rows.map(r => r.hidden), [false, true, true]);
  assert.ok(h.bar.root.children.filter(n => n.classList.contains("canvas-action-bar-completion")).every(r => !r.hidden));
  const reads = h.find("reset_transform").parentNode.reads;
  shown = 3; h.bar.place();
  assert.deepEqual(rows.map(r => r.hidden), [false, false, false]);
  assert.equal(h.find("reset_transform").parentNode.reads, reads, "Moving the bar performs no DOM reads");
  assert.equal(h.measures.length, 2);
  assert.deepEqual(h.measures[1], h.measures[0]);
});

test("state changes update retained controls; schema changes rebuild them", () => {
  const h = harness();
  h.bar.refresh(view());
  const uniform = h.find("transform_aspect");
  assert.equal(uniform.getAttribute("aria-pressed"), "false");
  h.bar.refresh(view({ items: [action("transform_aspect", "Uniform", { checkable: true, selected: true }), action("transform_flip_horizontal", "Flip H", { enabled: false }), action("reset_transform", "Reset")] }));
  assert.equal(h.find("transform_aspect"), uniform, "Toggling retains the control");
  assert.equal(uniform.getAttribute("aria-pressed"), "true");
  assert.equal(h.find("transform_flip_horizontal").getAttribute("aria-disabled"), "true");
  assert.equal(h.find("transform_flip_horizontal").disabled, false, "Disabled items keep pointer events for their reason tooltip");
  assert.equal(h.find("transform_flip_horizontal").title, "transform_flip_horizontal is unavailable");
  assert.equal(h.find("reset_transform").title, "Reset tooltip");
  assert.equal(h.measures.length, 2);
  h.bar.refresh(view({ generation: 2n }));
  assert.notEqual(h.find("transform_aspect"), uniform, "A new context rebuilds the controls");
});

test("every item dispatches a canvas bar edit for its context; disabled items do nothing", () => {
  const h = harness(), v = view({ items: [action("transform_flip_vertical", "Flip V", { enabled: false })] });
  h.bar.refresh(v);
  h.find("apply_transform").click();
  assert.deepEqual(h.dispatched, [{ type: "canvas_bar_edit", context: v.context, action: { type: "invoke", command: "apply_transform" } }]);
  h.find("transform_flip_vertical").click();
  assert.equal(h.dispatched.length, 1);
  assert.ok(h.bar.root.querySelectorAll("button").every(b => b.tabIndex === -1), "Controls do not join the tab order");
});

test("segmented mode choices show icon and label and dispatch their own action", () => {
  const h = harness(), modes = ["Free", "Uniform", "Distort", "Warp"];
  const choice = { label: "Mode", option: { Choice: { id: "transform-mode", label: "Mode", segmented: true, items: modes.map((label, i) => ({
    label, icon: label.toLowerCase(), selected: i === 0, preview: null, action: { type: "invoke", command: `transform_${label.toLowerCase()}` } })) } } };
  const v = view({ items: [choice, action("reset_transform", "Reset")] });
  h.bar.refresh(v);
  const segments = h.bar.root.children.find(n => n.dataset.toolbarChoice === "transform-mode");
  assert.equal(segments.getAttribute("role"), "radiogroup");
  assert.deepEqual(segments.children.map(b => b.children.map(c => c.textContent || c.className)), modes.map(m => [m.toLowerCase(), m]));
  assert.deepEqual(segments.children.map(b => b.getAttribute("aria-checked")), ["true", "false", "false", "false"]);
  segments.children[3].click();
  assert.deepEqual(h.dispatched, [{ type: "canvas_bar_edit", context: v.context, action: { type: "invoke", command: "transform_warp" } }]);
  assert.equal(h.measures[0].items[0], 4 * 40 + 8 * modes.join("").length);
});

test("contacts hide the bar at once and it returns once after the debounce", () => {
  const h = harness();
  h.bar.refresh(view());
  const queued = h.glass();
  h.bar.suppress(true);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  assert.equal(h.bar.bounds(), null);
  assert.equal(h.glass(), queued + 1, "Hiding republishes glass");
  for (let i = 0; i < 20; i++) h.bar.suppress(true);
  assert.equal(h.glass(), queued + 1, "Samples during a stroke do not touch the DOM");
  h.bar.suppress(false);
  h.bar.suppress(false);
  assert.equal(h.pending().length, 1, "Hover replies do not restart the debounce");
  h.advance(179);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  h.advance(1);
  assert.ok(!h.bar.root.classList.contains("suppressed"));
  assert.equal(h.measures.length, 2, "Reappearing re-places the bar");
  assert.ok(h.bar.bounds());
});

test("camera changes defer near-object bars only, and not while a contact holds them", () => {
  const h = harness();
  h.bar.refresh(view());
  h.bar.defer();
  assert.ok(h.bar.root.classList.contains("suppressed"));
  h.advance(100); h.bar.defer(); h.advance(100);
  assert.ok(h.bar.root.classList.contains("suppressed"), "Each camera change restarts the debounce");
  h.advance(80);
  assert.ok(!h.bar.root.classList.contains("suppressed"));
  h.bar.suppress(true); h.bar.defer();
  assert.equal(h.pending().length, 0, "A pinch in progress keeps the bar hidden");
  h.bar.suppress(false); h.advance(180);
  assert.ok(!h.bar.root.classList.contains("suppressed"));
  h.bar.refresh(view({ placement: "bottom_edge" }));
  h.bar.defer();
  assert.ok(!h.bar.root.classList.contains("suppressed"), "Bottom-edge bars stay during navigation");
});

test("workspace drags hold the bar hidden until they end", () => {
  const h = harness();
  h.bar.refresh(view());
  h.bar.hold(true);
  h.bar.suppress(true); h.bar.suppress(false); h.advance(500);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  h.bar.hold(false); h.advance(180);
  assert.ok(!h.bar.root.classList.contains("suppressed"));
});

test("a stale context leaves the bar unplaced", () => {
  const h = harness({ layout: () => null });
  h.bar.refresh(view());
  assert.equal(h.bar.root.hidden, false);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  assert.equal(h.bar.bounds(), null);
});

test("More opens the shared menu for the shown count, toggles closed, and closes when the bar hides", () => {
  let shown = 2;
  const h = harness({ layout: measure => ({ bounds: { x: 0, y: 0, width: 100, height: measure.height }, items: BigInt(shown), side: "below" }) });
  const v = view();
  h.bar.refresh(v);
  const more = h.bar.root.children.find(n => n.className.includes("canvas-action-bar-more"));
  assert.equal(more.getAttribute("aria-haspopup"), "menu");
  more.click();
  assert.deepEqual(h.menus, [{ context: v.context, shown: 2 }]);
  assert.ok(h.bar.menuOpen());
  h.menu.open = false;
  more.dispatch("pointerdown"); more.click();
  assert.equal(h.menus.length, 1, "A press on More while its menu is open closes it");
  h.menu.dispatch("toggle", { newState: "closed" });
  more.dispatch("pointerdown"); more.click();
  assert.equal(h.menus.length, 2);
  assert.ok(h.menu.open);
  h.bar.suppress(true);
  assert.equal(h.menu.open, false, "Hiding the bar closes its menu");
});

test("the bar consumes its own context menu and never takes focus on press", () => {
  const h = harness();
  h.bar.refresh(view());
  const events = {};
  h.bar.root.dispatch("contextmenu", { preventDefault() { events.context = true; } });
  h.bar.root.dispatch("mousedown", { target: h.find("apply_transform"), preventDefault() { events.mouse = true; } });
  assert.deepEqual(events, { context: true, mouse: true });
});
