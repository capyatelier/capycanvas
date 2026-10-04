import {checkGradientDefinitions} from './gradient.test.mjs';
import {checkScopes,checkScopesSmoke} from './scopes.test.mjs';
import {checkTonalControls} from './tonal-controls.test.mjs';
import {checkDocumentErrors} from './document-errors-journey.test.mjs';
import {measureSessionRestart} from './session-restart-performance.test.mjs';
import {runJourney} from "./journeys.mjs";
import {checkPointwiseEffects,checkLookupTransport} from './pointwise-effects.test.mjs';
import {checkTonalSelections} from './tonal-selection.test.mjs';
import {checkColorPicker} from './color-picker.test.mjs';
import {checkInputDevices} from './input-devices.test.mjs';
import {checkCommandBar} from './command-bar.test.mjs';
import {checkToolbarComponents} from "./toolbar-components.test.mjs";
import {checkToolVariations} from "./tool-variations.test.mjs";
import {checkSelectionTools} from "./selection-tools.test.mjs";
import {checkFilterDrawer} from "./filter-drawer.test.mjs";
import {checkFilterPreviews} from "./filter-previews.test.mjs";
import {checkSpatialFilterWindows} from "./effects.test.mjs";
import {checkBrushDrawers} from "./brush-drawers.test.mjs";
import {checkStrokeRecording} from './stroke-recording.test.mjs';
import {checkContactBrushes} from "./contact-brushes.test.mjs";
import {checkUiUpdates,checkSettingsUpdates} from "./ui-updates.test.mjs";
import {checkDrawingTabs,checkDrawingTabRecovery} from "./drawing-tabs.test.mjs";
import {measureHdr} from "./hdr-performance.test.mjs";
import {checkPortablePhoto} from "./portable-photo.test.mjs";
import {checkExportMetadata} from "./export-metadata-journey.test.mjs";
import {checkSdrColor,checkColorEdits,checkSourceImports,checkSourceEdits,checkExportPresets,checkProfileLibrary,checkFlattenedCopy,checkPhotoCorrections} from "./color-m2.test.mjs";
import {checkHdr} from "./hdr.test.mjs";
import {checkProof} from "./proof.test.mjs";
import {checkPackageView} from "./package-view.test.mjs";
import {checkColorPanel} from "./color-panel.test.mjs";
import {checkDragPickup} from "./drag-pickup.test.mjs";
import {checkZen} from "./zen.test.mjs";
import {checkIcons} from "./icons.test.mjs";
import {checkPrediction} from "./prediction.test.mjs";
import {checkPenRendering} from "./pen-rendering.test.mjs";
import {checkTooltips} from "./tooltips.test.mjs";
import {checkColumnStacks} from "./column-stacks.test.mjs";
import {checkLayoutDrops} from "./layout-drops.test.mjs";
import {checkColumnDrops} from "./column-drops.test.mjs";
import {checkWorkspaceFocus,checkWorkspaceSwitcher,checkWorkspaceOptions,checkWorkspaceMenuRefresh} from "./workspace-switcher.test.mjs";
import {checkWorkspaceManagerVisual} from "./workspace-manager-visual.test.mjs";
import {checkWorkspaceManager} from "./workspace-manager.test.mjs";
import {checkTitleBarState} from "./title-bar-state.test.mjs";
import {checkTitleBar} from "./title-bar.test.mjs";
import {checkTitleBarFeedback} from "./title-bar-feedback.test.mjs";
import {checkTitleBarOverflow} from "./title-bar-overflow.test.mjs";
import {checkCompactWorkspaces} from "./compact-workspaces.test.mjs";
import {checkMenuLabels} from "./menu-labels.test.mjs";
import {checkHeaderControls} from "./header-controls.test.mjs";
import {checkWorkspaceWindows} from "./workspace-windows.test.mjs";
import {checkWorkspaceStore,checkStaleStorageStartup} from "./workspace-store.test.mjs";
import {checkLayerHolding,checkLayerSwipes} from "./layer-hold.test.mjs";
import {checkLongPressDragging} from "./long-press-drag.test.mjs";
import {checkPalettes} from "./palettes.test.mjs";
// Real Chrome + Wasm + WebGPU smoke/conformance test. No browser framework.
import {checkDrawerDragging,checkDrawerStyling,checkToolbarDrawerSwitching} from "./drawers.test.mjs";
import { checkDragCursors } from "./drag-cursors.test.mjs";
import { mkdir, writeFile } from "node:fs/promises";
import assert from "node:assert/strict";
import { launchChrome } from "../../tools/cdp.mjs";
import { checkRaster } from "./raster.test.mjs";
import { checkPhotoPaint } from "./photo-paint.test.mjs";
import { checkImagePlacement } from "./image-placement.test.mjs";
import { checkCanvasBar } from "./canvas-bar-journey.test.mjs";
import { checkNotices } from "./notice-journey.test.mjs";
import { checkZoomReadout } from "./zoom-readout-journey.test.mjs";
import { checkBlendMenu } from "./blend-menu-journey.test.mjs";
import { checkColorMixing } from "./color-mixing-journey.test.mjs";
import { checkPassThrough } from "./pass-through-journey.test.mjs";
import { checkBlending } from "./blending-journey.test.mjs";
import { checkScreenStatus } from "./screen-status-journey.test.mjs";
import { checkPipelineTakeover } from "./pipeline-takeover.test.mjs";
import { checkPhotoEdit } from "./photo-edit-journey.test.mjs";
import { checkCanvasSize } from "./canvas-size-journey.test.mjs";
import { checkCrop } from "./crop-journey.test.mjs";
import { checkImageCommands } from "./image-commands-journey.test.mjs";
import { checkMoveSelection } from "./move-selection-journey.test.mjs";
import { checkClone, checkHeal } from "./clone-journey.test.mjs";
import { checkClipboard } from "./clipboard-journey.test.mjs";
import { checkMerges } from "./merge-journey.test.mjs";
import { checkRetouchLayers } from "./retouch-layers-journey.test.mjs";
import { checkEditor } from "./editor.test.mjs";
import { checkColumnSizing } from "./columns.test.mjs";
import { checkFullscreen } from "./fullscreen.test.mjs";
import { checkParity } from "./parity.mjs";
import { checkLayers, checkSelectedPainting, checkLayerRelationships } from "./layers.test.mjs";
import { checkAdjustments, checkCurves } from "./effects.test.mjs";
import { checkPreferences, checkSettingsParity, checkLiveLanguage } from "./preferences.test.mjs";
import {checkLiveHistogramLanguage,checkLiveWorkspaceLanguage,checkLiveDeliveryLanguage,checkLiveProofLanguage,checkLiveColorFormLanguage,checkLiveToolbarLanguage,checkLiveEffectLanguage,checkLiveToolLanguage} from "./localization-journey.test.mjs";
import { checkPwa, servePackage } from "./pwa.test.mjs";
import { checkGpuStartup, checkGpuCompatibility } from "./gpu.test.mjs";
import { checkStagedStartup } from "./startup.test.mjs";
import { checkMediumTiles } from "./tiles.test.mjs";
import { checkCustomization, checkWorkspace, checkTabStyles, checkToolbarManager, checkToolPicker } from "./customization.test.mjs";
import { checkWorkspaceMotion } from "./workspace-motion.test.mjs";
import { checkWorkspaceResize } from "./workspace-resize.test.mjs";
import { checkWorkspaceRendering } from "./workspace-rendering.test.mjs";

const packageHost = process.argv.includes("--package") ? await servePackage() : null;

const cdp = await launchChrome(
  [
    ...(process.argv.includes("--headless")
      ? ["--headless=new", "--ozone-platform=headless"]
      : ["--ozone-platform=wayland"]),
    // GTK reference PNGs are sRGB; do not bake the monitor's gamma into captures.
    "--force-color-profile=srgb",
    "--enable-gpu",
    "--enable-unsafe-webgpu",
    // Offscreen Vulkan avoids Wayland's Vulkan swapchain incompatibility while
    // retaining hardware rendering. Other hosts use their native GPU backend.
    ...(process.platform === "linux" ? ["--use-angle=vulkan",
      "--enable-features=Vulkan", "--disable-vulkan-surface"] : []),
    "--window-size=1440,1000",
  ],
  {
    timeout: process.argv.some(x=>['--drawing-tabs-recovery','--scopes','--tonal-controls','--gradients'].includes(x))?300000:process.argv.some(x=>["--selection-tools","--color-mixing","--contact-brushes","--filter-drawer","--spatial-filter-windows","--lookup-transport","--local-adjustments","--drawing-tabs","--shared-workflows","--live-language-color","--live-language-proof","--live-language-delivery","--live-language-surfaces","--live-language-toolbar","--live-language-effects","--hdr","--hdr-performance","--proof","--portable-photo","--package-view","--export-metadata","--filter-investigation","--pipeline-takeover","--blending"].includes(x)) ? 180000 : 30000,
    onEvent: (event) => {
      if (
        event.method === "Runtime.consoleAPICalled" &&
        ["error", "warning"].includes(event.params.type)
      )
        cdp.report(
          event.params.args.map((a) => a.value || a.description).join(" "),
        );
      else if (
        event.method === "Log.entryAdded" &&
        ["error", "warning"].includes(event.params.entry.level) &&
        !event.params.entry.text.includes("favicon") &&
        !(event.params.entry.level === "warning" && event.params.entry.text.startsWith('Compilation log for [ShaderModule "connected region"]:') && !/\berror(?:s)?\b/i.test(event.params.entry.text))
      ) {
        if (process.env.LAYER_TEST_VERBOSE) process.stderr.write(`${JSON.stringify(event.params.entry)}\n`);
        cdp.report([event.params.entry.text, event.params.entry.url].filter(Boolean).join(" "));
      }
    },
  },
);
const { call, settle, errors } = cdp;
function checkRasterErrors() {
  if(!process.argv.includes("--offscreen-raster")){assert.deepEqual(errors,[]);return;}
  // Chrome 150 / NVIDIA 610 headless presentation loses its Dawn instance.
  // This explicit mode qualifies exported GPU pixels and frame creation only;
  // keep the normal editor/screenshot suite strict and reject every other error.
  const remaining=errors.filter(error=>!error.startsWith("A valid external Instance reference no longer exists."));
  assert.deepEqual(remaining,[]);
  if(remaining.length!==errors.length)console.log("Presentation NOT qualified: Chrome headless Dawn instance failure.");
}
async function evaluate(expression) {
  if(process.env.LAYER_TEST_VERBOSE)process.stderr.write(`Evaluate: ${expression.slice(0,300)}\n`);
  return cdp.evaluate(expression);
}
const reload = async () => { await call("Page.reload", {ignoreCache:true}); await new Promise(r=>setTimeout(r,1000)); };
async function canvasPixels() {
  const clip = await evaluate(
    "(() => { const r = layerApp.canvas.getBoundingClientRect(); return {x:r.x,y:r.y,width:r.width,height:r.height,scale:1}; })()",
  );
  const shot = await call("Page.captureScreenshot", { format: "png", clip });
  // Inspect the presented framebuffer. Reading a WebGPU canvas backbuffer in a
  // later task can return its newly cleared buffer instead of the visible frame.
  return evaluate(
    `(async () => { const image = new Image(); image.src = ${JSON.stringify("data:image/png;base64,")} + ${JSON.stringify(shot.data)}; await image.decode(); const canvas = document.createElement('canvas'); canvas.width=image.width; canvas.height=image.height; const ctx=canvas.getContext('2d',{willReadFrequently:true}); ctx.drawImage(image,0,0); const rgba=ctx.getImageData(0,0,canvas.width,canvas.height).data; let white=0,colored=0; for(let i=0;i<rgba.length;i+=4) { if(rgba[i]>245 && rgba[i+1]>245 && rgba[i+2]>245) white++; if(Math.max(rgba[i],rgba[i+1],rgba[i+2])-Math.min(rgba[i],rgba[i+1],rgba[i+2])>30) colored++; } return {white,colored,total:rgba.length/4}; })()`,
  );
}
async function click(selector) {
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);
  await settle();
}
try {
  const targetId = await cdp.attachPage();
  await call("Runtime.enable");
  await call("Page.enable");
  await call("Log.enable");
  // Keep the test tab focused even when the surrounding desktop is in use.
  if (!process.argv.includes("--fullscreen")) await call("Emulation.setFocusEmulationEnabled", { enabled: true });
  if (process.argv.includes("--parity") || process.argv.includes("--preferences"))
    await call("Input.setIgnoreInputEvents", { ignore: true });
  if (process.argv.includes("--native-input")) {
    const {windowId} = await call("Browser.getWindowForTarget", {targetId}, null);
    await call("Browser.setWindowBounds", {windowId,bounds:{windowState:"fullscreen"}}, null);
    await call("Page.bringToFront");
  } else if (!process.argv.includes("--fullscreen")) await call("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 1000,
    deviceScaleFactor: 1,
    mobile: false,
  });
  if (process.argv.includes("--fullscreen") || process.argv.includes("--header-controls") || process.argv.includes("--title-bar") || process.argv.includes("--title-bar-state")) await call("Page.addScriptToEvaluateOnNewDocument", {source:
    'window.__statusBattery=Object.assign(new EventTarget(),{level:.72,charging:true});Object.defineProperty(navigator,"getBattery",{configurable:true,value:async()=>window.__statusBattery});'});
  if (process.argv.includes("--title-bar-overflow")) await call("Page.addScriptToEvaluateOnNewDocument", {source:
    `window.__overflowEvents=[];for(const type of ['pointerdown','pointerup','click'])window.addEventListener(type,e=>{const value={type,id:e.pointerId,pointer:e.pointerType,target:e.target.tagName,source:e.target.closest('details')?.id};__overflowEvents.push(value);setTimeout(()=>{value.prevented=e.defaultPrevented},0);},true)`});
  await call("Page.navigate", {
    url: packageHost?.url || process.env.LAYER_WEB_URL || "http://127.0.0.1:4173",
  });
  await evaluate(
    `new Promise((resolve, reject) => { const started = performance.now(); function check() { if (window.layerApp && document.body.dataset.gpu === 'ready' && layerApp.app.brush_ready()) resolve(true); else if (performance.now() - started > 25000) reject(new Error(document.querySelector('#gpu-notice')?.textContent || document.querySelector('#status')?.textContent)); else setTimeout(check, 100); } check(); })`,
  );
  await settle();
  await evaluate(`new Promise((resolve,reject)=>{const deadline=performance.now()+30000;function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v?.ready&&!v.busy)resolve();else if(performance.now()>deadline)reject(Error('Workspace startup: '+JSON.stringify(v)));else setTimeout(check,100);}check();})`);
  const checkErrors = () => assert.deepEqual(errors, []);
  if (!await runJourney([
    [process.argv.includes("--filter-investigation"), async () => {
      await (await import('./filter-investigation.test.mjs')).investigate({call,evaluate,settle});
    }, checkErrors],
    [process.argv.includes("--ui-speed"), async () => {
      await checkUiUpdates({evaluate});
      await checkSettingsUpdates({evaluate,settle});
    }, checkErrors],
    [process.argv.includes("--drawing-tabs-recovery"), () => checkDrawingTabRecovery({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--session-restart-performance"), () => measureSessionRestart({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--drawing-tabs"), () => checkDrawingTabs({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--portable-photo"), () => checkPortablePhoto({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--package-view"), () => checkPackageView({evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--export-metadata"), () => checkExportMetadata({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--hdr-performance"), () => measureHdr({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--hdr"), () => checkHdr({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--image-placement"), () => checkImagePlacement({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--canvas-bar"), () => checkCanvasBar({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--notices"), () => checkNotices({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--screen-status"), () => checkScreenStatus({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--pipeline-takeover"), () => checkPipelineTakeover({evaluate,settle}), checkErrors],
    [(process.argv.includes("--zoom-readout") || process.argv.includes("--zoom-controls")), () => checkZoomReadout({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--blend-menu"), () => checkBlendMenu({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--color-mixing"), () => checkColorMixing({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--pass-through"), () => checkPassThrough({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--blending"), () => checkBlending({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--photo-edit"), () => checkPhotoEdit({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--clipboard"), () => checkClipboard({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--canvas-size"), () => checkCanvasSize({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--crop"), () => checkCrop({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--image-commands"), () => checkImageCommands({call,evaluate,settle})],
    [process.argv.includes("--merges"), () => checkMerges({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--retouch-layers"), () => checkRetouchLayers({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--move-selection"), () => checkMoveSelection({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--clone"), () => checkClone({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--heal"), () => checkHeal({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--photo-paint"), () => checkPhotoPaint({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--proof"), () => checkProof({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--document-errors"), () => checkDocumentErrors({evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--live-language-toolbar"), () => checkLiveToolbarLanguage({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--live-language-effects"), () => checkLiveEffectLanguage({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--live-language-color"), () => checkLiveColorFormLanguage({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--live-language-proof"), () => checkLiveProofLanguage({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--live-language-delivery"), () => checkLiveDeliveryLanguage({call,evaluate,settle}), checkRasterErrors],
    [process.argv.includes("--live-language-surfaces"), async () => {
      await checkLiveToolLanguage({call,evaluate,settle});
      await checkLiveHistogramLanguage({call,evaluate,settle});
      await checkLiveWorkspaceLanguage({call,evaluate,settle});
    }, checkRasterErrors],
    [process.argv.includes("--shared-workflows"), async () => {
      await checkSdrColor({call,evaluate,settle});
      await checkColorEdits({call,evaluate,settle});
      await checkSourceImports({call,evaluate,settle});
      await checkSourceEdits({call,evaluate,settle});
      await checkExportPresets({evaluate});
      await checkProfileLibrary({evaluate});
      await checkPhotoCorrections({evaluate,settle});
      await checkFlattenedCopy({evaluate});
    }, checkRasterErrors],
    [process.argv.includes("--raster"), () => checkRaster({call,evaluate,settle,canvasPixels}), checkRasterErrors],
    [process.argv.includes("--menu-labels"), () => checkMenuLabels({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--compact-workspaces"), () => checkCompactWorkspaces({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--title-bar-overflow"), () => checkTitleBarOverflow({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--title-bar-feedback"), () => checkTitleBarFeedback({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--title-bar-state"), () => checkTitleBarState({call,evaluate,settle,reload}), checkErrors],
    [process.argv.includes("--title-bar"), () => checkTitleBar({call,evaluate,settle,reload}), checkErrors],
    [process.argv.includes("--zen"), () => checkZen({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--local-adjustment-icons"), () => checkIcons({call,evaluate,settle,only:["layer-shadows-highlights-symbolic.svg","layer-clarity-symbolic.svg","layer-dehaze-symbolic.svg"]}), checkErrors],
    [process.argv.includes("--lookup-icon"), () => checkIcons({call,evaluate,settle,only:"layer-color_lookup-symbolic.svg"}), checkErrors],
    [process.argv.includes("--icons"), () => checkIcons({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--header-controls"), () => checkHeaderControls({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--layout-drops"), () => checkLayoutDrops({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--column-stacks"), () => checkColumnStacks({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--column-drops"), () => checkColumnDrops({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--tooltips"), () => checkTooltips({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--drag-pickup"), () => checkDragPickup({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--palettes"), () => checkPalettes({call,evaluate,settle,reload}), checkErrors],
    [process.argv.includes("--layer-swipes"), () => checkLayerSwipes({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--layer-hold"), () => checkLayerHolding({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-rendering"), () => checkWorkspaceRendering({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-motion"), () => checkWorkspaceMotion({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--long-press-drag"), () => checkLongPressDragging({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--drag-cursors"), () => checkDragCursors({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--workspace-windows"), () => checkWorkspaceWindows({call,evaluate}), checkErrors],
    [process.argv.includes("--workspace-focus"), () => checkWorkspaceFocus({evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-options-refresh"), () => checkWorkspaceMenuRefresh({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-options"), () => checkWorkspaceOptions({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-switcher"), () => checkWorkspaceSwitcher({call,evaluate,settle,reload}), checkErrors],
    [process.argv.includes("--workspace-manager-visual"), () => checkWorkspaceManagerVisual({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--workspace-manager"), () => checkWorkspaceManager({call,evaluate,settle,reload}), checkErrors],
    [process.argv.includes("--workspace-store"), () => checkWorkspaceStore({evaluate}), checkErrors],
    [process.argv.includes("--stale-storage"), () => checkStaleStorageStartup({evaluate,reload})],
    [process.argv.includes("--drawer-switch"), () => checkToolbarDrawerSwitching({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--drawer-style"), () => checkDrawerStyling({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--drawer-drag"), () => checkDrawerDragging({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--medium-tiles"), () => checkMediumTiles({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--fullscreen"), async () => {
      const {windowId} = await call("Browser.getWindowForTarget",{targetId},null);
      await checkFullscreen({call,evaluate,settle,windowId});
    }, checkErrors],
    [process.argv.includes("--color-panel"), () => checkColorPanel({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--filter-drawer"), () => checkFilterDrawer({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--filter-previews"), () => checkFilterPreviews({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--spatial-filter-windows"), () => checkSpatialFilterWindows({call,evaluate,settle,canvasPixels}), checkErrors],
    [process.argv.includes("--tonal-selection"), () => checkTonalSelections({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--toolbar-components"), () => checkToolbarComponents({call,evaluate,settle})],
    [process.argv.includes("--tool-variations"), () => checkToolVariations({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--selection-tools"), () => checkSelectionTools({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--command-bar"), () => checkCommandBar({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--input-devices"), () => checkInputDevices({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--color-picker"), () => checkColorPicker({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--brush-drawers"), () => checkBrushDrawers({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--editor"), () => checkEditor({call,evaluate,settle,canvasPixels}), checkErrors],
    [process.argv.includes("--staged-startup"), () => checkStagedStartup({ call, evaluate, settle, canvasPixels }), checkErrors],
    [process.argv.includes("--stroke-recording"), () => checkStrokeRecording({call,evaluate,settle})],
    [process.argv.includes("--scopes-smoke"), () => checkScopesSmoke({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--scopes"), () => checkScopes({call,evaluate,settle,canvasPixels}), checkErrors],
    [process.argv.includes('--gradients'), () => checkGradientDefinitions({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--tonal-controls"), () => checkTonalControls({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--curves"), () => checkCurves({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--pointwise-effects"), () => checkPointwiseEffects({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--pointwise-effects-smoke"), () => checkPointwiseEffects({call,evaluate,settle,motion:false,widths:[1100],effects:['threshold']}), checkErrors],
    [process.argv.includes("--local-adjustments"), () => checkPointwiseEffects({call,evaluate,settle,motion:false,localAdjustments:true,widths:process.env.LAYER_LOCAL_WIDTH?[Number(process.env.LAYER_LOCAL_WIDTH)]:[640,1100]}), checkErrors],
    [process.argv.includes("--lookup-transport"), () => checkLookupTransport({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--color-pages"), () => checkPointwiseEffects({call,evaluate,settle,motion:false,colorPages:true}), checkErrors],
    [process.argv.includes("--color-pages-smoke"), () => checkPointwiseEffects({call,evaluate,settle,motion:false,colorPages:true,widths:[1100]}), checkErrors],
    [process.argv.includes("--adjustments"), () => checkAdjustments({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--selection"), () => checkSelectedPainting({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--layer-relationships"), () => checkLayerRelationships({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--layers"), () => checkLayers({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--toolbar-manager"), () => checkToolbarManager({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--tab-styles"), () => checkTabStyles({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--settings-audit"), () => checkSettingsParity({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--workspace-resize"), () => checkWorkspaceResize({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--color-wheel-resize"), () => checkWorkspaceResize({call,evaluate,settle,scenarios:['color','color-float']}), checkErrors],
    [process.argv.includes("--columns"), () => checkColumnSizing({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--workspace"), () => checkWorkspace({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--tool-picker"), () => checkToolPicker({ call, evaluate, settle }), checkErrors],
    [process.argv.includes("--customization"), () => checkCustomization({ call, evaluate, settle, canvasPixels }), checkErrors],
    [process.argv.includes("--gpu-compatibility"), async () => {
      assert.ok(packageHost, "Use --package --gpu-compatibility to test the built distribution");
      await checkGpuCompatibility({ call, evaluate, settle, url: packageHost.url, errors });
    }, checkErrors],
    [process.argv.includes("--gpu-startup"), async () => {
      assert.ok(packageHost, "Use --package --gpu-startup to test the built distribution");
      await checkGpuStartup({ call, evaluate, settle, canvasPixels, url: packageHost.url, errors });
    }, checkErrors],
    [process.argv.includes("--contact-brushes"), () => checkContactBrushes({call,evaluate,settle},process.env.LAYER_BRUSH_PHOTO_URL)],
    [process.argv.includes("--pen"), async () => {
      await checkPenRendering({call, evaluate, settle});
      await checkPrediction({call, evaluate, settle});
    }, checkErrors],
    [process.argv.includes("--prediction"), () => checkPrediction({call, evaluate, settle}), checkErrors],
    [packageHost && !process.argv.includes("--preferences") && !process.argv.includes("--parity") && !process.argv.includes("--smoke"), () => checkPwa({ call, evaluate, settle, canvasPixels, host: packageHost, storageOnly: process.argv.includes("--package-offline") }), checkErrors],
    [process.argv.includes("--language-switching"), () => checkLiveLanguage({call,evaluate,settle}), checkErrors],
    [process.argv.includes("--preferences"), () => checkPreferences({ call, evaluate, settle, errors }), checkErrors],
    [process.argv.includes("--parity"), () => checkParity({ call, evaluate, settle }), checkErrors],
  ])) {
    assert.equal(
      await evaluate("layerApp.state().settings.theme ?? null"),
      null,
    );
    for (const value of ["light", "dark"]) {
      await call("Emulation.setEmulatedMedia", {
        features: [{ name: "prefers-color-scheme", value }],
      });
      await settle();
      assert.equal(await evaluate("layerApp.state().theme"), value);
      assert.equal(await evaluate("document.body.dataset.theme"), value);
    }
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'})");
    await call("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "light" }],
    });
    await settle();
    await call("Emulation.setEmulatedMedia", {
      features: [{ name: "prefers-color-scheme", value: "dark" }],
    });
    await settle();
    assert.equal(await evaluate("layerApp.state().theme"), "light");
    await evaluate("layerApp.dispatch({type:'set_theme',theme:null})");
    await settle();
    assert.equal(await evaluate("layerApp.state().theme"), "dark");
    const pixels = await canvasPixels();
    assert.ok(
      pixels.white > pixels.total * 0.1,
      `GPU canvas must visibly contain paper: ${JSON.stringify(pixels)}`,
    );
    assert.equal(await evaluate("layerApp.state().tabs.length"), 1);
    assert.equal(await evaluate("layerApp.state().layers.length"), 2);
    const chooseBrush = async (tool, group, id) => {
      await click(`[data-command="${tool}"]`);
      await click(`.brushes-control [data-tool-choice="${group}"]`);
      await click(`.brushes-control [data-brush="${id}"]`);
      assert.equal(await evaluate("layerApp.state().brush.preset"), id);
    };
    const chooseSize = async value => {
      await evaluate(`(() => { document.querySelector('[data-tool-setting="size"] .number-value').click(); const input=document.querySelector('[data-tool-setting="size"] .number-entry'); input.value='${value}'; input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})); })()`);
      await settle();
      assert.equal(await evaluate("layerApp.state().brush.diameter"), value);
    };
    const menuCommand = async (menu, command) => {
      await evaluate(`document.querySelector('.header-menu[data-menu="${menu}"]').open=true`);
      await settle();
      await evaluate(`(() => { const label=layerApp.state().commands.find(c=>c.id==='${command}').label; [...document.querySelectorAll('.header-menu[data-menu="${menu}"] .menu-label')].find(n=>n.textContent===label).closest('button').click(); })()`);
      await settle();
    };
    await chooseBrush("brush", "Paint", 4);
    await chooseSize(96);
    // Temporary Space shortcut must route to camera gestures, never pen records.
    const beforePan = await evaluate("layerApp.state().camera.translation");
    await evaluate(
      "window.penCalls=0; window.panEvents=[]; for(const name of ['pointerdown','pointermove','pointerup','pointercancel','lostpointercapture']) canvas.addEventListener(name,e=>panEvents.push({name,x:e.clientX,y:e.clientY,id:e.pointerId}),{capture:true}); window.originalPen=layerApp.app.pen; layerApp.app.pen=function(...args){window.penCalls++;return window.originalPen.apply(this,args);}",
    );
    await call("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: " ",
      code: "Space",
      windowsVirtualKeyCode: 32,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: 600,
      y: 450,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 650,
      y: 480,
      button: "left",
      buttons: 1,
    });
    await call("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: " ",
      code: "Space",
      windowsVirtualKeyCode: 32,
    });
    // Releasing Space before the mouse still finishes the same pan, not a stroke.
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 670,
      y: 490,
      button: "left",
      buttons: 1,
    });
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: 670,
      y: 490,
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await settle();
    assert.equal(await evaluate("window.penCalls"), 0);
    assert.deepEqual(
      await evaluate(
        "layerApp.state().camera.translation.map((v,i)=>Math.round(v-" +
          JSON.stringify(beforePan) +
          "[i]))",
      ),
      [70, 40],
      JSON.stringify(await evaluate("window.panEvents")),
    );
    await evaluate("layerApp.app.pen=window.originalPen");
    for (const [modifiers, axis] of [
      [0, 1],
      [8, 0],
    ]) {
      const before = await evaluate(
        "({t:layerApp.state().camera.translation,z:layerApp.state().camera.zoom,r:layerApp.state().camera.rotation})",
      );
      await evaluate(
        "window.lastWheel=null; document.querySelector('#canvas').addEventListener('wheel',e=>window.lastWheel={dx:e.deltaX,dy:e.deltaY,dpi:devicePixelRatio,mode:e.deltaMode},{once:true})",
      );
      await call("Input.dispatchMouseEvent", {
        type: "mouseWheel",
        x: 650,
        y: 450,
        deltaX: 0,
        deltaY: 40,
        modifiers,
      });
      await settle();
      const after = await evaluate(
        "({t:layerApp.state().camera.translation,z:layerApp.state().camera.zoom,r:layerApp.state().camera.rotation})",
      );
      const wheel = await evaluate("window.lastWheel");
      assert.equal(wheel.mode, 0);
      assert.ok(
        Math.abs(
          after.t[axis] -
            before.t[axis] +
            (wheel.dy + (axis === 0 ? wheel.dx : 0)) * wheel.dpi,
        ) < 0.1,
        JSON.stringify({ modifiers, axis, before, after, wheel }),
      );
      assert.equal(after.t[1 - axis], before.t[1 - axis]);
      assert.equal(after.z, before.z);
      assert.equal(after.r, before.r);
    }
    const beforeZoom = await evaluate("layerApp.state().camera.zoom");
    await call("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 650,
      y: 450,
      deltaX: 0,
      deltaY: -40,
      modifiers: 2,
    });
    await settle();
    assert.ok((await evaluate("layerApp.state().camera.zoom")) > beforeZoom);
    await menuCommand("view", "fit_canvas");
    assert.equal(await evaluate("layerApp.state().brush.diameter"), 96);
    await click('.layer-footer [aria-label="New layer"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    await click('[data-command="undo"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 2);
    await click('[data-command="redo"]');
    assert.equal(await evaluate("layerApp.state().layers.length"), 3);
    await click('[data-command="settings"]');
    assert.equal(
      await evaluate('document.querySelector("#settings").open'),
      true,
    );
    await evaluate('document.querySelector("#settings").close()');
    await settle();
    assert.equal(
      await evaluate("!layerApp.state().settings_open"),
      true,
    );
    await click('[data-command="settings"]');
    await evaluate(
      `(() => { document.querySelector('#setting-pressure .number-value').click(); const input=document.querySelector('#setting-pressure .number-entry'); input.value='1.5'; input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true})); })()`,
    );
    assert.equal(
      await evaluate("layerApp.state().settings.pressure_gamma"),
      1.5,
    );
    await click("#close-settings");
    assert.equal(
      await evaluate("layerApp.state().settings.pressure_gamma"),
      1.5,
    );
    assert.equal(
      await evaluate("!layerApp.state().settings_open"),
      true,
    );
    // Draw through Chrome mouse events, exercising DOM capture and Wasm input.
    await evaluate(
      "window.inkEvents=[]; for(const name of ['pointerdown','pointerup','pointercancel']) layerApp.canvas.addEventListener(name,e=>window.inkEvents.push({name,x:e.clientX,y:e.clientY,buttons:e.buttons}),{capture:true});",
    );
    const rect = await evaluate(
      "(() => { const r = layerApp.canvas.getBoundingClientRect(); return {x:r.x,y:r.y,w:r.width,h:r.height}; })()",
    );
    await call("Input.dispatchMouseEvent", {
      type: "mousePressed",
      x: rect.x + rect.w * 0.22,
      y: rect.y + rect.h * 0.48,
      button: "left",
      buttons: 1,
      clickCount: 1,
    });
    for (let i = 1; i <= 35; i++) {
      await call("Input.dispatchMouseEvent", {
        type: "mouseMoved",
        x: rect.x + rect.w * (0.22 + i * 0.015),
        y: rect.y + rect.h * (0.48 + 0.1 * Math.sin(i / 6)),
        button: "left",
        buttons: 1,
      });
    }
    await call("Input.dispatchMouseEvent", {
      type: "mouseReleased",
      x: rect.x + rect.w * 0.745,
      y: rect.y + rect.h * (0.48 + 0.1 * Math.sin(35 / 6)),
      button: "left",
      buttons: 0,
      clickCount: 1,
    });
    await settle();
    const painted = await canvasPixels();
    assert.ok(
      pixels.white - painted.white > 200,
      `Native mouse input must visibly paint: before=${pixels.white}, after=${painted.white}; ${JSON.stringify(await evaluate("({events:window.inkEvents,dpi:devicePixelRatio,w:layerApp.canvas.width,css:layerApp.canvas.clientWidth,camera:layerApp.state().camera.translation,zoom:layerApp.state().camera.zoom,undo:layerApp.state().commands.find(c=>c.id==='undo')})"))}`,
    );
    // Chrome-generated pen events must preserve pressure through DOM -> Wasm.
    await chooseBrush("pen", "Pen", 1);
    await chooseSize(96);
    const pressureInk = [];
    for (const force of [0.3, 1.0]) {
      const baseline = (await canvasPixels()).white;
      for (let i = 0; i <= 24; i++) {
        await call("Input.dispatchMouseEvent", {
          type:
            i === 0
              ? "mousePressed"
              : i === 24
                ? "mouseReleased"
                : "mouseMoved",
          x: rect.x + rect.w * (0.3 + (0.4 * i) / 24),
          y: rect.y + rect.h * 0.7,
          button: "left",
          buttons: i === 24 ? 0 : 1,
          clickCount: 1,
          pointerType: "pen",
          force,
          tiltX: 15,
          tiltY: 10,
        });
      }
      await settle();
      pressureInk.push(baseline - (await canvasPixels()).white);
      await click('[data-command="undo"]');
    }
    assert.ok(
      pressureInk[0] > 50 && pressureInk[1] > pressureInk[0] * 1.5,
      `pressure must visibly change ink width: ${pressureInk}`,
    );
    await chooseBrush("brush", "Paint", 4);
    await chooseSize(96);
    // Workspace gestures are covered by the shared browser suite below.
    const camera = await evaluate(
      "({zoom:layerApp.state().camera.zoom, rotation:layerApp.state().camera.rotation})",
    );
    const a = { id: 1, x: rect.x + rect.w * 0.4, y: rect.y + rect.h * 0.5 },
      b = { id: 2, x: rect.x + rect.w * 0.6, y: rect.y + rect.h * 0.5 };
    await call("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [a, b],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchMove",
      touchPoints: [
        { ...a, x: a.x - 30, y: a.y - 40 },
        { ...b, x: b.x + 70, y: b.y + 70 },
      ],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    await settle();
    const moved = await evaluate(
      "({zoom:layerApp.state().camera.zoom, rotation:layerApp.state().camera.rotation})",
    );
    assert.ok(
      moved.zoom > camera.zoom * 1.1 &&
        Math.abs(moved.rotation - camera.rotation) > 0.1,
      "Native two-touch input must zoom and rotate the shared camera",
    );
    await menuCommand("view", "fit_canvas");
    await mkdir("artifacts/ui", { recursive: true });
    // Match the GTK review window's logical viewport, without browser chrome.
    await call("Emulation.setDeviceMetricsOverride", {
      width: 1200,
      height: 900,
      deviceScaleFactor: 2,
      mobile: false,
    });
    await settle();
    await menuCommand("view", "fit_canvas");
    const geometryExpression =
      "JSON.parse(JSON.stringify({view:layerApp.state().camera, rect:layerApp.canvas.getBoundingClientRect().toJSON()}, (_,v)=>typeof v==='bigint'?String(v):v))";
    const geometry = await evaluate(geometryExpression);
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await evaluate("layerApp.dispatch({type:'restore_settings',settings:{...layerApp.state().settings,zen_show_capy:false,zen_reveal_at_edges:true}})");
    await click('[data-command="zen_mode"]');
    await evaluate("new Promise(r=>setTimeout(r,250))");
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      true,
    );
    assert.equal(
      await evaluate(
        "[...document.querySelectorAll('#header > *')].every(n=>getComputedStyle(n).opacity==='0')",
      ),
      true,
    );
    assert.equal(
      await evaluate("getComputedStyle(layerApp.canvas).outlineStyle"),
      "none",
    );
    assert.deepEqual(await evaluate(geometryExpression), geometry);
    const reviewClip = { x: 0, y: 0, width: 1200, height: 900, scale: 0.5 };
    const zen = await call("Page.captureScreenshot", {
      format: "png",
      clip: reviewClip,
    });
    await writeFile(
      "artifacts/ui/web-zen.png",
      Buffer.from(zen.data, "base64"),
    );
    // No hover first: hidden UI must consume a reveal tap, not paint beneath it.
    // Screenshot scaling can temporarily change emulation/hover; restore the
    // normal pointer position and wait for native hit-testing before input.
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await settle();
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      true,
    );
    assert.equal(
      await evaluate("document.elementFromPoint(20,450).id"),
      "canvas",
    );
    // Exercise the host's pointer handler without a desktop drag: entering a
    // hidden toolbar's old proximity zone must not reveal it any more.
    for (const [x, y, hidden] of [
      [600, 120, true],
      [600, 80, false],
      [600, 120, false],
      [600, 165, true],
      [600, 120, true],
      [600, 820, true], // Empty bottom edge: the status HUD is not a panel.
      [600, 820, true],
      [600, 450, true],
    ]) {
      assert.equal(
        await evaluate(
          `(() => {window.dispatchEvent(new PointerEvent('pointermove',{clientX:${x},clientY:${y},pointerType:'mouse',buttons:0,bubbles:true}));return document.querySelector('#workspace').classList.contains('zen-hidden')})()`,
        ),
        hidden,
        `Zen hover at ${x},${y}`,
      );
    }
    const beforeReveal = await evaluate("String(layerApp.state().revision)");
    await call("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{ id: 9, x: 20, y: 450 }],
    });
    await call("Input.dispatchTouchEvent", {
      type: "touchEnd",
      touchPoints: [],
    });
    await settle();
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    assert.equal(
      await evaluate("String(layerApp.state().revision)"),
      beforeReveal,
    );
    await evaluate("document.querySelector('details').open=true");
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await evaluate("document.querySelector('details').open=false");
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 24,
      y: 24,
    });
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await call("Input.dispatchMouseEvent", {
      type: "mouseMoved",
      x: 600,
      y: 450,
    });
    await click('[data-command="settings"]');
    assert.equal(
      await evaluate(
        "document.querySelector('#workspace').classList.contains('zen-hidden')",
      ),
      false,
    );
    await evaluate("layerApp.dispatch({type:'close_settings'})");
    await click('[data-command="zen_mode"]');
    assert.deepEqual(
      await evaluate("layerApp.state().camera.translation"),
      geometry.view.translation,
    );
    await evaluate("document.activeElement?.blur()");
    const transparency = await evaluate(
      "['off','low','medium','high'].indexOf(layerApp.state().settings.transparency)",
    );
    await evaluate(
      "layerApp.dispatch({type:'preferences',action:{type:'edit',id:'transparency',value:0}})",
    );
    for (const theme of ["dark", "light"]) {
      await evaluate(
        `layerApp.dispatch({type:'set_theme', theme:${JSON.stringify(theme)}})`,
      );
      await settle();
      await evaluate(
        "Promise.all([...document.images].map(image=>image.decode()))",
      );
      const surfaces = await evaluate(`(() => {
      const group=document.querySelector('.dock-group:not(.toolbar)'), tab=group.querySelector('.dock-tab[aria-selected=true]');
      return {panel:getComputedStyle(group.querySelector('.panel-frame')).backgroundColor, bar:getComputedStyle(group.querySelector('.dock-tabs')).backgroundColor, active:getComputedStyle(tab).backgroundColor, radius:getComputedStyle(tab).borderBottomRightRadius, foot:getComputedStyle(tab,'::after').width};
    })()`);
      assert.equal(
        surfaces.panel,
        theme === "dark" ? "rgb(65, 65, 65)" : "rgb(237, 237, 237)",
      );
      assert.equal(
        surfaces.bar,
        theme === "dark" ? "rgb(46, 46, 46)" : "rgb(210, 210, 210)",
      );
      assert.equal(surfaces.active, surfaces.panel);
      assert.equal(
        await evaluate(
          "getComputedStyle(document.querySelector('[data-tool-setting=\"size\"] .number-entry')).backgroundColor",
        ),
        theme === "dark" ? "rgb(51, 51, 51)" : "rgb(250, 250, 250)",
      );
      assert.equal(surfaces.radius, "0px");
      assert.equal(surfaces.foot, "6px");
      const spacing = await evaluate(
        `(() => {const strip=document.querySelector('.toolbar-controls[data-axis="horizontal"]'),box=strip.getBoundingClientRect(),tiles=[...strip.querySelectorAll('.tile-button')].map(n=>n.getBoundingClientRect()),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),tab=document.querySelector('.dock-tab').getBoundingClientRect();return{firstX:tiles[0].x-box.x,firstY:tiles[0].y-box.y,gaps:tiles.slice(1).map((b,i)=>b.x-tiles[i].right),gripRight:box.right-grip.right,gripCenter:grip.y+grip.height/2-box.y-box.height/2,tabHeight:tab.height,tileHeight:tiles[0].height};})()`,
      );
      assert.equal(spacing.firstX, 0);
      assert.equal(spacing.firstY, 0);
      assert.ok(spacing.gaps.length > 0);
      assert.deepEqual(spacing.gaps, spacing.gaps.map(() => 2));
      assert.equal(
        await evaluate(
          "(()=>{const [a,b]=[...document.querySelectorAll('.brushes-control .tool-subtools > .tool-choice-button')].slice(0,2).map(n=>n.getBoundingClientRect());return b.top-a.bottom;})()",
        ),
        2,
      );
      assert.equal(spacing.gripRight, 0);
      assert.equal(spacing.gripCenter, 0);
      assert.equal(spacing.tabHeight, spacing.tileHeight);
      assert.equal(spacing.tabHeight, 36);
      const chromeGeometry = await evaluate(`(() => {
      const strip=document.querySelector('.toolbar-controls[data-axis="horizontal"]'),panel=strip.parentElement,box=strip.getBoundingClientRect(),grip=strip.querySelector('.panel-grip').getBoundingClientRect(),tabGrip=document.querySelector('.dock-tabs>.panel-grip').getBoundingClientRect(),zen=document.querySelector('#zen-button').getBoundingClientRect();
      return {gripSize:[grip.width,grip.height],tabGripSize:[tabGrip.width,tabGrip.height],overflow:[panel.scrollWidth-panel.clientWidth,panel.scrollHeight-panel.clientHeight],top:box.top,above:zen.top,below:box.top-zen.bottom};
    })()`);
      assert.deepEqual(chromeGeometry.gripSize, [20, 36]);
      assert.deepEqual(chromeGeometry.tabGripSize, [20, 24]);
      assert.deepEqual(chromeGeometry.overflow, [0, 0]);
      assert.equal(chromeGeometry.top, 48);
      assert.ok(Math.abs(chromeGeometry.above - chromeGeometry.below) <= 1);
      const shot = await call("Page.captureScreenshot", {
        format: "png",
        clip: reviewClip,
      });
      await writeFile(
        `artifacts/ui/web-${theme}.png`,
        Buffer.from(shot.data, "base64"),
      );
    }
    await evaluate(
      `layerApp.dispatch({type:'preferences',action:{type:'edit',id:'transparency',value:${transparency}}})`,
    );
    assert.ok(
      await evaluate(
        `(() => {const n=document.querySelector('#view-info').getBoundingClientRect(),p=document.querySelector('[data-panel=layers]').getBoundingClientRect();return Math.abs(n.bottom-p.bottom)<1;})()`,
      ),
    );
    await checkWorkspace({ call, evaluate, settle });
    await checkCustomization({ call, evaluate, settle, canvasPixels });
    await evaluate("layerApp.dispatch({type:'set_theme',theme:'light'})");
    await settle();
    // Review the transparent decoration area over zoomed artwork.
    await call("Input.dispatchMouseEvent", {
      type: "mouseWheel",
      x: 600,
      y: 450,
      deltaX: 0,
      deltaY: -924,
      modifiers: 2,
    });
    await settle();
    const zoomShot = await call("Page.captureScreenshot", {
      format: "png",
      clip: { ...reviewClip, scale: 1 }, // The workspace suites restored 1× DPR.
    });
    await writeFile(
      "artifacts/ui/web-zoom.png",
      Buffer.from(zoomShot.data, "base64"),
    );
    assert.deepEqual(errors, []);
    console.log(
      "PASS: hardware Wasm/WebGPU ink, pen pressure, controls/layers/undo, settings persistence, dock moves/tabs/drag/resize, two-touch camera, Zen fade/reveal without viewport change, dark/light/Zen screenshots",
    );
  }
} catch (error) {
  await mkdir("artifacts/ui", { recursive: true });
  const failureShot = await call("Page.captureScreenshot", {
    format: "png",
  }).catch(() => null);
  if (failureShot)
    await writeFile(
      "artifacts/ui/web-failure.png",
      Buffer.from(failureShot.data, "base64"),
    );
  console.error(
    "WebGPU diagnostic:",
    await evaluate(
      "(async () => { try { const a = await navigator.gpu?.requestAdapter(); return { available: !!navigator.gpu, adapter: a ? {vendor:a.info.vendor, architecture:a.info.architecture, description:a.info.description, fallback:a.info.isFallbackAdapter} : null}; } catch (e) { return String(e); } })()",
    ).catch(String),
  );
  const info = await call("SystemInfo.getInfo", {}, null).catch(String);
  console.error(
    "Chrome GPU:",
    JSON.stringify({
      devices: info.gpu?.devices,
      features: info.gpu?.featureStatus,
      renderer: info.gpu?.auxAttributes.glRenderer,
    }),
  );
  if (process.argv.includes("--headless") && info.gpu?.featureStatus?.gpu_compositing !== "enabled")
    console.error("Headless Chrome is compositing in software, so screenshots omit WebGPU canvas pixels. Run presented-pixel checks headed, e.g. tools/performance/workspace-motion.sh web --pen");
  console.error("Page errors:", errors);
  throw error;
} finally {
  await cdp.close();
  await packageHost?.close();
}
