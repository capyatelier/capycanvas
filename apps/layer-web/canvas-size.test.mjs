import assert from "node:assert/strict";
import test from "node:test";
import { createCanvasSizeUi } from "./canvas-size.js";

class FakeElement {
  constructor(tag, className = "") {
    this.tagName = tag.toUpperCase(); this.className = className; this.children = []; this.parentNode = null;
    this.attributes = new Map(); this.dataset = {}; this.listeners = {}; this.textContent = ""; this.disabled = false; this.open = false;
  }
  get classList() {
    const names = () => new Set(this.className.split(/\s+/).filter(Boolean));
    return {
      contains: name => names().has(name),
      toggle: (name, force) => { const set = names(); if (force ?? !set.has(name)) set.add(name); else set.delete(name); this.className = [...set].join(" "); },
    };
  }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(type, listener) { (this.listeners[type] ??= []).push(listener); }
  dispatch(type, extra = {}) {
    const event = { type, target: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...extra };
    for (let node = this; node; node = node.parentNode) for (const listener of node.listeners[type] ?? []) listener(event);
    return event;
  }
  append(...nodes) { for (const node of nodes) { node.parentNode = this; this.children.push(node); } }
  replaceWith(node) { const siblings = this.parentNode.children; node.parentNode = this.parentNode; siblings[siblings.indexOf(this)] = node; this.parentNode = null; }
  remove() { if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1); this.parentNode = null; }
  click() { if (!this.disabled) this.dispatch("click"); }
  showModal() { this.open = true; }
  close() { this.open = false; }
  find(test) {
    for (const child of this.children) { if (test(child)) return child; const found = child.find(test); if (found) return found; }
    return null;
  }
}

const numeric = unit => ({ kind: "number", min: 1, max: 30000, digits: unit === "%" ? 2 : 0, unit });
function view(overrides = {}) {
  return {
    title: "Canvas Size", labels: ["Width", "Height"], values: [800, 600], numeric: [numeric("px"), numeric("px")],
    unit: "pixels", units: [{ unit: "pixels", label: "Pixels" }, { unit: "percent", label: "Percent" }],
    relative: false, relative_label: "Relative", anchor: "center", anchor_label: "Anchor",
    anchors: ["top_left", "top", "top_right", "left", "center", "right", "bottom_left", "bottom", "bottom_right"]
      .map(anchor => ({ anchor, label: anchor.replace("_", " ") })),
    message: "Current size: 800 × 600 px", can_apply: false, ...overrides,
  };
}

function harness() {
  const body = new FakeElement("body"), sent = [], fields = [];
  const state = { layer_tools: { canvas_size: view() } };
  const element = (tag, className, text) => { const node = new FakeElement(tag, className); if (text != null) node.textContent = text; return node; };
  const button = (text, action, className = "") => { const node = element("button", className, text); node.addEventListener("click", action); return node; };
  // A field keeps typed text until it commits, like the real number control.
  const numberField = (control, label, onChange) => {
    const root = element("div", "number-control"), entry = element("input", "number-entry");
    Object.assign(root, { control, label, entry, value: null, pending: null });
    root.update = value => { root.value = value; };
    root.commit = () => {
      if (root.pending == null) return true;
      const value = Number(root.pending);
      if (!Number.isFinite(value)) return false;
      root.pending = null; if (value !== root.value) { root.value = value; onChange(value); }
      return true;
    };
    root.append(entry); fields.push(root); return root;
  };
  const resolve = ({ operation }) => { const value = Number(operation.text); if (!operation.text || !Number.isFinite(value)) throw Error("Enter a number"); return { value }; };
  let ui;
  const dispatch = action => { sent.push(action.action); ui.refresh(); };
  ui = createCanvasSizeUi({ state: () => state, element, button, icon: name => element("svg", name), numberField, resolve, dispatch, host: () => body });
  const dialog = () => ui.dialog();
  const find = test => dialog()?.find(test);
  return {
    ui, state, sent, body, fields, dialog,
    set(next) { state.layer_tools.canvas_size = next && view(next); ui.refresh(); },
    field: axis => find(n => n.dataset.canvasSize === axis),
    anchor: name => find(n => n.dataset.anchor === name),
    unit: () => find(n => n.className === "canvas-size-unit"),
    relative: () => find(n => n.tagName === "INPUT" && n.type === "checkbox"),
    apply: () => find(n => n.tagName === "BUTTON" && n.textContent === "Apply"),
    cancel: () => find(n => n.tagName === "BUTTON" && n.textContent === "Cancel"),
    message: () => find(n => n.className === "canvas-size-message"),
  };
}

test("the dialog projects the shared view and closes when the view is gone", () => {
  const h = harness();
  h.ui.refresh();
  assert.equal(h.dialog().open, true);
  assert.equal(h.dialog().parentNode, h.body);
  assert.equal(h.field("width").label, "Width");
  assert.equal(h.field("height").label, "Height");
  assert.deepEqual([h.field("width").value, h.field("height").value], [800, 600]);
  assert.deepEqual(h.unit().children.map(o => [o.value, o.textContent]), [["pixels", "Pixels"], ["percent", "Percent"]]);
  assert.equal(h.unit().value, "pixels");
  assert.equal(h.relative().checked, false);
  assert.equal(h.relative().parentNode.children[1].textContent, "Relative");
  assert.equal(h.message().textContent, "Current size: 800 × 600 px");
  const cells = h.dialog().find(n => n.className === "canvas-anchor").children;
  assert.deepEqual(cells.map(c => c.dataset.anchor), view().anchors.map(a => a.anchor), "nine anchors in reading order");
  assert.deepEqual(cells.map(c => c.getAttribute("aria-pressed")), cells.map(c => String(c.dataset.anchor === "center")));
  assert.ok(cells.every(c => c.getAttribute("aria-label") && c.title), "each anchor is labelled");
  const open = h.dialog();
  h.set(null);
  assert.equal(open.open, false);
  assert.equal(open.parentNode, null);
  assert.equal(h.dialog(), null);
});

test("Apply stays disabled while the view cannot apply", () => {
  const h = harness();
  h.ui.refresh();
  assert.equal(h.apply().disabled, true);
  h.apply().click();
  assert.deepEqual(h.sent, []);
  h.set({ values: [900, 600], message: "New size: 900 × 600 px", can_apply: true });
  assert.equal(h.apply().disabled, false);
  assert.equal(h.message().textContent, "New size: 900 × 600 px");
  h.apply().click();
  assert.deepEqual(h.sent, [{ op: "apply" }]);
  h.set({ values: [40000, 600], message: "The canvas can be at most 30000 px on each side", can_apply: false });
  assert.equal(h.apply().disabled, true);
  assert.equal(h.message().textContent, "The canvas can be at most 30000 px on each side");
});

test("typed text is committed before an anchor, unit, Relative or Apply action", () => {
  const h = harness();
  h.ui.refresh();
  h.field("width").pending = "1024";
  h.anchor("top_left").click();
  assert.deepEqual(h.sent, [{ op: "width", value: 1024 }, { op: "anchor", anchor: "top_left" }]);
  h.sent.length = 0;
  h.field("height").pending = "700";
  h.unit().value = "percent";
  h.unit().dispatch("change");
  assert.deepEqual(h.sent, [{ op: "height", value: 700 }, { op: "unit", unit: "percent" }], "the chosen unit survives the commit's refresh");
  h.sent.length = 0;
  h.field("width").pending = "900";
  h.relative().checked = true;
  h.relative().dispatch("change");
  assert.deepEqual(h.sent, [{ op: "width", value: 900 }, { op: "relative", relative: true }]);
  h.sent.length = 0;
  h.set({ values: [900, 700], can_apply: true });
  h.field("height").pending = "650";
  h.apply().click();
  assert.deepEqual(h.sent, [{ op: "height", value: 650 }, { op: "apply" }]);
  h.sent.length = 0;
  h.field("width").pending = "900*";
  h.anchor("bottom").click();
  h.apply().click();
  assert.deepEqual(h.sent, [], "text that does not evaluate keeps the dialog and sends nothing");
});

test("anchors never take focus", () => {
  const h = harness();
  h.ui.refresh();
  const grid = h.dialog().find(n => n.className === "canvas-anchor");
  assert.ok(grid.children.every(c => c.tabIndex === -1), "anchors are outside the tab order");
  assert.equal(h.anchor("right").dispatch("mousedown").defaultPrevented, true, "a press does not move focus");
  h.set({ anchor: "right" });
  assert.equal(h.anchor("right").getAttribute("aria-pressed"), "true");
  assert.equal(h.anchor("right").classList.contains("selected"), true);
  assert.equal(h.anchor("center").getAttribute("aria-pressed"), "false");
});

test("a typed value reaches the draft while typing so the message and Apply follow it", () => {
  const h = harness();
  h.ui.refresh();
  const width = h.field("width");
  width.entry.value = "12";
  width.entry.dispatch("input");
  width.entry.value = "12+";
  width.entry.dispatch("input");
  width.entry.value = "800";
  width.entry.dispatch("input");
  assert.deepEqual(h.sent, [{ op: "width", value: 12 }], "incomplete text and the unchanged size send nothing");
});

test("a new unit or Relative rebuilds each field for its numeric range", () => {
  const h = harness();
  h.ui.refresh();
  const before = h.field("width");
  h.set({ values: [800, 600] });
  assert.equal(h.field("width"), before, "the same spec keeps the field and its typing");
  h.set({ unit: "percent", values: [100, 100], numeric: [numeric("%"), numeric("%")] });
  assert.notEqual(h.field("width"), before);
  assert.equal(h.field("width").control.unit, "%");
  assert.equal(h.field("width").value, 100);
  assert.equal(h.unit().value, "percent");
  h.set({ unit: "percent", relative: true, values: [0, 0], numeric: [numeric("%"), numeric("%")] });
  assert.equal(h.relative().checked, true);
});

test("Cancel and the Escape request both cancel through the session", () => {
  const h = harness();
  h.ui.refresh();
  h.cancel().click();
  assert.equal(h.dialog().dispatch("cancel").defaultPrevented, true, "the session closes the dialog, not the browser");
  assert.deepEqual(h.sent, [{ op: "cancel" }, { op: "cancel" }]);
});
