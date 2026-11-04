export const TIMEOUT_MS = 4000;

export function createNotice({ above, workspace, element, button, answer, layout, bar, tiles, glass, arrangement,
  timeoutMs = TIMEOUT_MS, setTimer = setTimeout, clearTimer = clearTimeout }) {
  const root = element("div", "canvas-notice");
  root.setAttribute("role", "status");
  root.style.setProperty("--bar-gap", `${tiles.gap}px`); root.style.setProperty("--bar-tile", `${tiles.size[1]}px`);
  root.hidden = true;
  const text = element("span", "canvas-notice-text");
  const actions = element("span", "canvas-notice-actions");
  actions.hidden = true;
  root.append(text, actions); workspace.append(root);
  root.addEventListener("mousedown", e => e.preventDefault());
  root.addEventListener("contextmenu", e => e.preventDefault());
  let shown = null, timer = 0, transform = "", presented = "";

  function accept(action) {
    if (shown === null || root.hidden) return;
    hide();
    answer(shown, true, action);
  }
  function show(list) {
    actions.replaceChildren(...list.map(({ id, label, enabled, accent, reason }) => {
      const node = button(label, () => accept(id), accent ? "canvas-notice-action suggested-action" : "canvas-notice-action");
      node.disabled = !enabled;
      if (reason) node.title = reason;
      return node;
    }));
    actions.hidden = list.length === 0;
    root.classList.toggle("text-only", list.length === 0);
  }
  function hide() {
    clearTimer(timer); timer = 0;
    if (root.hidden) return;
    root.hidden = true; presented = "";
    glass?.queue();
  }
  function place() {
    const resolved = layout();
    if (root.hidden || !resolved) return;
    const placement = above(resolved.notice, bar());
    const ratio = globalThis.devicePixelRatio || 1, align = v => Math.round(v * ratio) / ratio;
    root.style.maxWidth = "none"; root.classList.remove("stacked");
    const natural = node => node.hidden ? 0 : node.getBoundingClientRect().width;
    const content = placement.max_width, words = natural(text), offers = natural(actions);
    const beside = arrangement({ type: "notice_layout", text: words, content, actions: offers, gap: tiles.gap, min_text_width: placement.min_text_width }).beside;
    root.classList.toggle("stacked", !beside);
    root.style.maxWidth = `${placement.max_width}px`;
    const next = `translate(${align(placement.x)}px, ${align(placement.y)}px) translate(-50%, -100%)`;
    if (next !== transform) root.style.transform = transform = next;
    const shape = `${next} ${content} ${words} ${offers}`;
    if (shape !== presented) { presented = shape; glass?.queue(); }
  }
  function publish(notice) {
    if (!notice) { hide(); shown = null; return; }
    text.textContent = notice.text;
    root.setAttribute("aria-label", notice.name);
    show(notice.actions ?? []);
    if (shown === notice.id) { if (!root.hidden) place(); return; }
    const id = shown = notice.id;
    root.hidden = false;
    place();
    clearTimer(timer);
    timer = setTimer(() => { hide(); answer(id, false); }, timeoutMs);
  }
  return { root, publish, hide, place };
}
