import { zoomMenuPlacement } from "./zoom-readout.js";

export function createScreenStatus({ root, workspace, canvas, element, icon, dispatch, doc = globalThis.document,
  target = globalThis, viewport = () => ({ width: innerWidth, height: innerHeight }) }) {
  root.type = "button"; root.tabIndex = -1; root.hidden = true;
  root.title = "Screen details";
  root.setAttribute("aria-haspopup", "dialog"); root.setAttribute("aria-expanded", "false");
  const warning = icon("warning");
  warning.classList.add("warning");
  const label = element("span");
  root.append(warning, label);
  const popup = element("div", "screen-details");
  popup.popover = "auto"; popup.setAttribute("role", "dialog"); popup.setAttribute("aria-label", "Screen details");
  const title = element("div", "screen-details-title");
  const headline = element("div", "screen-details-headline");
  const body = element("p", "screen-details-body");
  const mark = element("label", "screen-details-mark");
  const check = element("input", "panel-check");
  check.type = "checkbox";
  mark.append(check, element("span", "", "Highlight these colors"));
  popup.append(title, headline, body, mark);
  workspace.append(popup);
  check.addEventListener("change", () => dispatch({ type: "show_clipped_colors", visible: check.checked }));
  let screen = null, wasOpen = false, previous = null;
  const open = () => popup.matches(":popover-open");
  const close = () => { if (open()) popup.hidePopover(); };
  root.addEventListener("mousedown", e => e.preventDefault());
  root.addEventListener("pointerdown", () => { wasOpen = open(); });
  root.addEventListener("click", () => {
    const skip = wasOpen; wasOpen = false;
    if (skip || open() || !screen?.details) { close(); return; }
    previous = doc.activeElement;
    render(screen.details);
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
  });
  target.addEventListener("keydown", e => {
    if (e.key !== "Escape" || !open()) return;
    e.preventDefault(); e.stopPropagation();
    close();
  }, { capture: true });
  function render(details) {
    title.textContent = details.title;
    headline.textContent = details.headline;
    headline.classList.toggle("warning", details.warning);
    body.textContent = details.body ?? "";
    body.hidden = !details.body;
    mark.hidden = details.show_clipped === null;
    check.checked = details.show_clipped === true;
  }
  function refresh(next) {
    screen = next;
    const chip = next?.chip;
    root.hidden = !chip;
    if (chip) {
      if (label.textContent !== chip.label) label.textContent = chip.label;
      warning.style.display = chip.warning ? "" : "none";
    }
    if (!open()) return;
    if (chip && next.details) render(next.details);
    else close();
  }
  return { root, popup, refresh, open, close };
}
