import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { createCanvasBar, GAP, PADDING } from "./canvas-bar.js";

class FakeElement extends SharedElement {
  constructor(tag, className = "") {
    super();
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
  showPopover() { this.open = true; }
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

function action(id, label, { enabled = true, selected = false, checkable = false, accent = false } = {}) {
  return { option: { Action: { state: { id, icon: id, label, tooltip: `${label} tooltip`, enabled,
    disabled_reason: enabled ? null : `${id} is unavailable`, selected }, checkable } }, label, accent };
}
function view({ kind = "transform", generation = 1n, items, completion, placement = "near_object", label = null } = {}) {
  return {
    context: { generation, kind }, label, placement, anchor: [0, 0, 100, 100],
    items: items ?? [action("transform_uniform", "Uniform", { checkable: true }), action("transform_flip_horizontal", "Flip H"), action("reset_transform", "Reset")],
    completion: completion ?? [action("cancel_transform", "Cancel"), action("apply_transform", "Apply", { accent: true })],
  };
}
function harness({ layout = measure => ({ bounds: { x: 100.2, y: 50, width: 300, height: measure.height }, items: measure.items.length, side: "below" }) } = {}) {
  const workspace = new FakeElement("main"), dispatched = [], measures = [], menus = [], choices = [], timers = [], explained = [], presented = [];
  const menu = new FakeElement("div", "panel-context-menu");
  let glassQueued = 0, clock = 0;
  const element = (tag, className, text) => { const node = new FakeElement(tag, className); if (text != null) node.textContent = text; return node; };
  const button = (text, click, className = "") => { const node = element("button", className, text); node.addEventListener("click", click); return node; };
  const app = {
    canvas_bar_layout: measure => { measures.push(structuredClone(measure)); return layout(measure); },
    canvas_bar_menu: (context, shown) => { menus.push({ context, shown }); return { title: "More", sections: [] }; },
    canvas_bar_choice_menu: (context, id) => { choices.push({ context, id }); return choiceMenus[id] ? choiceMenus[id](context) : null; },
  };
  const bar = createCanvasBar({
    app, workspace, element, button, icon: name => element("svg", name), dispatch: action => dispatched.push(action),
    glass: { queue: () => glassQueued++ }, reappearMs: 180,
    openMenu: node => { menu.menuOwner = node; menu.model = node.menuModel(); menu.open = true; return menu; },
    explain: node => explained.push(node.dataset.command), presented: () => presented.push(clock),
    setTimer: (callback, ms) => { const timer = { id: timers.length + 1, callback, at: clock + ms, cleared: false }; timers.push(timer); return timer.id; },
    clearTimer: id => { const timer = timers.find(t => t.id === id); if (timer) timer.cleared = true; },
  });
  const advance = ms => {
    clock += ms;
    for (const timer of timers.filter(t => !t.cleared && !t.fired && t.at <= clock)) { timer.fired = true; timer.callback(); }
  };
  const pending = () => timers.filter(t => !t.cleared && !t.fired);
  const find = command => bar.root.querySelectorAll("button").find(b => b.dataset.command === command);
  const menuButton = id => bar.root.querySelectorAll("button").find(b => b.dataset.canvasBarMenu === id);
  return { bar, workspace, dispatched, measures, menus, choices, menu, advance, pending, find, menuButton, explained, presented, glass: () => glassQueued };
}
const edit = (context, action) => ({ type: "canvas_bar_edit", context, action });
const entry = (label, action, sections = []) => ({ label, selected: null, action, enabled: true, hint: "", bindings: [], sections });
const choiceMenus = {
  copy_to_layer: context => ({ title: "Copy to Layer", sections: [[
    entry("Copy Selection to New Layer", edit(context, { type: "invoke", command: "copy_selection_to_layer" })),
    entry("Cut Selection to New Layer", edit(context, { type: "invoke", command: "cut_selection_to_layer" }))]] }),
  adjust: context => ({ title: "Adjust", sections: [[
    entry("Color", null, [[entry("Curves", edit(context, { type: "effect", action: { type: "insert", filter: "curves" } }))]])]] }),
};
function menuItem(menu, label, icon, primary) {
  return { label, menu, icon, option: primary ?? { Choice: { id: menu, label, segmented: false, items: [] } } };
}
function selectionView({ generation = 1n, enabled = true } = {}) {
  const copy = action("copy_selection_to_layer", "Copy Selection to New Layer", { enabled }).option;
  return view({ kind: "selection", generation, completion: [], items: [
    action("deselect", "Deselect"),
    menuItem("copy_to_layer", "Copy to Layer", "copy-to-layer", copy),
    menuItem("adjust", "Adjust", "add-filter"),
  ] });
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
  assert.deepEqual(order, ["label", "transform_uniform", "transform_flip_horizontal", "reset_transform", "more", "cancel_transform", "apply_transform"]);
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
  const uniform = h.find("transform_uniform");
  assert.equal(uniform.getAttribute("aria-pressed"), "false");
  h.bar.refresh(view({ items: [action("transform_uniform", "Uniform", { checkable: true, selected: true }), action("transform_flip_horizontal", "Flip H", { enabled: false }), action("reset_transform", "Reset")] }));
  assert.equal(h.find("transform_uniform"), uniform, "Toggling retains the control");
  assert.equal(uniform.getAttribute("aria-pressed"), "true");
  assert.equal(h.find("transform_flip_horizontal").getAttribute("aria-disabled"), "true");
  assert.equal(h.find("transform_flip_horizontal").disabled, false, "Disabled items keep pointer events for their reason tooltip");
  assert.equal(h.find("transform_flip_horizontal").title, "transform_flip_horizontal is unavailable", "the title is the published reason");
  assert.equal(h.find("reset_transform").title, "Reset tooltip");
  assert.equal(h.measures.length, 2);
  h.bar.refresh(view());
  assert.equal(h.find("transform_flip_horizontal").title, "Flip H tooltip", "an enabled item returns to its tooltip");
  h.bar.refresh(view({ generation: 2n }));
  assert.notEqual(h.find("transform_uniform"), uniform, "A new context rebuilds the controls");
});

test("a mode bar shows its label and accented exit, and a relabelled item is rebuilt with its new label", () => {
  const h = harness();
  const mode = label => view({ kind: "layer_mask", label: "Editing Ink mask", placement: "bottom_edge",
    items: [action("invert_layer_mask", "Invert"), action("layer_mask_enabled", label)],
    completion: [action("edit_layer_content", "Edit Content", { accent: true })] });
  const caption = command => h.find(command).children.find(n => n.className === "toolbar-action-label")?.textContent;
  h.bar.refresh(mode("Disable"));
  const title = h.bar.root.children.find(n => n.className === "canvas-action-bar-label");
  assert.equal(title.hidden, false);
  assert.equal(title.textContent, "Editing Ink mask");
  assert.equal(caption("layer_mask_enabled"), "Disable");
  assert.ok(h.find("edit_layer_content").classList.contains("suggested-action"), "the exit uses the accent");
  const toggle = h.find("layer_mask_enabled");
  h.bar.refresh(mode("Enable"));
  assert.notEqual(h.find("layer_mask_enabled"), toggle, "a new item label rebuilds the controls");
  assert.equal(caption("layer_mask_enabled"), "Enable");
  h.find("layer_mask_enabled").click();
  assert.deepEqual(h.dispatched.at(-1), { type: "canvas_bar_edit", context: mode("Enable").context, action: { type: "invoke", command: "layer_mask_enabled" } });
});

test("every item dispatches a canvas bar edit for its context; disabled items explain themselves on tap", () => {
  const h = harness(), v = view({ items: [action("transform_flip_vertical", "Flip V", { enabled: false })] });
  h.bar.refresh(v);
  h.find("apply_transform").click();
  assert.deepEqual(h.dispatched, [{ type: "canvas_bar_edit", context: v.context, action: { type: "invoke", command: "apply_transform" } }]);
  assert.deepEqual(h.explained, []);
  h.find("transform_flip_vertical").click();
  assert.equal(h.dispatched.length, 1);
  assert.deepEqual(h.explained, ["transform_flip_vertical"], "a tap reveals the published reason");
  assert.equal(h.find("transform_flip_vertical").title, "transform_flip_vertical is unavailable");
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

test("dropdown choices list their items beside the bar and dispatch the chosen one", () => {
  globalThis.innerWidth ??= 1440; globalThis.innerHeight ??= 1000;
  const h = harness(), filters = ["Nearest", "Bilinear", "Bicubic"];
  const choice = { label: "Interpolation", option: { Choice: { id: "transform-interpolation", label: "Interpolation", segmented: false, items: filters.map((label, i) => ({
    label, icon: label.toLowerCase(), selected: i === 2, preview: null, action: { type: "invoke", command: `transform_${label.toLowerCase()}` } })) } } };
  const v = view({ items: [choice] });
  h.bar.refresh(v);
  const dropdown = h.bar.root.children.find(n => n.dataset.toolbarChoice === "transform-interpolation").children[0];
  assert.deepEqual(dropdown.children.map(c => c.textContent || c.className), ["bicubic", "Bicubic", "chevron-down"]);
  assert.equal(dropdown.tabIndex, -1);
  dropdown.click();
  const popup = h.bar.root.children.find(n => n.className.includes("toolbar-editor-popover"));
  assert.ok(popup?.open, "the dropdown opens its items");
  const entries = popup.children[0].children;
  assert.deepEqual(entries.map(e => e.getAttribute("aria-checked")), ["false", "false", "true"]);
  entries[0].click();
  assert.equal(popup.parentNode, null, "choosing closes the dropdown");
  assert.deepEqual(h.dispatched, [{ type: "canvas_bar_edit", context: v.context, action: { type: "invoke", command: "transform_nearest" } }]);
});

test("each new hold hides the bar at once and it returns once after the debounce", () => {
  const h = harness();
  h.bar.refresh(view());
  const queued = h.glass();
  h.bar.hold(1);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  assert.equal(h.bar.bounds(), null);
  assert.equal(h.glass(), queued + 1, "Hiding republishes glass");
  const presented = h.presented.length;
  for (let i = 0; i < 20; i++) h.bar.hold(1);
  assert.equal(h.glass(), queued + 1, "Samples during a stroke do not touch the DOM");
  assert.equal(h.pending().length, 0, "An odd hold keeps the bar hidden");
  h.bar.hold(0);
  h.bar.hold(0);
  assert.equal(h.pending().length, 1, "Hover replies do not restart the debounce");
  h.advance(179);
  assert.ok(h.bar.root.classList.contains("suppressed"));
  assert.equal(h.presented.length, presented, "Stroke samples do not re-place the notice");
  h.advance(1);
  assert.ok(!h.bar.root.classList.contains("suppressed"));
  assert.equal(h.presented.length, presented + 1, "Reappearing lets the notice move above the bar");
  assert.equal(h.measures.length, 2, "Reappearing re-places the bar");
  assert.ok(h.bar.bounds());
  h.bar.hold(2);
  h.advance(100); h.bar.hold(4); h.advance(100);
  assert.ok(h.bar.root.classList.contains("suppressed"), "Each camera change restarts the debounce");
  h.advance(80);
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
  h.bar.hold(1);
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

test("menu items are menu buttons with their icon and label that open the shared menu for the current context", () => {
  const h = harness(), v = selectionView();
  h.bar.refresh(v);
  const copy = h.menuButton("copy_to_layer"), adjust = h.menuButton("adjust");
  assert.deepEqual(copy.children.map(c => c.textContent || c.className), ["copy-to-layer", "Copy to Layer", "chevron-down"]);
  assert.deepEqual(adjust.children.map(c => c.textContent || c.className), ["add-filter", "Adjust", "chevron-down"]);
  assert.equal(copy.getAttribute("aria-label"), "Copy to Layer", "More lists an overflowed menu by the same label");
  assert.equal(copy.getAttribute("aria-haspopup"), "menu");
  assert.equal(copy.tabIndex, -1, "A menu button never joins the tab order");
  assert.equal(copy.dataset.command, undefined, "A menu button does not run its primary command");
  assert.equal(copy.title, "Copy Selection to New Layer tooltip", "An available primary command names itself");
  assert.equal(adjust.title, "Adjust");
  assert.equal(h.measures[0].items[1], 20 + 20 + 8 * "Copy to Layer".length + 20, "The fitter measures icon, label and arrow");
  copy.click();
  assert.deepEqual(h.choices, [{ context: v.context, id: "copy_to_layer" }]);
  assert.equal(h.menu.menuOwner, copy);
  assert.deepEqual(h.menu.model, choiceMenus.copy_to_layer(v.context), "The menu is the shared model, unchanged");
  assert.ok(h.bar.menuOpen());
  assert.deepEqual(h.dispatched, [], "Opening a menu runs nothing");
  h.menu.open = false;
  copy.dispatch("pointerdown"); copy.click();
  assert.equal(h.choices.length, 1, "A press on an open menu's button closes it");
  h.menu.dispatch("toggle", { newState: "closed" });
  assert.ok(!h.bar.menuOpen());
  adjust.dispatch("pointerdown"); adjust.click();
  assert.deepEqual(h.choices.at(-1), { context: v.context, id: "adjust" });
  assert.equal(h.menu.menuOwner, adjust);
  const [[category]] = h.menu.model.sections;
  assert.deepEqual(category.sections[0][0].action, edit(v.context, { type: "effect", action: { type: "insert", filter: "curves" } }), "Submenu entries keep their wrapped bar edit");
});

test("a menu whose primary command is disabled explains itself and opens nothing", () => {
  const h = harness();
  h.bar.refresh(selectionView({ enabled: false }));
  const copy = h.menuButton("copy_to_layer");
  assert.equal(copy.getAttribute("aria-disabled"), "true");
  assert.equal(copy.disabled, false, "Disabled menus keep pointer events for their reason tooltip");
  assert.equal(copy.title, "copy_selection_to_layer is unavailable");
  copy.click();
  assert.deepEqual(h.choices, []);
  assert.equal(h.explained.length, 1);
  h.bar.refresh(selectionView());
  assert.equal(h.menuButton("copy_to_layer"), copy, "Enabling the primary command retains the button");
  assert.equal(copy.getAttribute("aria-disabled"), "false");
  copy.click();
  assert.equal(h.choices.length, 1);
});

test("a new context closes an open bar menu, rebuilds its buttons and queries the new context", () => {
  const h = harness(), first = selectionView();
  h.bar.refresh(first);
  const copy = h.menuButton("copy_to_layer");
  copy.click();
  assert.ok(h.menu.open);
  const next = selectionView({ generation: 2n });
  h.bar.refresh(next);
  assert.equal(h.menu.open, false, "A menu for a previous selection closes");
  assert.ok(!h.bar.menuOpen());
  assert.notEqual(h.menuButton("copy_to_layer"), copy);
  h.menuButton("copy_to_layer").click();
  assert.deepEqual(h.choices.at(-1), { context: next.context, id: "copy_to_layer" });
  h.bar.refresh(view({ kind: "selection", generation: 3n, completion: [], items: [menuItem("clear", "Clear", "clear-selection")] }));
  h.menu.open = false;
  h.menuButton("clear").click();
  assert.deepEqual(h.choices.at(-1).id, "clear");
  assert.ok(!h.menu.open && !h.bar.menuOpen(), "A menu the core no longer serves opens nothing");
});
