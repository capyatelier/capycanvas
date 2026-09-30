import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { createCanvasSizeUi } from "./canvas-size.js";
import { createImageSizeUi } from "./image-size.js";

class FakeElement extends SharedElement {
  constructor(tag, className = "") {
    super();
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
  dispatch(type, extra = {}) {
    const event = { type, target: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...extra };
    for (let node = this; node; node = node.parentNode) for (const listener of node.listeners[type] ?? []) listener(event);
    return event;
  }
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

const numeric = (unit, max = 30000) => ({ kind: "number", min: unit === "%" ? 0.01 : 1, max, digits: unit === "%" ? 2 : 0, unit });
const units = [{ unit: "pixels", label: "Pixels" }, { unit: "percent", label: "Percent" }];
function view(overrides = {}) {
  return {
    title: "Canvas Size", labels: ["Width", "Height"], values: [800, 600], numeric: [numeric("px"), numeric("px")],
    unit: "pixels", units,
    relative: false, relative_label: "Relative", anchor: "center", anchor_label: "Anchor",
    anchors: ["top_left", "top", "top_right", "left", "center", "right", "bottom_left", "bottom", "bottom_right"]
      .map(anchor => ({ anchor, label: anchor.replace("_", " ") })),
    message: "Current size: 800 × 600 px", can_apply: false, ...overrides,
  };
}
const resamples = [["automatic", "Automatic"], ["bicubic", "Bicubic"], ["lanczos", "Lanczos"], ["bilinear", "Bilinear"], ["nearest", "Nearest neighbor"]]
  .map(([resample, label]) => ({ resample, label }));
function imageView(overrides = {}) {
  return {
    title: "Image Size", labels: ["Width", "Height"], values: [800, 600], numeric: [numeric("px"), numeric("px")],
    unit: "pixels", units, resolution_label: "Resolution", resolution: 72, resolution_numeric: numeric("ppi", 10000),
    constrain: true, constrain_label: "Constrain proportions", resample: "automatic", resample_label: "Resample", resamples,
    message: "Current size: 800 × 600 px", can_apply: false, ...overrides,
  };
}

function harness({ create = createCanvasSizeUi, key = "canvas_size", data = "canvasSize", model = view } = {}) {
  const body = new FakeElement("body"), sent = [], fields = [];
  const state = { layer_tools: { [key]: model() } };
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
  const dispatch = action => { assert.equal(action.type, key); sent.push(action.action); ui.refresh(); };
  ui = create({ state: () => state, element, button, icon: name => element("svg", name), numberField, resolve, dispatch, host: () => body });
  const dialog = () => ui.dialog();
  const find = test => dialog()?.find(test);
  return {
    ui, state, sent, body, fields, dialog,
    set(next) { state.layer_tools[key] = next && model(next); ui.refresh(); },
    field: name => find(n => n.dataset[data] === name),
    anchor: name => find(n => n.dataset.anchor === name),
    select: label => find(n => n.className === "size-dialog-select" && n.getAttribute("aria-label") === label),
    unit: () => find(n => n.className === "size-dialog-select"),
    check: () => find(n => n.tagName === "INPUT" && n.type === "checkbox"),
    title: () => find(n => n.tagName === "H2"),
    apply: () => find(n => n.tagName === "BUTTON" && n.textContent === "Apply"),
    cancel: () => find(n => n.tagName === "BUTTON" && n.textContent === "Cancel"),
    message: () => find(n => n.className === "size-dialog-message"),
  };
}
const imageHarness = () => harness({ create: createImageSizeUi, key: "image_size", data: "imageSize", model: imageView });
const choose = (select, value) => { select.value = value; select.dispatch("change"); };

function applyTest(name, create, ready, invalid) {
  test(`${name} keeps Apply disabled while the view cannot apply`, () => {
    const h = create();
    h.ui.refresh();
    assert.equal(h.apply().disabled, true);
    h.apply().click();
    assert.deepEqual(h.sent, []);
    h.set(ready);
    assert.equal(h.apply().disabled, false);
    assert.equal(h.message().textContent, ready.message);
    h.apply().click();
    assert.deepEqual(h.sent, [{ op: "apply" }]);
    h.set(invalid);
    assert.equal(h.apply().disabled, true);
    assert.equal(h.message().textContent, invalid.message);
  });
}
function cancelTest(name, create) {
  test(`${name}'s Cancel and the Escape request both cancel through the session`, () => {
    const h = create();
    h.ui.refresh();
    h.cancel().click();
    assert.equal(h.dialog().dispatch("cancel").defaultPrevented, true, "the session closes the dialog, not the browser");
    assert.deepEqual(h.sent, [{ op: "cancel" }, { op: "cancel" }]);
  });
}

test("Canvas Size projects the shared view and closes when the view is gone", () => {
  const h = harness();
  h.ui.refresh();
  assert.equal(h.dialog().open, true);
  assert.equal(h.dialog().parentNode, h.body);
  assert.equal(h.field("width").label, "Width");
  assert.equal(h.field("height").label, "Height");
  assert.deepEqual([h.field("width").value, h.field("height").value], [800, 600]);
  assert.deepEqual(h.unit().children.map(o => [o.value, o.textContent]), [["pixels", "Pixels"], ["percent", "Percent"]]);
  assert.equal(h.unit().value, "pixels");
  assert.equal(h.check().checked, false);
  assert.equal(h.check().parentNode.children[1].textContent, "Relative");
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

applyTest("Canvas Size", harness,
  { values: [900, 600], message: "New size: 900 × 600 px", can_apply: true },
  { values: [40000, 600], message: "The canvas can be at most 30000 px on each side", can_apply: false });

test("Canvas Size commits typed text before an anchor, unit, Relative or Apply action", () => {
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
  h.check().checked = true;
  h.check().dispatch("change");
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

test("Canvas Size anchors never take focus", () => {
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

test("Canvas Size sends a typed value while typing so the message and Apply follow it", () => {
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

test("Canvas Size rebuilds each field for the numeric range of a new unit or Relative", () => {
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
  assert.equal(h.check().checked, true);
});

cancelTest("Canvas Size", harness);

test("Image Size projects the shared view, opens without taking the keyboard, and closes when the view is gone", () => {
  const h = imageHarness();
  h.ui.refresh();
  assert.equal(h.dialog().open, true);
  assert.equal(h.dialog().id, "image-size-dialog");
  assert.equal(h.title().textContent, "Image Size");
  assert.equal(h.title().autofocus, true, "the title, not a field, takes focus when the dialog opens");
  assert.equal(h.title().tabIndex, -1);
  assert.deepEqual(["width", "height", "resolution"].map(name => [h.field(name).label, h.field(name).value]),
    [["Width", 800], ["Height", 600], ["Resolution", 72]]);
  assert.equal(h.field("resolution").control.unit, "ppi");
  assert.deepEqual(h.select("Unit").children.map(o => [o.value, o.textContent]), [["pixels", "Pixels"], ["percent", "Percent"]]);
  assert.equal(h.check().checked, true);
  assert.equal(h.check().parentNode.children[1].textContent, "Constrain proportions");
  assert.deepEqual(h.select("Resample").children.map(o => o.textContent), ["Automatic", "Bicubic", "Lanczos", "Bilinear", "Nearest neighbor"]);
  assert.equal(h.select("Resample").value, "automatic");
  assert.equal(h.select("Resample").parentNode.children[0].textContent, "Resample");
  assert.equal(h.message().textContent, "Current size: 800 × 600 px");
  h.set({ resample: "lanczos", constrain: false });
  assert.equal(h.select("Resample").value, "lanczos");
  assert.equal(h.check().checked, false);
  const open = h.dialog();
  h.set(null);
  assert.equal(open.open, false);
  assert.equal(open.parentNode, null);
  assert.equal(h.dialog(), null);
});

applyTest("Image Size", imageHarness,
  { values: [400, 300], message: "New size: 400 × 300 px", can_apply: true },
  { values: [40000, 30000], message: "The canvas can be at most 32768 px on each side", can_apply: false });

test("Image Size commits typed text before a unit, Constrain, Resample or Apply action", () => {
  const h = imageHarness();
  h.ui.refresh();
  h.field("width").pending = "400";
  choose(h.select("Unit"), "percent");
  assert.deepEqual(h.sent, [{ op: "width", value: 400 }, { op: "unit", unit: "percent" }], "the chosen unit survives the commit's refresh");
  h.sent.length = 0;
  h.field("resolution").pending = "300";
  h.check().checked = false;
  h.check().dispatch("change");
  assert.deepEqual(h.sent, [{ op: "resolution", value: 300 }, { op: "constrain", constrain: false }]);
  h.sent.length = 0;
  h.field("height").pending = "250";
  choose(h.select("Resample"), "nearest");
  assert.deepEqual(h.sent, [{ op: "height", value: 250 }, { op: "resample", resample: "nearest" }]);
  h.sent.length = 0;
  h.set({ values: [400, 300], can_apply: true });
  h.field("width").pending = "500";
  h.apply().click();
  assert.deepEqual(h.sent, [{ op: "width", value: 500 }, { op: "apply" }]);
  h.sent.length = 0;
  h.field("resolution").pending = "300/";
  choose(h.select("Resample"), "bicubic");
  h.apply().click();
  assert.deepEqual(h.sent, [], "text that does not evaluate keeps the dialog and sends nothing");
});

test("Image Size sends each typed size and resolution as soon as it reads as a number", () => {
  const h = imageHarness();
  h.ui.refresh();
  for (const [name, texts] of [["width", ["4", "40", "40-", "400"]], ["resolution", ["3", "30", "300"]]]) {
    for (const text of texts) {
      h.field(name).entry.value = text;
      h.field(name).entry.dispatch("input");
    }
  }
  assert.deepEqual(h.sent, [4, 40, 400, 3, 30, 300].map((value, i) => ({ op: i < 3 ? "width" : "resolution", value })));
  h.sent.length = 0;
  h.field("height").entry.value = "600";
  h.field("height").entry.dispatch("input");
  assert.deepEqual(h.sent, [], "the unchanged height sends nothing");
});

test("Image Size rebuilds each field only when its numeric range changes", () => {
  const h = imageHarness();
  h.ui.refresh();
  const [width, resolution] = [h.field("width"), h.field("resolution")];
  h.set({ values: [400, 300], resolution: 300 });
  assert.equal(h.field("width"), width, "the same spec keeps the field and its typing");
  assert.equal(h.field("width").value, 400);
  assert.equal(h.field("resolution").value, 300);
  h.set({ unit: "percent", values: [50, 50], numeric: [numeric("%", 4096), numeric("%", 5461.33)] });
  assert.notEqual(h.field("width"), width);
  assert.equal(h.field("width").control.unit, "%");
  assert.equal(h.field("height").value, 50);
  assert.equal(h.field("resolution"), resolution, "the resolution field keeps its spec");
  assert.equal(h.select("Unit").value, "percent");
});

cancelTest("Image Size", imageHarness);
