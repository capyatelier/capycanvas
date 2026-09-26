import init, { WebApp, WebGpu, configure_raster_worker, automatic_tab_names } from "./pkg/layer_web.js";
import { createRasterWorker } from "./raster-worker-client.js";
import { createDocumentStorage } from "./document-storage.js";
import { workspaceStore, modulePromise, setWorkspaceWake } from "./workspace-preload.js";
import { createWorkspaceManager } from "./workspace-manager.js";
import { createPreferences } from "./preferences.js";
import { createCommandBar } from "./command-bar.js";
import { showGpuNotice } from "./gpu.js";
import { createCustomization } from "./customization.js";
import { createEditorPanels } from "./editor-panels.js";
import { createWorkspaceChrome } from "./workspace-chrome.js";
import { createGlass } from "./glass.js";
import { createDocuments } from "./documents.js";
import { createSystemStatus } from "./system-status.js";
import { createHeader } from "./header.js";
import { createNumberField } from "./numeric.js";
import { createSelectionUi } from "./selection-masks.js";
import { createLayerPanel } from "./layers.js";
import { createPalettes } from "./palettes.js";
import { createEffectPanels, fetchFilterPackage } from "./effects.js";
import { installTooltips } from "./tooltips.js";
import { installPenScrolling } from "./pen-scroll.js";

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
// Workspace extent changes only with its viewport, not with panel content.
// Retain it so chrome notifications do not force style/layout after DOM writes.
let workspaceViewport = [workspace.clientWidth, workspace.clientHeight];
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
let refreshPreferences, customization, layerPanel, effectPanels, palettes, editor, selectionUi, workspaceChrome, glass, documents, systemStatus, header;
let commandBar;
const fullscreenRequests = new Set();
let gpuStarting = false;
let gpuReady = false;
let compilerScheduled = false, compilerFailed = false, compilerEpoch = 0;
let compilerResumeTimer;
const startupTimes = { canvas: null, document: null, brush: null, complete: null };
installTooltips();
installPenScrolling();
let startupNotice;
let firstCanvasRendered = false;
let servicingRequests = false;
const settingsKey = "layer.preferences.v1", workspaceKey = "layer.workspace.v1";
let savedWorkspace = "";
let workspaceManager;
const pending = [];
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
let appliedTheme;
applyTheme(systemTheme.matches ? "dark" : "light");

function applyTheme(theme, palette) {
  const key = JSON.stringify([theme, palette]);
  if (key === appliedTheme) return;
  appliedTheme = key;
  document.body.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
  document.querySelector('meta[name="color-scheme"]').content = theme;
  if (palette) for (const [name, color] of Object.entries(palette)) {
    if (typeof color === "string") document.body.style.setProperty(`--${name.replaceAll("_", "-")}`, name === "button" ? `${color}0d` : color);
  }
  if (palette) for (const [name, color] of Object.entries(palette.glass)) {
    if (Array.isArray(color)) document.body.style.setProperty(`--glass-${name.replaceAll("_", "-")}`, `rgb(${color.slice(0, 3).map(v => v * 255).join(" ")} / ${color[3]})`);
  }
  glass?.queue();
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
// Read all dirty scrollbar geometry before changing any thumb. Mutation and
// resize observers can report several panels in one update.
const dirtyScrollbars = new Set();
let scrollbarFrame;
function queueScrollbar(read) {
  dirtyScrollbars.add(read);
  if (scrollbarFrame) return;
  scrollbarFrame = requestAnimationFrame(() => {
    scrollbarFrame = null;
    const writes = [...dirtyScrollbars].map(read => read());
    dirtyScrollbars.clear();
    for (const write of writes) write();
  });
}
function panelFrame(panel, scrollable = true) {
  const frame = element("div", "panel-frame");
  frame.append(panel);
  if (!scrollable) return frame;
  const thumb = element("div", "scroll-thumb");
  thumb.setAttribute("aria-hidden", "true");
  frame.append(thumb);
  let origin;
  const read = () => {
    const visible = panel.clientHeight, total = panel.scrollHeight, top = panel.scrollTop;
    const overflow = total - visible;
    const height = total ? Math.max(28, visible ** 2 / total) : 0;
    return () => {
      thumb.hidden = !visible || overflow <= 1;
      thumb.style.height = `${height}px`;
      thumb.style.top = `${overflow > 0 ? (top / overflow) * (visible - height) : 0}px`;
    };
  };
  const update = () => queueScrollbar(read);
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
  const response = await fetch(asset("icons.svg"));
  if (!response.ok) throw new Error("Cannot load application icons");
  const document = new DOMParser().parseFromString(await response.text(), "image/svg+xml");
  if (document.querySelector("parsererror")) throw new Error("Invalid application icons");
  for (const svg of document.documentElement.children) {
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    icons.set(svg.dataset.asset, svg);
  }
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
function draggable(node, item, pickup = item.kind === "tile" ? "hold" : "immediate") {
  // All workspace contacts share stable capture, including mouse tile reorders.
  // Classify the visible source separately from its Rust docking payload.
  node.draggable = false;
  node.dataset.workspaceDrag = JSON.stringify({ type: "drag_workspace", item });
  node.dataset.dragPickup = pickup;
  return node;
}
function grip(item) {
  const node = button("", () => {}, "panel-grip");
  node.title = "Drag to move panel";
  node.setAttribute(
    "aria-label",
    item.kind === "column" ? "Move column" : item.kind === "group" ? "Move all tabs" : "Move " + (customization?.view(item.panel)?.title || panelNames[item.panel] || "toolbar"),
  );
  node.append(icon("grip"));
  return draggable(node, item);
}
const dropIndicator = element("div", "drop-indicator");
dropIndicator.hidden = true;
workspace.append(dropIndicator);

function dispatch(action) {
  try {
    if(documents?.busy()&&!['complete_request','measure_panels','measure_titlebar','measure_workspace_bottom','measure_column_drawers','measure_drawer_tiles','measure_column_scroll'].includes(action.type))return;
    if (action.type === "measure_column_drawers" && workspaceGesture) workspaceGesture.hits = null;
    if (["move_panel", "move_group", "move_tile", "double_click_panel_handle", "reset_column_width"].includes(action.type))
      action = {
        ...action,
        viewport: workspaceViewport,
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
  if (change.regions & 512) {
    state.command_search = app.command_search();
    commandBar?.refresh(state.command_search);
    if (change.regions === 512) { if (change.canvas_wake) wake(); return; }
  }
  if(change.regions & 256) editor?.refreshColorPreview();
  if(change.regions===256){if(change.canvas_wake)wake();return;}
  if (change.regions & (1 | 2 | 4 | 128)) workspaceManager?.observe();
  if (change.regions) {
    const presentation = app.workspace_update();
    if (workspaceModelRevision !== presentation.model_revision) {
      if (workspaceContentRevision === presentation.content_revision) {
        queueWorkspaceLayout(presentation);
      } else {
        const moving = presentation.drag || workspacePresentation?.drag;
        workspaceModelRevision = presentation.model_revision;
        workspaceContentRevision = presentation.content_revision;
        workspaceLayoutPending = null;
        workspacePresentation = null;
        const patch = app.state_update();
        const reopeningCanvas = state.settings_open && patch.settings_open === false;
        Object.assign(state, patch);
        // A concurrent model change rebases retained placement too.
        if (!moving && !(change.regions & ~(16 | 128)) &&
            Object.keys(patch).every(key => key === "revision" || key === "settings_open")) {
          // Opening, closing, searching and navigating Settings do not change
          // the workspace behind it. Keep its controls and geometry intact.
          refreshPreferences(app.preferences_cached());
          updateZen();
        } else update(change.regions | (moving ? 1 : 0));
        if (reopeningCanvas) {
          deferOptionalCompiler();
          wake();
        }
      }
    } else if (change.regions & 32) {
      // Camera-only publications intentionally retain the model revision.
      state.camera = app.camera();
      update(32);
    }
    if (workspaceModelRevision === presentation.model_revision)
      queueWorkspacePresentation(presentation);
  }
  if (change.canvas_wake) wake();
  if (change.regions && !workspaceGesture?.started) wake();
}
let canvasCursorActive = false;
function cursorInput(e) {
  if (!gpuReady) return;
  const onCanvas =
    e &&
    e.pointerType !== "touch" &&
    ((e.target === canvas && lastPenEvent?.pointerId === e.pointerId) ||
      document.elementFromPoint(e.clientX, e.clientY) === canvas);
  // Clear once on exit; ordinary UI hover must not redraw the GPU viewport.
  if (!onCanvas && !canvasCursorActive) return;
  canvasCursorActive = !!onCanvas;
  if(!onCanvas){applyChange(app.input({type:"cursor_leave"}).change);return;}
  app.cursor_input(
    onCanvas
      ? new Float64Array([
          ...position(e),
          e.pointerType === "pen" ? e.pressure : 1,
          ((e.tiltX || 0) * Math.PI) / 180,
          ((e.tiltY || 0) * Math.PI) / 180,
          ((e.twist || 0) * Math.PI) / 180,
          e.timeStamp, e.pointerId, e.buttons, e.pointerType === "pen" ? ((e.buttons & 32) ? 2 : 0) : 1,
          (e.buttons & 1 ? 2 : 0) | (e.buttons & 2 ? 4 : 0),
        ])
      : new Float64Array(),
  );
  // Cursor geometry is drawn with the canvas by the shared GPU presenter.
  wake();
}
function wake() {
  if (gpuReady && !scheduled) {
    scheduled = true;
    requestAnimationFrame(frame);
  }
}
const frameIntervals = [];
let previousFrameTime;
let displayInterval = 1000 / 60;
function frame(frameTime) {
  scheduled = false;
  if (previousFrameTime !== undefined) {
    const interval = frameTime - previousFrameTime;
    if (interval > 250) frameIntervals.length = 0;
    else if (interval >= 4 && interval < 50) {
      frameIntervals.push(interval);
      if (frameIntervals.length > 32) frameIntervals.shift();
      // Missed callbacks are multiples of the display interval. Use the lower
      // tail, and let the bounded window follow a changed monitor/refresh rate.
      const sorted = frameIntervals.toSorted((a, b) => a - b);
      displayInterval = sorted[Math.floor((sorted.length - 1) * .1)];
    }
  }
  previousFrameTime = frameTime;
  // During startup the modal editor owns interaction. Resume preparation on
  // dismissal instead of competing with Settings for the UI thread/GPU.
  if (state.settings_open && startupTimes.complete === null) return;
  try {
    flushWorkspacePresentation();
    glass?.flush();
    while (pending.length) {
      const batch = pending[0],
        count = app.pen(batch.records, batch.revision);
      if (count * 11 === batch.records.length) pending.shift();
      else {
        batch.records = batch.records.subarray(count * 11);
        break;
      }
    }
    // rAF's timestamp can precede the newest input by an entire display tick.
    // Model against current time and the next estimated display opportunity.
    const now = performance.now();
    const elapsedTicks = Math.floor(Math.max(0, now - frameTime) / displayInterval);
    const presentation = frameTime + (elapsedTicks + 1) * displayInterval;
    applyChange(app.frame(now, presentation));
    refreshStartup();
    scheduleCompiler();
    if (pending.length) wake();
  } catch (error) { stopGpu(error); }
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
  const hidden = !stages.canvas || app.brush_ready();
  const text = documentReady ? "Preparing brush…" : "Preparing canvas…";
  if (startupNotice.hidden !== hidden) startupNotice.hidden = hidden;
  if (startupNotice.textContent !== text) startupNotice.textContent = text;
}
// Host timing and contacts govern admission; Rust also checks queued input,
// strokes, gestures and unfinished edits. Required jobs always retain priority.
const compilerContacts = new Set();
function deferOptionalCompiler() {
  app.shader_input();
  compilerResumeTimer ??= setTimeout(resumeOptionalCompiler, Math.ceil(app.shader_wait_ms()));
}
function resumeOptionalCompiler() {
  const remaining = app.shader_wait_ms();
  compilerResumeTimer = remaining > 0 ? setTimeout(resumeOptionalCompiler, Math.ceil(remaining)) : null;
  if (compilerResumeTimer === null) wake();
}
function optionalCompilerReady() {
  return !compilerContacts.size && !pending.length
    && !documents?.busy()
    && !document.querySelector('dialog[open],#header details[open],:popover-open:not(.hover-tooltip)');
}
for (const type of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'keydown', 'wheel']) {
  window.addEventListener(type, event => {
    if (type === 'pointerdown') compilerContacts.add(event.pointerId);
    if (type === 'pointerup' || type === 'pointercancel') compilerContacts.delete(event.pointerId);
    deferOptionalCompiler();
  }, { capture: true, passive: true });
}
window.addEventListener('blur', () => { compilerContacts.clear(); deferOptionalCompiler(); });
document.addEventListener('visibilitychange', () => { if (!document.hidden) deferOptionalCompiler(); });
document.addEventListener('toggle', deferOptionalCompiler, true);
document.addEventListener('close', deferOptionalCompiler, true);
function scheduleCompiler() {
  if (!gpuReady || document.hidden || state.settings_open || compilerScheduled || compilerFailed || !app.shader_work_pending(optionalCompilerReady())) return;
  compilerScheduled = true;
  const epoch=compilerEpoch;
  // Start after this display callback can present. The next job is scheduled
  // by a later frame, with input/UI opportunities between each GPU scope.
  setTimeout(async () => {
    try {
      if(epoch!==compilerEpoch || document.hidden || state.settings_open)return;
      if (!firstCanvasRendered) {
        // A display callback alone does not mean the GPU has rendered paper.
        // Starting document compilation sooner can hold up Chrome's GPU-process
        // command batch, including the pending first canvas presentation.
        await gpuOperation(() => app.wait_for_canvas());
        if(epoch!==compilerEpoch)return;
        firstCanvasRendered = true;
        await new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)));
      }
      if(epoch!==compilerEpoch || document.hidden || state.settings_open)return;
      await gpuOperation(() => app.compile_startup_step(optionalCompilerReady()));
      if(epoch!==compilerEpoch)return;
      refreshStartup();
      wake();
    } catch (error) {
      if(epoch===compilerEpoch){compilerFailed = true;stopGpu(error);}
    } finally {
      if(epoch===compilerEpoch)compilerScheduled = false;
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
  glass?.queue();
}
function tabLabel(tab, view, automatic = false) {
  tab.classList.toggle("icon-only-tab", !view.tab.show_name);
  if (view.tab.show_icon || automatic) tab.append(icon(view.icon));
  if (view.tab.show_name || automatic) { const label = element("span", "", view.title); label.hidden = !view.tab.show_name; tab.append(label); }
}
const fullTabWidths = new Map(), pendingTabFits = new Set();
let tabFitFrame = 0;
const tabFit = new ResizeObserver(entries => {
  for (const { target } of entries) pendingTabFits.add(target);
  tabFitFrame ||= requestAnimationFrame(fitTabs);
});
function automaticTabs(list) { list.dataset.automatic = "true"; tabFit.observe(list); }
function releaseTabs(root) { for (const list of root.querySelectorAll("[data-automatic]")) tabFit.unobserve(list); }
function fitTabs() {
  tabFitFrame = 0;
  const lists = [...pendingTabFits].filter(list => list.isConnected && list.dataset.automatic); pendingTabFits.clear();
  const size = document.documentElement.style.getPropertyValue("--ui-text-size");
  const key = tab => `${tab.dataset.panel}\n${tab.getAttribute("aria-label")}\n${size}`;
  const missing = lists.flatMap(list => [...list.children]).filter(tab => !fullTabWidths.has(key(tab)));
  for (const tab of missing) { tab.classList.remove("icon-only-tab"); tab.lastElementChild.hidden = false; }
  for (const tab of missing) fullTabWidths.set(key(tab), tab.getBoundingClientRect().width);
  const fits = lists.map(list => [list, list.clientWidth]).filter(([, width]) => width > 0).map(([list, width]) => {
    const tabs = [...list.children];
    return [tabs, automatic_tab_names(width, new Float32Array(tabs.flatMap(tab => [fullTabWidths.get(key(tab)), 36])))];
  });
  for (const [tabs, names] of fits) tabs.forEach((tab, i) => { tab.classList.toggle("icon-only-tab", !names[i]); tab.lastElementChild.hidden = !names[i]; });
}
function arrange(nextLayout, layoutOnly = false) {
  if (!app) return;
  clearWorkspacePlacement();
  if (workspaceGesture) workspaceGesture.hits = null;
  layout = layoutOnly ? nextLayout : app.layout(...workspaceViewport);
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
    const tabStyle = app.group_tab_style(group.id);
    const key = JSON.stringify([group.panels.map((id) => {
      const view = customization.view(id); return [id, view.title, view.tab, view.icon];
    }), group.active, group.tabs_visible, tabStyle]);
    if (node.dataset.key !== key) {
      node.dataset.key = key;
      node.dataset.panel = group.active;
      node.dataset.group = group.id;
      node.setAttribute("aria-label", customization.view(group.active).title);
      node.classList.toggle("toolbar", !!group.tiles);
      const preview = node.querySelector(".panel-preview");
      releaseTabs(preview);
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
          tabLabel(tab, view, tabStyle === "automatic");
          customization.target(tab, { kind: "panel", panel });
          tab.setAttribute("aria-selected", String(panel === group.active));
          labels.append(draggable(tab, { kind: "panel", panel }));
        });
        if (tabStyle === "automatic") automaticTabs(labels);
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
    node.classList.toggle("tool-strip", !!group.tiles && !group.tabs_visible);
    if (group.tiles) node.style.setProperty("--tile-radius", `${customization.view(group.active).tile_corner_radius}px`);
    node.dataset.zIndex = group.floating ? String(100 + layout.groups.indexOf(group) * 2) : "1";
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
      releaseTabs(node);
      node.remove();
      groups.delete(id);
    }
  const liveDividers = new Set();
  const handles = [
    ...layout.dividers.filter(d => !d.fixed).map(d => ({ key: `${d.band}:${d.id}`, bounds: d.bounds,
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
  if (!layoutOnly) {
    customization.arrange(layout);
  }
  workspaceChrome?.arrange(layout,layoutOnly);
  if (layoutOnly) {
    if (editor.flushPositions() && gpuReady && app.reflow_navigators()) wake();
  } else editor.queuePositions();
  place($("canvas-status"), layout.status);
  // The help occupies the same unobstructed area used for fitting the document.
  // Panels remain native UI siblings above the full-window drawing surface.
  place($("gpu-notice"), layout.work_area);
  if (!layoutOnly) {
    resizeCanvas();
    updateZen();
  }
  queuePanelMeasurements();
}
// Measure intrinsic widget content only when its width/copy changes. Rust owns
// tab growth and floating sizes; moving a float reuses these cached DOM facts.
const panelMeasurements = new Map();
let measuringPanels = false;
const measureBox = element("div", "panel-measure");
measureBox.hidden = true;
measureBox.inert = true;
measureBox.setAttribute("aria-hidden", "true"); workspace.append(measureBox);
// An isolated tree retains measurement controls without exposing duplicate IDs
// to application lookup. It uses the same CSS and inherits the workspace theme.
const measurementRoot = measureBox.attachShadow({mode:"closed"});
const measurementStyle = new CSSStyleSheet();
measurementStyle.replaceSync([...$("workspace-style").sheet.cssRules].map(rule=>rule.cssText).join("\n"));
measurementRoot.adoptedStyleSheets = [measurementStyle];
function invalidatePanelMeasurement(id) {
  panelMeasurements.get(id)?.root.remove();
  panelMeasurements.delete(id);
}
function panelContentChanged(id) {
  invalidatePanelMeasurement(id);
  queuePanelMeasurements();
}
function queuePanelMeasurements() {
  if (measuringPanels) return;
  measuringPanels = true;
  requestAnimationFrame(() => {
    measuringPanels = false;
    measurePanels();
  });
}
function measurePanels() {
  const pending = [];
  // Batch writes before reads. Intrinsic measurement keeps one offscreen DOM
  // copy per intrinsic content change, so width changes reflow it without cloning.
  for (const config of state.workspace.layout.panels) {
    const view = customization.view(config.id);
    const width = layout.groups.find(g => g.panels.includes(config.id))?.bounds.width || 232;
    // Model publication also advances for numeric values and canvas poses.
    // Intrinsic copies change only with panel structure/copy, appearance or
    // available width; each content widget invalidates its own schema below.
    const key = JSON.stringify([state.theme, view.title, view.tab, view.icon, view.controls, view.tile_style]);
    let cached = panelMeasurements.get(config.id);
    if (cached?.key !== key) {
      invalidatePanelMeasurement(config.id);
      const root = element("div");
      const tab = element("button", "dock-tab");
      tab.style.width = "max-content";
      tabLabel(tab, view);
      root.append(tab);
      const content = config.content.kind === "toolbar" ? null : panels.get(config.id).cloneNode(true);
      if (content) {
        content.style.height = "auto"; content.style.width = "100%";
        root.append(content);
      }
      cached = { key, root, tab, content };
      panelMeasurements.set(config.id, cached); measurementRoot.append(root);
    }
    // Toolbar intrinsic height is fixed at zero and its tab width is
    // independent of the available column width.
    if (cached.value && !cached.content) continue;
    if (cached.width !== width) {
      cached.width = width; cached.root.style.width = `${width}px`;
      pending.push([config.id, cached]);
    }
  }
  // Retain the copies and their measured values, but lay them out only when
  // measuring. An always-laid-out shadow tree also joins modal style/layout.
  if (pending.length) measureBox.hidden = false;
  try {
    for (const [id, cached] of pending) {
      const content_height = cached.content?.getBoundingClientRect().height || 0;
      // The unconstrained copy lays out every row, including offscreen rows.
      // Subtract the list itself to keep headers/footers outside the scroll budget.
      const list = cached.content?.querySelector(".layer-rows, .filter-picker-list, .palette-scroll");
      const row = list?.querySelector(".layer-row, .filter-row, .palette-tile");
      cached.value = {
        panel: id,
        tab_width: cached.value?.tab_width ?? cached.tab.getBoundingClientRect().width,
        content_height,
        ...(cached.content && id !== "color" && id !== "proof" ? { scroll: {
          fixed_height: list ? Math.max(0, content_height - list.getBoundingClientRect().height) : 0,
          unit_height: row ? row.getBoundingClientRect().height + (row.matches(".palette-tile") ? 4 : 0) : 0,
        }} : {}),
      };
    }
  } finally { measureBox.hidden = true; }
  for (const id of panelMeasurements.keys()) if (!panels.has(id)) invalidatePanelMeasurement(id);
  const measurements = state.workspace.layout.panels.map(config => panelMeasurements.get(config.id).value);
  // Compare against the shared publication, not a second authoritative cache.
  // Measurements are transient and deliberately omitted from saved workspace
  // state. Read Rust's live values rather than repeatedly publishing because a
  // freshly serialized full state lacks that field.
  const current = app.panel_measurements();
  if (!current || current.length !== measurements.length || measurements.some((m, i) =>
    m.panel !== current[i].panel || m.tab_width !== current[i].tab_width || m.content_height !== current[i].content_height ||
    m.scroll?.fixed_height !== current[i].scroll?.fixed_height || m.scroll?.unit_height !== current[i].scroll?.unit_height)) {
    dispatch({ type: "measure_panels", measurements });
  }
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
  for (const [panel, control] of [["brush_sets","brush_sets"],["sculpt_sets","sculpt_sets"],["tools","tools"],["tool_settings","tool_settings"],["color","color_wheel"],["navigator","navigator"]])
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
  palettes.mount(panels.get("palettes"));
  layerPanel = createLayerPanel({ app, catalog, state: () => state, panel: panels.get("layers"), element, button, icon, dispatch, applyChange, message, numberField, wake, dismissContext: () => customization.dismissContext(), contentChanged: panelContentChanged });
  effectPanels = createEffectPanels({app,wake,catalog,state:()=>state,panels,element,button,icon,dispatch,numberField,message,
    contentChanged:panelContentChanged});
}
function contentPanel(id, splitPicker=false) {
  const panel=element("div",`panel ${id}-panel`);
  if(id==="proof") {
    panel.disposePanel=documents.mountProof(panel);panel.refreshPanel=()=>{};
  } else if(id==="palettes") {
    const view=palettes.mount(panel);panel.refreshPanel=view.refresh;panel.disposePanel=view.dispose;
  } else if(id==="layers") {
    const view=createLayerPanel({app,catalog,state:()=>state,panel,element,button,icon,dispatch,applyChange,message,numberField,wake,dismissContext:()=>customization.dismissContext()});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else if(["filter_types","adjustments","properties","stats"].includes(id)) {
    const copies=new Map(["filter_types","adjustments","properties","stats"].map(name=>[name,name===id?panel:element("div","panel")]));
    const view=createEffectPanels({app,wake,catalog,state:()=>state,panels:copies,element,button,icon,dispatch,numberField,message,contentChanged:()=>{},splitPicker});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else {
    for(const control of customization.view(id).controls.filter(c=>c.visible_in_panel)) panel.append(customization.field(control.control,control.label));
    panel.refreshPanel=()=>{};
  }
  panel.refreshPanel();return panel;
}
function update(regions) {
  commandBar?.refresh(state.command_search);
  // Canvas-based controls read these colors while refreshing their pixels.
  if (regions & 16) applyTheme(state.theme, state.palette);
  if (regions & (1 | 2 | 4 | 8 | 16 | 128)) customization.refresh();
  if (regions & 2) {
    for (const [size, button] of sizeButtons) {
      const pressed = String(size === state.brush.diameter);
      if (button.getAttribute("aria-pressed") !== pressed) button.setAttribute("aria-pressed", pressed);
    }
    $("size-number").update(state.brush.diameter);
  }
  if (regions & 4) {
    const tab = state.tabs[0];
    const title = `${tab.title} · ${tab.width} × ${tab.height}`;
    if ($("document-title").textContent !== title) $("document-title").textContent = title;
    layerPanel.refresh();
    effectPanels.refresh();
  }
  if (regions & (1 | 2 | 4 | 8 | 16 | 128)) header?.refresh();
  if (regions & (4 | 8)) documents?.refresh();
  if (regions & (2 | 4 | 16)) palettes.refresh(regions);
  if (regions & (1 | 2 | 4 | 8 | 16 | 32 | 128)) { editor.refresh(); selectionUi.refresh(); workspaceChrome?.refresh(); }
  if (regions & (1 | 4 | 128)) arrange();
  if (regions & (1 | 128)) persistWorkspace();
  if (regions & (1 | 4 | 8 | 128)) refreshWorkspaceMenu();
  if (regions & (4 | 8))
    for (const command of state.commands)
      for (const node of commands.get(command.id) || []) {
        const disabled = !command.enabled || (command.id === "fullscreen" && !document.fullscreenEnabled);
        if (node.disabled !== disabled) node.disabled = disabled;
        if (node.title !== command.tooltip) node.title = command.tooltip;
        if (node.getAttribute("aria-label") !== command.label) node.setAttribute("aria-label", command.label);
        const pressed = String(command.selected);
        if (node.getAttribute("aria-pressed") !== pressed) node.setAttribute("aria-pressed", pressed);
        if (node.dataset.icon === "true") {
          const glyph = node.querySelector("svg");
          if (glyph?.dataset.asset !== command.icon) {
            const next = icon(command.icon);
            next.style.cssText = glyph?.style.cssText || "";
            node.replaceChildren(next);
          }
        } else {
          const label = node.querySelector(".command-label");
          if (label) {
            const shortcut = node.querySelector(".shortcut-hint");
            if (label.textContent !== command.label) label.textContent = command.label;
            if (shortcut.textContent !== command.shortcut) shortcut.textContent = command.shortcut;
          } else if (node.textContent !== command.label) node.textContent = command.label;
        }
      }
  if (regions & 16) {
    systemStatus?.sync();
    refreshPreferences(app.preferences_cached());
  }
  if (regions & 32) {
    const info = `${Math.round(state.camera.zoom * 100)}% · ${Math.round((state.camera.rotation * 180) / Math.PI)}°`;
    if ($("view-info").textContent !== info) $("view-info").textContent = info;
  }
  if (regions & (1 | 16)) updateZen();
  editor.flushPaint();
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
            else if (request.kind.type === "workspace") { workspaceManager?.handle(request); }
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
// Watch only until pickup: an invalidated held tile must not leave a grab cursor.
// Disconnect before dragging, when shared updates may deliberately reparent it.
const workspaceGestureSourceObserver = new MutationObserver(() => {
  const drag = workspaceGesture;
  if (drag && (!drag.node.isConnected || drag.node.parentNode !== drag.parent))
    endWorkspaceGesture(null, true);
});
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
// Shared workspace_update publication: retain content at content_revision and
// apply layout-only reflow once per display frame. Geometry-only dragging still
// retains models at model_revision, without layout or measurement work.
// Dispatch every input to Rust; replace only pending absolute presentation and
// apply it on the display clock. No scaled textures or per-motion DOM rebuilds.
let workspaceModelRevision, workspaceContentRevision, workspacePresentation, workspacePresentationFrame = 0;
let workspaceLayoutPending, workspaceLayoutFrame = 0;
function queueWorkspaceLayout(presentation) {
  workspaceLayoutPending = presentation;
  if (!workspaceLayoutFrame) workspaceLayoutFrame = requestAnimationFrame(function presentWorkspaceLayout() {
    workspaceLayoutFrame = 0;
    if (!workspaceLayoutPending) return;
    workspaceLayoutPending = null;
    // Fetch only the latest absolute layout, after all queued input reached Rust.
    const packet = app.layout_update(...workspaceViewport);
    const update = packet.workspace_update;
    if (update.content_revision !== workspaceContentRevision) return;
    workspaceModelRevision = update.model_revision;
    state.revision = update.revision;
    state.camera = packet.camera;
    Object.assign(state.workspace.layout, packet.workspace_layout);
    state.workspace.layout.measurements = packet.panel_measurements;
    arrange(packet.layout, true);
    queueWorkspacePresentation(update);
  });
}
workspace.addEventListener("scroll", () => { if (workspaceGesture) workspaceGesture.hits = null; }, true);
const workspacePlacements = new Map();
const deviceAligned = value => Math.round(value * devicePixelRatio) / devicePixelRatio;
function clearWorkspacePlacement() {
  if (workspacePlacements.size) glass?.queue();
  for (const [node, original] of workspacePlacements) {
    node.style.removeProperty("transform");
    node.style.width = original.width;
    node.style.height = original.height;
    node.style.setProperty("--panel-body-height", original.bodyHeight);
  }
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
    const group = layout.groups.find(g => g.id === moving.id), base = group?.bounds;
    if (base) {
      const dw = moving.bounds.width - base.width, dh = moving.bounds.height - base.height;
      const nodes = [groups.get(moving.id), ...[...dividers.values()].filter(n => n.dragAction.group === moving.id)];
      for (const node of nodes.filter(Boolean)) {
        if (!workspacePlacements.has(node)) workspacePlacements.set(node, {
          width: node.style.width, height: node.style.height,
          bodyHeight: node.style.getPropertyValue("--panel-body-height"),
        });
        const original = workspacePlacements.get(node), edge = node.dataset.edge;
        const x = deviceAligned(moving.bounds.x) - base.x + (edge?.includes("right") ? dw : 0);
        const y = deviceAligned(moving.bounds.y) - base.y + (edge?.includes("bottom") ? dh : 0);
        const transform = `translate(${x}px, ${y}px)`;
        if (node.style.transform !== transform) { node.style.transform = transform; glass?.queue(); }
        // Freeze native allocation too, without scaling the retained controls.
        const width = `${parseFloat(original.width) + (!edge || edge === "top" || edge === "bottom" ? dw : 0)}px`;
        const height = `${parseFloat(original.height) + (!edge || edge === "left" || edge === "right" ? dh : 0)}px`;
        if (node.style.width !== width) { node.style.width = width; glass?.queue(); }
        if (node.style.height !== height) { node.style.height = height; glass?.queue(); }
        if (!edge) {
          const bodyHeight = `${moving.bounds.height - (group.tabs_visible ? layout.tab_bar_height : 0)}px`;
          if (node.style.getPropertyValue("--panel-body-height") !== bodyHeight) node.style.setProperty("--panel-body-height", bodyHeight);
        }
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
    if (phase === "up") measurePanels();
    drag.hits = null;
  }
  dispatch({ ...drag.action, phase, position: [e.clientX, e.clientY],
    viewport: workspaceViewport,
    ...(drag.action.type === "drag_workspace" ? { tabs: drag.hits ??= tabHits() } : {}),
  });
}
function endWorkspaceGesture(e, cancel = false) {
  const drag = workspaceGesture;
  if (!drag || (e && drag.id !== e.pointerId)) return;
  workspaceGestureSourceObserver.disconnect();
  if (drag.started) {
    if (drag.tile) {
      if (!cancel) dropItem(drag.action.item, dropHint(e || drag.last, drag.action.item));
      dragItem = null; drag.node.classList.remove("drag-source");
    } else workspaceGestureEvent(cancel ? "cancel" : "up", e || drag.last);
  }
  if (cancel && drag.context) customization.dismissContext();
  workspaceGesture = null;
  clearTabSlide(drag);
  workspaceCursor(null);
  if (workspace.hasPointerCapture(drag.id)) workspace.releasePointerCapture(drag.id);
  dropIndicator.hidden = true;
  if (drag.started || drag.held || drag.context) { revealPointer = drag.id; e?.preventDefault(); e?.stopPropagation(); }
  updateZen();
}
workspace.addEventListener("pointerdown", e => {
  if (e.button !== 0 || !e.isPrimary || workspaceGesture) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  workspaceGesture = { id: e.pointerId, action: JSON.parse(node.dataset.workspaceDrag),
    start: e, last: e, started: false, node, parent: node.parentNode,
    waitForHold: node.dataset.dragPickup === "hold", cursor: getComputedStyle(node).cursor };
  workspaceGesture.tile = workspaceGesture.action.item?.kind === "tile";
  if (workspaceGesture.waitForHold)
    workspaceGestureSourceObserver.observe(workspace, { childList: true, subtree: true });
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
    if (!drag.node.isConnected || drag.node.parentNode !== drag.parent) {
      endWorkspaceGesture(e, true); return;
    }
    const distance = Math.hypot(e.clientX - drag.start.clientX, e.clientY - drag.start.clientY);
    if (distance <= (drag.action.type === "drag_workspace" ? 8 : 0)) return;
    if (drag.waitForHold && !drag.held) { endWorkspaceGesture(e, true); return; }
    customization.dismissContext();
    drag.started = true;
    workspaceGestureSourceObserver.disconnect();
    // Capture on the stable workspace before Rust tears off/rebuilds a tab.
    workspace.setPointerCapture(e.pointerId);
    groups.forEach(node => node.getAnimations().forEach(a => a.cancel()));
    if (drag.tile) {
      dragItem = drag.action.item; drag.node.classList.add("drag-source"); updateZen();
    } else {
      startTabSlide(drag);
      workspaceGestureEvent("down", drag.start);
      if (drag.tabSlide) app.begin_tab_drag(drag.tabSlide.tabs.map(t => t.hit), drag.tabSlide.clip);
    }
  }
  if (drag.tile) showDropHint(dropHint(e, drag.action.item));
  else workspaceGestureEvent("move", e);
  if (drag.action.type === "drag_workspace") {
    workspaceCursor("grabbing");
  } else {
    workspaceCursor(drag.cursor);
  }
  e.preventDefault(); e.stopPropagation();
}, { capture: true });
workspace.addEventListener("touchmove", e => {
  if (workspaceGesture?.context && e.touches.length === 1) e.preventDefault();
}, { passive: false });
window.addEventListener("pointerup", e => endWorkspaceGesture(e), { capture: true });
window.addEventListener("pointercancel", e => endWorkspaceGesture(e, true), { capture: true });
workspace.addEventListener("lostpointercapture", e => {
  // Touch starts with implicit capture on the tab. Transferring capture to the
  // stable workspace releases that child; only losing our own capture cancels.
  if (e.target === workspace || (!workspace.hasPointerCapture(e.pointerId)
    && workspaceGesture?.node.contains(e.target))) endWorkspaceGesture(e, true);
});
function armWorkspaceDrag(drag) {
  drag.held = true;
  if (drag.waitForHold && ["mouse", "pen"].includes(drag.start.pointerType))
    workspaceCursor("grab");
  // Keep this contact even when a context menu covers the original tile/tab.
  workspace.setPointerCapture(drag.id);
}
workspace.addEventListener("workspace-context-claimed", e => {
  const drag = workspaceGesture;
  if (drag && (drag.node.contains(e.target) || e.target.contains(drag.node))) {
    // A late native contextmenu event must not interrupt an existing drag.
    if (drag.started) { e.preventDefault(); return; }
    drag.context = true;
    armWorkspaceDrag(drag);
  } else endWorkspaceGesture(null, true);
});
workspace.addEventListener("workspace-drag-held", e => {
  const drag = workspaceGesture;
  if (drag?.waitForHold && !drag.started && drag.node.contains(e.target)) {
    armWorkspaceDrag(drag);
  }
});
workspace.addEventListener("dblclick", e => {
  const column = e.target.closest(".collapsed-column");
  if (column && !e.target.closest("button")) {
    e.preventDefault(); e.stopPropagation();
    dispatch({ type: "customize", action: {
      type: "set_column_collapsed", group: Number(column.dataset.column), collapsed: false,
    } });
    return;
  }
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
    const capy = $("zen-capy");
    if (capy && capy.hidden !== !reply.keep_zen_button)
      capy.hidden = !reply.keep_zen_button;
    const cursor = reply.pan_cursor ? "grab" : "";
    if (canvas.style.cursor !== cursor) canvas.style.cursor = cursor;
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
  const capy = $("zen-capy");
  const capyBounds = capy && !capy.hidden ? capy.getBoundingClientRect() : null;
  return input({
    type: "chrome",
    event,
    viewport: workspaceViewport,
    facts: {
      zen_button: capyBounds ? {x:capyBounds.x,y:capyBounds.y,width:capyBounds.width,height:capyBounds.height} : null,
      expanded_panel: customization?.placement(),
      ...workspaceChrome?.facts(),
      contact_tab: event.kind === "contact"
        ? document.elementFromPoint(...event.position)?.closest(".dock-tab")?.dataset.panel ?? null
        : null,
      held: chromeHeld,
      dragging: dragItem !== null,
      popup_open:
        !!(document.querySelector("dialog[open], details[open]") || document.querySelector(":popover-open:not(.hover-tooltip)")) ||
        !!document.activeElement?.matches("select"),
    },
  });
}
function updateZen() {
  const hidden = workspace.classList.contains("zen-hidden");
  chromeInput({ kind: "refresh" });
  if (hidden !== workspace.classList.contains("zen-hidden")) {
    workspaceChrome?.refresh();
    editor?.queuePositions();
    glass?.queue();
  }
}
function buildHeader() {
  systemStatus = createSystemStatus({element, changed:fullscreen => {
    if(customization && state.fullscreen !== fullscreen) dispatch({type:"window_fullscreen",fullscreen});
    header?.queue();
  }});
}
function refreshWorkspaceMenu() {
  for(const menu of document.querySelectorAll('#header details[open]')) menu.refreshMenu?.();
}
function persistWorkspace() {
  workspaceManager?.observe();
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
    if (e.target.closest?.("dialog[open]")) return;
    if (!e.buttons) chromeHeld = false;
    // A captured paint contact cannot reveal chrome. The canvas listener owns
    // its samples/cursor; querying popups and hit-testing here duplicates work
    // and can force layout between input and the next drawing submission.
    if (!(e.target === canvas && lastPenEvent?.pointerId === e.pointerId))
      chromeInput({ kind: "motion", position: [e.clientX, e.clientY] });
    if (e.target !== canvas) cursorInput(e);
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
    // Native DOM modals own their contacts. Workspace transitions deliberately
    // consume editor input, so forwarding a dialog contact would swallow its
    // buttons before the DOM click handler can run.
    if (e.target.closest("dialog[open]")) return;
    if (e.target === canvas && e.pointerType === "pen" && !(e.buttons & 33)) {
      e.preventDefault();
      return;
    }
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
function position(e, rect = canvas.getBoundingClientRect()) {
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
function queuePen(e, stage, predictionsOnly = false) {
  const predict = state.settings.feedback && state.settings.platform_prediction;
  if (predictionsOnly && !predict) return;
  if (!predictionsOnly) {
    lastPenEvent = stage === 3 || stage === 4 ? null : e;
    if (stage !== 2) rawPenPointer = null;
  }
  const records = [];
  const rect = canvas.getBoundingClientRect();
  const append = (item, predicted) => {
    const [x, y] = position(item, rect),
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
      predicted ? 3 : 2,
      pen ? (item.buttons & 32 ? 2 : 0) : 1,
    );
  };
  if (!predictionsOnly) {
    const history = stage === 2 ? e.getCoalescedEvents?.() || [] : [];
    for (const item of history.length ? history : [e]) append(item, false);
  }
  if (stage === 2 && e.pointerType === "pen" && predict) {
    const latestActualTime = Math.max(e.timeStamp, lastPenEvent?.timeStamp ?? 0);
    for (const item of e.getPredictedEvents?.() || []) {
      if (item.timeStamp > latestActualTime) append(item, true);
    }
  }
  if (!records.length) return;
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
// Use raw updates only for the captured painting contact. UI rows, touch
// navigation and hover keep their ordinary pointermove arbitration. Observe
// actual raw delivery, so browsers/devices without it still paint via moves.
let rawPenPointer = null;
const canvasPenContacts = new Set();
const canvasFingers=new Set();
let pickerHold=null;
function cancelPickerHold(){if(pickerHold)clearTimeout(pickerHold.timer);pickerHold=null;}
window.addEventListener('blur',()=>{cancelPickerHold();canvasFingers.clear();});
function pickerTouch(e,stage){
  if(e.pointerType!=='touch')return;
  if(stage===1){
    canvasFingers.add(e.pointerId);cancelPickerHold();
    if(canvasFingers.size===1){
      pickerHold={id:e.pointerId,x:e.clientX,y:e.clientY,timer:setTimeout(()=>{
        pickerHold=null;
        applyChange(app.input({type:'color_picker_hold',id:corePointerId(e),position:position(e),offset:44*(devicePixelRatio||1)}).change);
      },500)};
    }
  } else if(stage===2&&pickerHold?.id===e.pointerId&&Math.hypot(e.clientX-pickerHold.x,e.clientY-pickerHold.y)>8)cancelPickerHold();
  else if(stage===3||stage===4){canvasFingers.delete(e.pointerId);cancelPickerHold();}
}

function canvasPointer(e, stage) {
  pickerTouch(e,stage);
  if (e.cancelable) e.preventDefault();
  if (e.pointerType === "pen") {
    const active = canvasPenContacts.has(e.pointerId), touching = !!(e.buttons & 33);
    // DOM button chording reports tip down/up as pointermove while a barrel
    // button is held. Only tip/eraser contact starts drawing or navigation.
    if (stage === 1 && !touching) return;
    if (stage === 2 && !active && touching && (e.button === 0 || e.button === 5)) stage = 1;
    if (stage === 2 && active && !touching &&
        (e.pressure === 0 || e.button === 0 || e.button === 5)) stage = 3;
    if (stage === 1) canvasPenContacts.add(e.pointerId);
  }
  if (stage === 3 || stage === 4) canvasPenContacts.delete(e.pointerId);
  if (
    stage === 2 && e.type === "pointermove" && rawPenPointer === e.pointerId &&
    ((e.buttons & 33) || e.pressure !== 0)
  ) {
    // The real samples were already delivered by pointerrawupdate. Chrome's
    // native predictions usually arrive only with the matching pointermove.
    queuePen(e, stage, true);
    return;
  }
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
      (e.type !== "pointerup" && stage === 3))) {
    sample = lastPenEvent;
    stage = 3;
  }
  const reply = pointerInput(sample, stage);
  if (reply.paint) queuePen(sample, stage);
  // Normal release drops capture after pointerup already restored hover.
  if (e.type === "lostpointercapture" && !reply.handled) return;
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
if ("onpointerrawupdate" in globalThis) {
  canvas.addEventListener("pointerrawupdate", e => {
    if (lastPenEvent?.pointerId !== e.pointerId || e.pointerType !== "pen") return;
    rawPenPointer = e.pointerId;
    canvasPointer(e, 2);
  }, { passive: true });
}
function pointerInput(e, stage, point = position(e)) {
  return input({
    type: "pointer",
    id: BigInt(corePointerId(e)),
    phase: ["move", "down", "move", "up", "cancel"][stage],
    kind: e.pointerType || "mouse",
    button:
      e.pointerType === "pen" || e.button === 0 || e.button === 5
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
  if (pressed && e.target instanceof Element && e.target.closest("dialog[open]:not(#settings, #shortcut-capture, #shortcut-editor)")) return;
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
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && workspaceGesture) { endWorkspaceGesture(null, true); e.preventDefault(); return; }
  if(documents?.key(e))return;
  keyInput(e, true);
});
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
      viewport: workspaceViewport,
      position: [e.clientX, e.clientY],
      tabs: tabHits(),
      item,
      expansion: customization.placement(),
    });
  } catch {
    return null;
  }
}
function showDropHint(hint) {
  dropIndicator.hidden = !hint;
  if (hint) {
    place(dropIndicator, hint.bounds);
    dropIndicator.dataset.kind = hint.target.kind;
    dropIndicator.classList.toggle("drop-body", hint.target.kind === "tab" && hint.bounds.width > 3 && hint.bounds.height > 3);
  }
}
function dropItem(item, hint) {
  if (hint && item.kind === "tile")
    dispatch({ type: "move_tile", panel: item.panel, tile: item.tile, target: hint.target });
}
try {
  // Compile once and share the immutable module with workspace storage. Its
  // independent instance keeps validation off the UI thread without fetching
  // and compiling the whole application a second time.
  performance.mark("capy.startup.module");
  setWorkspaceWake(() => workspaceManager?.wake());
  const [wasmModule] = await Promise.all([modulePromise, loadIcons()]);
  workspaceStore.initialize(wasmModule);
  await init({module_or_path: wasmModule});
  performance.mark("capy.startup.wasm");
  // Let the browser start the worker while the main thread builds controls.
  await new Promise(resolve => setTimeout(resolve, 0));
  const fileWorker = createRasterWorker(),documentStorage=createDocumentStorage();
  const rasterWorker = request=>request.operation.startsWith('tab-')?documentStorage(request):fileWorker(request);
  configure_raster_worker(rasterWorker);
  canvas.width = 800;
  canvas.height = 600;
  app = WebApp.create(canvas);
  app.prediction_availability(typeof globalThis.PointerEvent?.prototype.getPredictedEvents === "function");
  let restoreError, workspaceRestoreError;
  try {
    const saved = localStorage.getItem(settingsKey);
    if (saved) app.dispatch({ type: "restore_settings", settings: JSON.parse(saved) });
  } catch (error) { restoreError = `Cannot restore preferences: ${error}`; }
  try {
    const saved = localStorage.getItem(workspaceKey);
    if(saved) { app.dispatch({type:"restore_workspace", workspace:JSON.parse(saved)}); savedWorkspace=saved; }
  } catch(error) { restoreError = workspaceRestoreError = `Cannot restore workspace: ${error}`; }
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
  state = app.state_update();
  catalog = app.catalog();
  performance.mark("capy.startup.model");
  document.documentElement.style.setProperty("--ui-text-size", `${catalog.text_size_pt}pt`);
  document.title = `${catalog.app_name} — drawing workspace`;
  refreshPreferences = createPreferences({ app, element, button, icon, numberField, panelFrame, dispatch, view: () => app.preferences_cached() });
  commandBar = createCommandBar({element, button, icon, dispatch, style:catalog.command_search_style, canvas, layoutChanged:() => glass?.queue()});
  panelNames = Object.fromEntries(catalog.panels.map((p) => [p.id, p.label]));
  // Issue the first storage request before constructing panel controls. Replies
  // run in later tasks, after this synchronous UI construction is complete.
  workspaceManager = createWorkspaceManager({ app, store: workspaceStore, applyChange, element, button, icon, message, dispatch, hasLegacy: !!savedWorkspace || !!workspaceRestoreError, legacyError: workspaceRestoreError });
  selectionUi = createSelectionUi({app,state:()=>state,element,button,icon,numberField,dispatch});
  editor = createEditorPanels({selectionUi,app,state:()=>state,workspace,canvas,element,button,icon,numberField,dispatch,asset,wake,applyChange,contentChanged:panelContentChanged});
  palettes = createPalettes({ app, state: () => state, workspace, element, button, icon, panelFrame, applyChange, rasterWorker,
    dismissContext: () => customization?.dismissContext(), contentChanged: panelContentChanged });
  buildHeader();
  buildPanels();
  customization = createCustomization({ app, catalog, state: () => state, workspace, panels, groups,
    element, button, icon, numberField, panelFrame,
    dispatch, draggable, grip, place, updateZen, editor });
  workspaceChrome = createWorkspaceChrome({app,state:()=>state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,panels,draggable,grip,contentPanel,tabLabel,automaticTabs,releaseTabs});
  glass = createGlass({app,canvas,workspace,connections:()=>workspaceChrome.connections(),enabled:()=>state.palette?.glass.transparency!=="off",wake});
  documents = createDocuments({app,state:()=>state,canvas,dispatch,applyChange,wake,element,button,icon,numberField,message,gpuOperation,rasterWorker,resumeCanvas:resumeDocumentCanvas,contentChanged:panelContentChanged});
  documents.mountProof(panels.get("proof"));
  header = createHeader({app,state:()=>state,workspace,element,button,icon,place,dispatch,customization,systemStatus,updateZen,documents});
  const capy = iconButton("zen_mode");
  capy.id = "zen-capy"; capy.hidden = true;
  customization.target(capy, {kind:"zen_mode"});
  workspace.append(capy);
  performance.mark("capy.startup.controls");
  update(255);
  systemStatus.sync();
  $("status").textContent = "";
  if (restoreError) message(restoreError);
  new ResizeObserver(() => {
    workspaceViewport = [workspace.clientWidth, workspace.clientHeight];
    arrange();
  }).observe(workspace);
  // Test harness accesses the actual Wasm instance and native widgets.
  window.layerApp = { app, dispatch, state: () => app.state(), wake, canvas, loadFilters, startupTimes, documents, restartGpu };
  performance.mark("capy.startup.ui");
  // Present the controls and let storage replies run before GPU setup starts.
  await new Promise(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)));
  // Adopt the saved UI before competing with initial GPU allocation. A storage
  // failure must still allow canvas startup and the workspace recovery UI.
  await Promise.race([workspaceManager.ready, new Promise(resolve => setTimeout(resolve, 1000))]);
  documents.startRecovery().catch(error => message(`Recovery unavailable: ${error}`));
  performance.mark("capy.startup.gpu");
  await startGpu();
  if (window.launchQueue?.setConsumer) {
    let launches=Promise.resolve();
    window.launchQueue.setConsumer(params=>{
      const files=params.files??[];
      launches=launches.then(async()=>{
        if(!files.length)return;
        const deadline=performance.now()+240000;
        while(documents.busy()||!app.document_park_ready()||document.querySelector('dialog[open]')) {
          if(performance.now()>deadline)throw Error('Finish the current operation, then open the files again.');
          await new Promise(resolve=>setTimeout(resolve,50));
        }
        const selected=[];for(const handle of files)selected.push({file:await handle.getFile(),handle});
        await documents.openFiles(selected);
      }).catch(error=>message(String(error)));
    });
  }
} catch (error) {
  $("gpu-notice").replaceChildren(element("h1", "", "Capy Canvas could not load"),
    element("p", "", "Reload the page. If the problem continues, check that the complete app package is being served."), element("pre", "", String(error)));
  $("status").textContent = "";
  console.error(error);
}

function stopGpu(error) {
  // A retired device may finish compilation late, or never settle its Promise.
  // Neither case may hold the new renderer’s compilation lane or stop it.
  compilerEpoch++;compilerScheduled=false;
  gpuReady=false;pending.length=0;
  applyChange(app.suspend_gpu());
  if(startupNotice)startupNotice.hidden=true;
  const notice=$("gpu-notice");notice.hidden=false;
  notice.replaceChildren(element("p","",String(error)),button("Restart Canvas",()=>restartGpu()));
  document.body.dataset.gpu="unavailable";
}
setInterval(()=>{if(gpuReady){const error=app.gpu_failure();if(error)stopGpu(error);}},1000);
async function restartGpu() {
  if(gpuReady)stopGpu("Restarting canvas…");
  compilerFailed=false;firstCanvasRendered=false;
  for(const key of Object.keys(startupTimes))startupTimes[key]=null;
  await startGpu();
}

async function resumeDocumentCanvas() {
  compilerEpoch++;compilerScheduled=false;compilerFailed=false;
  gpuReady=app.gpu_ready();firstCanvasRendered=false;pending.length=0;
  for(const key of Object.keys(startupTimes))startupTimes[key]=null;
  if(!gpuReady){
    try{gpuReady=app.resume_document_gpu();}
    catch(error){stopGpu(error);return;}
  }
  if(!gpuReady)await startGpu();
  else {document.body.dataset.gpu='ready';$('gpu-notice').hidden=true;wake();}
  // Renderer replacement refreshes shared command availability. Publish that
  // change after attachment so retained controls do not keep their parked state.
  if(gpuReady){update(255);wake();}
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
    // The immutable application bundle already contains its filter catalog.
    // Runtime imports still validate atomically through loadFilters().
  } catch (error) {
    document.body.dataset.gpu = "unavailable";
    showGpuNotice({ container: notice, error, element, button });
    console.warn("GPU canvas unavailable:", error?.message ?? error);
  } finally {
    gpuStarting = false;
  }
}

async function createGpu() {
  return gpuOperation(() => WebGpu.create(canvas, app.document_color()));
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

async function loadFilters(url, mode="add", moduleUrl, libraryOnly=false) {
  const change=await fetchFilterPackage(app,new URL(url,location.href),mode,moduleUrl,libraryOnly);
  update(change.regions);
  if(change.canvas_wake)wake();
  return app.state().filter_load.request_id;
}
