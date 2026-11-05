import {createContextPopover} from './context-popover.js';
import {bindCopy,liveCopy} from "./localization.js";
// DOM gestures and chrome only. Rust owns visibility, order and publication.
export function createWorkspaceSwitcher({app,dialog, list, element, button, icon, send, getView, redraw}) {
  const copy=liveCopy(app,"catalog").native_copy.header;
  const root = element("div", "workspace-switcher");
  root.tabIndex = -1; root.setAttribute("role", "group");
  document.querySelector("#document-title").after(root);
  const choices = element("div", "workspace-switcher-choices");
  const options = button("", () => root.openOptions?.(options), "workspace-switcher-options");
  options.append(icon("more-small")); options.setAttribute("aria-haspopup", "menu");
  root.append(choices, options);
  const buttons = new Map();
  const contextPopover=createContextPopover({element,classes:"workspace-row-menu"}),menu=contextPopover.root;
  dialog.append(menu);
  let contact, suppressClick = false, menuOwner, menuAnchor, animation;
  const unavailable = () => !getView()?.ready || getView().busy || getView().switcher_busy;
  const edit = value => send({type:"edit_switcher", edit:value});
  const menuOpen = () => menu.matches(":popover-open");
  function closeMenu(focus = false) {
    if (menuOpen()) menu.hidePopover();
    menuAnchor?.setAttribute("aria-expanded", "false");
    if (focus && menuAnchor?.isConnected) menuAnchor.focus({preventScroll:true});
  }
  function showMenu(row, point, focus = false, reorder = true, anchor = row.querySelector(".workspace-grip")) {
    const view = getView(), item = view.rows.find(item => item.id === row.dataset.id);
    if (!item || view.page !== "workspaces" || unavailable()) return;
    closeMenu(); menuOwner = row; menuAnchor = anchor; contextPopover.body.replaceChildren(); menu.dataset.id = item.id;
    bindCopy(menu,()=>app.native_caption({type:"options_for",title:getView()?.rows.find(row=>row.id===menuOwner?.dataset.id)?.title??""}),"ariaLabel");
    const action = (label, kind, run, enabled = true) => {
      const node = button("", () => { closeMenu(true); run(); });
      node.dataset.action = kind; node.disabled = !enabled;
      node.setAttribute("role", "menuitem");
      const caption=element("span");bindCopy(caption,label);node.append(caption);contextPopover.body.append(node);
    };
    if(!reorder) for (const offered of item.options) {
      action(()=>getView()?.rows.find(row=>row.id===item.id)?.options.find(value=>value.action.type===offered.action.type)?.label??"", offered.action.type,
        () => send({type:"action",action:offered.action}));
    }
    {
      const index = view.order.indexOf(item.id);
      action(()=>copy.move_up, "up", () => edit({type:"move", id:item.id, before:view.order[index-1]}), index > 0);
      action(()=>copy.move_down, "down", () => edit({type:"move", id:item.id, before:view.order[index+2] ?? null}), index < view.order.length-1);
    }
    menuAnchor?.setAttribute("aria-expanded", "true");
    menu.showPopover();
    contextPopover.place({...point,width:1,height:1},true);
    if (focus) menu.querySelector("button:not(:disabled)")?.focus({preventScroll:true});
  }
  function decorate(row, choice, item, view) {
    row.dataset.id = item.id;
    // Keep a button hit target: Chrome can redirect touches on a narrow passive
    // icon to the neighboring row button, which would require a hold to drag.
    const handle = button("", () => {
      if (menuOpen() && menuAnchor === handle) { closeMenu(true); return; }
      const r = handle.getBoundingClientRect(); showMenu(row, {x:r.left, y:r.bottom}, true);
    }, "workspace-grip"); handle.append(icon("grip"));
    const reorderCaption=()=>app.native_caption({type:"reorder",title:getView()?.rows.find(row=>row.id===item.id)?.title??item.title});
    bindCopy(handle,reorderCaption,"title");bindCopy(handle,reorderCaption,"ariaLabel");
    handle.setAttribute("aria-haspopup", "menu"); handle.setAttribute("aria-expanded", "false");

    {
      const mark = element("span", "workspace-row-mark workspace-check"); mark.append(icon("check"));
      bindCopy(mark,()=>copy.current_workspace,"title");bindCopy(mark,()=>copy.current_workspace,"ariaLabel"); mark.setAttribute("role", "img");mark.hidden=view.id!==item.id; row.append(mark);
    }
    const pin = button("", () => {
      const value = getView()?.rows.find(row => row.id === item.id)?.pin;
      if (value?.enabled) edit(value.edit);
    }, "workspace-pin");
    bindCopy(pin,()=>getView()?.rows.find(row=>row.id===item.id)?.pin?.label??"","title");
    bindCopy(pin,()=>getView()?.rows.find(row=>row.id===item.id)?.pin?.label??"","ariaLabel");
    row.append(pin); updatePin(row, item.pin);
    row.append(handle);
    const remove=button("",()=>{const offered=getView()?.rows.find(row=>row.id===item.id)?.options.find(item=>item.action.type==="delete");if(offered?.enabled)send({type:"action",action:offered.action});},"workspace-swipe-delete");
    bindCopy(remove,()=>getView()?.rows.find(row=>row.id===item.id)?.options.find(item=>item.action.type==="delete")?.label??"");
    remove.hidden=true;row.append(remove);
    row.addEventListener("click", e => {
      if (!e.target.closest(".workspace-choice,.workspace-swipe-delete,.workspace-pin,.workspace-grip") && !unavailable()) choice.click();
    });
    choice.addEventListener("keydown", e => {
      if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
        e.preventDefault(); const r = row.getBoundingClientRect(); showMenu(row, {x:r.left+28, y:r.bottom}, true,false,choice);
      }
    });
  }
  function updatePin(row, value) {
    const pin = row.querySelector(".workspace-pin");
    if (!pin || !value) return;
    pin.setAttribute("aria-disabled", String(!value.enabled));
    pin.setAttribute("aria-pressed", String(value.selected));
    if (pin.dataset.icon !== value.icon) {
      pin.dataset.icon = value.icon; pin.replaceChildren(icon(value.icon));
    }
  }
  function closeSwipes(except) {for(const row of list.children)if(row!==except){row.style.removeProperty("--workspace-swipe");const remove=row.querySelector(".workspace-swipe-delete");if(remove)remove.hidden=true;}}
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
    if(previous.swipe){
      const offered=getView()?.rows.find(row=>row.id===previous.row.dataset.id)?.options.find(item=>item.action.type==="delete");
      const open=event.type==="pointerup"&&previous.dx< -36&&offered?.enabled;
      previous.row.style.setProperty("--workspace-swipe",open?"72px":"0px");previous.row.querySelector(".workspace-swipe-delete").hidden=!open;
    }
    if (previous.swipe || previous.held || previous.ghost || event.type !== "pointerup") suppressClick = true;
    if (event.type !== "pointerup") closeMenu();
    if (event.type === "pointerup" && previous.ghost && previous.before !== undefined) edit({type:"move", id:previous.row.dataset.id, before:previous.before});
    else if (menuOpen()) menu.querySelector("button:not(:disabled)")?.focus({preventScroll:true});
    redraw();
  }
  list.addEventListener("pointerdown", e => {
    if (contact) finish();
    suppressClick = false;
    if (unavailable() || getView().page !== "workspaces" || e.button || !e.isPrimary || e.target.closest("input,.workspace-swipe-delete,.workspace-pin")) return;
    const row = e.target.closest(".workspace-row"); if (!row) return;
    const handle = !!e.target.closest(".workspace-grip");closeSwipes(row);
    contact = {id:e.pointerId, row, point:{x:e.clientX,y:e.clientY}, x:e.clientX, y:e.clientY,
      wait:e.pointerType !== "mouse" && !handle, held:false};
    // Mouse rows already drag immediately; holding never opens their menu.
    if (e.pointerType !== "touch" && e.pointerType !== "pen") return;
    const current = contact;
    current.timer = setTimeout(() => {
      if (contact !== current || !row.isConnected) return;
      current.held = true; row.setPointerCapture(current.id); showMenu(row, current.point,false,handle,handle?row.querySelector(".workspace-grip"):row.querySelector(".workspace-choice"));
    }, 500);
  });
  list.addEventListener("pointermove", e => {
    if (!contact || contact.id !== e.pointerId) return;
    const dx=e.clientX-contact.x,dy=e.clientY-contact.y,distance=Math.hypot(dx,dy);
    const canDelete=getView()?.rows.find(row=>row.id===contact.row.dataset.id)?.options.some(item=>item.action.type==="delete"&&item.enabled);
    if(contact.wait&&!contact.held&&!contact.swipe&&dx< -8&&Math.abs(dx)>Math.abs(dy)*1.5&&canDelete){clearTimeout(contact.timer);contact.swipe=true;contact.row.setPointerCapture(contact.id);}
    if(contact.swipe){e.preventDefault();contact.dx=dx;contact.row.querySelector(".workspace-swipe-delete").hidden=false;contact.row.style.setProperty("--workspace-swipe",`${Math.min(72,Math.max(0,-dx))}px`);return;}
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
    if(e.target.closest("input"))return; e.preventDefault(); if (!contact?.ghost) showMenu(row, {x:e.clientX,y:e.clientY}, !contact,false,row.querySelector(".workspace-choice"));
  });
  list.addEventListener("scroll", () => { if (!contact?.ghost && !contact?.held) { finish(); closeMenu(); } });
  window.addEventListener("pointerdown", e => { if(!list.contains(e.target))closeSwipes(); if (menuOpen() && !menu.contains(e.target) && !menuAnchor?.contains(e.target)) closeMenu(); }, true);
  window.addEventListener("keydown", e => {
    if (!contact) suppressClick = false;
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
    const previousFirst = choices.firstElementChild?.dataset.workspaceId;
    // A background preference refresh leaves workspace switching available.
    const switchUnavailable = !view.ready || view.busy || !!view.page || !!view.prompt;
    const ids = new Set(view.switcher_display.map(row => row.id));
    for (const [id, node] of buttons) if (!ids.has(id)) { node.remove(); buttons.delete(id); }
    for (const [index, item] of view.switcher_display.entries()) {
      let node = buttons.get(item.id);
      if (!node) { node = button("", () => send({type:"switch",id:item.id})); node.append(element("span")); node.dataset.workspaceId = item.id; buttons.set(item.id,node); }
      if (choices.children[index] !== node) choices.insertBefore(node, choices.children[index] ?? null);
      node.firstElementChild.textContent = item.title;bindCopy(node,()=>app.native_caption({type:"switch_workspace",title:getView()?.switcher_display.find(row=>row.id===item.id)?.title??item.title}),"title");
      node.setAttribute("aria-pressed", String(item.id === view.id)); node.disabled = switchUnavailable;
    }
    options.title = view.switcher_options_label;
    options.setAttribute("aria-label", view.switcher_options_label);
    root.setAttribute("aria-label", view.switcher_menu.title);
    options.disabled = switchUnavailable;
    root.hidden = !ids.size;
    if (view.id !== previousFirst && view.switcher_display[0]?.id === view.id) choices.scrollLeft = 0;
    if (!view.page || (view.prompt && !(view.page==="workspaces"&&view.prompt_action?.type==="rename")) || (contact && !view.order.includes(contact.row.dataset.id))) { finish(); closeMenu(); }
    for (const control of list.querySelectorAll(".workspace-grip")) control.disabled = unavailable();
    for (const row of list.children) {updatePin(row, view.rows.find(item => item.id === row.dataset.id)?.pin);const mark=row.querySelector(".workspace-check");if(mark)mark.hidden=row.dataset.id!==view.id;}
  }
  return {root, render, decorate, closeMenu, dragging:() => !!contact, cancel:() => { finish(); closeMenu(); }};
}
