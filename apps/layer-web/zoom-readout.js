// Browser projection of the canvas zoom readout. The "N% · D°" button opens the
// shared zoom menu under a typed zoom field. Rust supplies the menu, the field's
// control and every change; the field shows the camera's zoom. Opening the menu
// or choosing an item never takes focus from the canvas; typing borrows it and
// closing hands it back.
export const MARGIN = 6;

export const readoutText = camera => `${Math.round(camera.zoom * 100)}% · ${Math.round((camera.rotation * 180) / Math.PI)}°`;

export function zoomMenuPlacement(anchor, size, viewport) {
  const left = Math.max(MARGIN, Math.min(anchor.right - size.width, viewport.width - size.width - MARGIN));
  const above = anchor.top - size.height - MARGIN;
  return { left, top: above >= MARGIN ? above : Math.max(MARGIN, Math.min(anchor.bottom + MARGIN, viewport.height - size.height - MARGIN)) };
}

export function createZoomReadout({ root, workspace, canvas, element, numberField, control, menu, renderMenu, refreshMenu,
  dispatch, camera, toggled = () => {}, doc = globalThis.document, target = globalThis,
  viewport = () => ({ width: innerWidth, height: innerHeight }) }) {
  root.type = "button"; root.tabIndex = -1;
  root.title = "Canvas zoom and rotation";
  root.setAttribute("aria-haspopup", "menu"); root.setAttribute("aria-expanded", "false");
  const field = numberField(control, "Zoom", zoom => dispatch({ type: "set_zoom", zoom }), true);
  const popup = element("div", "zoom-menu");
  popup.popover = "auto"; popup.setAttribute("aria-label", "Zoom");
  const items = element("div", "zoom-menu-items");
  items.setAttribute("role", "menu");
  popup.append(field, element("hr"), items); workspace.append(popup);
  let text = "", wasOpen = false, previous = null;
  const open = () => popup.matches(":popover-open");
  const close = () => { if (open()) popup.hidePopover(); };
  const keepFocus = e => { if (!e.target.closest?.("input")) e.preventDefault(); };
  root.addEventListener("mousedown", keepFocus);
  popup.addEventListener("mousedown", keepFocus);
  root.addEventListener("contextmenu", e => e.preventDefault());
  root.addEventListener("pointerdown", () => { wasOpen = open(); });
  root.addEventListener("click", () => {
    const skip = wasOpen; wasOpen = false;
    if (skip || open()) { close(); return; }
    previous = doc.activeElement;
    renderMenu(items, menu(), close);
    field.update(camera().zoom);
    popup.showPopover();
    const { left, top } = zoomMenuPlacement(root.getBoundingClientRect(), popup.getBoundingClientRect(), viewport());
    popup.style.left = `${left}px`; popup.style.top = `${top}px`;
  });
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
    if (e.key !== "Escape" || !open() || e.target === field.entry) return;
    e.preventDefault(); e.stopPropagation();
    close();
  }, { capture: true });
  function refresh(state) {
    const next = readoutText(state);
    if (next !== text) root.textContent = text = next;
    if (!open()) return;
    field.update(state.zoom);
    refreshMenu(items, menu(), close);
  }
  return { root, popup, field, items, refresh, open, close };
}
