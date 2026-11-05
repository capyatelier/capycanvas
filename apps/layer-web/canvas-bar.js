import { actionField, choiceField } from "./toolbar-components.js";
import { revealTooltip } from "./tooltips.js";

const text = value => JSON.stringify(value, (_, v) => typeof v === "bigint" ? String(v) : v);
export const barSchema = view => text([view.context, view.label ?? null,
  ...[view.items, view.completion].map(items => items.map(({ option, label, menu, icon, icon_only, separated }) => [icon_only, separated, menu
    ? ["menu", label, menu, icon, option.Action?.state.id]
    : option.Action
      ? ["action", label, option.Action.state.id, option.Action.state.icon, option.Action.state.label, option.Action.checkable]
      : option.Choice
        ? ["choice", label, option.Choice.id, option.Choice.label, option.Choice.segmented, option.Choice.columns, option.Choice.beside, option.Choice.items.map(i => [i.label, i.icon])]
        : ["other", label]]))]);

export function createCanvasBar({ app, workspace, element, button, icon, marker, dispatch, glass, openMenu, presented,
  reappearMs, metrics = () => "", setTimer = setTimeout, clearTimer = clearTimeout, explain = revealTooltip }) {
  const tiles = app.toolbar_ui({ type: "style", style: "small" });
  const root = element("section", "canvas-action-bar suppressed");
  root.setAttribute("role", "toolbar"); root.setAttribute("aria-label", "Canvas actions");
  root.style.setProperty("--bar-gap", `${tiles.gap}px`); root.style.setProperty("--bar-tile", `${tiles.size[1]}px`);
  root.style.setProperty("--tile-icon-size", `${tiles.icon}px`);
  root.hidden = true;
  const label = element("span", "canvas-action-bar-label"); label.hidden = true;
  const more = button("", toggleMenu, "canvas-action-bar-more");
  more.append(icon("more")); more.title = "More"; more.tabIndex = -1;
  more.setAttribute("aria-label", "More"); more.setAttribute("aria-haspopup", "menu");
  root.append(label, more); workspace.append(root);
  root.addEventListener("contextmenu", e => e.preventDefault());
  root.addEventListener("mousedown", e => { if (!e.target.closest?.("input,select,textarea")) e.preventDefault(); });
  let view = null, schema = "", widthSchema = "", fields = [], sizes = null, layout = null, popup = null;
  let publication = null, timer = 0, timerKey = "", metricsKey = "";
  let menu = null, owner = null;
  const shown = { visible: false, transform: "", items: -1 };
  function toggleMenu() {
    showMenu(more, () => app.canvas_bar_menu(view.context, Number(layout?.items ?? 0)));
  }
  function showMenu(node, query) {
    if (!view) return;
    node.menuTarget={kind:'canvas_bar'};node.menuModel = query;
    const opened = openMenu(node);
    if (menu !== opened) {
      menu = opened;
      menu.addEventListener("toggle", e => { if (e.newState === "closed") owner = null; });
    }
    owner = node;
  }
  function closeMenu() {
    if (owner && menu?.menuOwner === owner && menu.matches(":popover-open")) menu.hidePopover();
    owner = null;
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

  function menuField(item, context) {
    const row = element("div", "toolbar-option toolbar-action canvas-action-bar-menu");
    row.dataset.toolbarField = "";
    const b = button("", () => {
      if (b.getAttribute("aria-disabled") === "true") explain(b);
      else showMenu(b, () => app.canvas_bar_choice_menu(context, item.menu));
    });
    b.dataset.canvasBarMenu = item.menu;
    b.setAttribute("aria-label", item.label); b.setAttribute("aria-haspopup", "menu");
    if (item.icon_only) b.append(icon(item.icon), marker());
    else { row.classList.add("text-tile"); b.append(icon(item.icon), element("span", "toolbar-action-label", item.label), icon("chevron-down")); }
    row.append(b);
    let tooltip, disabled;
    function update(option) {
      const state = option.Action?.state, off = state ? !state.enabled : false;
      const text = !state ? item.label : state.enabled ? state.tooltip : state.disabled_reason ?? state.tooltip;
      if (disabled !== off) b.setAttribute("aria-disabled", String(disabled = off));
      if (tooltip !== text) b.title = tooltip = text;
    }
    update(item.option);
    return { row, update };
  }
  function build(item, context, completion) {
    const send = action => dispatch({ type: "canvas_bar_edit", context, action });
    const { option } = item;
    let field;
    if (item.menu) field = menuField(item, context);
    else if (option.Action) {
      field = actionField({ element, button, icon }, option.Action, send, { label: item.icon_only ? undefined : item.label, ariaDisabled: true, explain });
      field.button.dataset.command = option.Action.state.id;
      if (item.accent) field.button.classList.add("suggested-action");
      if (!item.icon_only) field.row.classList.add("text-tile");
    } else if (option.Choice) {
      field = choiceField({ app, element, button, icon, openPopup, closePopup }, option.Choice, send, { labels: true });
      if (!option.Choice.segmented) field.row.classList.add("text-tile");
    } else field = { row: element("div", "toolbar-option"), update() {} };
    for (const node of field.row.querySelectorAll("button")) node.tabIndex = -1;
    const slot = element("div", completion ? "canvas-action-bar-completion" : "canvas-action-bar-item");
    if (item.separated) {
      const divider = element("span", "tile-divider canvas-action-bar-divider");
      divider.setAttribute("role", "separator"); divider.setAttribute("aria-orientation", "vertical");
      slot.append(divider);
    }
    slot.append(field.row);
    return { ...field, slot };
  }
  function rebuild() {
    closeMenu(); closePopup();
    for (const field of fields) field.slot.remove();
    fields = []; sizes = null; layout = null;
    if (!view) return;
    label.textContent = view.label ?? ""; label.hidden = !view.label; label.style.width = "";
    fields = [...view.items.map(item => build(item, view.context, false)), ...view.completion.map(item => build(item, view.context, true))];
    more.before(...fields.slice(0, view.items.length).map(f => f.slot));
    root.append(...fields.slice(view.items.length).map(f => f.slot));
  }
  function measure(key = metrics()) {
    if (key !== metricsKey) { metricsKey = key; sizes = null; }
    if (!view || sizes) return;
    root.hidden = false;
    for (const field of fields) field.slot.hidden = false;
    const count = view.items.length, slots = fields.map(f => f.slot);
    const worded = [label, ...slots.flatMap(node => [...node.querySelectorAll(".text-tile > button, .toolbar-segments > button")])];
    worded.forEach(node => { node.style.width = ""; });
    const natural = worded.map(node => node.getBoundingClientRect().width);
    worded.forEach((node, i) => { node.style.width = natural[i] ? `${Math.ceil(natural[i])}px` : ""; });
    const heights = [more, ...slots].map(node => node.getBoundingClientRect().height);
    sizes = {
      label: view.label ? label.getBoundingClientRect().width : 0,
      items: slots.slice(0, count).map(node => node.getBoundingClientRect().width),
      completion: slots.slice(count).map(node => node.getBoundingClientRect().width),
      more: more.getBoundingClientRect().width,
      height: Math.max(...heights), gap: tiles.gap, padding: 0,
    };
    shown.items = -1;
    dispatch({ type: "measure_canvas_bar", measure: { context: view.context, ...sizes } });
    apply(publication);
  }
  function present() {
    const visible = !!(view && layout);
    if (root.hidden !== !view) root.hidden = !view;
    const count = Number(layout?.items ?? 0);
    let changed = visible !== shown.visible;
    if (view && count !== shown.items) {
      fields.slice(0, view.items.length).forEach((field, i) => { if (field.slot.hidden !== i >= count) field.slot.hidden = i >= count; });
      shown.items = count; changed ||= visible;
    }
    if (visible) {
      const ratio = globalThis.devicePixelRatio || 1, align = v => Math.round(v * ratio) / ratio;
      const transform = `translate(${align(layout.bounds.x)}px, ${align(layout.bounds.y)}px)`;
      if (transform !== shown.transform) { root.style.transform = shown.transform = transform; changed = true; }
    }
    if (visible !== shown.visible) { root.classList.toggle("suppressed", !visible); shown.visible = visible; }
    if (changed) { glass?.queue(); presented?.(); }
  }
  function refresh(next = null) {
    const nextSchema = next ? barSchema(next) : "";
    const nextWidths = text(next ? [...next.items, ...next.completion].map(({option}) => {
      const choice = option.Choice;
      return choice && !choice.segmented ? (choice.items.find(item => item.selected) || choice.items[0])?.label : null;
    }) : []);
    if (nextWidths !== widthSchema) { widthSchema = nextWidths; sizes = null; }
    view = next;
    if (nextSchema !== schema) { schema = nextSchema; rebuild(); }
    else if (view) {
      const options = [...view.items, ...view.completion];
      fields.forEach((field, i) => field.update(options[i].option));
    }
    measure();
    apply(publication);
  }
  function apply(next) {
    publication = next;
    const current = !!(view && next && text(view.context) === text(next.context));
    layout = current ? next.layout : null;
    const key = current && sizes && !layout && next.hold % 2 === 0 ? text([next.context, next.hold]) : "";
    if (key !== timerKey) {
      clearTimer(timer); timer = 0; timerKey = key;
      if (key) {
        const { context, hold } = next;
        timer = setTimer(() => { timer = 0; dispatch({ type: "reveal_canvas_bar", context, hold }); }, reappearMs);
      }
    }
    if (!layout) { closeMenu(); closePopup(); }
    present();
  }
  return {
    root, refresh, measure, apply,
    bounds: () => shown.visible ? layout.bounds : null,
    menuOpen: () => !!owner && menu?.menuOwner === owner && menu.matches(":popover-open"),
  };
}
