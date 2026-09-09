import init, { WebApp, WebGpu } from "./pkg/layer_web.js";
import { createPreferences } from "./preferences.js";
import { showGpuNotice } from "./gpu.js";
import { createCustomization } from "./customization.js";
import { createNumberField } from "./numeric.js";

// The static packager fills this map with fingerprinted artwork filenames.
const assetPaths = {};
const asset = (path) => new URL(assetPaths[path.replace(/^\.\//, "")] || path, import.meta.url).href;

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
let refreshPreferences, customization;
let gpuStarting = false;
let gpuReady = false;
let servicingRequests = false;
const settingsKey = "layer.preferences.v1";
const pending = [];
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
applyTheme(systemTheme.matches ? "dark" : "light");

function applyTheme(theme, palette) {
  document.body.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  document.querySelector('meta[name="color-scheme"]').content = theme;
  if (palette) for (const [name, color] of Object.entries(palette)) {
    if (typeof color === "string") document.body.style.setProperty(`--${name.replaceAll("_", "-")}`, name === "button" ? `${color}0d` : color);
  }
  document.querySelector('meta[name="theme-color"]').content = palette?.bg || (theme === "dark" ? "#333333" : "#b8b8b8");
}

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
function numberField(control, label, onChange) {
  return createNumberField({ control, label, onChange, icon, resolve: request => app.number_input(request) });
}
// Overlay scrollbars do not take width away from previews or tiles. Scrolling
// itself stays in the browser; this one thumb also supports pointer dragging.
function panelFrame(panel, scrollable = true) {
  const frame = element("div", "panel-frame");
  frame.append(panel);
  if (!scrollable) return frame;
  const thumb = element("div", "scroll-thumb");
  thumb.setAttribute("aria-hidden", "true");
  frame.append(thumb);
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
    [...catalog.icons, "fullscreen-enter", "fullscreen-exit"].map(async (name) => {
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
  const glyph = icon(state.commands.find((c) => c.id === id).icon);
  if (id === "zen_mode") {
    glyph.style.width = glyph.style.height = `${catalog.zen_icon_size}px`;
  }
  node.append(glyph);
  return node;
}
function fullscreenButton() {
  // Browser-window state belongs to the host, not the shared drawing session.
  const node = button("", async () => {
    node.disabled = true;
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
    } catch {
      message("Could not change fullscreen mode.");
    } finally {
      node.disabled = !document.fullscreenEnabled;
      sync();
    }
  }, "tile-button");
  node.id = "fullscreen";
  node.disabled = !document.fullscreenEnabled;
  function sync() {
    const active = !!document.fullscreenElement;
    node.title = !document.fullscreenEnabled ? "Fullscreen unavailable"
      : active ? "Exit fullscreen" : "Enter fullscreen";
    node.setAttribute("aria-label", node.title);
    node.replaceChildren(icon(active ? "fullscreen-exit" : "fullscreen-enter"));
  }
  document.addEventListener("fullscreenchange", sync);
  sync();
  return node;
}
function draggable(node, item) {
  if (item.kind !== "tile") {
    node.draggable = false;
    node.dataset.workspaceDrag = JSON.stringify({ type: "drag_workspace", item });
    return node;
  }
  node.draggable = true;
  let pointer;
  node.addEventListener("workspace-context-claimed", () => { pointer = null; });
  node.addEventListener("pointerdown", (e) => {
    if (e.pointerType !== "touch" || e.button !== 0) return;
    pointer = { id: e.pointerId, x: e.clientX, y: e.clientY, dragging: false };
    node.setPointerCapture(e.pointerId);
  });
  node.addEventListener("pointermove", (e) => {
    if (pointer?.id !== e.pointerId) return;
    if (!pointer.dragging && Math.hypot(e.clientX - pointer.x, e.clientY - pointer.y) > 8) {
      pointer.dragging = true; dragItem = item; node.classList.add("drag-source"); updateZen();
    }
    if (pointer.dragging) { e.preventDefault(); showDropHint(dropHint(e, item)); }
  });
  const endPointer = (e) => {
    if (pointer?.id !== e.pointerId) return;
    const moved = pointer.dragging; pointer = null;
    if (!moved) return;
    revealPointer = e.pointerId;
    if (e.type === "pointerup") dropItem(item, dropHint(e, item));
    dragItem = null; dropIndicator.hidden = true; node.classList.remove("drag-source"); updateZen();
  };
  for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) node.addEventListener(event, endPointer);
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
    item.kind === "group" ? "Move all tabs" : "Move " + (customization?.view(item.panel)?.title || panelNames[item.panel] || "toolbar"),
  );
  node.append(icon("grip"));
  return draggable(node, item);
}
const dropIndicator = element("div", "drop-indicator");
dropIndicator.hidden = true;
workspace.append(dropIndicator);

function dispatch(action) {
  try {
    if (["move_panel", "move_group", "move_tile", "cycle_floating_size"].includes(action.type))
      action = {
        ...action,
        viewport: [workspace.clientWidth, workspace.clientHeight],
      };
    const animated = ["cycle_floating_size", "select_panel_tab"].includes(action.type) ? groups.get(action.group) : null;
    const before = animated?.getBoundingClientRect();
    applyChange(app.dispatch(action));
    if (before && animated?.classList.contains("floating-panel") && !animated.classList.contains("expanded-panel")
      && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
      const after = animated.getBoundingClientRect();
      if (before.width !== after.width || before.height !== after.height) {
        animated.getAnimations().forEach(a => a.cancel());
        const frame = r => ({ left: `${r.x}px`, top: `${r.y}px`, width: `${r.width}px`, height: `${r.height}px` });
        animated.animate([frame(before), frame(after)], { duration: catalog.panel_expansion_ms, easing: "cubic-bezier(.2,0,0,1)" });
      }
    }
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
      const clip = element("div", "panel-columns");
      clip.append(element("div", "panel-preview")); node.append(clip);
      groups.set(group.id, node);
      workspace.append(node);
    }
    const key = JSON.stringify([group.panels.map((id) => {
      const view = customization.view(id); return [id, view.title, view.tab_style, view.icon];
    }), group.active, group.tabs_visible]);
    if (node.dataset.key !== key) {
      node.dataset.key = key;
      node.dataset.panel = group.active;
      node.dataset.group = group.id;
      node.setAttribute("aria-label", customization.view(group.active).title);
      node.classList.toggle("toolbar", !!group.tiles);
      const preview = node.querySelector(".panel-preview");
      const tabs = element("nav", "dock-tabs");
      draggable(tabs, { kind: "group", group: group.id });
      customization.target(tabs, { kind: "group", group: group.id });
      tabs.setAttribute("aria-label", "Panel tabs");
      if (group.tabs_visible) {
        const labels = element("div", "tab-list");
        group.panels.forEach((panel, index) => {
          const tab = button(
            "",
            () =>
              dispatch({ type: "select_panel_tab", group: group.id, panel }),
            "dock-tab",
          );
          tab.dataset.index = index;
          tab.dataset.panel = panel;
          const view = customization.view(panel);
          tab.title = view.title; tab.setAttribute("aria-label", view.title);
          if (view.tab_style === "icon") tab.append(icon(view.icon));
          else tab.textContent = view.title;
          customization.target(tab, { kind: "panel", panel });
          tab.setAttribute("aria-selected", String(panel === group.active));
          labels.append(draggable(tab, { kind: "panel", panel }));
        });
        tabs.append(labels, grip({ kind: "group", group: group.id }));
        preview.replaceChildren(tabs, panels.get(group.active).parentElement);
      } else {
        preview.replaceChildren(panels.get(group.active).parentElement);
        if (group.footer_grip) {
          const footer = element("div", "panel-footer");
          footer.style.height = `${group.footer_grip.height}px`;
          draggable(footer, { kind: "group", group: group.id });
          customization.target(footer, { kind: "group", group: group.id });
          footer.append(grip({ kind: "group", group: group.id })); preview.append(footer);
        }
      }
    }
    node.classList.toggle("floating-panel", group.floating);
    node.dataset.zIndex = group.floating ? String(100 + layout.groups.indexOf(group) * 2) : "0";
    if (!node.classList.contains("expanded-panel")) node.style.zIndex = node.dataset.zIndex;
    place(node, group.bounds);
    if (group.tiles) {
      const strip = node.querySelector(".toolbar-controls");
      const geometry = group.tiles;
      strip.dataset.axis = group.axis;
      strip.dataset.standalone = !group.tabs_visible;
      customization.layoutTiles(strip, geometry);
    }
  }
  for (const [id, node] of groups)
    if (!live.has(id)) {
      node.remove();
      groups.delete(id);
    }
  const liveDividers = new Set();
  const handles = [
    ...layout.dividers.map(d => ({ key: `${d.band}:${d.id}`, bounds: d.bounds,
      axis: d.axis, action: { type: "drag_divider", id: d.id } })),
    ...layout.groups.filter(g => g.floating).flatMap(g => g.resize_handles.map(h => ({
      key: `floating:${g.id}:${h.edge}`, bounds: h.bounds, edge: h.edge,
      zIndex: Number(groups.get(g.id).dataset.zIndex) + 1,
      action: { type: "resize_floating", group: g.id, edge: h.edge },
    }))),
  ];
  for (const handle of handles) {
    const { key } = handle;
    liveDividers.add(key);
    let node = dividers.get(key);
    if (!node) {
      node = element("div");
      node.tabIndex = 0;
      node.setAttribute("role", "separator");
      node.setAttribute("aria-label", "Resize dock");
      node.addEventListener("keydown", (e) => {
        if (node.dragAction.type === "drag_divider") keyInput(e, true, node.dragAction.id);
      });
      dividers.set(key, node);
      workspace.append(node);
    }
    node.dragAction = handle.action;
    node.dataset.workspaceDrag = JSON.stringify(handle.action);
    node.className = handle.edge ? "floating-resize" : `divider ${handle.axis}`;
    if (handle.edge) {
      node.dataset.edge = handle.edge; node.style.zIndex = handle.zIndex;
      node.hidden = state.customization.expanded != null && layout.groups.find(g => g.id === handle.action.group)?.panels.includes(state.customization.expanded);
    } else node.setAttribute("aria-orientation", handle.axis === "horizontal" ? "vertical" : "horizontal");
    place(node, handle.bounds);
  }
  for (const [key, node] of dividers)
    if (!liveDividers.has(key)) {
      node.remove();
      dividers.delete(key);
    }
  customization.arrange(layout);
  place($("canvas-status"), layout.status);
  // The help occupies the same unobstructed area used for fitting the document.
  // Panels remain native UI siblings above the full-window drawing surface.
  place($("gpu-notice"), layout.work_area);
  resizeCanvas();
  updateZen();
  queuePanelMeasurements();
}
// Measure intrinsic widget content only when its width/copy changes. Rust owns
// tab growth and floating sizes; moving a float reuses these cached DOM facts.
const panelMeasurements = new Map();
let measuringPanels = false;
const measureBox = element("div", "panel-measure");
measureBox.setAttribute("aria-hidden", "true"); workspace.append(measureBox);
function queuePanelMeasurements() {
  if (measuringPanels) return;
  measuringPanels = true;
  requestAnimationFrame(() => {
    measuringPanels = false;
    const measurements = state.workspace.layout.panels.map(config => {
      const view = customization.view(config.id);
      const width = layout.groups.find(g => g.panels.includes(config.id))?.bounds.width || 232;
      const key = JSON.stringify([width, view.title, view.tab_style, view.icon, view.controls, view.tile_style, state.layers.length]);
      let cached = panelMeasurements.get(config.id);
      if (cached?.key !== key) {
        const tab = element("button", "dock-tab");
        tab.style.width = "max-content";
        if (view.tab_style === "icon") tab.append(icon(view.icon)); else tab.textContent = view.title;
        measureBox.style.width = `${width}px`; measureBox.replaceChildren(tab);
        const tabWidth = tab.getBoundingClientRect().width;
        const content = panels.get(config.id).cloneNode(true);
        content.style.height = "auto"; content.style.width = `${width}px`;
        measureBox.replaceChildren(content);
        cached = { key, value: { panel: config.id, tab_width: tabWidth,
          content_height: config.content.kind === "toolbar" ? 0 : content.getBoundingClientRect().height } };
        panelMeasurements.set(config.id, cached); measureBox.replaceChildren();
      }
      return cached.value;
    });
    for (const id of panelMeasurements.keys()) if (!panels.has(id)) panelMeasurements.delete(id);
    // The core returns no change for identical measurements, including after
    // workspace restore. This avoids keeping a second authoritative size cache.
    dispatch({ type: "measure_panels", measurements });
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
  for (const { id: name, kind } of catalog.panels) {
    const panel = element("div", `panel ${name}-panel`);
    if (kind === "tiles") panel.classList.add("tile-panel");
    panels.set(name, panel);
    panelFrame(panel, kind !== "tiles");
  }
  const list = element("div", "brush-list");
  list.dataset.control = "brushes";
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
  controls.dataset.control = "brush_size";
  const size = numberField(catalog.brush_size, "Brush size", value => dispatch({ type: "set_brush_size", value }));
  size.id = "size-number"; controls.append(size);
  const grid = element("div", "size-grid");
  grid.dataset.control = "size_presets";
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
  tools.dataset.control = "layer_actions";
  for (const id of catalog.layer_commands) tools.append(iconButton(id));
  const rows = element("div", "layer-rows");
  rows.dataset.control = "layers";
  rows.id = "layer-rows";
  const layerOpacity = numberField(catalog.opacity, "Layer opacity", opacity => dispatch({ type: "set_layer_opacity", opacity }));
  layerOpacity.id = "layer-opacity";
  const opacityRow = element("div", "layer-opacity-control");
  opacityRow.dataset.control = "layer_opacity"; opacityRow.append(layerOpacity);
  layers.append(tools, rows, opacityRow);
  panels.get("layers").append(layers);
}
function update(regions) {
  if (regions & (1 | 2 | 4 | 8 | 128)) customization.refresh();
  if (regions & 2) {
    for (const [id, button] of brushButtons)
      button.setAttribute("aria-pressed", String(id === state.brush.preset));
    for (const [size, button] of sizeButtons)
      button.setAttribute(
        "aria-pressed",
        String(size === state.brush.diameter),
      );
    $("size-number").update(state.brush.diameter);
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
    $("layer-opacity").update(state.layers.find((l) => l.selected).opacity);
  }
  if (regions & (1 | 4 | 128)) arrange();
  if (regions & (1 | 4 | 8 | 128)) refreshWorkspaceMenu();
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
    applyTheme(state.theme, state.palette);
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
let workspaceGesture = null;
function workspaceGestureEvent(phase, e) {
  const drag = workspaceGesture;
  if (!drag) return;
  dispatch({ ...drag.action, phase, position: [e.clientX, e.clientY],
    viewport: [workspace.clientWidth, workspace.clientHeight],
    ...(drag.action.type === "drag_workspace" ? { tabs: tabHits() } : {}),
  });
}
function endWorkspaceGesture(e, cancel = false) {
  const drag = workspaceGesture;
  if (!drag || (e && drag.id !== e.pointerId)) return;
  if (drag.started) workspaceGestureEvent(cancel ? "cancel" : "up", e || drag.last);
  workspaceGesture = null;
  if (workspace.hasPointerCapture(drag.id)) workspace.releasePointerCapture(drag.id);
  dropIndicator.hidden = true;
  if (drag.started) { revealPointer = drag.id; e?.preventDefault(); e?.stopPropagation(); }
  updateZen();
}
workspace.addEventListener("pointerdown", e => {
  if (e.button !== 0 || workspaceGesture) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  workspaceGesture = { id: e.pointerId, action: JSON.parse(node.dataset.workspaceDrag),
    start: e, last: e, started: false };
  // External resize strips are outside the unselectable panel. Prevent a
  // native text-selection drag from stealing their pointer sequence.
  if (workspaceGesture.action.type !== "drag_workspace") e.preventDefault();
}, { capture: true });
workspace.addEventListener("pointermove", e => {
  const drag = workspaceGesture;
  if (!drag || drag.id !== e.pointerId) return;
  drag.last = e;
  if (!drag.started) {
    const distance = Math.hypot(e.clientX - drag.start.clientX, e.clientY - drag.start.clientY);
    if (distance <= (drag.action.type === "drag_workspace" ? 8 : 0)) return;
    drag.started = true;
    // Capture on the stable workspace before Rust tears off/rebuilds a tab.
    workspace.setPointerCapture(e.pointerId);
    groups.forEach(node => node.getAnimations().forEach(a => a.cancel()));
    workspaceGestureEvent("down", drag.start);
  }
  workspaceGestureEvent("move", e);
  if (drag.action.type === "drag_workspace") showDropHint(dropHint(e, drag.action.item));
  e.preventDefault(); e.stopPropagation();
}, { capture: true });
workspace.addEventListener("pointerup", e => endWorkspaceGesture(e), { capture: true });
workspace.addEventListener("pointercancel", e => endWorkspaceGesture(e, true), { capture: true });
workspace.addEventListener("lostpointercapture", e => endWorkspaceGesture(e, true));
workspace.addEventListener("workspace-context-claimed", () => endWorkspaceGesture(null, true));
workspace.addEventListener("dblclick", e => {
  if (e.target.closest(".dock-tab")) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  const action = JSON.parse(node.dataset.workspaceDrag);
  if (action.type !== "drag_workspace") return;
  const group = app.floating_size_target(action.item);
  if (group == null) return;
  e.preventDefault(); e.stopPropagation();
  dispatch({ type: "cycle_floating_size", group });
});
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
      expanded_panel: customization?.placement(),
      contact_tab: event.kind === "contact"
        ? document.elementFromPoint(...event.position)?.closest(".dock-tab")?.dataset.panel ?? null
        : null,
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
  function menu(label, sections, icon) {
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
    if (!sections.length) {
      contents.id = "workspace-menu";
      details.addEventListener("toggle", () => { if (details.open) refreshWorkspaceMenu(); });
    }
    for (const [index, ids] of sections.entries()) {
      if (index) contents.append(element("hr"));
      for (const id of ids) {
        const control = commandButton(id, id);
        control.replaceChildren(element("span", "command-label"), element("span", "shortcut-hint"));
        control.addEventListener("click", () => {
          details.open = false;
          updateZen();
        });
        contents.append(control);
      }
    }
    details.append(summary, contents);
    details.addEventListener("toggle", updateZen);
    return details;
  }
  $("header-start").append(iconButton("zen_mode"));
  for (const spec of catalog.menus)
    $("header-start").append(menu(spec.label, spec.sections));
  $("header-end").append(fullscreenButton(), iconButton("settings"));
}
function refreshWorkspaceMenu() {
  const contents = $("workspace-menu");
  if (contents && customization) customization.renderMenu(contents, app.workspace_menu(), () => {
    contents.parentElement.open = false; updateZen();
  });
}
function pointerStyle(e) {
  // Touch leaves :hover stuck until the next tap; track actual pointer input
  // instead of disabling hover for a whole device that may also have a pen/mouse.
  const touch = e.pointerType === "touch", root = document.documentElement;
  if (root.hasAttribute("data-touch") !== touch) root.toggleAttribute("data-touch", touch);
}
window.addEventListener(
  "pointermove",
  (e) => {
    pointerStyle(e);
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
    pointerStyle(e);
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
// DOM IDs are signed 32-bit (Safari can use negative IDs). Preserve their bits
// at the unsigned Rust boundary; DOM pointer capture keeps the original ID.
function corePointerId(e) {
  return e.pointerId >>> 0;
}
function queuePen(e, stage) {
  lastPenEvent = stage === 3 || stage === 4 ? null : e;
  const records = [];
  const history = stage === 2 ? e.getCoalescedEvents?.() || [] : [];
  for (const item of history.length ? history : [e]) {
    const [x, y] = position(item),
      pen = item.pointerType === "pen";
    records.push(
      corePointerId(item),
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
    id: BigInt(corePointerId(e)),
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
  endWorkspaceGesture(null, true);
  cursorInput(null);
  chromeHeld = false;
  if (input({ type: "blur" }).cancel_paint && lastPenEvent)
    queuePen(lastPenEvent, 4);
});
function tabHits() {
  return [...groups.entries()].flatMap(([group, node]) =>
    [...node.querySelectorAll(".dock-tab")].map((tab) => {
      const b = tab.getBoundingClientRect();
      return {
        group,
        index: Number(tab.dataset.index),
        bounds: { x: b.x, y: b.y, width: b.width, height: b.height },
      };
    }),
  );
}
function dropHint(e, item) {
  try {
    return app.drop_hint({
      viewport: [workspace.clientWidth, workspace.clientHeight],
      position: [e.clientX, e.clientY],
      tabs: tabHits(),
      item,
      expansion: customization.placement(),
    });
  } catch {
    return null;
  } // Invalid/foreign payloads have no accepted core target.
}
function draggedItem(e) {
  if (dragItem) return dragItem;
  try {
    const item = JSON.parse(e.dataTransfer.getData("text/layer-dock"));
    return item.kind === "tile" ? item : null;
  } catch {
    /* External drags cannot provide data during protected dragover. */
  }
  return null;
}
function showDropHint(hint) {
  dropIndicator.hidden = !hint;
  if (hint) { place(dropIndicator, hint.bounds); dropIndicator.dataset.kind = hint.target.kind; }
}
function dropItem(item, hint) {
  if (hint && item.kind === "tile")
    dispatch({ type: "move_tile", panel: item.panel, tile: item.tile, target: hint.target });
}
workspace.addEventListener("dragover", (e) => {
  if (!e.dataTransfer.types.includes("text/layer-dock")) return;
  e.preventDefault();
  const item = draggedItem(e);
  if (!item) return;
  const hint = dropHint(e, item);
  showDropHint(hint);
  if (hint) {
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
  dropItem(item, hint);
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
  document.documentElement.style.setProperty("--ui-text-size", `${catalog.text_size_pt}pt`);
  document.title = `${catalog.app_name} — drawing workspace`;
  await loadIcons();
  refreshPreferences = createPreferences({ element, button, icon, numberField, panelFrame, dispatch, view: () => app.preferences() });
  panelNames = Object.fromEntries(catalog.panels.map((p) => [p.id, p.label]));
  buildHeader();
  buildPanels();
  customization = createCustomization({ app, catalog, state: () => state, workspace, panels, groups,
    element, button, icon, numberField, panelFrame,
    dispatch, draggable, grip, place, updateZen });
  update(255);
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
    showGpuNotice({ container: notice, error, element, button });
    console.warn("GPU canvas unavailable:", error);
  } finally {
    gpuStarting = false;
  }
}
