import { addScrollbar } from "./scrollbars.js";
import { observeButtonFeedback } from "./button-feedback.js";
import { edgeContactClass, edgeResetClass } from "./edge-reach.js";
import { brushSizeGrid } from "./brush-sizes.js";
import init, { WebApp, WebGpu, configure_raster_worker, configure_gpu_diagnostics } from "./pkg/layer_web.js";
import { createRasterWorker } from "./raster-worker-client.js";
import { createPaintPairIcon } from './color-controls.js';
import { chooseColor, configureColorEditor, refreshColorEditors } from './color-editor.js';
import { refreshTextureDialogs } from './paper-picker.js';
import { createDocumentStorage } from "./document-storage.js";
import { workspaceStore, modulePromise, setWorkspaceWake } from "./workspace-preload.js";
import { createWorkspaceManager } from "./workspace-manager.js";
import { createPreferences } from "./preferences.js";
import {createPenResponse} from "./pen-response.js";
import {observePenInput} from "./pen-input.js";
import { createCommandBar } from "./command-bar.js";
import { showGpuNotice } from "./gpu.js";
import { createGpuDiagnostics, gpuErrorText, gpuReportDetails, suspendGpuForReport } from './gpu-diagnostics.js';
import { createCustomization } from "./customization.js";
import {createDropdown} from './dropdown.js';
import { createCanvasBar } from "./canvas-bar.js";
import { createNotice } from "./notice.js";
import { createZoomReadout } from "./zoom-readout.js";
import { createScreenStatus } from "./screen-status.js";
import { screenInfo } from "./new-drawing.js";
import { createEditorPanels } from "./editor-panels.js";
import { createWorkspaceChrome, tabHover, releaseTabSurfaces, prepareTabPreview, orderPanelTabs } from "./workspace-chrome.js";
import { createGlass } from "./glass.js";
import { createPanelVectors } from "./panel-vectors.js";
import { createBubbles } from "./bubbles.js";
import { createDocuments } from "./documents.js";
import { createSystemStatus } from "./system-status.js";
import { createHeader } from "./header.js";
import { createNumberField } from "./numeric.js";
import { captureTextComposition, composingKey, latinKey } from "./text-input.js";
import { createSelectionUi } from "./selection-masks.js";
import { createPreviewPanel } from "./preview-panel.js";
import { createCanvasSizeUi } from "./canvas-size.js";
import { createImageSizeUi } from "./image-size.js";
import { createLayerPanel } from "./layers.js";
import { createPalettes } from "./palettes.js";
import { createEffectPanels } from "./effects.js";
import { installTooltips } from "./tooltips.js";
import { installPenScrolling } from "./pen-scroll.js";
import { bindCopy, liveCopy, refreshBindings, refreshCopy } from "./localization.js";

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
captureTextComposition(document);
const workspace = $("workspace"),
  canvas = $("canvas"),
  center = $("center");
// Workspace extent changes only with its viewport, not with panel content.
// Retain it so chrome notifications do not force style/layout after DOM writes.
let workspaceViewport = [workspace.clientWidth, workspace.clientHeight];
const commands = new Map();
let sizePresets;
let app,
  catalog,
  bootstrap,
  delivery,
  panelNames,
  state,
  scheduled = false,
  layout,
  lastPenEvent = null,
  heldPointers = new Map(),
  dragItem = null,
  statusTimer,
  statusError = null,
  shownHostError = null;
let penResponse, refreshPreferences, customization, layerPanel, effectPanels, palettes, editor, selectionUi, frequencySeparationUi, canvasSizeUi, imageSizeUi, workspaceChrome, panelVectors, glass, documents, systemStatus, header, canvasBar, notice, zoomReadout, screenStatus;
let commandBar, paintPairView;
let bubbles;
observeButtonFeedback(window);
const compilerContacts = new Set();
for (const type of ['pointerdown', 'pointermove', 'pointerup', 'pointercancel', 'keydown', 'wheel']) {
  window.addEventListener(type, event => {
    if (event.isTrusted && type === 'pointerdown') compilerContacts.add(event.pointerId);
    if (event.isTrusted && (type === 'pointerup' || type === 'pointercancel')) compilerContacts.delete(event.pointerId);
    deferOptionalCompiler();
  }, { capture: true, passive: true });
}
for (const type of ["pointerdown", "pointermove"]) window.addEventListener(type, pointerStyle, true);
window.addEventListener(
  "pointercancel",
  e => {
    heldPointers.delete(e.pointerId);
    updateZen();
  },
  { capture: true },
);
for (const name of ["pointerup", "lostpointercapture"]) window.addEventListener(
  name,
  e => {
    const owner=heldPointers.get(e.pointerId);if(!owner)return;
    setTimeout(()=>{
      if(heldPointers.get(e.pointerId)!==owner)return;
      heldPointers.delete(e.pointerId);updateZen();
    },0);
  },
  { capture: true },
);
for (const type of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture','contextmenu','click','dblclick'])
  window.addEventListener(type,e=>{if(!e.target.closest?.('.utility-panel'))bubbles?.handle(e);},{capture:true,passive:false});
const fullscreenRequests = new Set(), colorSlotRequests = new Set();
let gpuStarting = false;
let gpuReady = false;
let compilerScheduled = false, compilerFailed = false, compilerEpoch = 0;
let compilerResumeTimer;
const startupTimes = { canvas: null, document: null, brush: null, complete: null };
installTooltips();
installPenScrolling();
let startupNotice;
let firstCanvasRendered = false;
const gpuDiagnostics=createGpuDiagnostics();
let wasmMemory;
let servicingRequests = false;
const settingsKey = "layer.preferences.v1";
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
  const rgba = color => `rgb(${color.slice(0, 3).map(v => v * 255).join(" ")} / ${color[3]})`;
  if (palette) for (const [name, color] of Object.entries(palette.glass)) {
    if (Array.isArray(color)) {
      const alpha8 = Math.round(color[3] * 255);
      const channels = color.slice(0, 3).map(value => alpha8 ? Math.round(value * alpha8) / alpha8 : 0);
      document.body.style.setProperty(`--glass-${name.replaceAll("_", "-")}`, rgba([...channels, alpha8 / 255]));
    }
  }
  if (palette) {
    document.body.style.setProperty("--control-fill", rgba(palette.control_fill));
    const { settings, panel, sidebar_border } = palette.lines;
    for (const [name, color] of [["card-shade", settings.card_shade], ["sidebar-edge", sidebar_border], ["menu-divider", panel.border]]) {
      document.body.style.setProperty(`--${name}`, rgba(color));
    }
    document.body.style.setProperty("--card-shadow", `0 0 0 1px ${rgba(settings.card_ring)}, 0 1px 3px 1px ${rgba(settings.card_shadow)}, 0 2px 6px 2px ${rgba(settings.card_far_shadow)}`);
  }
  glass?.queue();
  document.querySelector('meta[name="theme-color"]').content = palette?.bg || (theme === "dark" ? "#333333" : "#b8b8b8");
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (typeof text === "function") bindCopy(node, text);
  else if (text != null) node.textContent = text;
  return node;
}
function message(error) {
  statusError = error;
  refreshMessage();
  clearTimeout(statusTimer);
  statusTimer = setTimeout(() => {
    statusError = null;
    $("status").textContent = "";
  }, 7000);
}
function refreshMessage() {
  if(statusError!==null)$("status").textContent=typeof statusError==="function"?statusError():app.color_feature_error_copy(statusError);
}
function answerNotice(id, accept, action = null) {
  if (accept) return dispatch({ type: "notice", id, accept, action });
  try { applyChange(app.dispatch({ type: "notice", id, accept, action })); } catch {}
}
function button(text, action, className = "") {
  const node = element("button", className, text);
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}
function dropdown(label,change) { return createDropdown({element,button,icon,openMenu:node=>customization.openMenu(node)},label,change); }
function numberField(control, label, onChange, inline = false, valueOnly = false, panel = true) {
  const read = typeof label === "function" ? label : null;
  const number = createNumberField({ control, label:read ? read() : label, labels:next => app.numeric_labels(next), onChange, inline, valueOnly, panel, icon, resolve: request => app.number_input(request), errorCaption: reason => app.native_caption({type:"numeric_error",reason}) });
  if(read){Object.defineProperty(number,"localizedLabel",{set:next=>number.relabel(next)});bindCopy(number,read,"localizedLabel");}
  return number;
}
function panelFrame(panel, scrollable = true) {
  const frame = element("div", "panel-frame");
  if (scrollable) {
    frame.classList.add("scrollable");
    const viewport = element("div", "panel-viewport"); viewport.append(panel); frame.append(viewport);
    addScrollbar(viewport, frame, panel);
  } else frame.append(panel);
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
function icon(name, live = false) {
  const svg = icons.get(name).cloneNode(true);
  return live && name === 'colors' ? createPaintPairIcon(svg, paintPairView) : svg;
}
function iconButton(id) {
  const node = commandButton(id, "");
  node.dataset.icon = "true";
  node.classList.add("tile-button");
  const glyph = icon(state.commands.find((c) => c.id === id).icon);
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
  bindCopy(node,()=>delivery.drag_panel,"title");
  if (item.kind === "panel") {
    let retainedTitle;
    node.updatePanelTitle = title => {
      if (retainedTitle === title) return;
      retainedTitle = title;
      node.setAttribute("aria-label", app.document_delivery_message({type:"move_panel",name:title}));
    };
    node.updatePanelTitle(customization.view(item.panel).title);
  } else bindCopy(node,()=>item.kind === "column" ? delivery.move_column : delivery.move_group,"ariaLabel");
  node.append(icon("grip"));
  return draggable(node, item);
}
const dropIndicator = element("div", "drop-indicator");
dropIndicator.hidden = true;
workspace.append(dropIndicator);

function dispatch(action) {
  try {
    if(documents?.interactionBusy()&&!['pen_input','restore_saved_settings','complete_request','measure_panels','measure_titlebar','measure_workspace_bottom','measure_column_drawers','measure_drawer_tiles','measure_column_scroll'].includes(action.type))return;
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
let languageTimer;
function queueLanguage() {
  if (languageTimer) return;
  languageTimer = setTimeout(() => {
    languageTimer = null;
    const change = app.prepare_language(composingKey({target:document.activeElement}) || !!workspaceGesture || heldPointers.size > 0 || compilerContacts.size > 0);
    if (change) applyChange(change);
    if (app.language_pending()) queueLanguage();
  }, 4);
}
function publishAppName(tag = bootstrap?.active_tag ?? 'en') {
  document.title = catalog.app_name;
  document.querySelector('meta[name="apple-mobile-web-app-title"]')?.setAttribute('content', catalog.app_name);
  document.querySelector('link[rel="manifest"]')?.setAttribute('href', tag === 'en' ? './manifest.webmanifest' : `./manifest.${tag}.webmanifest`);
}
function publishLanguage(patch) {
  if (!patch.localization) return false;
  refreshCopy(app);
  refreshStartup();
  refreshMessage();
  document.documentElement.lang = patch.localization.tag;
  publishAppName(patch.localization.tag);
  for (const [id, key] of [["workspace","drawing_workspace"],["canvas","drawing_canvas"],["gpu-notice","canvas_availability"],["header-start","application_menus"],["header-end","workspace_controls"],["canvas-status","canvas_status"]]) $(id)?.setAttribute("aria-label", bootstrap[key]);
  canvas.setAttribute("aria-description", bootstrap.drawing_canvas_help);
  $("size-number")?.relabel(catalog.native_copy.color.brush_size);
  documents?.localize();
  workspaceManager?.localize();
  refreshBindings();
  return true;
}
function applyChange(change) {
  if (change.regions & (1 | 2 | 4 | 8 | 16 | 128 | 256 | 8192)) bubbles?.refresh();
  if (app.language_pending()) queueLanguage();
  canvasBar?.hold(app.canvas_bar_hold());
  if (change.regions & 512) {
    state.command_search = app.command_search();
    commandBar?.refresh(state.command_search);
    if (change.regions === 512) { if (change.canvas_wake) wake(); return; }
  }
  if(change.regions & 256) { editor?.refreshColorPreview(); refreshColorEditors(); }
  if(change.regions===256){if(change.canvas_wake)wake();return;}
  if (change.regions & (1 | 2 | 4 | 8 | 128)) workspaceManager?.observe();
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
        const languageChanged = publishLanguage(patch);
        // A concurrent model change rebases retained placement too.
        if (!moving && !(change.regions & ~(16 | 128)) &&
            Object.keys(patch).every(key => key === "revision" || key === "settings_open")) {
          // Opening, closing, searching and navigating Settings do not change
          // the workspace behind it. Keep its controls and geometry intact.
          refreshPreferences(app.preferences_cached());
          updateZen();
        } else update(languageChanged ? 32767 : change.regions | (moving ? 1 : 0));
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
  if (change.regions) { refreshColorEditors(); refreshTextureDialogs(); }
  if (change.canvas_wake) wake();
  if (change.regions && !workspaceGesture?.started) wake();
}
let canvasCursorActive = false;
function cursorInput(e) {
  if (!gpuReady) return;
  const onCanvas =
    e &&
    !workspaceGesture?.started &&
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
    if (bubbles?.tick(frameTime)) wake();
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
  const text = documentReady ? bootstrap.preparing_brush : bootstrap.preparing_canvas;
  if (startupNotice.hidden !== hidden) startupNotice.hidden = hidden;
  if (startupNotice.textContent !== text) startupNotice.textContent = text;
}
// Host timing and contacts govern admission; Rust also checks queued input,
// strokes, gestures and unfinished edits. Required jobs always retain priority.
function deferOptionalCompiler() {
  if (!app) return;
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
window.addEventListener('blur', () => { compilerContacts.clear(); deferOptionalCompiler(); });
document.addEventListener('visibilitychange', () => { if (!document.hidden) deferOptionalCompiler(); });
document.addEventListener('toggle', deferOptionalCompiler, true);
document.addEventListener('close', deferOptionalCompiler, true);
function scheduleCompiler() {
  if (!gpuReady || document.hidden || state.settings_open || compilerScheduled || compilerFailed) return;
  const remaining = app.shader_wait_ms();
  if (remaining > 0) compilerResumeTimer ??= setTimeout(resumeOptionalCompiler, Math.ceil(remaining));
  if (!app.shader_work_pending(optionalCompilerReady())) return;
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
  for (const [property,value] of Object.entries({
    left: `${rect.x}px`,
    top: `${rect.y}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
  })) if (node.style[property] !== value) node.style[property] = value;
  glass?.queue();
}
function tabLabel(tab, view, presentation) {
  tabHover(tab,presentation);
  tab.classList.toggle("icon-only-tab", !presentation.show_name);
  if (presentation.show_icon) tab.append(icon(view.icon, true));
  if (presentation.show_name) tab.append(element("span", "", view.title));
}
function releaseTabs(root) { releaseTabSurfaces(root); }
function arrange(nextLayout, layoutOnly = false) {
  if (!app || !panelVectors) return;
  const resolved=layoutOnly?nextLayout:app.layout(...workspaceViewport);
  const request=++workspaceLayoutRequested;
  panelVectors.schedule(resolved,layoutOnly,(resolved,result,only)=>{
    presentWorkspaceLayout(resolved,only);
    panelVectors.apply(resolved,result);
    workspaceLayoutFinished=request;finishWorkspaceTabDrop();
    if(workspaceLatestPresentation?.model_revision===workspaceModelRevision) {
      workspacePresentation=workspaceLatestPresentation;flushWorkspacePresentation();
    }
  });
}
function presentWorkspaceLayout(resolved,layoutOnly) {
  clearWorkspacePlacement();
  if (workspaceGesture) workspaceGesture.hits = null;
  layout=resolved;
  const tabBarHeight=`${layout.tab_bar_height}px`;
  if(workspace.style.getPropertyValue("--tab-bar-height")!==tabBarHeight)
    workspace.style.setProperty("--tab-bar-height",tabBarHeight);
  const live = new Set();
  for (const group of layout.groups) {
    live.add(group.id);
    let node = groups.get(group.id);
    const body = panels.get(group.active).closest(".panel-frame");
    if (!layoutOnly || !node) {
      if (!node) {
        node = element("section", "dock-group");
        const clip = element("div", "panel-columns");
        clip.append(element("div", "panel-preview")); node.append(clip);
        groups.set(group.id, node);
        workspace.append(node);
      }
      node.classList.toggle("zen-kept", group.zen_kept);
      const tabStyle = app.group_tab_style(group.id);
      const key = JSON.stringify([[...group.panels].sort(), group.tabs_visible, !!group.footer_grip, tabStyle]);
      if (node.dataset.key !== key) {
        node.dataset.key = key;
        node.dataset.panel = group.active;
        node.dataset.group = group.id;
        node.classList.toggle("toolbar", !!group.tiles);
        const preview = node.querySelector(".panel-preview");
        releaseTabs(preview);
        const tabs = element("nav", "dock-tabs");
        draggable(tabs, { kind: "group", group: group.id });
        customization.target(tabs, { kind: "group", group: group.id });
        tabs.setAttribute("aria-label", delivery.panel_tabs);
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
            tab.dataset.group = group.id;
            const view = customization.view(panel);
            tab.title = view.title; tab.setAttribute("aria-label", view.title);
            customization.target(tab, { kind: "panel", panel });
            tab.setAttribute("aria-selected", String(panel === group.active));
            labels.append(draggable(tab, { kind: "panel", panel }));
          });
          tabs.append(labels, grip({ kind: "group", group: group.id }));
          preview.replaceChildren(tabs, body);
        } else {
          preview.replaceChildren(body);
          if (group.footer_grip) {
            const footer = element("div", "panel-footer");
            footer.style.height = `${group.footer_grip.height}px`;
            draggable(footer, { kind: "group", group: group.id });
            customization.target(footer, { kind: "group", group: group.id });
            footer.append(grip({ kind: "group", group: group.id })); preview.append(footer);
          }
        }
      }
      const preview = node.querySelector(".panel-preview");
      const strip=preview.querySelector('.tab-list');
      orderPanelTabs(strip,group.panels);
      const previous = preview.querySelector(":scope > .panel-frame");
      if (previous !== body) previous ? previous.replaceWith(body) : preview.append(body);
      node.dataset.panel = group.active;
      node.classList.toggle("toolbar", !!group.tiles);
      node.setAttribute("aria-label", customization.view(group.active).title);
      for(const tab of node.querySelectorAll('.dock-tab')) {
        const view=customization.view(tab.dataset.panel),title=view.title;
        tab.setAttribute('aria-selected',String(tab.dataset.panel===group.active));
        const presentation=group.tab_presentations[group.panels.indexOf(tab.dataset.panel)];
        tabHover(tab,presentation);
        const labelKey=JSON.stringify([view.title,view.icon,presentation]);
        if(tab.dataset.labelKey===labelKey)continue;
        tab.dataset.labelKey=labelKey;tab.replaceChildren();tabLabel(tab,view,presentation);
        tab.title=title;tab.setAttribute('aria-label',title);
        const label=tab.querySelector('span');if(label)label.textContent=title;
      }
      node.classList.toggle("floating-panel", group.floating);
      node.classList.toggle("tool-strip", !!group.tiles && !group.tabs_visible);
      node.dataset.zIndex = group.floating ? String(100 + layout.groups.indexOf(group) * 2) : "1";
      if (!node.classList.contains("expanded-panel")) node.style.zIndex = node.dataset.zIndex;
    }
    place(node, group.bounds);
    if (group.tiles) node.style.setProperty("--tile-radius", `${group.tiles.tile_corner_radius}px`);
    const bodyHeight = Math.max(0, group.bounds.height - (group.tabs_visible ? layout.tab_bar_height : 0) - (group.footer_grip?.height || 0));
    body.classList.toggle("body-scroll", !group.tiles && group.min_body_height > bodyHeight + .01);
    const minimumBody = `${group.min_body_height || 0}px`;
    if (node.style.getPropertyValue("--panel-min-body-height") !== minimumBody)
      node.style.setProperty("--panel-min-body-height", minimumBody);
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
      node.setAttribute("aria-label", delivery.resize_dock);
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
  editor.queuePositions();
  place($("canvas-status"), layout.status);
  // The help occupies the same unobstructed area used for fitting the document.
  // Panels remain native UI siblings above the full-window drawing surface.
  place($("gpu-notice"), layout.work_area);
  if (!layoutOnly) {
    resizeCanvas();
    updateZen();
  }
  canvasBar?.place();
  selectionUi?.place();
  frequencySeparationUi?.place();
  notice?.place();
  queuePanelMeasurements();
}
// Measure intrinsic widget content only when its width/copy changes. Rust owns
// tab growth and floating sizes; moving a float reuses these cached DOM facts.
const panelMeasurements = new Map();
let measuringPanels = false;
let panelMetricContext, panelFontGeneration = 0, lastPanelReport;
function panelMetrics() {
  const scale = devicePixelRatio || 1;
  const style = `web:${getComputedStyle(workspace).font}:${panelFontGeneration}`;
  if (!panelMetricContext || panelMetricContext.scale !== scale || panelMetricContext.style !== style) {
    panelMetricContext = {generation: (panelMetricContext?.generation || 0) + 1, scale, text_scale: 1, style};
  }
  return panelMetricContext;
}
document.fonts.addEventListener("loadingdone", () => { panelFontGeneration++; queuePanelMeasurements(); });
const measureBox = element("div", "panel-measure");
measureBox.hidden = true;
measureBox.inert = true;
measureBox.setAttribute("aria-hidden", "true"); workspace.append(measureBox);
// An isolated tree retains measurement controls without exposing duplicate IDs
// to application lookup. It uses the same CSS and inherits the workspace theme.
const measurementRoot = measureBox.attachShadow({mode:"closed"});
const measurementStyle = $("workspace-style").cloneNode();
measurementStyle.removeAttribute("id");
await new Promise((resolve,reject)=>{
  measurementStyle.addEventListener("load",resolve,{once:true});
  measurementStyle.addEventListener("error",reject,{once:true});
  measurementRoot.append(measurementStyle);
});
function invalidatePanelMeasurement(id) {
  const cached=panelMeasurements.get(id);
  if(cached){releaseTabs(cached.root);cached.root.remove();}
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
  const pending = [], context = panelMetrics();
  const requests = new Map((layout.measurement_requests || []).map(request => [request.panel, request]));
  // Batch writes before reads. Intrinsic measurement keeps one offscreen DOM
  // copy per intrinsic content change, so width changes reflow it without cloning.
  for (const config of state.workspace.layout.panels) {
    const view = customization.view(config.id);
    const request = requests.get(config.id);
    if (!request) continue;
    const width = request.width;
    // Model publication also advances for numeric values and canvas poses.
    // Intrinsic copies change only with panel structure/copy, appearance or
    // available width; each content widget invalidates its own schema below.
    const key = JSON.stringify([request.tab, request.body, request.split_filters, context]);
    let cached = panelMeasurements.get(config.id);
    if (cached?.key !== key) {
      invalidatePanelMeasurement(config.id);
      const root = element("div");
      const tab = element("button", "dock-tab");
      tab.style.width = "max-content";
      tabLabel(tab, view, {show_icon:true,show_name:true,hover_bottom:[0,0]});
      root.append(tab);
      const content = config.content.kind === "toolbar" ? null : panels.get(config.id).cloneNode(true);
      if (content) {
        content.style.height = "auto"; content.style.width = "100%";
        root.append(content);
      }
      cached = { key, root, tab, content, request: structuredClone(request), context };
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
      const list = cached.content?.querySelector(".layer-rows, .filter-picker-list, .palette-scroll, .navigator-overview");
      const row = list?.querySelector(".layer-row, .filter-row, .palette-tile");
      const scroll = cached.content && !["color","proof","histogram","waveform"].includes(id) ? {
        fixed_height: list ? Math.max(0, content_height - list.getBoundingClientRect().height) : 0,
        unit_height: row ? row.getBoundingClientRect().height + (row.matches(".palette-tile") ? 4 : 0) : 0,
      } : null;
      cached.value = {
        request: {...cached.request, width: cached.width}, context: cached.context,
        tab_widths: [cached.tab.getBoundingClientRect().width, 36],
        body: {actual_width: cached.content?.getBoundingClientRect().width || cached.width, content_height, scroll},
      };
    }
  } finally { measureBox.hidden = true; }
  for (const id of panelMeasurements.keys()) if (!panels.has(id)) invalidatePanelMeasurement(id);
  const reports = [...requests.keys()].map(id => panelMeasurements.get(id)?.value).filter(Boolean);
  const measurements = {context, reports};
  const encoded = JSON.stringify(measurements);
  if (encoded !== lastPanelReport) {
    lastPanelReport = encoded;
    dispatch({type: "measure_panels", measurements});
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
    panelFrame(panel, kind !== "tiles" && name !== "layers");
  }
  panels.get("brushes").append(editor.control("brushes"));
  for (const [panel, control] of [["brush_sets","brush_sets"],["sculpt_sets","sculpt_sets"],["tools","tools"],["tool_settings","tool_settings"],["color","color_wheel"],["navigator","navigator"],["histogram","histogram"],["waveform","waveform"]])
    panels.get(panel).append(editor.control(control));
  const controls = element("div", "size-controls");
  controls.dataset.control = "brush_size";
  const size = numberField(catalog.brush_size, catalog.native_copy.color.brush_size, value => dispatch({ type: "set_brush_size", value }));
  size.onReset = () => dispatch({ type: "reset_tool_setting", id: "size" });
  size.id = "size-number"; controls.append(size);
  const grid = brushSizeGrid({ app, catalog, state: () => state, dispatch, element, button });
  grid.dataset.control = "size_presets";
  sizePresets = grid;
  panels.get("sizes").append(controls, grid);
  palettes.mount(panels.get("palettes"));
  layerPanel = createLayerPanel({ app, catalog, state: () => state, panel: panels.get("layers"), element, button, icon,dropdown, dispatch, applyChange, message, numberField, wake, dismissContext: () => customization.dismissContext(), openMenu: node => customization.openMenu(node), contentChanged: panelContentChanged });
  effectPanels = createEffectPanels({app,wake,catalog,state:()=>state,panels,element,button,icon,dropdown,dispatch,numberField,message,asset,
    contentChanged:panelContentChanged});
}
function contentPanel(id, splitPicker=false, readToolSet=null) {
  const panel=element("div",`panel ${id}-panel`);
  if(id==="brushes"&&readToolSet) {
    const control=editor.control("brushes",readToolSet);panel.append(control);panel.refreshPanel=control.refreshEditor;
  } else if(id==="proof") {
    panel.disposePanel=documents.mountProof(panel);panel.refreshPanel=()=>{};
  } else if(id==="palettes") {
    const view=palettes.mount(panel);panel.refreshPanel=view.refresh;panel.disposePanel=view.dispose;
  } else if(id==="layers") {
    const view=createLayerPanel({app,catalog,state:()=>state,panel,element,button,icon,dropdown,dispatch,applyChange,message,numberField,wake,dismissContext:()=>customization.dismissContext(),openMenu:node=>customization.openMenu(node)});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else if(["filter_types","adjustments","properties","stats"].includes(id)) {
    const copies=new Map(["filter_types","adjustments","properties","stats"].map(name=>[name,name===id?panel:element("div","panel")]));
    const view=createEffectPanels({app,wake,catalog,state:()=>state,panels:copies,element,button,icon,dropdown,dispatch,numberField,message,asset,contentChanged:()=>{},splitPicker});
    panel.refreshPanel=view.refresh; panel.disposePanel=view.dispose;
  } else {
    for(const control of customization.view(id).controls.filter(c=>c.visible_in_panel)) panel.append(customization.field(control.control,control.label));
    panel.refreshPanel=()=>{};
  }
  panel.refreshPanel();return panel;
}
function update(regions) {
  document.body.toggleAttribute("data-workspace-ready", state.workspace_ready);
  if(regions & (4096 | 16384 | 16)) penResponse?.refresh();
  commandBar?.refresh(state.command_search);
  if (regions & (1 | 2 | 4 | 8 | 16 | 128)) paintPairView = app.paint_pair();
  // Canvas-based controls read these colors while refreshing their pixels.
  if (regions & 16) applyTheme(state.theme, state.palette);
  if (regions & (1 | 2 | 4 | 8 | 16 | 128)) customization.refresh();
  if (regions & 2) {
    sizePresets.refresh();
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
  if (regions & 16) effectPanels.refresh();
  if (regions & 2048) effectPanels.refreshHistograms();
  if (regions & (1 | 2 | 4 | 8 | 16 | 32 | 128 | 2048)) { editor.refresh(); selectionUi.refresh(); frequencySeparationUi.refresh(); canvasSizeUi.refresh(); imageSizeUi.refresh(); workspaceChrome?.refresh(); }
  if (regions & (1 | 4 | 128)) arrange();
  if (regions & (1 | 8 | 1024)) canvasBar?.refresh(state.canvas_bar);
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
  if (regions & (16 | 32)) zoomReadout?.refresh(state.camera);
  if (regions & (1 | 16)) updateZen();
  editor.flushPaint();
  if (regions & 64) {
    documents?.refresh();
    notice?.publish(state.notice);
    screenStatus?.refresh(state.screen);
    if (state.host_error && state.host_error !== shownHostError) message(state.host_error);
    shownHostError = state.host_error;
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
            else if (request.kind.type === "export_keymap") {
              const link = Object.assign(document.createElement("a"), {
                href: URL.createObjectURL(new Blob([request.kind.text], { type: "application/json" })),
                download: request.kind.name,
              });
              link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 0);
            }
            else if (request.kind.type === "import_keymap") {
              const input = Object.assign(document.createElement("input"), { type: "file", accept: ".capykeys" });
              input.addEventListener("change", async () => {
                const file = input.files?.[0];
                if (!file) return;
                if (file.size > 1 << 20) { message(delivery.keymap_too_large); return; }
                dispatch({ type: "preferences", action: { type: "import_keymap", text: await file.text() } });
              });
              input.click();
            }
            else if (request.kind.type === "color_slot") {
              if (!colorSlotRequests.has(request.id)) {
                colorSlotRequests.add(request.id);
                const slot=request.kind.slot, epoch=state.document_file.epoch;
                chooseColor({app,slot,element,button}).then(selected=>{
                  if(selected && app.state().document_file.epoch===epoch)dispatch({type:'color',action:selected.intensity==null?{op:'set_slot',slot,color:selected.color}:{op:'set_slot_intensity',slot,color:selected.color,stops:selected.intensity}});
                  dispatch({type:'complete_request',id:request.id,error:null});
                },error=>dispatch({type:'complete_request',id:request.id,error:String(error)}))
                  .finally(()=>colorSlotRequests.delete(request.id));
              }
              continue;
            }
            else if (request.kind.type === "workspace") { workspaceManager?.wake(); continue; }
            else if (request.kind.type !== "save_settings") { documents.handle(request); continue; }
            else
            localStorage.setItem(settingsKey, JSON.stringify(request.kind.settings));
          } catch (e) { console.error(e); error = app.document_delivery_message({type:"save_preferences",detail:String(e)}); }
          dispatch({ type: "complete_request", id: request.id, error });
        }
      } finally { servicingRequests = false; }
    }
  }
}

// Rust owns transient interaction policy. These are only DOM event/capture
// records; CSS animates the returned visibility without resizing the canvas.
let workspaceGesture = null,workspaceTabDrop = null,workspaceLayoutRequested=0,workspaceLayoutFinished=0;
function workspaceLayoutReady(){return !workspaceLayoutPending&&workspaceLayoutFinished===workspaceLayoutRequested;}
function clearWorkspaceTabDrop(){
  if(workspaceTabDrop)clearTabSlide(workspaceTabDrop);
  workspaceTabDrop=null;
}
function finishWorkspaceTabDrop(){
  if(!workspaceTabDrop||!workspaceLayoutReady())return;
  clearWorkspaceTabDrop();
}
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
  const owner = drag.node.closest('.panel-preview,.content-drawer');
  if (owner) {
    const bounds = owner.getBoundingClientRect(), style = getComputedStyle(owner);
    if (Math.abs(bounds.left - clip.x) < 1 && Math.abs(bounds.top - clip.y) < 1)
      overlay.style.borderTopLeftRadius = style.borderTopLeftRadius;
    if (Math.abs(bounds.right - clip.x - clip.width) < 1 && Math.abs(bounds.top - clip.y) < 1)
      overlay.style.borderTopRightRadius = style.borderTopRightRadius;
  }
  overlay.setAttribute("aria-hidden", "true"); overlay.inert = true;
  // Freeze insertion geometry; only these noninteractive copies move.
  const tabs = grabbed.map(({ source, hit }) => {
    const bounds = hit.bounds, preview = source.cloneNode(true);
    prepareTabPreview(preview);
    for (const name of [...preview.attributes].map(a => a.name)) {
      if (name.startsWith("data-") || name === "id") preview.removeAttribute(name);
    }
    const dragged = source === drag.node, hole = source.matches(".tab-list > [aria-selected=true]");
    preview.classList.add(dragged ? "dragged-tab-preview" : "neighbor-tab-preview", ...(hole ? ["docked-tab-preview"] : []));
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
  for (const { source } of drag.tabSlide.tabs) {
    source.classList.remove("dragged-tab-source");
  }
  drag.tabSlide.overlay.remove();
  drag.tabSlide = null;
}
function updateTabSlide(drag, presentation) {
  const slide = drag.tabSlide;
  if (!slide || drag.finishing) return;
  const preview = presentation?.preview;
  if (!preview) { clearTabSlide(drag); return; }
  for (const tab of slide.tabs) {
    const offset = tab.source === drag.node ? preview.bounds.x - presentation.source.x
      : preview.offsets.find(o => Number(o.index) === tab.hit.index)?.x || 0;
    const x = `${deviceAligned(offset)}px`;
    tab.preview.style.transform = `translateX(${x})`;
  }
}
let workspaceCapture = null;
function workspacePointer() {
  const drag = workspaceGesture, edge = edgeContact;
  const capture = edge?.cursor ? edge.pointerId : drag?.started || drag?.held ? drag.id : null;
  const previous = workspaceCapture;
  workspaceCapture = capture;
  if (previous !== capture && previous != null && workspace.hasPointerCapture(previous)) workspace.releasePointerCapture(previous);
  if (capture != null && !workspace.hasPointerCapture(capture)) workspace.setPointerCapture(capture);
  const cursor = edge?.cursor ?? (drag?.started ? drag.tile || drag.action.type === "drag_workspace" ? "grabbing" : drag.cursor
    : drag?.waitForHold && drag.held && ["mouse", "pen"].includes(drag.start.pointerType) ? "grab" : null);
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
let workspaceModelRevision, workspaceContentRevision, workspaceLatestPresentation, workspacePresentation, workspacePresentationFrame = 0;
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
  }
  workspacePlacements.clear();
}
function queueWorkspacePresentation(presentation) {
  workspaceLatestPresentation=presentation;
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
  if(!update.drag){
    if(!workspaceLayoutReady()){workspacePresentation=update;return false;}
    finishWorkspaceTabDrop();
  }
  const drag = update.drag, moving = drag?.group;
  if (moving) {
    const group = layout.groups.find(g => g.id === moving.id), base = group?.bounds;
    if (base) {
      const dw = moving.bounds.width - base.width, dh = moving.bounds.height - base.height;
      const nodes = [groups.get(moving.id), ...[...dividers.values()].filter(n => n.dragAction.group === moving.id)];
      for (const node of nodes.filter(Boolean)) {
        if (!workspacePlacements.has(node)) workspacePlacements.set(node, {
          width: node.style.width, height: node.style.height,
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
  drag.finishing=true;
  if (drag.started) {
    if (drag.tile) {
      const point=e||drag.last;
      if (!cancel && !bubbles?.takeTile(drag.action.item,[point.clientX,point.clientY])) dropItem(drag.action.item, dropHint(point, drag.action.item));
      dragItem = null; drag.node.classList.remove("drag-source");
    } else workspaceGestureEvent(cancel ? "cancel" : "up", e || drag.last);
  }
  if (cancel && drag.context) customization.dismissContext();
  if (!cancel && !drag.started && drag.selectTab) dispatch(drag.selectTab);
  workspaceGesture = null;
  if(!cancel&&drag.tabSlide){
    workspaceTabDrop=drag;finishWorkspaceTabDrop();
  }else clearTabSlide(drag);
  workspacePointer();
  dropIndicator.hidden = true;
  if (!cancel && (drag.started || drag.held || drag.context)) { e?.preventDefault(); e?.stopPropagation(); }
  updateZen();
}
let edgeContact = null, edgeContacts = 0, lastPointerKind = "mouse";
function edgeReply(reply, ended = false) {
  const edge = edgeContact;
  if (!edge) return;
  edge.cursor = ended ? null : reply.edge_cursor ?? null;
  edge.hold = !!reply.edge_hold || !!reply.edge_select_on_release;
  edge.selectOnRelease = !!reply.edge_select_on_release;
  if (ended || reply.edge_retired || !edge.cursor && !reply.edge_pending) edgeContact = null;
  workspacePointer();
}
function edgeInput(phase, e, contact = "free") {
  const reply = input({ type: "edge_contact", id: BigInt(edgeContact.id), phase, kind: e.pointerType || "mouse", contact,
    position: [e.clientX, e.clientY], viewport: workspaceViewport, time_ns: BigInt(Math.round(performance.now() * 1e6)) });
  edgeReply(reply, phase === "up" || phase === "cancel");
  return reply;
}
function endEdgeContact(e, phase) {
  const edge = edgeContact;
  if (!edge || !e.isTrusted || edge.pointerId !== e.pointerId) return;
  const claimed = !!edge.cursor;
  edgeInput(phase, e);
  if (!claimed) return;
  e.preventDefault(); e.stopImmediatePropagation();
}
workspace.addEventListener("pointerdown", e => {
  if (!e.isTrusted) return;
  lastPointerKind = e.pointerType || "mouse";
  if (e.button !== 0) return;
  if (edgeContact) {
    if (!edgeContact.cursor && e.pointerType === "touch") edgeInput("cancel", e);
    return;
  }
  const contact = e.isPrimary ? edgeContactClass(e.target, canvas) : null;
  if (!contact) return;
  edgeContact = { id: ++edgeContacts, pointerId: e.pointerId, canvas: e.target === canvas, cursor: null, hold: false };
  edgeInput("down", e, contact);
}, { capture: true });
workspace.addEventListener("pointermove", e => {
  const edge = edgeContact;
  if (!edge || !e.isTrusted || edge.pointerId !== e.pointerId) return;
  if (!edge.cursor && (workspaceGesture?.started || workspaceGesture?.held)) { edgeInput("cancel", e); return; }
  const claimed = !!edge.cursor;
  const reply = edgeInput("move", e);
  if (!edgeContact) return;
  if (edge.cursor && !claimed) {
    if (workspaceGesture && !workspaceGesture.started) endWorkspaceGesture(null, true);
    customization.dismissContext();
    if (edge.canvas) canvas.dispatchEvent(new PointerEvent("pointercancel", { pointerId: e.pointerId, pointerType: e.pointerType, clientX: e.clientX, clientY: e.clientY, bubbles: true }));
  }
  if (reply.edge_cursor || reply.edge_hold) { e.preventDefault(); e.stopImmediatePropagation(); }
}, { capture: true });
window.addEventListener("pointerup", e => endEdgeContact(e, "up"), { capture: true });
window.addEventListener("pointercancel", e => endEdgeContact(e, "cancel"), { capture: true });
workspace.addEventListener("pointerdown", e => {
  if (e.button !== 0 || !e.isPrimary || workspaceGesture) return;
  const node = e.target.closest("[data-workspace-drag]");
  if (!node) return;
  const tab=node.matches('.dock-tab[data-group]')?{type:"select_panel_tab",group:Number(node.dataset.group),panel:node.dataset.panel}:null;
  if(workspaceTabDrop){if(tab&&!edgeContact?.selectOnRelease)dispatch(tab);return;}
  workspaceGesture = { id: e.pointerId, action: JSON.parse(node.dataset.workspaceDrag),
    start: e, last: e, started: false, node, parent: node.parentNode,
    waitForHold: node.dataset.dragPickup === "hold", cursor: getComputedStyle(node).cursor };
  workspaceGesture.tile = workspaceGesture.action.item?.kind === "tile";
  if (workspaceGesture.waitForHold)
    workspaceGestureSourceObserver.observe(workspace, { childList: true, subtree: true });
  if (tab) {
    if (edgeContact?.selectOnRelease) workspaceGesture.selectTab = tab;
    else dispatch(tab);
  }
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
    if (distance <= 8) return;
    if (drag.waitForHold && !drag.held) { endWorkspaceGesture(e, true); return; }
    customization.dismissContext();
    drag.started = true;
    workspaceGestureSourceObserver.disconnect();
    workspacePointer();
    groups.forEach(node => node.getAnimations().forEach(a => a.cancel()));
    if (drag.tile) {
      dragItem = drag.action.item; drag.node.classList.add("drag-source"); updateZen();
    } else {
      grabTabSlide(drag);
      startTabSlide(drag);
      workspaceGestureEvent("down", drag.start);
      if (drag.tabSlide) app.begin_tab_drag(drag.tabSlide.tabs.map(t => t.hit), drag.tabSlide.clip);
    }
  }
  if (drag.tile) showDropHint(dropHint(e, drag.action.item));
  else workspaceGestureEvent("move", e);
  workspacePointer();
  e.preventDefault(); e.stopPropagation();
}, { capture: true });
workspace.addEventListener("touchmove", e => {
  if ((workspaceGesture?.context || edgeContact?.hold || edgeContact?.cursor) && e.touches.length === 1 && e.cancelable) e.preventDefault();
}, { passive: false });
window.addEventListener("pointerup", e => endWorkspaceGesture(e), { capture: true });
window.addEventListener("pointercancel", e => endWorkspaceGesture(e, true), { capture: true });
workspace.addEventListener("lostpointercapture", e => {
  if (e.target === workspace && edgeContact?.pointerId === e.pointerId) endEdgeContact(e, "cancel");
  // Touch starts with implicit capture on the tab. Transferring capture to the
  // stable workspace releases that child; only losing our own capture cancels.
  if (e.target === workspace || (!workspace.hasPointerCapture(e.pointerId)
    && workspaceGesture?.node.contains(e.target))) endWorkspaceGesture(e, true);
});
function armWorkspaceDrag(drag) {
  drag.held = true;
  workspacePointer();
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
  if (!node) {
    const contact = edgeResetClass(e.target, canvas);
    if (contact && input({ type: "edge_double_press", kind: lastPointerKind, contact, position: [e.clientX, e.clientY], viewport: workspaceViewport }).handled) {
      e.preventDefault(); e.stopPropagation();
      endWorkspaceGesture(null, true);
    }
    return;
  }
  const action = JSON.parse(node.dataset.workspaceDrag);
  if (action.type === "drag_divider" && layout.dividers.some(d =>
    d.id === action.id && d.owner_side && d.axis === "horizontal")) {
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
    header?.interaction(reply);
    const cursor = ({pan:"grab", zoom:"zoom-in", zoom_out:"zoom-out", rotate:"crosshair"})[reply.navigation_cursor] ?? "";
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
  return input({
    type: "chrome",
    event,
    viewport: workspaceViewport,
    facts: {
      expanded_panel: customization?.placement(),
      ...workspaceChrome?.facts(),
      contact_tab: event.kind === "contact"
        ? document.elementFromPoint(...event.position)?.closest(".dock-tab")?.dataset.panel ?? null
        : null,
      held: heldPointers.size > 0,
      dragging: dragItem !== null,
      popup_open:
        !!(document.querySelector("dialog[open], details[open]") || document.querySelector(":popover-open:not(.hover-tooltip)")) ||
        (CSS.supports("selector(select:open)") && !!document.querySelector("select:open")),
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
function closeMenusOutside(target) {
  for (const menu of document.querySelectorAll("details[open]"))
    if (!menu.contains(target)) menu.open = false;
}
function persistWorkspace() {
  workspaceManager?.observe();
}
function pointerStyle(e) {
  // Touch leaves :hover stuck until the next tap; track actual pointer input
  // instead of disabling hover for a whole device that may also have a pen/mouse.
  const touch = e.pointerType === "touch", root = document.documentElement;
  root.toggleAttribute("data-mouse", e.pointerType === "mouse");
  if (root.hasAttribute("data-touch") !== touch) root.toggleAttribute("data-touch", touch);
}
window.addEventListener(
  "pointermove",
  (e) => {
    if (e.target.closest?.("dialog[open]")) return;
    if(e.target.closest?.('.utility-panel')){cursorInput(e);return;}
    if (!e.buttons) heldPointers.delete(e.pointerId);
    if (workspaceGesture?.started && workspaceGesture.id === e.pointerId) {
      cursorInput(null);
      return;
    }
    // A captured paint contact cannot reveal chrome. The canvas listener owns
    // its samples/cursor; querying popups and hit-testing here duplicates work
    // and can force layout between input and the next drawing submission.
    if (!(e.target === canvas && lastPenEvent?.pointerId === e.pointerId))
      chromeInput({ kind: "motion" });
    if (e.target !== canvas) cursorInput(e);
  },
  { capture: true },
);
document.addEventListener("pointerleave", () => {
  cursorInput(null);
  chromeInput({ kind: "refresh" });
});
window.addEventListener(
  "pointerdown",
  (e) => {
    // Native DOM modals own their contacts. Workspace transitions deliberately
    // consume editor input, so forwarding a dialog contact would swallow its
    // buttons before the DOM click handler can run.
    if (e.target.closest("dialog[open]")) return closeMenusOutside(e.target);
    if(e.target.closest('.utility-panel'))return closeMenusOutside(e.target);
    if (e.target === canvas && e.pointerType === "pen" && !(e.buttons & 33)) {
      e.preventDefault();
      return;
    }
    if (e.target.closest("#header") || e.target.closest("#zen-capy") && !workspace.classList.contains("zen-hidden")) heldPointers.set(e.pointerId,e);
    const reply = chromeInput({
      kind: "contact",
      position: [e.clientX, e.clientY],
      canvas: e.target === canvas,
    });
    if (reply.handled) {
      workspace.setPointerCapture(e.pointerId);
      heldPointers.delete(e.pointerId);
      updateZen();
      e.preventDefault();
      e.stopImmediatePropagation();
    }
    closeMenusOutside(e.target);
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
    try {
      batch.records = batch.records.subarray(app.pen(batch.records, batch.revision) * 11);
    } catch (error) {
      message(error);
      return;
    }
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
        try{applyChange(app.input({type:'color_picker_hold',id:corePointerId(e),position:position(e),offset:44*(devicePixelRatio||1)}).change);}
        catch(error){message(error);}
      },500)};
    }
  } else if(stage===2&&pickerHold?.id===e.pointerId&&Math.hypot(e.clientX-pickerHold.x,e.clientY-pickerHold.y)>8)cancelPickerHold();
  else if(stage===3||stage===4){canvasFingers.delete(e.pointerId);cancelPickerHold();}
}

const penSideButtons = new Map();
window.addEventListener("blur", () => penSideButtons.clear());
function penButtons(e) {
  if (e.pointerType !== "pen") return;
  const next = e.buttons & 6, previous = penSideButtons.get(e.pointerId) ?? 0;
  if (next === previous) return;
  penSideButtons.set(e.pointerId, next);
  for (const [bit, button] of [[2, "primary"], [4, "secondary"]])
    if ((next ^ previous) & bit) input({ type: "pen_button", button, pressed: !!(next & bit) });
}
function canvasPointer(e, stage) {
  penButtons(e);
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
    notice?.hide();
  }
  let sample = e;
  const activePen = lastPenEvent?.pointerType === "pen"
    && lastPenEvent.pointerId === e.pointerId;
  if (activePen && ((stage === 4 && e.type !== "pointercancel") ||
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
    time_ns: BigInt(Math.round((e.timeStamp || 0) * 1e6)),
  });
}
canvas.addEventListener("contextmenu", (e) => e.preventDefault());
let trackpadGesture = null;
canvas.addEventListener("gesturestart", e => {
  e.preventDefault();
  trackpadGesture = {point: position(e), scale: e.scale, rotation: e.rotation};
  app.begin_view_gesture();
}, {passive: false});
canvas.addEventListener("gesturechange", e => {
  if (!trackpadGesture) return;
  e.preventDefault();
  const point = position(e), previous = trackpadGesture;
  if (e.scale > 0 && Number.isFinite(e.rotation)) {
    applyChange(app.gesture(...previous.point, ...point, e.scale / previous.scale, (e.rotation - previous.rotation) * Math.PI / 180));
    trackpadGesture = {point, scale: e.scale, rotation: e.rotation};
  }
}, {passive: false});
canvas.addEventListener("gestureend", e => { e.preventDefault(); trackpadGesture = null; }, {passive: false});
window.addEventListener("blur", () => { trackpadGesture = null; });
window.addEventListener(
  "wheel",
  (e) => {
    if (document.elementFromPoint(e.clientX, e.clientY) !== canvas) return;
    e.preventDefault();
    if (trackpadGesture) return;
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
  { capture: true, passive: false },
);
function keyInput(e, pressed, divider = null) {
  if (pressed && (e.defaultPrevented || composingKey(e))) return;
  const nativeDialog=!!document.querySelector("dialog:modal:not(#settings)") && !state.preferences.capture;
  if (pressed && nativeDialog) return;
  updateZen();
  const reply = input({
    type: "key",
    key: e.key,
    latin: latinKey(e),
    pressed,
    repeat: e.repeat,
    modifiers: {
      command: e.ctrlKey || e.metaKey,
      shift: e.shiftKey,
      alt: e.altKey,
    },
    editing:
      composingKey(e) ||
      (e.target instanceof Element &&
        (!!e.target.closest("input,select,textarea,[contenteditable]:not([contenteditable=false])") ||
          (!!e.target.closest("button,summary") && app.control_takes_key(e.key, e.ctrlKey || e.metaKey, e.altKey, keyboardFocused.has(e.target))))),
    divider,
  });
  if (reply.handled && !nativeDialog) {
    if(!documents?.allowNativePaste())e.preventDefault();
    e.stopPropagation();
  }
}
const keyboardFocused = new WeakSet();
let keyboardModality = false;
window.addEventListener("pointerdown", () => { keyboardModality = false; keyboardFocused.delete(document.activeElement); }, { capture: true });
window.addEventListener("keydown", e => { if (!["Shift", "Control", "Alt", "Meta"].includes(e.key)) keyboardModality = true; }, { capture: true });
document.addEventListener("focusin", e => { if (keyboardModality) keyboardFocused.add(e.target); else keyboardFocused.delete(e.target); }, { capture: true });
window.addEventListener("keydown", e => {
  if (!composingKey(e) && e.key === "Escape" && workspaceGesture) { endWorkspaceGesture(null, true); e.preventDefault(); e.stopPropagation(); }
}, {capture:true});
window.addEventListener("keydown", (e) => {
  if(!composingKey(e) && documents?.key(e,()=>keyInput(e,true)))return;
  keyInput(e, true);
});
window.addEventListener("keyup", (e) => keyInput(e, false));
const GAMEPAD_BUTTONS = ["a", "b", "x", "y", "l1", "r1", "l2", "r2", "select", "start", "l3", "r3", "up", "down", "left", "right", "home"];
const gamepads = new Map();
let gamepadFrame = 0;
function gamepadKey(name, pressed, repeat) {
  input({ type: "key", key: `gamepad_${name}`, pressed, repeat, modifiers: { command: false, shift: false, alt: false }, editing: false, divider: null });
}
function releaseGamepad(index) {
  const pad = gamepads.get(index);
  if (!pad) return;
  for (const [button, held] of pad.held) { clearTimeout(held); gamepadKey(GAMEPAD_BUTTONS[button], false, false); }
  pad.held.clear();
  if (pad.axes) input({ type: "axes", pan: [0, 0], zoom: 0 });
  pad.axes = null;
}
function pollGamepads() {
  gamepadFrame = 0;
  for (const pad of navigator.getGamepads?.() ?? []) {
    if (!pad?.connected || pad.mapping !== "standard") continue;
    const state = gamepads.get(pad.index) ?? { held: new Map(), axes: null };
    gamepads.set(pad.index, state);
    pad.buttons.slice(0, GAMEPAD_BUTTONS.length).forEach((button, index) => {
      const name = GAMEPAD_BUTTONS[index];
      if (button.pressed && !state.held.has(index)) {
        gamepadKey(name, true, false);
        const repeat = () => { gamepadKey(name, true, true); state.held.set(index, setTimeout(repeat, 50)); };
        state.held.set(index, setTimeout(repeat, 500));
      } else if (!button.pressed && state.held.has(index)) {
        clearTimeout(state.held.get(index));
        state.held.delete(index);
        gamepadKey(name, false, false);
      }
    });
    const axes = [pad.axes[0] ?? 0, pad.axes[1] ?? 0, -(pad.axes[3] ?? 0)].map(v => Math.round(Math.max(-1, Math.min(1, v)) * 100) / 100);
    if (String(axes) !== String(state.axes)) {
      state.axes = axes;
      input({ type: "axes", pan: axes.slice(0, 2), zoom: axes[2] });
    }
  }
  if (gamepads.size) gamepadFrame = requestAnimationFrame(pollGamepads);
}
window.addEventListener("gamepadconnected", () => { gamepadFrame ||= requestAnimationFrame(pollGamepads); });
window.addEventListener("gamepaddisconnected", (e) => {
  releaseGamepad(e.gamepad.index);
  gamepads.delete(e.gamepad.index);
});
window.addEventListener("blur", () => {
  for (const pad of gamepads.values()) { for (const held of pad.held.values()) clearTimeout(held); pad.held.clear(); pad.axes = null; }
});
window.addEventListener("blur", () => {
  endWorkspaceGesture(null, true);
  cursorInput(null);
  heldPointers.clear();
  const reply = input({ type: "blur" });
  edgeReply(reply);
  if (reply.cancel_paint && lastPenEvent)
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
  const wasmInstance=await init({module_or_path: wasmModule});
  wasmMemory=wasmInstance.memory;
  configure_gpu_diagnostics(event=>gpuDiagnostics.record(event));
  performance.mark("capy.startup.wasm");
  // Let the browser start the worker while the main thread builds controls.
  await new Promise(resolve => setTimeout(resolve, 0));
  const fileWorker = createRasterWorker(wasmModule,event=>gpuDiagnostics.record(event)),documentStorage=createDocumentStorage();
  const rasterWorker = async request=>{
    try{return await(request.operation.startsWith('tab-')?documentStorage(request):fileWorker(request));}
    finally{if(request.operation==='image-decode'||request.operation==='nearest-coordinates')wake();}
  };
  configure_raster_worker(rasterWorker);
  canvas.width = 800;
  canvas.height = 600;
  let savedPreferences, restoreError;
  try { savedPreferences = localStorage.getItem(settingsKey); }
  catch (error) { restoreError = error; }
  const preferredLanguages = Array.from(navigator.languages ?? []);
  const mobileBrowser = navigator.userAgentData?.mobile || /Android|iPhone|iPad|iPod/.test(navigator.userAgent)
    || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  app = WebApp.create(canvas, savedPreferences, preferredLanguages, screenInfo(), mobileBrowser);
  bootstrap = liveCopy(app, "bootstrap_view");
  delivery = liveCopy(app, "document_delivery_copy");
  document.documentElement.lang = bootstrap.active_tag;
  for (const [id, label] of [["workspace", bootstrap.drawing_workspace], ["canvas", bootstrap.drawing_canvas], ["gpu-notice", bootstrap.canvas_availability], ["header-start", bootstrap.application_menus], ["header-end", bootstrap.workspace_controls], ["canvas-status", bootstrap.canvas_status]]) $(id).setAttribute("aria-label", label);
  $("canvas").setAttribute("aria-description", bootstrap.drawing_canvas_help);
  $("gpu-notice").replaceChildren(element("div", "gpu-help", bootstrap.starting_canvas));
  $("status").textContent = bootstrap.starting_webgpu;
  app.prediction_availability(typeof globalThis.PointerEvent?.prototype.getPredictedEvents === "function");
  const themeAction = () => ({
    type: "system_theme_changed",
    theme: systemTheme.matches ? "dark" : "light",
  });
  app.dispatch(themeAction());
  window.addEventListener("storage", (event) => {
    if (event.key !== settingsKey) return;
    try { dispatch({ type: "restore_saved_settings", saved: localStorage.getItem(settingsKey) ?? "{}" }); }
    catch (error) { console.error(error); message({document_host_error:{type:"delivery",reason:{type:"restore_preferences",detail:String(error)}}}); }
  });
  systemTheme.addEventListener("change", () => dispatch(themeAction()));
  window.addEventListener("languagechange", () => { app.preferred_languages(Array.from(navigator.languages ?? [])); queueLanguage(); });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) return;
    app.preferred_languages(Array.from(navigator.languages ?? []));
    try { const saved = localStorage.getItem(settingsKey); if (saved) dispatch({type:"restore_saved_settings",saved}); } catch {}
    queueLanguage();
  });
  state = app.state_update();
  catalog = liveCopy(app, "catalog");
  performance.mark("capy.startup.model");
  document.documentElement.style.setProperty("--ui-text-size", `${catalog.text_size_pt}pt`);
  for(const [key,value] of Object.entries(catalog.panel_metrics))document.documentElement.style.setProperty(`--panel-${key.replaceAll("_","-")}`,`${value}px`);
  for(const [key,value] of Object.entries(app.divider_metrics()))document.documentElement.style.setProperty(`--divider-${key}`,`${value}px`);
  publishAppName();
  penResponse=createPenResponse({state:()=>state,workspace,element,button,icon,numberField,dispatch});
  observePenInput({workspace,canvas,dispatch,position,owner:()=>state.document_file.epoch});
  refreshPreferences = createPreferences({ app, element, button, icon, numberField, panelFrame, dispatch, nativeCopy: catalog.native_copy, view: () => app.preferences_cached() });
  commandBar = createCommandBar({element, button, icon, dispatch, style:catalog.command_search_style, nativeCopy:catalog.native_copy, canvas, layoutChanged:() => glass?.queue()});
  panelNames = Object.fromEntries(catalog.panels.map((p) => [p.id, p.label]));
  // Issue the first storage request before constructing panel controls. Replies
  // run in later tasks, after this synchronous UI construction is complete.
  workspaceManager = createWorkspaceManager({ app, store: workspaceStore, applyChange, element, button, icon, dropdown, message });
  selectionUi = createSelectionUi({app,state:()=>state,element,button,dropdown,numberField,dispatch,workspace,layout:()=>layout,bar:()=>canvasBar?.bounds()??null});
  const separate=action=>dispatch({type:'frequency_separation',action});
  frequencySeparationUi = createPreviewPanel({app,name:'frequency-separation',view:()=>state.layer_tools.frequency_separation,kind:v=>v.label,
    value:radius=>separate({op:'radius',radius}),apply:()=>separate({op:'apply'}),cancel:()=>separate({op:'cancel'}),
    element,button,numberField,workspace,layout:()=>layout,bar:()=>canvasBar?.bounds()??null});
  canvasSizeUi = createCanvasSizeUi({state:()=>state,element,button,dropdown,icon,numberField,resolve:request=>app.number_input(request),dispatch});
  imageSizeUi = createImageSizeUi({state:()=>state,element,button,dropdown,numberField,resolve:request=>app.number_input(request),dispatch});
  configureColorEditor({dispatch,icon,wasmModule});
  editor = createEditorPanels({selectionUi,app,wasmModule,state:()=>state,workspace,canvas,element,button,icon,dropdown,numberField,dispatch,asset,wake,applyChange,contentChanged:panelContentChanged,showContext:(node,point)=>customization.showContext(node,point)});
  palettes = createPalettes({ app, state: () => state, workspace, element, button, icon, panelFrame, applyChange, rasterWorker,
    dismissContext: () => customization?.dismissContext(), contentChanged: panelContentChanged });
  buildHeader();
  buildPanels();
  paintPairView = app.paint_pair();
  customization = createCustomization({ app, rasterWorker, catalog, state: () => state, paintPair: () => paintPairView, workspace, panels, groups,
    element, button, icon,dropdown, numberField, panelFrame,
    dispatch, draggable, grip, place, updateZen, editor });
  workspaceChrome = createWorkspaceChrome({app,state:()=>state,workspace,element,button,icon,place,dispatch,customization,editor,panelFrame,panels,draggable,grip,contentPanel,tabLabel,releaseTabs});
  bubbles = createBubbles({app,workspace,element,button,icon,state:()=>state,dispatch,applyChange,wake,glassChanged:()=>glass?.queueBubbles(),customization,contentPanel,rasterWorker,asset,cancelWorkspace:()=>endWorkspaceGesture(null,true),viewport:()=>workspaceViewport,leaveCanvas:()=>cursorInput(null)});
  panelVectors=createPanelVectors({workspace,groups,place,
    project:metadata=>rasterWorker({operation:'panel-vectors',metadata,buffers:[]}),
    changed:()=>glass?.queue(),failed:error=>{workspaceLayoutFinished=workspaceLayoutRequested;clearWorkspaceTabDrop();message(error.message);}});
  glass = createGlass({app,canvas,workspace,connections:()=>workspaceChrome.connections(),vectors:()=>panelVectors.glass(),bubbles:()=>bubbles.glass(),enabled:()=>state.palette?.glass.transparency!=="off",wake});
  documents = createDocuments({app,bootstrap,delivery,state:()=>state,canvas,canvasReady:()=>gpuReady,dispatch,applyChange,wake,element,button,icon,dropdown,numberField,message,gpuOperation,rasterWorker,resumeCanvas:resumeDocumentCanvas,contentChanged:panelContentChanged,refreshInputContext:()=>chromeInput({kind:"refresh"})});
  documents.mountProof(panels.get("proof"));
  const capy = button("",()=>dispatch({type:"invoke",command:"zen_mode"}),"tile-button");
  capy.id = "zen-capy"; capy.hidden = true;
  customization.target(capy, {kind:"zen_mode"});workspace.append(capy);
  header = createHeader({app,state:()=>state,paintPair:()=>paintPairView,workspace,element,button,icon,place,dispatch,customization,systemStatus,updateZen,documents,capy});
  canvasBar = createCanvasBar({ app, workspace, element, button, icon, marker: customization.variationMarker, dispatch, glass, reappearMs: catalog.canvas_bar_reappear_ms,
    openMenu: node => customization.openMenu(node), presented: () => { selectionUi.place(); frequencySeparationUi.place(); notice?.place(); } });
  notice = createNotice({ workspace, element, button, glass, tiles: app.toolbar_ui({ type: "style", style: "small" }), answer: answerNotice, layout: () => layout, bar: () => selectionUi.bounds() ?? frequencySeparationUi.bounds() ?? canvasBar.bounds() });
  zoomReadout = createZoomReadout({ root: $("view-info"), workspace, canvas, element, button, icon, numberField, catalog,
    menu: () => app.zoom_menu(), renderMenu: customization.renderMenu, refreshMenu: customization.refreshMenu,
    dispatch, camera: () => state.camera, toggled: updateZen });
  const screenChip = element("button", "proof-status");
  screenChip.id = "screen-status";
  $("canvas-status").insertBefore(screenChip, $("view-info"));
  screenStatus = createScreenStatus({ root: screenChip, workspace, canvas, element, icon, dispatch, nativeCopy:catalog.native_copy });
  performance.mark("capy.startup.controls");
  update(255);
  systemStatus.sync();
  $("status").textContent = "";
  if (restoreError) { console.error(restoreError); message({document_host_error:{type:"delivery",reason:{type:"restore_preferences",detail:String(restoreError)}}}); }
  new ResizeObserver(() => {
    workspaceViewport = [workspace.clientWidth, workspace.clientHeight];
    arrange();
  }).observe(workspace);
  // Test harness accesses the actual Wasm instance and native widgets.
  window.layerApp = { app, dispatch, state: () => app.state(), wake, canvas, startupTimes, documents, restartGpu };
  performance.mark("capy.startup.ui");
  await workspaceManager.ready;
  documents.startRecovery().catch(error => {const detail=String(error);message(()=>app.document_recovery_unavailable(detail));});
  performance.mark("capy.startup.gpu");
  await startGpu();
  documents.loadPapers().catch(error => message(String(error)));
  if (window.launchQueue?.setConsumer) {
    let launches=Promise.resolve();
    window.launchQueue.setConsumer(params=>{
      const files=params.files??[];
      launches=launches.then(async()=>{
        if(!files.length)return;
        const deadline=performance.now()+240000;
        while(documents.busy()||!app.document_park_ready()||document.querySelector('dialog[open]')) {
          if(performance.now()>deadline)throw Error(delivery.open_drawings_operation);
          await new Promise(resolve=>setTimeout(resolve,50));
        }
        const selected=[];for(const handle of files)selected.push({file:await handle.getFile(),handle});
        await documents.openFiles(selected);
      }).catch(error=>message(String(error)));
    });
  }
} catch (error) {
  $("gpu-notice").replaceChildren(element("h1", "", bootstrap?.application_start_failed??"Capy Canvas"), element("pre", "", String(error)));
  $("status").textContent = "";
  console.error(error);
}

function stopGpu(error) {
  // A retired device may finish compilation late, or never settle its Promise.
  // Neither case may hold the new renderer’s compilation lane or stop it.
  compilerEpoch++;compilerScheduled=false;
  gpuReady=false;pending.length=0;
  const report=gpuDiagnostics.capture(error,()=>({
    ...app.gpu_diagnostics(),user_agent:navigator.userAgent,device_memory_gib:navigator.deviceMemory??null,
    main_wasm_bytes:wasmMemory?.buffer.byteLength??null,
    navigation:performance.getEntriesByType('navigation')[0]?.type??null,
  }));
  suspendGpuForReport(report,()=>applyChange(app.suspend_gpu()));
  if(startupNotice)startupNotice.hidden=true;
  const notice=$("gpu-notice");notice.hidden=false;
  notice.replaceChildren(element("p","",report.recovery_error?bootstrap.canvas_recovery_failed:bootstrap.canvas_stopped),
    element('p','diagnostic-detail',report.error),button(bootstrap.restart_canvas,()=>restartGpu()),
    gpuReportDetails({report,diagnostics:gpuDiagnostics,copy:bootstrap,element,button,clipboard:navigator.clipboard}));
  document.body.dataset.gpu="unavailable";
  return !report.recovery_error;
}
setInterval(()=>{if(gpuReady){const error=app.gpu_failure();if(error)stopGpu(error);}},1000);
async function restartGpu() {
  if(!stopGpu(bootstrap.restarting_canvas))return;
  compilerFailed=false;firstCanvasRendered=false;
  for(const key of Object.keys(startupTimes))startupTimes[key]=null;
  await startGpu();
}

function documentGpuCurrent(owner) {
  const current = app.state().document_file;
  return owner.epoch === current.epoch && owner.revision === current.revision;
}

async function resumeDocumentCanvas() {
  compilerEpoch++;compilerScheduled=false;compilerFailed=false;
  gpuReady=app.gpu_ready();firstCanvasRendered=false;pending.length=0;
  for(const key of Object.keys(startupTimes))startupTimes[key]=null;
  if(!gpuReady){
    const owner=app.state().document_file;
    try{
      await app.await_document_backing();
      if(!documentGpuCurrent(owner))return;
      const gpu=await gpuOperation(()=>app.resume_document_gpu());
      if(!documentGpuCurrent(owner)){gpu?.free();return;}
      if(gpu)app.attach_gpu(gpu);
      gpuReady=app.gpu_ready();
    }catch(error){if(documentGpuCurrent(owner))stopGpu(error);return;}
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
  notice.replaceChildren(element("div", "gpu-help", bootstrap.starting_canvas));
  try {
    if (!isSecureContext) throw new Error("WebGPU requires HTTPS or localhost.");
    if (!navigator.gpu) throw new Error("navigator.gpu is unavailable.");
    while(!gpuReady){
      const owner=app.state().document_file;
      let gpu;
      try{
        await app.await_document_backing();
        if(!documentGpuCurrent(owner))continue;
        gpu=await createGpu();
      }catch(error){if(!documentGpuCurrent(owner))continue;throw error;}
      if(!documentGpuCurrent(owner)){gpu.free();continue;}
      app.attach_gpu(gpu);
      gpuReady=true;
    }
    document.body.dataset.gpu = "ready";
    notice.hidden = true;
    wake();
  } catch (error) {
    document.body.dataset.gpu = "unavailable";
    showGpuNotice({ container: notice, error, restart: restartGpu, restartLabel: bootstrap.restart_canvas });
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
