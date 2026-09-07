import init, { WebApp, WebGpu } from "./pkg/layer_web.js";
import { createPreferences } from "./preferences.js";
import { showGpuNotice } from "./gpu.js";

// Resolve assets beside the module, including in a versioned static package.
const asset = (path) => new URL(path, import.meta.url).href;

const panels = new Map(),
  groups = new Map(),
  dividers = new Map();
const $ = (id) =>
  document.getElementById(id) ||
  [...panels.values()]
    .map((panel) => panel.querySelector(`#${id}`))
    .find(Boolean);
const workspace = $("workspace"),
  canvas = $("canvas"),
  center = $("center");
const commands = new Map(),
  brushButtons = new Map(),
  sizeButtons = new Map();
let app,
  catalog,
  panelNames,
  state,
  scheduled = false,
  layout,
  lastPenEvent = null,
  chromeHeld = false,
  dragItem = null,
  statusTimer;
let refreshPreferences;
let gpuStarting = false;
let gpuReady = false;
let servicingRequests = false;
const settingsKey = "layer.preferences.v1";
const pending = [];
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
document.body.dataset.theme = systemTheme.matches ? "dark" : "light";

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function message(error) {
  $("status").textContent = String(error);
  clearTimeout(statusTimer);
  statusTimer = setTimeout(() => {
    $("status").textContent = "";
  }, 7000);
}
function button(text, action, className = "") {
  const node = element("button", className, text);
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}
// Native DOM inputs keep editing, keyboard and assistive-technology behavior.
// The adjacent buttons give number inputs the same compact layout as GTK spins.
function spin(input, digits) {
  const node = element("div", "spin");
  input.dataset.digits = digits;
  node.append(input);
  for (const [label, direction] of [
    ["Decrease", -1],
    ["Increase", 1],
  ]) {
    const step = () => {
      if (direction < 0) input.stepDown();
      else input.stepUp();
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    let timer,
      repeated = false;
    const control = button(direction < 0 ? "−" : "+", () => {
      if (!repeated) step();
      repeated = false;
    });
    control.replaceChildren(icon(direction < 0 ? "minus" : "plus"));
    const stop = () => clearTimeout(timer);
    control.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      repeated = false;
      control.setPointerCapture(e.pointerId);
      timer = setTimeout(function repeat() {
        repeated = true;
        step();
        timer = setTimeout(repeat, 50);
      }, 400);
    });
    for (const event of ["pointerup", "pointercancel", "lostpointercapture"])
      control.addEventListener(event, stop);
    window.addEventListener("blur", stop);
    control.setAttribute(
      "aria-label",
      `${label} ${input.getAttribute("aria-label")}`,
    );
    node.append(control);
  }
  input.addEventListener("blur", () => {
    if (input.value !== "") input.value = Number(input.value).toFixed(digits);
    syncSpin(input);
  });
  input.addEventListener("input", () => syncSpin(input));
  input.addEventListener("keydown", (e) => {
    // Enter commits the numeric edit, not the dialog's first submit button.
    if (e.key === "Enter") {
      e.preventDefault();
      input.dispatchEvent(new Event("input", { bubbles: true }));
    }
  });
  return node;
}
function setNumber(input, value) {
  if (document.activeElement !== input)
    input.value = value.toFixed(Number(input.dataset.digits));
  syncSpin(input);
}
function syncSpin(input) {
  const [decrease, increase] = input.parentElement.querySelectorAll("button");
  if (!decrease || !increase) return;
  decrease.disabled = input.disabled || input.value === "" || Number(input.value) <= Number(input.min);
  increase.disabled = input.disabled || input.value === "" || Number(input.value) >= Number(input.max);
}
function setRange(input, value) {
  input.value = value;
  const fraction =
    (Number(input.value) - Number(input.min)) /
    (Number(input.max) - Number(input.min));
  input.style.setProperty("--fill", `${fraction * 100}%`);
}
function numericControl(input, spec) {
  input.min = spec.min;
  input.max = spec.max;
  input.step = spec.step;
}
// Overlay scrollbars do not take width away from previews or tiles. Scrolling
// itself stays in the browser; this one thumb also supports pointer dragging.
function panelFrame(panel) {
  const frame = element("div", "panel-frame");
  const thumb = element("div", "scroll-thumb");
  thumb.setAttribute("aria-hidden", "true");
  frame.append(panel, thumb);
  let origin;
  const update = () => {
    if (!panel.clientHeight) {
      thumb.hidden = true;
      return;
    }
    const overflow = panel.scrollHeight - panel.clientHeight;
    thumb.hidden = overflow <= 1;
    const height = Math.max(28, panel.clientHeight ** 2 / panel.scrollHeight);
    thumb.style.height = `${height}px`;
    thumb.style.top = `${overflow > 0 ? (panel.scrollTop / overflow) * (panel.clientHeight - height) : 0}px`;
  };
  panel.addEventListener("scroll", update);
  new ResizeObserver(update).observe(panel);
  new MutationObserver(update).observe(panel, {
    childList: true,
    subtree: true,
  });
  thumb.addEventListener("pointerdown", (e) => {
    origin = [e.clientY, panel.scrollTop];
    thumb.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  thumb.addEventListener("pointermove", (e) => {
    if (thumb.hasPointerCapture(e.pointerId))
      panel.scrollTop =
        origin[1] +
        ((e.clientY - origin[0]) * (panel.scrollHeight - panel.clientHeight)) /
          (panel.clientHeight - thumb.offsetHeight);
  });
  return frame;
}
function commandButton(id, text) {
  const node = button(text, () => dispatch({ type: "invoke", command: id }));
  node.dataset.command = id;
  node.dataset.icon = String(text !== id && text.length <= 2);
  const list = commands.get(id) || [];
  list.push(node);
  commands.set(id, list);
  return node;
}
const icons = new Map();
async function loadIcons() {
  await Promise.all(
    catalog.icons.map(async (name) => {
      const response = await fetch(asset(`./icons/layer-${name}-symbolic.svg`));
      if (!response.ok) throw new Error(`Cannot load icon ${name}`);
      const svg = new DOMParser().parseFromString(
        await response.text(),
        "image/svg+xml",
      ).documentElement;
      svg.dataset.asset = name;
      svg.setAttribute("aria-hidden", "true");
      svg.setAttribute("focusable", "false");
      icons.set(name, svg);
    }),
  );
}
function icon(name) {
  return icons.get(name).cloneNode(true);
}
function iconButton(id) {
  const node = commandButton(id, "");
  node.dataset.icon = "true";
  node.classList.add("tile-button");
  node.append(icon(state.commands.find((c) => c.id === id).icon));
  return node;
}
function draggable(node, item) {
  node.draggable = true;
  node.addEventListener("dragstart", (e) => {
    e.dataTransfer.setData("text/layer-dock", JSON.stringify(item));
    e.dataTransfer.effectAllowed = "move";
    dragItem = item;
    updateZen();
  });
  node.addEventListener("dragend", () => {
    dragItem = null;
    dropIndicator.hidden = true;
    updateZen();
  });
  return node;
}
function grip(item) {
  const node = button("", () => {}, "panel-grip");
  node.title = "Drag to move panel";
  node.setAttribute(
    "aria-label",
    item.kind === "group" ? "Move all tabs" : "Move " + panelNames[item.panel],
  );
  node.append(icon("grip"));
  return draggable(node, item);
}
function tilePopover(name, label, control) {
  const node = button("", () => {}, "tile-button");
  node.append(icon(name));
  node.title = label;
  node.setAttribute("aria-label", label);
  const popover = element("div", "tile-popover");
  popover.popover = "auto";
  popover.append(element("label", "", label), control);
  workspace.append(popover);
  node.addEventListener("click", () => {
    for (const menu of document.querySelectorAll("details[open]"))
      menu.open = false;
    const b = node.getBoundingClientRect();
    Object.assign(popover.style, {
      left: Math.min(b.left, innerWidth - 220) + "px",
      top: Math.min(b.bottom + 6, innerHeight - 120) + "px",
    });
    popover.togglePopover();
  });
  popover.addEventListener("toggle", updateZen);
  return node;
}
const dropIndicator = element("div", "drop-indicator");
dropIndicator.hidden = true;
workspace.append(dropIndicator);

function dispatch(action) {
  try {
    if (action.type === "move_panel" || action.type === "move_group")
      action = {
        ...action,
        viewport: [workspace.clientWidth, workspace.clientHeight],
      };
    applyChange(app.dispatch(action));
  } catch (error) {
    message(error);
  }
}
function applyChange(change) {
  if (change.regions) {
    state = app.state();
    update(change.regions);
  }
  if (change.canvas_wake) wake();
  scheduleCursor();
}
let cursorScheduled = false;
function scheduleCursor() {
  if (cursorScheduled || !gpuReady) return;
  cursorScheduled = true;
  requestAnimationFrame(() => {
    cursorScheduled = false;
    const view = app.canvas_cursor();
    for (const kind of ["outline", "marker"])
      for (const node of document.querySelectorAll(
        `#canvas-cursor .cursor-${kind}-back, #canvas-cursor .cursor-${kind}-front`,
      ))
        node.setAttribute("d", view?.[kind] || "");
  });
}
function cursorInput(e) {
  if (!gpuReady) return;
  const onCanvas =
    e &&
    e.pointerType !== "touch" &&
    document.elementFromPoint(e.clientX, e.clientY) === canvas;
  app.cursor_input(
    onCanvas
      ? new Float64Array([
          ...position(e),
          e.pointerType === "pen" ? e.pressure : 1,
          ((e.tiltX || 0) * Math.PI) / 180,
          ((e.tiltY || 0) * Math.PI) / 180,
          ((e.twist || 0) * Math.PI) / 180,
          e.timeStamp,
        ])
      : new Float64Array(),
  );
  scheduleCursor();
}
function wake() {
  if (gpuReady && !scheduled) {
    scheduled = true;
    requestAnimationFrame(frame);
  }
}
function frame(now) {
  scheduled = false;
  try {
    while (pending.length) {
      const batch = pending[0],
        count = app.pen(batch.records, batch.revision);
      if (count * 11 === batch.records.length) pending.shift();
      else {
        batch.records = batch.records.subarray(count * 11);
        break;
      }
    }
    applyChange(app.frame(now, now + 1000 / 120));
    if (pending.length) wake();
  } catch (error) {
    message(error);
    console.error(error);
  }
}
function place(node, rect) {
  Object.assign(node.style, {
    left: `${rect.x}px`,
    top: `${rect.y}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  });
}
function arrange() {
  if (!app) return;
  layout = app.layout(workspace.clientWidth, workspace.clientHeight);
  workspace.style.setProperty("--tab-bar-height", `${layout.tab_bar_height}px`);
  const live = new Set();
  for (const group of layout.groups) {
    live.add(group.id);
    let node = groups.get(group.id);
    if (!node) {
      node = element("section", "dock-group");
      groups.set(group.id, node);
      workspace.append(node);
    }
    const key = `${group.panels.join(",")}:${group.active}`;
    if (node.dataset.key !== key) {
      node.dataset.key = key;
      node.dataset.panel = group.active;
      node.dataset.group = group.id;
      node.setAttribute("aria-label", panelNames[group.active]);
      node.className = `dock-group ${group.active === "toolbar" ? "toolbar" : ""}`;
      const tabs = element("nav", "dock-tabs");
      tabs.setAttribute("aria-label", "Panel tabs");
      if (group.tabs_visible) {
        const labels = element("div", "tab-list");
        group.panels.forEach((panel, index) => {
          const tab = button(
            panelNames[panel],
            () =>
              dispatch({ type: "select_panel_tab", group: group.id, panel }),
            "dock-tab",
          );
          tab.dataset.index = index;
          tab.setAttribute("aria-selected", String(panel === group.active));
          labels.append(draggable(tab, { kind: "panel", panel }));
        });
        tabs.append(labels, grip({ kind: "group", group: group.id }));
        node.replaceChildren(tabs, panels.get(group.active).parentElement);
      } else node.replaceChildren(panels.get(group.active).parentElement);
    }
    place(node, group.bounds);
    if (group.tiles) {
      const strip = node.querySelector(".toolbar-controls");
      const geometry = group.tiles;
      strip.dataset.axis = group.axis;
      const handle = strip.querySelector(".panel-grip");
      handle.hidden = !geometry.grip;
      if (geometry.grip) place(handle, geometry.grip);
      [...strip.querySelectorAll(":scope > .tile-button")].forEach((tile, i) =>
        place(tile, geometry.tiles[i]),
      );
    }
  }
  for (const [id, node] of groups)
    if (!live.has(id)) {
      node.remove();
      groups.delete(id);
    }
  const liveDividers = new Set();
  for (const divider of layout.dividers) {
    const key = `${divider.band}:${divider.id}`;
    liveDividers.add(key);
    let node = dividers.get(key);
    if (!node) {
      node = element("div", `divider ${divider.axis}`);
      node.tabIndex = 0;
      node.setAttribute("role", "separator");
      node.setAttribute("aria-label", "Resize dock");
      node.setAttribute(
        "aria-orientation",
        divider.axis === "horizontal" ? "vertical" : "horizontal",
      );
      node.addEventListener("pointerdown", (e) => {
        if (e.button !== 0) return;
        dragDivider(node.divider.id, "down", e);
        chromeHeld = true;
        updateZen();
        node.setPointerCapture(e.pointerId);
        e.preventDefault();
      });
      node.addEventListener("pointermove", (e) => {
        if (node.hasPointerCapture(e.pointerId))
          dragDivider(node.divider.id, "move", e);
      });
      node.addEventListener("lostpointercapture", () => {
        dragDivider(node.divider.id, "cancel");
        chromeHeld = false;
        updateZen();
      });
      node.addEventListener("keydown", (e) => {
        keyInput(e, true, node.divider.id);
      });
      dividers.set(key, node);
      workspace.append(node);
    }
    node.divider = divider;
    place(node, divider.bounds);
  }
  for (const [key, node] of dividers)
    if (!liveDividers.has(key)) {
      node.remove();
      dividers.delete(key);
    }
  place($("canvas-status"), layout.status);
  // The help occupies the same unobstructed area used for fitting the document.
  // Panels remain native UI siblings above the full-window drawing surface.
  place($("gpu-notice"), layout.work_area);
  resizeCanvas();
  updateZen();
}
function dragDivider(id, phase, e) {
  dispatch({
    type: "drag_divider",
    id,
    phase,
    position: e ? [e.clientX, e.clientY] : [0, 0],
    viewport: [workspace.clientWidth, workspace.clientHeight],
  });
}
function resizeCanvas() {
  const rect = canvas.getBoundingClientRect(),
    scale = devicePixelRatio || 1;
  const width = Math.max(1, Math.round(rect.width * scale)),
    height = Math.max(1, Math.round(rect.height * scale));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  applyChange(app.viewport(rect.width, rect.height, width, height));
}
function buildPanels() {
  for (const name of Object.keys(panelNames)) {
    const panel = element("div", `panel ${name}-panel`);
    panels.set(name, panel);
    panelFrame(panel);
  }
  const toolbar = element("div", "toolbar-controls");
  toolbar.append(grip({ kind: "panel", panel: "toolbar" }));
  const color = element("input");
  color.type = "color";
  color.id = "color";
  color.title = "Paint color";
  color.setAttribute("aria-label", "Paint color");
  color.addEventListener("input", () =>
    dispatch({
      type: "set_color",
      rgba: [1, 3, 5]
        .map((i) => parseInt(color.value.slice(i, i + 2), 16) / 255)
        .concat(1),
    }),
  );
  const opacity = element("input");
  opacity.type = "range";
  numericControl(opacity, catalog.opacity);
  opacity.id = "opacity";
  opacity.setAttribute("aria-label", "Brush opacity");
  opacity.addEventListener("input", () =>
    dispatch({ type: "set_brush_opacity", value: Number(opacity.value) }),
  );
  for (const item of catalog.toolbar) {
    switch (item.kind) {
      case "command":
        toolbar.append(iconButton(item.command));
        break;
      case "color":
        toolbar.append(tilePopover("color", "Brush color", color));
        break;
      case "opacity":
        toolbar.append(tilePopover("opacity", "Brush opacity", opacity));
        break;
      default:
        throw new Error(`Unsupported toolbar control: ${item.kind}`);
    }
  }
  panels.get("toolbar").classList.add("tile-panel");
  panels.get("toolbar").append(toolbar);
  const list = element("div", "brush-list");
  for (const { label: category, brushes } of catalog.brush_categories) {
    list.append(element("h3", "", category));
    for (const brush of brushes) {
      const choice = button(
        "",
        () => dispatch({ type: "select_brush", id: brush.id }),
        "brush-choice",
      );
      choice.dataset.brush = brush.id;
      choice.dataset.category = category;
      const preview = element("img", "brush-preview");
      preview.src = asset(`brush-previews/${brush.id}-${state.theme}.png`);
      preview.alt = "";
      preview.draggable = false;
      choice.append(preview, element("span", "", brush.label));
      brushButtons.set(brush.id, choice);
      list.append(choice);
    }
  }
  panels.get("brushes").append(list);
  const controls = element("div", "size-controls");
  for (const type of ["range", "number"]) {
    const input = element("input");
    input.id = `size-${type}`;
    input.type = type;
    numericControl(
      input,
      type === "range" ? catalog.brush_size_slider : catalog.brush_size,
    );
    input.setAttribute("aria-label", "Brush diameter in pixels");
    input.addEventListener("input", () => {
      if (input.value !== "" && input.validity.valid)
        dispatch({ type: "set_brush_size", value: Number(input.value) });
    });
    input.addEventListener("blur", () =>
      setNumber(input, state.brush.diameter),
    );
    controls.append(
      type === "number" ? spin(input, catalog.brush_size.digits) : input,
    );
  }
  const grid = element("div", "size-grid");
  for (const value of catalog.brush_sizes) {
    const choice = button(
      "",
      () => dispatch({ type: "set_brush_size", value }),
      "size-button",
    );
    choice.title = `${value} px`;
    choice.dataset.size = value;
    const dot = element("span", "size-dot");
    dot.style.width =
      dot.style.height = `${Math.min(27, 2 + Math.sqrt(value) * 1.2)}px`;
    const glyph = element("span", "size-glyph");
    glyph.append(dot);
    choice.append(glyph, element("span", "", String(value)));
    const cell = element("div", "size-cell");
    cell.append(choice);
    grid.append(cell);
    sizeButtons.set(value, choice);
  }
  panels.get("sizes").append(controls, grid);
  const layers = element("div", "layers-content");
  const tools = element("div", "layer-tools");
  for (const id of catalog.layer_commands) tools.append(iconButton(id));
  const rows = element("div", "layer-rows");
  rows.id = "layer-rows";
  const label = element("label", "", "Layer opacity");
  label.htmlFor = "layer-opacity";
  const layerOpacity = element("input");
  layerOpacity.id = "layer-opacity";
  layerOpacity.type = "range";
  numericControl(layerOpacity, catalog.opacity);
  layerOpacity.addEventListener("input", () =>
    dispatch({
      type: "set_layer_opacity",
      opacity: Number(layerOpacity.value),
    }),
  );
  layers.append(tools, rows, label, layerOpacity);
  panels.get("layers").append(layers);
}
function update(regions) {
  if (regions & 1) arrange();
  if (regions & 2) {
    for (const [id, button] of brushButtons)
      button.setAttribute("aria-pressed", String(id === state.brush.preset));
    for (const [size, button] of sizeButtons)
      button.setAttribute(
        "aria-pressed",
        String(size === state.brush.diameter),
      );
    setRange($("size-range"), state.brush.diameter);
    setNumber($("size-number"), state.brush.diameter);
    setRange($("opacity"), state.brush.opacity);
    $("color").value = `#${state.brush.color
      .slice(0, 3)
      .map((v) =>
        Math.round(v * 255)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`;
    workspace.style.setProperty("--paint-color", $("color").value);
  }
  if (regions & 4) {
    const tab = state.tabs[0];
    $("document-title").textContent =
      `${tab.title} · ${tab.width} × ${tab.height}`;
    const container = $("layer-rows");
    const key = JSON.stringify(
      state.layers.map(({ id, label, editable }) => [
        String(id),
        label,
        editable,
      ]),
    );
    if (container.dataset.key !== key) {
      container.dataset.key = key;
      container.replaceChildren(
        ...state.layers.map((layer) => {
          const row = element(
            "div",
            `layer-row ${layer.selected ? "selected" : ""}`,
          );
          row.dataset.layer = String(layer.id);
          const visible = element("input");
          visible.type = "checkbox";
          visible.checked = layer.visible;
          visible.setAttribute("aria-label", `Show ${layer.label}`);
          visible.addEventListener("change", () =>
            dispatch({
              type: "set_layer_visibility",
              id: layer.id,
              visible: visible.checked,
            }),
          );
          const select = button(
            "",
            () => dispatch({ type: "select_layer", id: layer.id }),
            "layer-select",
          );
          select.disabled = !layer.editable;
          select.textContent = layer.label;
          row.append(visible, select);
          return row;
        }),
      );
    }
    for (const [index, layer] of state.layers.entries()) {
      const row = container.children[index];
      row.classList.toggle("selected", layer.selected);
      row.querySelector("input").checked = layer.visible;
      row
        .querySelector("button")
        .setAttribute("aria-pressed", String(layer.selected));
    }
    setRange($("layer-opacity"), state.layers.find((l) => l.selected).opacity);
  }
  if (regions & (4 | 8))
    for (const command of state.commands)
      for (const node of commands.get(command.id) || []) {
        node.disabled = !command.enabled;
        node.title = command.label;
        node.setAttribute("aria-label", command.label);
        node.setAttribute("aria-pressed", String(command.selected));
        if (node.dataset.icon !== "true") {
          if (node.querySelector(".command-label")) {
            node.querySelector(".command-label").textContent = command.label;
            node.querySelector(".shortcut-hint").textContent = command.shortcut;
          } else node.textContent = command.label;
        }
      }
  if (regions & 16) {
    document.body.dataset.theme = state.theme;
    for (const [id, button] of brushButtons)
      button.querySelector("img").src =
        asset(`brush-previews/${id}-${state.theme}.png`);
    refreshPreferences(app.preferences());
  }
  if (regions & 32)
    $("view-info").textContent =
      `${Math.round(state.camera.zoom * 100)}% · ${Math.round((state.camera.rotation * 180) / Math.PI)}°`;
  if (regions & (1 | 16)) updateZen();
  if (regions & 64) {
    if (state.host_error) message(state.host_error);
    // Small applied-settings snapshots only, never per-input/frame writes.
    if (!servicingRequests) {
      servicingRequests = true;
      try {
        for (const request of state.requests) {
          let error = null;
          try {
            if (request.kind.type !== "save_settings") throw new Error("Unsupported host request");
            localStorage.setItem(settingsKey, JSON.stringify(request.kind.settings));
          } catch (e) { error = `Cannot save preferences: ${e}`; }
          dispatch({ type: "complete_request", id: request.id, error });
        }
      } finally { servicingRequests = false; }
    }
  }
}

// Rust owns transient interaction policy. These are only DOM event/capture
// records; CSS animates the returned visibility without resizing the canvas.
let revealPointer = null;
function input(event) {
  if (!app) return {};
  try {
    const reply = app.input(event);
    workspace.classList.toggle("zen-hidden", reply.chrome_hidden);
    canvas.style.cursor = reply.pan_cursor ? "grab" : "";
    if (reply.dismiss_popups) {
      for (const popup of document.querySelectorAll(
        "details[open], :popover-open",
      )) {
        if (popup.matches("details")) popup.open = false;
        else popup.hidePopover();
      }
    }
    applyChange(reply.change);
    return reply;
  } catch (error) {
    message(error);
    return {};
  }
}
function chromeInput(event) {
  return input({
    type: "chrome",
    event,
    viewport: [workspace.clientWidth, workspace.clientHeight],
    facts: {
      held: chromeHeld,
      dragging: dragItem !== null,
      popup_open:
        !!document.querySelector("details[open], :popover-open") ||
        !!document.activeElement?.matches("select"),
    },
  });
}
function updateZen() {
  chromeInput({ kind: "refresh" });
}
function buildHeader() {
  function menu(label, ids, icon) {
    const details = element("details", "header-menu");
    details.name = "workspace-menu";
    const summary = element("summary", "", icon || label);
    summary.setAttribute("aria-label", label);
    summary.title = label;
    summary.addEventListener("click", () => {
      for (const popup of document.querySelectorAll(":popover-open"))
        popup.hidePopover();
    });
    const contents = element("div", "popover");
    for (const id of ids) {
      const control = commandButton(id, id);
      control.replaceChildren(element("span", "command-label"), element("span", "shortcut-hint"));
      control.addEventListener("click", () => {
        details.open = false;
        updateZen();
      });
      contents.append(control);
    }
    details.append(summary, contents);
    details.addEventListener("toggle", updateZen);
    return details;
  }
  $("header-start").append(iconButton("zen_mode"));
  for (const spec of catalog.menus)
    $("header-start").append(menu(spec.label, spec.commands));
  $("header-end").append(iconButton("settings"));
}
window.addEventListener(
  "pointermove",
  (e) => {
    if (!e.buttons) chromeHeld = false;
    chromeInput({ kind: "motion", position: [e.clientX, e.clientY] });
    cursorInput(e);
  },
  { capture: true },
);
document.addEventListener("pointerleave", (e) => {
  cursorInput(null);
  chromeInput({ kind: "leave", touch: e.pointerType === "touch" });
});
window.addEventListener(
  "pointerdown",
  (e) => {
    revealPointer = null;
    if (e.target.closest("#header")) chromeHeld = true;
    const reply = chromeInput({
      kind: "contact",
      position: [e.clientX, e.clientY],
      canvas: e.target === canvas,
    });
    if (reply.handled) {
      revealPointer = e.pointerId;
      e.preventDefault();
      e.stopImmediatePropagation();
    }
    for (const menu of document.querySelectorAll("details[open]"))
      if (!menu.contains(e.target)) menu.open = false;
  },
  { capture: true },
);
window.addEventListener(
  "click",
  (e) => {
    // A reveal/dismiss contact must not activate a newly uncovered control.
    if (revealPointer === e.pointerId) {
      revealPointer = null;
      e.preventDefault();
      e.stopImmediatePropagation();
    }
  },
  { capture: true },
);
window.addEventListener(
  "pointercancel",
  () => {
    revealPointer = null;
    chromeHeld = false;
    updateZen();
  },
  { capture: true },
);
window.addEventListener(
  "pointerup",
  () => {
    chromeHeld = false;
    updateZen();
  },
  { capture: true },
);
window.addEventListener("focusout", () => requestAnimationFrame(updateZen));
function position(e) {
  const rect = canvas.getBoundingClientRect();
  return [
    ((e.clientX - rect.left) * canvas.width) / rect.width,
    ((e.clientY - rect.top) * canvas.height) / rect.height,
  ];
}
function queuePen(e, stage) {
  lastPenEvent = stage === 3 || stage === 4 ? null : e;
  const records = [];
  const history = stage === 2 ? e.getCoalescedEvents?.() || [] : [];
  for (const item of history.length ? history : [e]) {
    const [x, y] = position(item),
      pen = item.pointerType === "pen";
    records.push(
      item.pointerId,
      stage,
      x,
      y,
      pen ? item.pressure : 1,
      ((item.tiltX || 0) * Math.PI) / 180,
      ((item.tiltY || 0) * Math.PI) / 180,
      ((item.twist || 0) * Math.PI) / 180,
      item.timeStamp,
      2,
      pen ? (item.buttons & 32 ? 2 : 0) : 1,
    );
  }
  const batch = {
    records: new Float64Array(records),
    revision: state.camera.revision,
  };
  // Capture the transform when input arrives, even across later viewport resize.
  if (!pending.length) {
    const count = app.pen(batch.records, batch.revision);
    batch.records = batch.records.subarray(count * 11);
  }
  if (batch.records.length) pending.push(batch);
  wake();
}
for (const [name, stage] of [
  ["pointerdown", 1],
  ["pointermove", 2],
  ["pointerup", 3],
  ["pointercancel", 4],
])
  canvas.addEventListener(name, (e) => {
    e.preventDefault();
    const point = position(e);
    if (stage === 1) {
      canvas.focus();
      canvas.setPointerCapture(e.pointerId);
    }
    const reply = pointerInput(e, stage, point);
    if (reply.paint) queuePen(e, stage);
    cursorInput(stage === 4 ? null : e);
  });
canvas.addEventListener("lostpointercapture", (e) => {
  if (pointerInput(e, 4).paint) queuePen(lastPenEvent || e, 4);
});
function pointerInput(e, stage, point = position(e)) {
  return input({
    type: "pointer",
    id: BigInt(e.pointerId),
    phase: ["move", "down", "move", "up", "cancel"][stage],
    kind: e.pointerType || "mouse",
    button:
      e.button === 0 || e.button === 5
        ? "primary"
        : e.button === 1 || e.button === 2
          ? "pan"
          : "other",
    position: point,
  });
}
canvas.addEventListener("contextmenu", (e) => e.preventDefault());
canvas.addEventListener(
  "wheel",
  (e) => {
    e.preventDefault();
    const point = position(e),
      unit =
        e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? canvas.clientHeight : 1;
    try {
      applyChange(
        app.scroll(
          ...point,
          e.deltaX * unit,
          e.deltaY * unit,
          canvas.width / canvas.clientWidth,
          (e.ctrlKey ? 1 : 0) | (e.shiftKey ? 2 : 0),
        ),
      );
    } catch (error) {
      message(error);
    }
  },
  { passive: false },
);
function keyInput(e, pressed, divider = null) {
  updateZen();
  const reply = input({
    type: "key",
    key: e.key,
    pressed,
    repeat: e.repeat,
    modifiers: {
      command: e.ctrlKey || e.metaKey,
      shift: e.shiftKey,
      alt: e.altKey,
    },
    editing:
      e.isComposing ||
      (e.target instanceof Element &&
        e.target.matches("input,select,textarea,[contenteditable=true]")),
    divider,
  });
  if (reply.handled) {
    e.preventDefault();
    e.stopPropagation();
  }
}
window.addEventListener("keydown", (e) => keyInput(e, true));
window.addEventListener("keyup", (e) => keyInput(e, false));
window.addEventListener("blur", () => {
  cursorInput(null);
  chromeHeld = false;
  if (input({ type: "blur" }).cancel_paint && lastPenEvent)
    queuePen(lastPenEvent, 4);
});
function dropHint(e, item) {
  const tabs = [...groups.entries()].flatMap(([group, node]) =>
    [...node.querySelectorAll(".dock-tab")].map((tab) => {
      const b = tab.getBoundingClientRect();
      return {
        group,
        index: Number(tab.dataset.index),
        bounds: { x: b.x, y: b.y, width: b.width, height: b.height },
      };
    }),
  );
  try {
    return app.drop_hint(
      workspace.clientWidth,
      workspace.clientHeight,
      e.clientX,
      e.clientY,
      tabs,
      item,
    );
  } catch {
    return null;
  } // Invalid/foreign payloads have no accepted core target.
}
function draggedItem(e) {
  if (dragItem) return dragItem;
  try {
    return JSON.parse(e.dataTransfer.getData("text/layer-dock"));
  } catch {
    /* External drags cannot provide data during protected dragover. */
  }
  return null;
}
workspace.addEventListener("dragover", (e) => {
  if (!e.dataTransfer.types.includes("text/layer-dock")) return;
  e.preventDefault();
  const item = draggedItem(e);
  if (!item) return;
  const hint = dropHint(e, item);
  dropIndicator.hidden = !hint;
  if (hint) {
    place(dropIndicator, hint.bounds);
    dropIndicator.dataset.kind = hint.target.kind;
    e.dataTransfer.dropEffect = "move";
  }
});
workspace.addEventListener("dragleave", (e) => {
  if (!workspace.contains(e.relatedTarget)) dropIndicator.hidden = true;
});
workspace.addEventListener("drop", (e) => {
  e.preventDefault();
  dropIndicator.hidden = true;
  const item = draggedItem(e);
  if (!item) return;
  const hint = dropHint(e, item);
  if (hint) {
    const { kind, ...source } = item;
    dispatch({
      type: kind === "group" ? "move_group" : "move_panel",
      ...source,
      target: hint.target,
    });
  }
});
try {
  await init();
  canvas.width = 800;
  canvas.height = 600;
  app = WebApp.create(canvas);
  let restoreError;
  try {
    const saved = localStorage.getItem(settingsKey);
    if (saved) app.dispatch({ type: "restore_settings", settings: JSON.parse(saved) });
  } catch (error) { restoreError = `Cannot restore preferences: ${error}`; }
  const themeAction = () => ({
    type: "system_theme_changed",
    theme: systemTheme.matches ? "dark" : "light",
  });
  app.dispatch(themeAction());
  window.addEventListener("storage", (event) => {
    if (event.key !== settingsKey || !event.newValue) return;
    try { dispatch({ type: "restore_settings", settings: JSON.parse(event.newValue) }); }
    catch (error) { message(`Cannot restore preferences: ${error}`); }
  });
  systemTheme.addEventListener("change", () => dispatch(themeAction()));
  state = app.state();
  catalog = app.catalog();
  document.title = `${catalog.app_name} — drawing workspace`;
  await loadIcons();
  refreshPreferences = createPreferences({ element, button, icon, spin, setNumber, numericControl, panelFrame, dispatch, view: () => app.preferences() });
  panelNames = Object.fromEntries(catalog.panels.map((p) => [p.id, p.label]));
  buildHeader();
  buildPanels();
  update(127);
  $("status").textContent = "";
  if (restoreError) message(restoreError);
  new ResizeObserver(arrange).observe(workspace);
  // Test harness accesses the actual Wasm instance and native widgets.
  window.layerApp = { app, dispatch, state: () => app.state(), wake, canvas };
  await startGpu();
} catch (error) {
  $("gpu-notice").replaceChildren(element("h1", "", "Capy Canvas could not load"),
    element("p", "", "Reload the page. If the problem continues, check that the complete app package is being served."), element("pre", "", String(error)));
  $("status").textContent = "";
  console.error(error);
}

async function startGpu() {
  if (gpuStarting || gpuReady) return;
  gpuStarting = true;
  document.body.dataset.gpu = "starting";
  const notice = $("gpu-notice");
  notice.replaceChildren(element("div", "gpu-help", "Starting the canvas…"));
  try {
    if (!isSecureContext) throw new Error("WebGPU requires HTTPS or localhost.");
    if (!navigator.gpu) throw new Error("navigator.gpu is unavailable.");
    app.attach_gpu(await WebGpu.create(canvas));
    gpuReady = true;
    document.body.dataset.gpu = "ready";
    notice.hidden = true;
    wake();
  } catch (error) {
    document.body.dataset.gpu = "unavailable";
    showGpuNotice({ container: notice, error, retry: startGpu, element, button });
    console.warn("GPU canvas unavailable:", error);
  } finally {
    gpuStarting = false;
  }
}
