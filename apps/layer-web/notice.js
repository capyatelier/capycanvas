export const TIMEOUT_MS = 4000, MARGIN = 12, BAR_REACH = 72, MAX_WIDTH = 720;

// Bottom-centre anchor and width in workspace units, from the shared layout:
// above the canvas status strip, or above a canvas action bar placed along it.
export function noticePlacement({ work_area: area, status }, bar) {
  const floor = Math.min(area.y + area.height, status.height > 0 ? status.y : Infinity);
  const y = bar && bar.y + bar.height > floor - BAR_REACH ? bar.y - MARGIN : floor - MARGIN;
  return { x: area.x + area.width / 2, y, width: Math.min(MAX_WIDTH, Math.max(0, area.width - 2 * MARGIN)) };
}

export function createNotice({ workspace, element, button, answer, layout, bar,
  timeoutMs = TIMEOUT_MS, setTimer = setTimeout, clearTimer = clearTimeout }) {
  const root = element("div", "canvas-notice");
  root.setAttribute("role", "status"); root.setAttribute("aria-label", "Canvas notice");
  root.hidden = true;
  const text = element("span", "canvas-notice-text");
  const action = button("", accept, "canvas-notice-action");
  action.tabIndex = -1; action.hidden = true;
  root.append(text, action); workspace.append(root);
  root.addEventListener("mousedown", e => e.preventDefault());
  root.addEventListener("contextmenu", e => e.preventDefault());
  let shown = null, timer = 0, transform = "", width = "";

  function accept() {
    if (shown === null || root.hidden) return;
    hide();
    answer(shown, true);
  }
  function hide() {
    clearTimer(timer); timer = 0;
    if (!root.hidden) root.hidden = true;
  }
  function place() {
    const resolved = layout();
    if (root.hidden || !resolved) return;
    const placement = noticePlacement(resolved, bar());
    const ratio = globalThis.devicePixelRatio || 1, align = v => Math.round(v * ratio) / ratio;
    const next = `translate(${align(placement.x)}px, ${align(placement.y)}px) translate(-50%, -100%)`;
    if (next !== transform) root.style.transform = transform = next;
    if (`${placement.width}px` !== width) root.style.maxWidth = width = `${placement.width}px`;
  }
  function publish(notice) {
    if (!notice) { hide(); shown = null; return; }
    text.textContent = notice.text;
    action.textContent = notice.action?.label ?? "";
    action.hidden = !notice.action;
    if (shown === notice.id) return;
    const id = shown = notice.id;
    root.hidden = false;
    place();
    clearTimer(timer);
    timer = setTimer(() => { timer = 0; root.hidden = true; answer(id, false); }, timeoutMs);
  }
  return { root, publish, hide, place };
}
