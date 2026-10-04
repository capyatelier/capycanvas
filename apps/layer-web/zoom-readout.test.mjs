import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { createZoomReadout, readoutText, zoomMenuPlacement, MARGIN } from "./zoom-readout.js";

class FakeElement extends SharedElement {
  constructor(tag, className = "") {
    super();
    this.tagName = tag.toUpperCase(); this.className = className; this.children = []; this.parentNode = null;
    this.attributes = new Map(); this.style = {}; this.hidden = false; this.listeners = {}; this.textContent = "";
    this.dataset = {}; this.classList = { add: value => { this.className += ` ${value}`; } };
    this.open = false; this.isConnected = true; this.rect = { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 };
  }
  dispatch(type, event = {}) {
    const record = { type, target: this, defaultPrevented: false, propagationStopped: false,
      preventDefault() { this.defaultPrevented = true; }, stopPropagation() { this.propagationStopped = true; }, ...event };
    for (let node = this; node && !record.propagationStopped; node = node.parentNode)
      for (const listener of node.listeners[type] ?? []) listener(record);
    return record;
  }
  contains(node) { for (let n = node; n; n = n.parentNode) if (n === this) return true; return false; }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.tagName === selector.toUpperCase()) return n; return null; }
  matches(selector) { return selector === ":popover-open" && this.open; }
  showPopover() { this.open = true; this.dispatch("toggle", { newState: "open" }); }
  hidePopover() { this.open = false; this.dispatch("toggle", { newState: "closed" }); }
  getBoundingClientRect() { return this.rect; }
  focus() { document.activeElement = this; }
  click() { this.dispatch("pointerdown"); this.dispatch("mousedown"); this.dispatch("click"); }
}

const document = { body: new FakeElement("body"), activeElement: null };
const commands = ["zoom_out", "zoom_in", "rotate_left", "rotate_right", "flip_horizontal", "flip_vertical"];
const menuModel = zoomIn => ({ buttons: commands.map(id => ({ id, icon: id, label: id, tooltip: id, enabled: id !== "zoom_in" || zoomIn, selected: false })), title: "Zoom", rotation_section: 3n, sections: [
  [["Zoom in", { type: "invoke", command: "zoom_in" }, zoomIn], ["Actual Pixels", { type: "invoke", command: "actual_pixels" }, true]],
  [["50%", { type: "set_zoom", zoom: 0.5 }, true], ["200%", { type: "set_zoom", zoom: 2 }, true]],
  [["Lock zoom", { type: "set_zoom_locked", locked: true }, true]],
  [["Reset rotation", { type: "set_rotation", rotation: 0 }, true], ["Lock rotation", { type: "set_rotation_locked", locked: true }, true]],
].map(section => section.map(([label, action, enabled]) => ({ label, action, enabled, hint: "", selected: null, sections: [] }))) });

function harness() {
  const workspace = new FakeElement("main"), canvas = new FakeElement("canvas"), root = new FakeElement("button");
  const window = new FakeElement("window"), dispatched = [], rendered = [];
  workspace.append(canvas, root);
  let camera = { zoom: 0.5, rotation: Math.PI / 2 }, zoomIn = true;
  const element = (tag, className) => new FakeElement(tag, className);
  const numberField = (control, label, onChange, inline) => {
    label = typeof label === "function" ? label() : label;
    const field = element("div", "number-control");
    const entry = element("input", "number-entry"), valueButton = element("button", "number-value");
    field.append(valueButton, entry);
    Object.assign(field, { control, label, inline, entry, valueButton, values: [], onChange, update(value) { this.values.push(value); } });
    return field;
  };
  const renderMenu = (container, model, close) => {
    rendered.push(model);
    container.children = [];
    for (const item of model.sections.flat()) {
      const row = element("button", "menu-row"); row.textContent = item.label; row.disabled = !item.enabled;
      row.addEventListener("click", () => { close(); dispatched.push(item.action); });
      container.append(row);
    }
  };
  const readout = createZoomReadout({
    root, workspace, canvas, element, numberField,
    button: (text, action) => { const node = element("button"); node.addEventListener("click", action); return node; },
    icon: name => element("svg", name),
    catalog: { zoom: { kind: "slider", unit: "%" }, rotation: { kind: "slider", unit: "°" }, navigator_commands: commands,
      native_copy: { header: { zoom: "Zoom", rotation: "Rotation" } } },
    menu: () => menuModel(zoomIn), renderMenu, refreshMenu: renderMenu,
    dispatch: action => dispatched.push(action), camera: () => camera, doc: document, target: window,
    viewport: () => ({ width: 1440, height: 1000 }),
  });
  root.rect = { left: 1300, top: 950, right: 1400, bottom: 972, width: 100, height: 22 };
  readout.popup.rect = { width: 240, height: 300 };
  const row = label => [...readout.items.children, ...readout.rotationItems.children].find(n => n.textContent === label);
  const key = (node, name = "Escape") => { document.activeElement = node; return window.dispatch("keydown", { key: name, target: node }); };
  return { readout, root, canvas, window, dispatched, rendered, row, key,
    setCamera: next => { camera = next; }, setZoomIn: value => { zoomIn = value; } };
}

test("the readout shows zoom and rotation as a button that never takes focus", () => {
  const h = harness();
  h.readout.refresh({ zoom: 2, rotation: -Math.PI / 2 });
  assert.equal(h.root.textContent, "200% · -90°");
  assert.equal(readoutText({ zoom: 0.371, rotation: 0.3 }), "37% · 17°");
  assert.equal(h.root.type, "button");
  assert.equal(h.root.tabIndex, -1, "the readout stays out of the tab order");
  assert.equal(h.root.getAttribute("aria-haspopup"), "menu");
  assert.equal(h.root.dispatch("mousedown").defaultPrevented, true, "a press keeps focus on the canvas");
  assert.equal(h.readout.popup.popover, "auto");
  assert.equal(h.readout.field.label, "Zoom");
  assert.equal(h.readout.field.inline, true);
  assert.deepEqual(h.readout.field.control, { kind: "slider", unit: "%" }, "Rust describes the field");
});

test("opening shows the shared menu and the camera zoom above the readout", () => {
  const h = harness();
  h.canvas.focus();
  h.root.click();
  assert.equal(h.readout.open(), true);
  assert.equal(h.root.getAttribute("aria-expanded"), "true");
  const model = menuModel(true);
  assert.deepEqual(h.rendered.map(m => m.sections), [model.sections.slice(0, Number(model.rotation_section)), model.sections.slice(Number(model.rotation_section))]);
  const content = h.readout.popup.children.filter(n => n.tagName !== "HR");
  assert.deepEqual(content, [h.readout.field, h.readout.items, h.readout.rotation, h.readout.rotationItems, h.readout.controls]);
  assert.deepEqual(h.readout.field.values, [0.5], "the field starts at the camera's zoom");
  assert.deepEqual(h.readout.popup.style, { left: `${1400 - 240}px`, top: `${950 - 300 - MARGIN}px` });
  assert.equal(document.activeElement, h.canvas, "opening leaves focus on the canvas");
  assert.equal(h.readout.popup.dispatch("mousedown").defaultPrevented, true, "menu rows do not take focus");
  const entryPress = h.readout.field.entry.dispatch("mousedown");
  assert.equal(entryPress.defaultPrevented, false, "the typed field can take focus");
});

test("a menu level or command dispatches its shared action and closes", () => {
  const h = harness();
  h.canvas.focus();
  h.root.click();
  h.row("200%").click();
  assert.deepEqual(h.dispatched, [{ type: "set_zoom", zoom: 2 }]);
  assert.equal(h.readout.open(), false);
  assert.equal(document.activeElement, h.canvas);
  h.root.click();
  h.row("Actual Pixels").click();
  assert.deepEqual(h.dispatched.at(-1), { type: "invoke", command: "actual_pixels" });
});

test("the typed field sets the zoom, follows the camera and keeps the menu current", () => {
  const h = harness();
  h.canvas.focus();
  h.root.click();
  h.readout.field.onChange(0.5);
  assert.deepEqual(h.dispatched, [{ type: "set_zoom", zoom: 0.5 }]);
  h.setZoomIn(false);
  h.readout.refresh({ zoom: 16, rotation: 0 });
  assert.deepEqual(h.readout.field.values, [0.5, 16]);
  assert.equal(h.row("Zoom in").disabled, true, "the menu follows command availability while open");
  h.readout.close();
  h.readout.refresh({ zoom: 4, rotation: 0 });
  assert.deepEqual(h.readout.field.values, [0.5, 16], "a closed field does no work during navigation");
  assert.equal(h.root.textContent, "400% · 0°");
});

test("Escape closes the menu and hands focus back, but first cancels typing", () => {
  const h = harness();
  h.canvas.focus();
  h.root.click();
  const typing = h.key(h.readout.field.entry);
  assert.equal(typing.defaultPrevented, false, "the field handles its own Escape");
  assert.equal(h.readout.open(), true);
  const escape = h.key(h.readout.field.valueButton);
  assert.equal(escape.defaultPrevented, true);
  assert.equal(escape.propagationStopped, true, "the canvas shortcut does not also run");
  assert.equal(h.readout.open(), false);
  assert.equal(document.activeElement, h.canvas, "focus returns to the canvas");
  assert.equal(h.root.getAttribute("aria-expanded"), "false");
  assert.equal(h.key(h.canvas).defaultPrevented, false, "Escape passes through when the menu is closed");
});

test("pressing the readout again closes the open menu", () => {
  const h = harness();
  h.root.click();
  h.root.dispatch("pointerdown");
  h.readout.popup.hidePopover();
  h.root.dispatch("click");
  assert.equal(h.readout.open(), false, "the press that light-dismissed the menu does not reopen it");
  h.root.click();
  assert.equal(h.readout.open(), true);
});

test("the menu stays inside the viewport", () => {
  const size = { width: 240, height: 300 }, viewport = { width: 800, height: 600 };
  assert.deepEqual(zoomMenuPlacement({ left: 700, top: 560, right: 798, bottom: 580 }, size, viewport), { left: 800 - 240 - MARGIN, top: 560 - 300 - MARGIN });
  assert.deepEqual(zoomMenuPlacement({ left: 0, top: 40, right: 60, bottom: 60 }, size, viewport), { left: MARGIN, top: 60 + MARGIN });
});

test("rotation and navigation buttons follow the open menu and keep it open", () => {
  const h = harness();
  h.root.dispatch("contextmenu");
  assert.equal(h.readout.open(), true);
  assert.equal(h.readout.rotation.label, "Rotation");
  assert.equal(h.readout.rotation.inline, h.readout.field.inline);
  assert.deepEqual(h.readout.rotation.values, [Math.PI / 2]);
  h.readout.rotation.onChange(Math.PI / 4);
  h.readout.controls.children[3].click();
  assert.deepEqual(h.dispatched, [{ type: "set_rotation", rotation: Math.PI / 4 }, { type: "invoke", command: "rotate_right" }]);
  assert.equal(h.readout.open(), true);
  h.setZoomIn(false);
  h.readout.refresh({ zoom: 16, rotation: Math.PI / 4 });
  assert.deepEqual(h.readout.rotation.values, [Math.PI / 2, Math.PI / 4]);
  assert.equal(h.readout.controls.children[1].disabled, true);
  assert.equal(h.key(h.readout.rotation.entry).defaultPrevented, false);
  h.readout.close();
  h.readout.refresh({ zoom: 4, rotation: 0 });
  assert.deepEqual(h.readout.rotation.values, [Math.PI / 2, Math.PI / 4]);
});
