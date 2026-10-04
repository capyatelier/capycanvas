import { createNavigationControls } from "./navigation-controls.js";
import { bindCopy } from "./localization.js";

export const MARGIN = 6;

export const readoutText = camera => `${Math.round(camera.zoom * 100)}% · ${Math.round((camera.rotation * 180) / Math.PI)}°`;

export function zoomMenuPlacement(anchor, size, viewport) {
  const left = Math.max(MARGIN, Math.min(anchor.right - size.width, viewport.width - size.width - MARGIN));
  const above = anchor.top - size.height - MARGIN;
  return { left, top: above >= MARGIN ? above : Math.max(MARGIN, Math.min(anchor.bottom + MARGIN, viewport.height - size.height - MARGIN)) };
}

export function createZoomReadout({ root, workspace, canvas, element, button, icon, numberField, catalog, menu, renderMenu, refreshMenu,
  dispatch, camera, toggled = () => {}, doc = globalThis.document, target = globalThis,
  viewport = () => ({ width: innerWidth, height: innerHeight }) }) {
  root.type = "button"; root.tabIndex = -1;
  const copy = catalog.native_copy.header;
  bindCopy(root, () => copy.zoom, "title");
  root.setAttribute("aria-haspopup", "menu"); root.setAttribute("aria-expanded", "false");
  const field = numberField(catalog.zoom, () => copy.zoom, zoom => dispatch({ type: "set_zoom", zoom }), true);
  const rotation = numberField(catalog.rotation, () => copy.rotation, rotation => dispatch({ type: "set_rotation", rotation }));
  rotation.classList.add("rotation-field");
  const popup = element("div", "zoom-menu");
  popup.popover = "auto";
  bindCopy(popup, () => copy.zoom, "ariaLabel");
  const items = element("div", "zoom-menu-items");
  items.setAttribute("role", "menu");
  const rotationItems = element("div", "zoom-menu-items");
  rotationItems.setAttribute("role", "menu");
  const controls = createNavigationControls({ element, button, icon, dispatch, commands: catalog.navigator_commands, prefix: "zoom" });
  popup.append(field, element("hr"), items, element("hr"), rotation, element("hr"), rotationItems, element("hr"), controls); workspace.append(popup);
  let text = "", wasOpen = false, previous = null;
  const open = () => popup.matches(":popover-open");
  const close = () => { if (open()) popup.hidePopover(); };
  const keepFocus = e => { if (!e.target.closest?.("input")) e.preventDefault(); };
  root.addEventListener("mousedown", keepFocus);
  popup.addEventListener("mousedown", keepFocus);
  root.addEventListener("contextmenu", e => { e.preventDefault(); if (!open()) show(); });
  root.addEventListener("pointerdown", () => { wasOpen = open(); });
  root.addEventListener("click", () => {
    const skip = wasOpen; wasOpen = false;
    if (skip || open()) { close(); return; }
    show();
  });
  function show() {
    previous = doc.activeElement;
    updateMenu(renderMenu);
    field.update(camera().zoom);
    rotation.update(camera().rotation);
    popup.showPopover();
    const { left, top } = zoomMenuPlacement(root.getBoundingClientRect(), popup.getBoundingClientRect(), viewport());
    popup.style.left = `${left}px`; popup.style.top = `${top}px`;
  }
  function updateMenu(render) {
    const model = menu();
    const boundary = Number(model.rotation_section);
    render(items, { title: model.title, sections: model.sections.slice(0, boundary) }, close);
    render(rotationItems, { title: copy.rotation, sections: model.sections.slice(boundary) }, close);
    controls.update(model.buttons);
  }
  popup.addEventListener("toggle", e => {
    const shown = e.newState === "open";
    root.setAttribute("aria-expanded", String(shown));
    if (!shown) {
      const focus = doc.activeElement;
      if (!focus || focus === doc.body || popup.contains(focus)) {
        const back = previous && previous !== doc.body && previous.isConnected && !popup.contains(previous) ? previous : canvas;
        back.focus({ preventScroll: true });
      }
      previous = null;
    }
    toggled();
  });
  target.addEventListener("keydown", e => {
    if (e.key !== "Escape" || !open() || e.target === field.entry || e.target === rotation.entry) return;
    e.preventDefault(); e.stopPropagation();
    close();
  }, { capture: true });
  function refresh(state) {
    const next = readoutText(state);
    if (next !== text) root.textContent = text = next;
    if (!open()) return;
    field.update(state.zoom);
    rotation.update(state.rotation);
    updateMenu(refreshMenu);
  }
  return { root, popup, field, rotation, controls, items, rotationItems, refresh, open, close };
}
