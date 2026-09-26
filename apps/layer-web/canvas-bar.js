import { actionField, choiceField } from "./toolbar-components.js";

export const GAP = 4, PADDING = 6;
const accent = new Set(["apply_transform", "complete_selection"]);
const text = value => JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v) : v);
export const barSchema = view => text([view.context, view.label ?? null,
  ...[view.items, view.completion].map(items => items.map(({ option, label }) => option.Action
    ? ["action", label, option.Action.state.id, option.Action.state.icon, option.Action.state.label, option.Action.checkable]
    : option.Choice
      ? ["choice", label, option.Choice.id, option.Choice.label, option.Choice.segmented, option.Choice.items.map(i => [i.label, i.icon])]
      : ["other", label]))]);

export function createCanvasBar({ app, workspace, element, button, icon, dispatch, glass, openMenu,
  reappearMs = app.canvas_bar_reappear_ms(), setTimer = setTimeout, clearTimer = clearTimeout }) {
  const root = element("section", "canvas-action-bar suppressed");
  root.setAttribute("role", "toolbar"); root.setAttribute("aria-label", "Canvas actions");
  root.style.gap = `${GAP}px`; root.style.padding = `${PADDING}px`; root.hidden = true;
  const label = element("span", "canvas-action-bar-label"); label.hidden = true;
  const more = button("", toggleMenu, "canvas-action-bar-more");
  more.append(icon("more")); more.title = "More"; more.tabIndex = -1;
  more.setAttribute("aria-label", "More"); more.setAttribute("aria-haspopup", "menu");
  root.append(label, more); workspace.append(root);
  root.addEventListener("contextmenu", e => e.preventDefault());
  root.addEventListener("mousedown", e => { if (!e.target.closest?.("input,select,textarea")) e.preventDefault(); });
  let view = null, schema = "", fields = [], sizes = null, layout = null, popup = null;
  let suppressed = false, contact = false, held = false, timer = 0;
  let menu = null, menuOpen = false, reopen = false;
  const shown = { visible: false, transform: "", items: -1 };
  more.addEventListener("pointerdown", () => { reopen = menuOpen && menu?.menuOwner === more; });

  function toggleMenu() {
    const skip = reopen; reopen = false;
    if (skip || !view) return;
    const model = app.canvas_bar_menu(view.context, Number(layout?.items ?? 0));
    if (!model) return;
    more.menuModel = () => model;
    const opened = openMenu(more);
    if (menu !== opened) {
      menu = opened;
      menu.addEventListener("toggle", e => { if (e.newState === "closed") menuOpen = false; });
    }
    menuOpen = true;
  }
  function closeMenu() {
    if (menuOpen && menu?.menuOwner === more && menu.matches(":popover-open")) menu.hidePopover();
    menuOpen = false;
  }
  function openPopup(anchor, content) {
    closePopup();
    popup = element("div", "toolbar-editor-popover panel"); popup.popover = "auto";
    popup.append(content); root.append(popup); popup.showPopover();
    const a = anchor.getBoundingClientRect(), b = popup.getBoundingClientRect();
    const below = a.bottom + 6, above = a.top - b.height - 6;
    popup.style.left = `${Math.max(6, Math.min(a.left, innerWidth - b.width - 6))}px`;
    popup.style.top = `${Math.max(6, below + b.height <= innerHeight - 6 || above < 6 ? below : above)}px`;
  }
  function closePopup() { popup?.remove(); popup = null; }

  function build(item, context, completion) {
    const send = action => dispatch({ type: "canvas_bar_edit", context, action });
    const { option } = item;
    let field;
    if (option.Action) {
      field = actionField({ element, button, icon }, option.Action, send, { label: item.label, ariaDisabled: true, reason: id => app.command_disabled_reason(id) });
      field.button.dataset.command = option.Action.state.id;
      if (completion && accent.has(option.Action.state.id)) field.button.classList.add("suggested-action");
    } else if (option.Choice) {
      field = choiceField({ element, button, icon, openPopup, closePopup }, option.Choice, send, { labels: true });
    } else field = { row: element("div", "toolbar-option"), update() {} };
    for (const node of field.row.querySelectorAll("button")) node.tabIndex = -1;
    field.row.classList.add(completion ? "canvas-action-bar-completion" : "canvas-action-bar-item");
    return field;
  }
  function rebuild() {
    closeMenu(); closePopup();
    for (const field of fields) field.row.remove();
    fields = []; sizes = null; layout = null;
    if (!view) return;
    label.textContent = view.label ?? ""; label.hidden = !view.label;
    fields = [...view.items.map(item => build(item, view.context, false)), ...view.completion.map(item => build(item, view.context, true))];
    more.before(...fields.slice(0, view.items.length).map(f => f.row));
    root.append(...fields.slice(view.items.length).map(f => f.row));
  }
  function measure() {
    root.hidden = false;
    for (const field of fields) field.row.hidden = false;
    const count = view.items.length, rows = fields.map(f => f.row);
    const heights = [more, ...rows].map(node => node.getBoundingClientRect().height);
    const widths = rows.map(node => node.getBoundingClientRect().width);
    sizes = {
      label: view.label ? label.getBoundingClientRect().width : 0,
      items: widths.slice(0, count), completion: widths.slice(count),
      more: more.getBoundingClientRect().width,
      height: Math.max(...heights) + 2 * PADDING, gap: GAP, padding: PADDING,
    };
    shown.items = -1;
  }
  function present() {
    const visible = !!(view && layout && !suppressed);
    if (root.hidden !== !view) root.hidden = !view;
    const count = Number(layout?.items ?? 0);
    let changed = visible !== shown.visible;
    if (view && count !== shown.items) {
      fields.slice(0, view.items.length).forEach((field, i) => { if (field.row.hidden !== i >= count) field.row.hidden = i >= count; });
      shown.items = count; changed ||= visible;
    }
    if (visible) {
      const ratio = globalThis.devicePixelRatio || 1, align = v => Math.round(v * ratio) / ratio;
      const transform = `translate(${align(layout.bounds.x)}px, ${align(layout.bounds.y)}px)`;
      if (transform !== shown.transform) { root.style.transform = shown.transform = transform; changed = true; }
    }
    if (visible !== shown.visible) { root.classList.toggle("suppressed", !visible); shown.visible = visible; }
    if (changed) glass?.queue();
  }
  function place() {
    if (suppressed) { present(); return; }
    if (view && !sizes) measure();
    layout = view ? app.canvas_bar_layout({ context: view.context, ...sizes }) ?? null : null;
    present();
  }
  function refresh(next = null) {
    const nextSchema = next ? barSchema(next) : "";
    view = next;
    if (nextSchema !== schema) { schema = nextSchema; rebuild(); }
    else if (view) {
      const options = [...view.items, ...view.completion];
      fields.forEach((field, i) => field.update(options[i].option));
    }
    place();
  }
  function hide() {
    clearTimer(timer); timer = 0;
    if (suppressed) return;
    suppressed = true; closeMenu(); closePopup(); present();
  }
  function settle() {
    clearTimer(timer);
    timer = setTimer(() => {
      timer = 0;
      if (contact || held) return;
      suppressed = false; place();
    }, reappearMs);
  }
  function suppress(hidden) {
    if (hidden) { contact = true; hide(); return; }
    const cleared = contact; contact = false;
    if (cleared && suppressed && !held && !timer) settle();
  }
  function defer() {
    if (view?.placement !== "near_object") return;
    hide();
    if (!contact && !held) settle();
  }
  function hold(active) {
    if (held === active) return;
    held = active;
    if (active) hide();
    else if (suppressed && !contact) settle();
  }
  return {
    root, refresh, place, suppress, defer, hold,
    bounds: () => shown.visible ? layout.bounds : null,
    menuOpen: () => menuOpen,
  };
}
