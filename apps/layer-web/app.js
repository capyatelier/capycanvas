import init, { WebApp, WebGpu } from "./pkg/layer_web.js";
import { createPreferences } from "./preferences.js";
import { showGpuNotice } from "./gpu.js";
import { createCustomization } from "./customization.js";
import { createEditorPanels } from "./editor-panels.js";
import { createWorkspaceChrome } from "./workspace-chrome.js";
import { createDocuments } from "./documents.js";
import { createSystemStatus } from "./system-status.js";
import { createNumberField } from "./numeric.js";
import { createLayerPanel } from "./layers.js";
import { createEffectPanels, fetchFilterPackage } from "./effects.js";

// The static packager fills this map with fingerprinted resource filenames.
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
const commands = new Map(), sizeButtons = new Map();
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
let refreshPreferences, customization, layerPanel, effectPanels, editor, workspaceChrome, documents, systemStatus;
const fullscreenRequests = new Set();
let gpuStarting = false;
let gpuReady = false;
let compilerScheduled = false, compilerFailed = false;
const startupTimes = { canvas: null, document: null, brush: null, complete: null };
let startupNotice;
let firstCanvasRendered = false;
let servicingRequests = false;
const settingsKey = "layer.preferences.v1", workspaceKey = "layer.workspace.v1";
let savedWorkspace = "", workspaceSaveTimer;
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
function numberField(control, label, onChange, inline = false) {
  return createNumberField({ control, label, onChange, inline, icon, resolve: request => app.number_input(request) });
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
    [...catalog.icons, "fullscreen-enter", "fullscreen-exit", "chevron-down"].map(async (name) => {
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
function draggable(node, item) {
  if (item.kind !== "tile") {
    node.draggable = false;
    node.dataset.workspaceDrag = JSON.stringify({ type: "drag_workspace", item });
    return node;
  }
  node.draggable = true;
  let pointer;
  node.addEventListener("workspace-context-claimed", e => {
    if (pointer?.dragging) e.preventDefault();
    else if (pointer) pointer.context = true;
  });
  node.addEventListener("pointerdown", (e) => {
    if (e.pointerType === "mouse" || e.button !== 0) return;
    pointer = { id: e.pointerId, x: e.clientX, y: e.clientY, dragging: false };
    node.setPointerCapture(e.pointerId);
  });
  node.addEventListener("pointermove", (e) => {
    if (pointer?.id !== e.pointerId) return;
    if (!pointer.dragging && Math.hypot(e.clientX - pointer.x, e.clientY - pointer.y) > 8) {
      customization.dismissContext();
      pointer.dragging = true; dragItem = item; node.classList.add("drag-source"); updateZen();
    }
    if (pointer.dragging) { e.preventDefault(); showDropHint(dropHint(e, item)); }
  });
  const endPointer = (e) => {
    if (pointer?.id !== e.pointerId) return;
    const moved = pointer.dragging, context = pointer.context; pointer = null;
    if (context) revealPointer = e.pointerId;
    if (!moved) return;
    revealPointer = e.pointerId;
    if (e.type === "pointerup") dropItem(item, dropHint(e, item));
    dragItem = null; dropIndicator.hidden = true; node.classList.remove("drag-source"); updateZen();
  };
  for (const event of ["pointerup", "pointercancel", "lostpointercapture"]) node.addEventListener(event, endPointer);
  node.addEventListener("dragstart", (e) => {
    if (pointer?.dragging) { e.preventDefault(); return; }
    customization.dismissContext();
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
    if (action.type === "measure_column_drawers" && workspaceGesture) workspaceGesture.hits = null;
    if (["move_panel", "move_group", "move_tile", "double_click_panel_handle", "reset_column_width"].includes(action.type))
      action = {
        ...action,
        viewport: [workspace.clientWidth, workspace.clientHeight],
      };
    const animated = ["double_click_panel_handle", "select_panel_tab"].includes(action.type) ? groups.get(action.group) : null;
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
    const presentation = app.workspace_update();
    if (workspaceModelRevision !== presentation.model_revision) {
      const moving = presentation.drag || workspacePresentation?.drag;
      workspaceModelRevision = presentation.model_revision;
      workspacePresentation = null;
      state = app.state();
      // A concurrent model change rebases retained placement too.
      update(change.regions | (moving ? 1 : 0));
    } else if (change.regions & 32) {
      // Camera-only publications intentionally retain the model revision.
      state.camera = app.camera();
      update(32);
    }
    if (workspaceModelRevision === presentation.model_revision)
      queueWorkspacePresentation(presentation);
  }
  if (change.canvas_wake) wake();
  if (!workspaceGesture?.started) scheduleCursor();
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
    flushWorkspacePresentation();
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
    refreshStartup();
    scheduleCompiler();
    if (pending.length) wake();
  } catch (error) {
    message(error);
    console.error(error);
  }
}
function refreshStartup() {
  if (!gpuReady || compilerFailed) return;
  const [documentReady, brushReady, complete] = app.startup_progress();
  const stages = { canvas: app.canvas_presented(), document: documentReady, brush: brushReady, complete };
  for (const [name, ready] of Object.entries(stages)) {
    if (ready && startupTimes[name] === null) {
      startupTimes[name] = performance.now();
      performance.mark(`capy.startup.${name}`);
    }
  }
  if (!startupNotice) {
    startupNotice = element("div", "startup-progress");
    startupNotice.setAttribute("role", "status");
    workspace.append(startupNotice);
  }
  startupNotice.hidden = !stages.canvas || app.brush_ready();
  startupNotice.textContent = documentReady ? "Preparing brush…" : "Preparing canvas…";
}
function scheduleCompiler() {
  if (!gpuReady || compilerScheduled || compilerFailed || !app.shader_work_pending()) return;
  compilerScheduled = true;
  // Start after this display callback can present. The next job is scheduled
  // by a later frame, with input/UI opportunities between each GPU scope.
  setTimeout(async () => {
    try {
      if (!firstCanvasRendered) {
        // A display callback alone does not mean the GPU has rendered paper.
        // Starting document compilation sooner can hold up Chrome's GPU-process
        // command batch, including the pending first canvas presentation.
        await gpuOperation(() => app.wait_for_canvas());
        firstCanvasRendered = true;
        await new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)));
      }
      await gpuOperation(() => app.compile_startup_step());
      refreshStartup();
      wake();
    } catch (error) {
      compilerFailed = true;
      if (startupNotice) startupNotice.hidden = true;
      message("Shader preparation failed. Reload the page to retry.");
      console.error(error);
    } finally {
      compilerScheduled = false;
    }
  }, 0);
}
function place(node, rect) {
  Object.assign(node.style, {
    left: `${rect.x}px`,
    top: `${rect.y}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  });
}
function tabLabel(tab, view) {
  tab.classList.toggle("icon-only-tab", !view.tab.show_name);
  if (view.tab.show_icon) tab.append(icon(view.icon));
  if (view.tab.show_name) tab.append(element("span", "", view.title));
}
function arrange() {
  if (!app) return;
  clearWorkspacePlacement();
  if (workspaceGesture) workspaceGesture.hits = null;
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
      const view = customization.view(id); return [id, view.title, view.tab, view.icon];
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
          tabLabel(tab, view);
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
    node.style.setProperty("--panel-body-height", `${group.bounds.height - (group.tabs_visible ? layout.tab_bar_height : 0)}px`);
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
  workspaceChrome?.arrange(layout);
  editor.queuePositions();
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
      const key = JSON.stringify([width, view.title, view.tab, view.icon, view.controls, view.tile_style, state.layers.length]);
      let cached = panelMeasurements.get(config.id);
      if (cached?.key !== key) {
        const tab = element("button", "dock-tab");
        tab.style.width = "max-content";
        tabLabel(tab, view);
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
  panels.get("brushes").append(editor.control("brushes"));
  for (const [panel, control] of [["tool_settings","tool_settings"],["color","color_wheel"],["navigator","navigator"]])
    panels.get(panel).append(editor.control(control));
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
    choice.onpointerenter = () => { choice.title = app.action_tooltip(`${value} px`, { type: "set_brush_size", value }); };
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
  layerPanel = createLayerPanel({ app, catalog, state: () => state, panel: panels.get("layers"), element, button, icon, dispatch, applyChange, message, numberField, dismissContext: () => customization.dismissContext() });
  effectPanels = createEffectPanels({app,catalog,state:()=>state,panels,element,button,icon,dispatch,numberField,
    contentChanged:id=>{panelMeasurements.delete(id);queuePanelMeasurements();}});
}
function contentPanel(id) {
  const panel=element("div",`panel ${id}-panel`);
  if(id==="layers") {
    const view=createLayerPanel({app,catalog,state:()=>state,panel,element,button,icon,dispatch,applyChange,message,numberField,dismissContext:()=>customization.dismissContext()});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else if(["adjustments","properties","stats"].includes(id)) {
    const copies=new Map(["adjustments","properties","stats"].map(name=>[name,name===id?panel:element("div","panel")]));
    const view=createEffectPanels({app,catalog,state:()=>state,panels:copies,element,button,icon,dispatch,numberField,contentChanged:()=>{}});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else {
    for(const control of customization.view(id).controls.filter(c=>c.visible_in_panel)) panel.append(customization.field(control.control,control.label));
    panel.refreshPanel=()=>{};
  }
  panel.refreshPanel();return panel;
}
function update(regions) {
  if (regions & (1 | 2 | 4 | 8 | 128)) customization.refresh();
  if (regions & 2) {
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
    layerPanel.refresh();
    effectPanels.refresh();
  }
  if (regions & (1 | 2 | 4 | 8 | 16 | 32 | 128)) { editor.refresh(); workspaceChrome?.refresh(); }
  if (regions & (1 | 4 | 128)) arrange();
  if (regions & (1 | 128)) persistWorkspace();
  if (regions & (1 | 4 | 8 | 128)) refreshWorkspaceMenu();
  if (regions & (4 | 8))
    for (const command of state.commands)
      for (const node of commands.get(command.id) || []) {
        node.disabled = !command.enabled || (command.id === "fullscreen" && !document.fullscreenEnabled);
        node.title = command.tooltip;
        node.setAttribute("aria-label", command.label);
        node.setAttribute("aria-pressed", String(command.selected));
        if (node.dataset.icon === "true") {
          const glyph = node.querySelector("svg");
          if (glyph?.dataset.asset !== command.icon) {
            const next = icon(command.icon);
            next.style.cssText = glyph?.style.cssText || "";
            node.replaceChildren(next);
          }
        } else {
          if (node.querySelector(".command-label")) {
            node.querySelector(".command-label").textContent = command.label;
            node.querySelector(".shortcut-hint").textContent = command.shortcut;
          } else node.textContent = command.label;
        }
      }
  if (regions & 16) {
    applyTheme(state.theme, state.palette);
    systemStatus?.setClockVisibility(state.settings.show_clock);
    refreshPreferences(app.preferences());
  }
  if (regions & 32)
    $("view-info").textContent =
      `${Math.round(state.camera.zoom * 100)}% · ${Math.round((state.camera.rotation * 180) / Math.PI)}°`;
  if (regions & (1 | 16)) updateZen();
  if (regions & 64) {
    documents?.refresh();
    if (state.host_error) message(state.host_error);
    // Small applied-settings snapshots only, never per-input/frame writes.
    if (!servicingRequests) {
      servicingRequests = true;
      try {
        for (const request of state.requests) {
          let error = null;
          try {
            if (request.kind.type === "set_fullscreen") {
              if (!fullscreenRequests.has(request.id)) {
                fullscreenRequests.add(request.id);
                systemStatus.setFullscreen(request.kind.fullscreen)
                  .then(() => dispatch({type:"complete_request",id:request.id,error:null}),
                    error => dispatch({type:"complete_request",id:request.id,error:String(error)}))
                  .finally(() => fullscreenRequests.delete(request.id));
              }
              continue;
            }
            else if (request.kind.type === "open_link") { window.open(app.application_link(request.kind.link), "_blank", "noopener"); }
            else if (request.kind.type !== "save_settings") { documents.handle(request); continue; }
            else
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
function grabTabSlide(drag) {
  if (!drag.node.matches(".dock-tab")) return;
  const strip = drag.node.parentElement;
  if (!strip.matches(".tab-list, .drawer-tab-strip")) return;
  const group = JSON.parse(strip.parentElement.dataset.workspaceDrag).item.group;
  const rect = node => {
    const b = node.getBoundingClientRect();
    return { x: b.x, y: b.y, width: b.width, height: b.height };
  };
  drag.tabGrab = { clip: rect(strip), tabs: [...strip.children].map((source, index) =>
    ({ source, hit: { group, index, bounds: rect(source) } })) };
}
function startTabSlide(drag) {
  if (!drag.tabGrab) return;
  const { clip, tabs: grabbed } = drag.tabGrab;
  drag.tabGrab = null;
  const overlay = element("div", "tab-slide-overlay");
  Object.assign(overlay.style, { left: `${clip.x}px`, top: `${clip.y}px`,
    width: `${clip.width}px`, height: `${clip.height}px` });
  overlay.setAttribute("aria-hidden", "true"); overlay.inert = true;
  // Freeze insertion geometry; only these noninteractive copies move.
  const tabs = grabbed.map(({ source, hit }) => {
    const bounds = hit.bounds, preview = source.cloneNode(true);
    for (const name of [...preview.attributes].map(a => a.name)) {
      if (name.startsWith("data-") || name === "id") preview.removeAttribute(name);
    }
    preview.classList.add(source === drag.node ? "dragged-tab-preview" : "neighbor-tab-preview");
    Object.assign(preview.style, { left: `${bounds.x - clip.x}px`, top: `${bounds.y - clip.y}px`,
      width: `${bounds.width}px`, height: `${bounds.height}px`, font: getComputedStyle(source).font, transform: "translateX(0px)" });
    source.classList.add("dragged-tab-source");
    overlay.append(preview);
    return { source, preview, hit };
  });
  workspace.append(overlay);
  overlay.getBoundingClientRect(); // Establish the neighbors' transition starting positions.
  drag.tabSlide = { tabs, clip, overlay };
}
function clearTabSlide(drag) {
  if (!drag.tabSlide) return;
  for (const tab of drag.tabSlide.tabs) tab.source.classList.remove("dragged-tab-source");
  drag.tabSlide.overlay.remove();
  drag.tabSlide = null;
}
function updateTabSlide(drag, presentation) {
  const slide = drag.tabSlide;
  if (!slide) return;
  const preview = presentation?.preview;
  if (!preview) { clearTabSlide(drag); return; }
  for (const tab of slide.tabs) {
    const offset = tab.source === drag.node ? preview.bounds.x - presentation.source.x
      : preview.offsets.find(o => Number(o.index) === tab.hit.index)?.x || 0;
    tab.preview.style.transform = `translateX(${deviceAligned(offset)}px)`;
  }
}
function workspaceCursor(cursor) {
  if ((workspace.dataset.workspaceCursor || null) === (cursor || null)) return;
  if (cursor) {
    workspace.dataset.workspaceCursor = cursor;
    workspace.style.setProperty("--workspace-cursor", cursor);
  } else {
    delete workspace.dataset.workspaceCursor;
    workspace.style.removeProperty("--workspace-cursor");
  }
}
// Shared workspace_update publication: retain DOM/content by model_revision.
// Dispatch every input to Rust; replace only pending absolute presentation and
// apply it on the display clock. No scaled textures or per-motion DOM rebuilds.
let workspaceModelRevision, workspacePresentation, workspacePresentationFrame = 0;
workspace.addEventListener("scroll", () => { if (workspaceGesture) workspaceGesture.hits = null; }, true);
const workspacePlacements = new Set();
const deviceAligned = value => Math.round(value * devicePixelRatio) / devicePixelRatio;
function clearWorkspacePlacement() {
  for (const node of workspacePlacements) node.style.removeProperty("transform");
  workspacePlacements.clear();
}
function queueWorkspacePresentation(presentation) {
  workspacePresentation = presentation;
  if (!presentation.drag) {
    flushWorkspacePresentation();
  } else if (!workspacePresentationFrame) {
    workspacePresentationFrame = requestAnimationFrame(function presentWorkspaceFrame() {
      workspacePresentationFrame = 0;
      flushWorkspacePresentation();
    });
  }
}
function flushWorkspacePresentation() {
  const update = workspacePresentation;
  workspacePresentation = null;
  if (!update || update.model_revision !== workspaceModelRevision) return false;
  const drag = update.drag, moving = drag?.group;
  if (moving) {
    const base = layout.groups.find(g => g.id === moving.id)?.bounds;
    if (base) {
      const transform = `translate(${deviceAligned(moving.bounds.x) - base.x}px, ${deviceAligned(moving.bounds.y) - base.y}px)`;
      const nodes = [groups.get(moving.id), ...[...dividers.values()].filter(n => n.dragAction.group === moving.id)];
      for (const node of nodes.filter(Boolean)) {
        if (node.style.transform !== transform) node.style.transform = transform;
        workspacePlacements.add(node);
      }
    }
  } else clearWorkspacePlacement();
  if (workspaceGesture) updateTabSlide(workspaceGesture, drag?.tab);
  showDropHint(drag?.drop_hint);
}
function workspaceGestureEvent(phase, e) {
  const drag = workspaceGesture;
  if (!drag) return;
  if (phase === "down" || phase === "up") {
    workspaceChrome?.measureColumnDrawers();
    drag.hits = null;
  }
  dispatch({ ...drag.action, phase, position: [e.clientX, e.clientY],
    viewport: [workspace.clientWidth, workspace.clientHeight],
    ...(drag.action.type === "drag_workspace" ? { tabs: drag.hits ??= tabHits() } : {}),
  });
}
function endWorkspaceGesture(e, cancel = false) {
  const drag = workspaceGesture;
  if (!drag || (e && drag.id !== e.pointerId)) return;
  if (drag.started) workspaceGestureEvent(cancel ? "cancel" : "up", e || drag.last);
  workspaceGesture = null;
  clearTabSlide(drag);
  workspaceCursor(null);
  if (workspace.hasPointerCapture(drag.id)) workspace.releasePointerCapture(drag.id);
  dropIndicator.hidden = true;
  if (drag.started || drag.context) { revealPointer = drag.id; e?.preventDefault(); e?.stopPropagation(); }
  updateZen();
}
workspace.addEventListener("pointerdown", e => {
  if (e.button !== 0 || workspaceGesture) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  workspaceGesture = { id: e.pointerId, action: JSON.parse(node.dataset.workspaceDrag),
    start: e, last: e, started: false, node, cursor: getComputedStyle(node).cursor };
  grabTabSlide(workspaceGesture);
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
    customization.dismissContext();
    drag.started = true;
    // Capture on the stable workspace before Rust tears off/rebuilds a tab.
    workspace.setPointerCapture(e.pointerId);
    groups.forEach(node => node.getAnimations().forEach(a => a.cancel()));
    startTabSlide(drag);
    workspaceGestureEvent("down", drag.start);
    if (drag.tabSlide) app.begin_tab_drag(drag.tabSlide.tabs.map(t => t.hit), drag.tabSlide.clip);
  }
  workspaceGestureEvent("move", e);
  if (drag.action.type === "drag_workspace") {
    workspaceCursor("grabbing");
  } else {
    workspaceCursor(drag.cursor);
  }
  e.preventDefault(); e.stopPropagation();
}, { capture: true });
workspace.addEventListener("pointerup", e => endWorkspaceGesture(e), { capture: true });
workspace.addEventListener("pointercancel", e => endWorkspaceGesture(e, true), { capture: true });
workspace.addEventListener("lostpointercapture", e => {
  // Touch starts with implicit capture on the tab. Transferring capture to the
  // stable workspace releases that child; only losing our own capture cancels.
  if (e.target === workspace) endWorkspaceGesture(e, true);
});
workspace.addEventListener("workspace-context-claimed", e => {
  const drag = workspaceGesture;
  if (drag && (drag.node.contains(e.target) || e.target.contains(drag.node))) {
    // A late native contextmenu event must not interrupt an existing drag.
    if (drag.started) { e.preventDefault(); return; }
    drag.context = true;
    // Keep receiving this contact even when the menu covers the original tab.
    workspace.setPointerCapture(drag.id);
  } else endWorkspaceGesture(null, true);
});
workspace.addEventListener("dblclick", e => {
  if (e.target.closest(".dock-tab")) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  const action = JSON.parse(node.dataset.workspaceDrag);
  if (action.type === "drag_divider" && layout.dividers.some(d =>
    d.id === action.id && d.band && d.axis === "horizontal")) {
    e.preventDefault(); e.stopPropagation();
    endWorkspaceGesture(null, true);
    dispatch({ type: "reset_column_width", id: action.id });
    return;
  }
  if (action.type !== "drag_workspace") return;
  const group = app.panel_handle_target(action.item);
  if (group == null) return;
  e.preventDefault(); e.stopPropagation();
  dispatch({ type: "double_click_panel_handle", group });
});
function input(event) {
  if (!app) return {};
  try {
    const reply = app.input(event);
    workspace.classList.toggle("zen-hidden", reply.chrome_hidden);
    workspace.classList.toggle("zen-hide-floating", reply.hide_floating_panels);
    workspace.classList.toggle("zen-keep-button", reply.keep_zen_button);
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
      ...workspaceChrome?.facts(),
      contact_tab: event.kind === "contact"
        ? document.elementFromPoint(...event.position)?.closest(".dock-tab")?.dataset.panel ?? null
        : null,
      held: chromeHeld,
      dragging: dragItem !== null,
      popup_open:
        !!document.querySelector("details[open], :popover-open, dialog[open]") ||
        !!document.activeElement?.matches("select"),
    },
  });
}
function updateZen() {
  chromeInput({ kind: "refresh" });
  workspaceChrome?.refresh();
  editor?.queuePositions();
}
function buildHeader() {
  const zen = iconButton("zen_mode");
  zen.id = "zen-button"; zen.classList.add("chrome");
  zen.dataset.context = JSON.stringify({ kind: "zen_mode" });
  workspace.prepend(zen);
  $("header-start").append(element("span", "zen-spacer"));
  for (const spec of app.editor_models(workspace.clientWidth, workspace.clientHeight).application_menus) {
    const details = element("details", "header-menu"); details.name = "workspace-menu"; details.dataset.menu = spec.id;
    const summary = element("summary", "", spec.label); summary.setAttribute("aria-label", spec.label);
    const contents = element("div", "popover"); contents.setAttribute("role","menu");
    if(spec.id === "window") contents.id = "workspace-menu";
    details.append(summary,contents);
    details.addEventListener("toggle",()=>{if(details.open) refreshWorkspaceMenu(); updateZen();});
    $("header-start").append(details);
  }
  systemStatus = createSystemStatus({element, changed:fullscreen => {
    if(customization && state.fullscreen !== fullscreen) dispatch({type:"window_fullscreen",fullscreen});
  }});
  $("header-end").append(systemStatus.root, iconButton("fullscreen"), iconButton("settings"));
  $("header-end").querySelector('[data-command="fullscreen"]').id = "fullscreen";
}
function refreshWorkspaceMenu() {
  if(!customization || !document.querySelector(".header-menu[open]")) return;
  for(const spec of app.editor_models(workspace.clientWidth,workspace.clientHeight).application_menus) {
    if (!document.fullscreenEnabled) for (const item of spec.model.sections.flat())
      if (item.action?.type === "invoke" && item.action.command === "fullscreen") item.enabled = false;
    const details = document.querySelector(`[data-menu="${spec.id}"]`);
    if(details?.open) customization.renderMenu(details.querySelector(".popover"),spec.model,()=>{details.open=false;updateZen();});
  }
}
function persistWorkspace() {
  clearTimeout(workspaceSaveTimer);
  workspaceSaveTimer = setTimeout(saveWorkspace, 250);
}
function saveWorkspace() {
  if(!app) return;
  try { const value=JSON.stringify(app.workspace_persistence()); if(value!==savedWorkspace){localStorage.setItem(workspaceKey,value);savedWorkspace=value;} }
  catch(error){message(`Cannot save workspace: ${error}`);}
}
window.addEventListener("pagehide",saveWorkspace);
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
function canvasPointer(e, stage) {
  e.preventDefault();
  if (stage === 1) {
    canvas.focus();
    canvas.setPointerCapture(e.pointerId);
  }
  let sample = e;
  const activePen = lastPenEvent?.pointerType === "pen"
    && lastPenEvent.pointerId === e.pointerId;
  // Losing the browser's input stream is not an instruction to erase ink.
  // Tablet lift can end with cancellation/capture loss, or a hover move before
  // pointerup. Finish once, at the last contact sample: termination events may
  // have reset coordinates/pressure or already be outside the drawing surface.
  if (activePen && (stage === 4 ||
      (stage === 2 && !(e.buttons & 33) && e.pressure === 0))) {
    sample = lastPenEvent;
    stage = 3;
  }
  const reply = pointerInput(sample, stage);
  if (reply.paint) queuePen(sample, stage);
  cursorInput(e.type === "pointercancel" || e.type === "lostpointercapture" ? null : e);
}
for (const [name, stage] of [
  ["pointerdown", 1],
  ["pointermove", 2],
  ["pointerup", 3],
  ["pointercancel", 4],
])
  canvas.addEventListener(name, (e) => canvasPointer(e, stage));
canvas.addEventListener("lostpointercapture", (e) => canvasPointer(e, 4));
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
    [...node.querySelectorAll(".dock-tab")].flatMap(tab => {
      const b = tab.getBoundingClientRect(), clip = tab.parentElement.getBoundingClientRect();
      const x = Math.max(b.x, clip.x), y = Math.max(b.y, clip.y);
      const width = Math.min(b.right, clip.right) - x, height = Math.min(b.bottom, clip.bottom) - y;
      return width > 0 && height > 0 ? [{ group, index: Number(tab.dataset.index),
        bounds: { x, y, width, height } }] : [];
    }),
  ).concat(workspaceChrome?.tabHits() ?? []);
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
  try {
    const saved = localStorage.getItem(workspaceKey);
    if(saved) { app.dispatch({type:"restore_workspace", workspace:JSON.parse(saved)}); savedWorkspace=saved; }
  } catch(error) { restoreError = `Cannot restore workspace: ${error}`; }
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
  editor = createEditorPanels({app,state:()=>state,workspace,canvas,element,button,icon,numberField,dispatch,asset,wake});
  buildHeader();
  buildPanels();
  customization = createCustomization({ app, catalog, state: () => state, workspace, panels, groups,
    element, button, icon, numberField, panelFrame,
    dispatch, draggable, grip, place, updateZen, editor });
  workspaceChrome = createWorkspaceChrome({app,state:()=>state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,panels,draggable,grip,contentPanel});
  documents = createDocuments({app,dispatch,applyChange,wake,element,button,numberField,message,gpuOperation});
  update(255);
  systemStatus.sync();
  $("status").textContent = "";
  if (restoreError) message(restoreError);
  new ResizeObserver(arrange).observe(workspace);
  // Test harness accesses the actual Wasm instance and native widgets.
  window.layerApp = { app, dispatch, state: () => app.state(), wake, canvas, loadFilters, startupTimes };
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
    app.attach_gpu(await createGpu());
    gpuReady = true;
    document.body.dataset.gpu = "ready";
    notice.hidden = true;
    wake();
    // Resource loading failure never disables the canvas or the working catalog.
    loadFilters(asset("filters/manifest.json"), "merge", name=>asset(`filters/${name}`))
      .catch(error=>console.warn("Using bundled filters:",error))
      .finally(() => { app.startup_catalog_submitted(); wake(); });
  } catch (error) {
    document.body.dataset.gpu = "unavailable";
    showGpuNotice({ container: notice, error, element, button });
    console.warn("GPU canvas unavailable:", error);
  } finally {
    gpuStarting = false;
  }
}

async function createGpu() {
  return gpuOperation(() => WebGpu.create(canvas));
}
async function gpuOperation(operation) {
  // A browser API exception can escape a Wasm future without rejecting its
  // Promise. Scope these handlers to GPU creation or one compilation job so a
  // browser exception becomes a visible error instead of a stuck startup.
  const events = new AbortController();
  try {
    const failure = new Promise((_, reject) => {
      window.addEventListener("error", e => reject(e.error || new Error(e.message)), { signal: events.signal });
      window.addEventListener("unhandledrejection", e => reject(e.reason), { signal: events.signal });
    });
    return await Promise.race([failure, operation()]);
  } finally {
    events.abort();
  }
}

async function loadFilters(url, mode="add", moduleUrl) {
  const change=await fetchFilterPackage(app,new URL(url,location.href),mode,moduleUrl);
  update(change.regions);
  if(change.canvas_wake)wake();
  return app.state().filter_load.request_id;
}
