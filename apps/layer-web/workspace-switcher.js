// DOM gestures and chrome only. Rust owns visibility, order and publication.
export function createWorkspaceSwitcher({dialog, list, element, button, icon, send, getView, redraw}) {
  const root = element("div", "workspace-switcher");
  root.setAttribute("role", "group"); root.setAttribute("aria-label", "Workspaces");
  document.querySelector("#document-title").after(root);
  const buttons = new Map();
  const menu = element("div", "workspace-row-menu");
  menu.popover = "manual"; menu.setAttribute("role", "menu"); dialog.append(menu);
  let contact, suppressClick = false, menuOwner, animation;
  const unavailable = () => !getView()?.ready || getView().busy || getView().switcher_busy;
  const edit = value => send({type:"edit_switcher", edit:value});
  const menuOpen = () => menu.matches(":popover-open");
  function closeMenu(focus = false) {
    if (menuOpen()) menu.hidePopover();
    menuOwner?.querySelector(".workspace-options")?.setAttribute("aria-expanded", "false");
    if (focus && menuOwner?.isConnected) menuOwner.querySelector(".workspace-options")?.focus({preventScroll:true});
  }
  function showMenu(row, point, focus = false) {
    const view = getView(), item = view.rows.find(item => item.id === row.dataset.id);
    if (!item || view.page !== "workspaces" || unavailable()) return;
    closeMenu(); menuOwner = row; menu.replaceChildren(); menu.dataset.id = item.id;
    menu.setAttribute("aria-label", `Options for ${item.title}`);
    const action = (label, kind, run, enabled = true, checked) => {
      const node = button("", () => { closeMenu(true); run(); });
      node.dataset.action = kind; node.disabled = !enabled;
      node.setAttribute("role", checked == null ? "menuitem" : "menuitemcheckbox");
      const mark = element("span", "workspace-menu-check");
      if (checked != null) { node.setAttribute("aria-checked", String(checked)); if (checked) mark.append(icon("check")); }
      node.append(mark, element("span", "", label)); menu.append(node);
    };
    const pinned = view.switcher.some(entry => entry.id === item.id), index = view.order.indexOf(item.id);
    action("Show in top bar", "pin", () => edit({type:"show", id:item.id, visible:!pinned}), true, pinned);
    action("Move Up", "up", () => edit({type:"move", id:item.id, before:view.order[index-1]}), index > 0);
    action("Move Down", "down", () => edit({type:"move", id:item.id, before:view.order[index+2] ?? null}), index < view.order.length-1);
    menu.append(element("hr"));
    action("Rename…", "rename", () => send({type:"form", kind:"rename", id:item.id}), item.options);
    if (item.delete) action("Delete…", "delete", () => send({type:"form", kind:"delete", id:item.id}));
    row.querySelector(".workspace-options").setAttribute("aria-expanded", "true");
    menu.showPopover();
    const r = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(6, Math.min(point.x, innerWidth-r.width-6))}px`;
    menu.style.top = `${Math.max(6, Math.min(point.y, innerHeight-r.height-6))}px`;
    if (focus) menu.querySelector("button:not(:disabled)")?.focus({preventScroll:true});
  }
  function decorate(row, choice, item, view) {
    row.dataset.id = item.id;
    const handle = button("", e => e.preventDefault(), "workspace-grip"); handle.append(icon("grip")); handle.tabIndex = -1;
    handle.title = "Drag to reorder"; handle.setAttribute("aria-label", handle.title);
    row.prepend(handle);
    for (const [visible, glyph, label] of [
      [view.switcher.some(entry => entry.id === item.id), "pin", "Shown in top bar"],
      [view.id === item.id, "check", "Current workspace"],
    ]) if (visible) {
      const mark = element("span", `workspace-row-mark workspace-${glyph}`); mark.append(icon(glyph));
      mark.title = label; mark.setAttribute("aria-label", label); mark.setAttribute("role", "img"); row.append(mark);
    }
    const more = button("⋮", () => {
      if (menuOpen() && menuOwner === row) { closeMenu(true); return; }
      const r = more.getBoundingClientRect(); showMenu(row, {x:r.left, y:r.bottom}, true);
    }, "workspace-options");
    more.setAttribute("aria-label", `Options for ${item.title}`); more.setAttribute("aria-haspopup", "menu"); more.setAttribute("aria-expanded", "false");
    row.append(more);
    choice.addEventListener("keydown", e => {
      if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
        e.preventDefault(); const r = row.getBoundingClientRect(); showMenu(row, {x:r.left+28, y:r.bottom}, true);
      }
    });
  }
  function clearHint() { for (const row of list.children) row.classList.remove("workspace-drop-before", "workspace-drop-after"); }
  function targetAt(point) {
    clearHint();
    const r = list.getBoundingClientRect();
    if (point.x < r.left || point.x > r.right || point.y < r.top || point.y > r.bottom) return undefined;
    const rows = [...list.children], row = rows.find(row => row.getBoundingClientRect().bottom > point.y) || rows.at(-1);
    if (!row) return undefined;
    const bounds = row.getBoundingClientRect(), after = point.y > bounds.top+bounds.height/2;
    row.classList.add(after ? "workspace-drop-after" : "workspace-drop-before");
    return after ? rows[rows.indexOf(row)+1]?.dataset.id ?? null : row.dataset.id;
  }
  function track() {
    if (!contact?.ghost) return;
    const r = list.getBoundingClientRect(), {point} = contact;
    if (point.x >= r.left && point.x <= r.right && point.y >= r.top && point.y <= r.bottom) {
      const speed = point.y < r.top+28 ? -8 : point.y > r.bottom-28 ? 8 : 0;
      if (speed) list.scrollTop += speed;
    }
    contact.before = targetAt(point); animation = requestAnimationFrame(track);
  }
  function finish(event = {type:"pointercancel"}) {
    if (!contact || (event.pointerId != null && event.pointerId !== contact.id)) return;
    // Taking capture from the initially pressed child emits lost capture there.
    // Only losing the row's own capture cancels this gesture.
    if (event.type === "lostpointercapture" && event.target !== contact.row) return;
    const previous = contact; contact = null;
    clearTimeout(previous.timer); cancelAnimationFrame(animation); clearHint();
    previous.ghost?.remove(); previous.row.classList.remove("workspace-row-dragging");
    if (previous.row.hasPointerCapture(previous.id)) previous.row.releasePointerCapture(previous.id);
    if (previous.held || previous.ghost || event.type !== "pointerup") suppressClick = true;
    if (event.type !== "pointerup") closeMenu();
    if (event.type === "pointerup" && previous.ghost && previous.before !== undefined) edit({type:"move", id:previous.row.dataset.id, before:previous.before});
    else if (menuOpen()) menu.querySelector("button:not(:disabled)")?.focus({preventScroll:true});
    redraw();
  }
  list.addEventListener("pointerdown", e => {
    if (contact) finish();
    suppressClick = false;
    if (unavailable() || getView().page !== "workspaces" || e.button || !e.isPrimary || e.target.closest(".workspace-options")) return;
    const row = e.target.closest(".workspace-row"); if (!row) return;
    const handle = !!e.target.closest(".workspace-grip");
    contact = {id:e.pointerId, row, point:{x:e.clientX,y:e.clientY}, x:e.clientX, y:e.clientY,
      wait:e.pointerType !== "mouse" && !handle, held:false};
    // Mouse rows already drag immediately; holding never opens their menu.
    if (e.pointerType !== "touch" && e.pointerType !== "pen") return;
    const current = contact;
    current.timer = setTimeout(() => {
      if (contact !== current || !row.isConnected) return;
      current.held = true; row.setPointerCapture(current.id); showMenu(row, current.point);
    }, 500);
  });
  list.addEventListener("pointermove", e => {
    if (!contact || contact.id !== e.pointerId) return;
    const distance = Math.hypot(e.clientX-contact.x, e.clientY-contact.y);
    if (contact.wait && !contact.held) { if (distance > 8) finish(); return; }
    contact.point = {x:e.clientX,y:e.clientY};
    if (!contact.ghost && distance > 6) {
      clearTimeout(contact.timer); closeMenu(); contact.row.setPointerCapture(contact.id);
      const r = contact.row.getBoundingClientRect(), ghost = contact.row.cloneNode(true);
      ghost.classList.add("workspace-drag-preview"); ghost.removeAttribute("data-id");
      ghost.setAttribute("aria-hidden", "true"); ghost.inert = true; ghost.popover = "manual";
      ghost.style.width = `${r.width}px`; ghost.style.left = `${r.left}px`; dialog.append(ghost); ghost.showPopover();
      contact.ghost = ghost; contact.top = r.top; contact.row.classList.add("workspace-row-dragging"); track();
    }
    if (contact.ghost) { e.preventDefault(); contact.ghost.style.top = `${contact.top+e.clientY-contact.y}px`; contact.before = targetAt(contact.point); }
  });
  list.addEventListener("touchmove", e => { if (contact?.held && e.touches.length === 1) e.preventDefault(); }, {passive:false});
  for (const name of ["pointerup", "pointercancel", "lostpointercapture"]) list.addEventListener(name, finish);
  list.addEventListener("click", e => { if (suppressClick) { suppressClick = false; e.preventDefault(); e.stopImmediatePropagation(); } }, true);
  list.addEventListener("contextmenu", e => {
    const row = e.target.closest(".workspace-row"); if (!row || getView().page !== "workspaces") return;
    e.preventDefault(); if (!contact?.ghost) showMenu(row, {x:e.clientX,y:e.clientY}, !contact);
  });
  list.addEventListener("scroll", () => { if (!contact?.ghost && !contact?.held) { finish(); closeMenu(); } });
  window.addEventListener("pointerdown", e => { if (menuOpen() && !menu.contains(e.target)) closeMenu(); }, true);
  window.addEventListener("keydown", e => {
    if (e.key === "Escape" && (contact || menuOpen())) { finish(); closeMenu(true); e.preventDefault(); e.stopImmediatePropagation(); }
  }, true);
  window.addEventListener("blur", () => { finish(); closeMenu(); });
  window.addEventListener("resize", () => { finish(); closeMenu(); });
  menu.addEventListener("keydown", e => {
    const items = [...menu.querySelectorAll("button:not(:disabled)")];
    const current = items.indexOf(document.activeElement);
    const next = {ArrowDown:(current+1)%items.length, ArrowUp:(current-1+items.length)%items.length, Home:0, End:items.length-1}[e.key];
    if (next != null) { e.preventDefault(); items[next]?.focus(); }
    if (e.key === "Tab") closeMenu();
  });
  function render(view) {
    const ids = new Set(view.switcher.map(row => row.id));
    for (const [id, node] of buttons) if (!ids.has(id)) { node.remove(); buttons.delete(id); }
    for (const [index, item] of view.switcher.entries()) {
      let node = buttons.get(item.id);
      if (!node) { node = button("", () => send({type:"switch",id:item.id})); node.append(element("span")); node.dataset.workspaceId = item.id; buttons.set(item.id,node); }
      if (root.children[index] !== node) root.insertBefore(node, root.children[index] ?? null);
      node.firstElementChild.textContent = item.title; node.title = `Switch to ${item.title} workspace`;
      node.setAttribute("aria-pressed", String(item.id === view.id)); node.disabled = unavailable() || !!view.page || !!view.form;
    }
    root.hidden = !ids.size;
    if (!view.page || view.form || (contact && !view.order.includes(contact.row.dataset.id))) { finish(); closeMenu(); }
    for (const more of list.querySelectorAll(".workspace-options")) more.disabled = unavailable();
  }
  return {root, render, decorate, closeMenu, dragging:() => !!contact, cancel:() => { finish(); closeMenu(); }};
}
