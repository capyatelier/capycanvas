import {liveCopy,bindCopy} from './localization.js';
import { composingKey } from "./text-input.js";
import { chooseColor } from "./color-editor.js";
let thumbnailRequest=0n;
const thumbnailPending=new Map();
// Layer widgets only. Selection, references, hierarchy and menu policy are Rust.
export function createLayerPanel({ app, catalog, state, panel, element, button, icon, dispatch, applyChange, message, numberField, wake, dismissContext, openMenu, contentChanged = () => {} }) {
  const copy=liveCopy(app,"catalog").native_copy.layers;
  const send = action => dispatch({ type: "layer", action });
  const sendObject = action => dispatch({ type: "object", action });
  const header = element("div", "layer-header"), footer = element("div", "layer-footer");
  header.dataset.control = "layer_opacity"; footer.dataset.control = "layer_actions";
  const active = () => state().layer_tools.editing_layer;
  let blendMenu = null, blendOpen = false, blendReopen = false;
  const blend = button("", () => {
    const skip = blendReopen; blendReopen = false;
    const id = active()?.id;
    if (skip || id == null) return;
    blend.menuModel = () => app.layer_blend_menu(id);
    const opened = openMenu(blend);
    if (blendMenu !== opened) { blendMenu = opened; opened.addEventListener("toggle", e => { if (e.newState === "closed") blendOpen = false; }); }
    blendOpen = true;
  }, "layer-blend");
  blend.addEventListener("pointerdown", () => { blendReopen = blendOpen && blendMenu?.menuOwner === blend; });
  const blendLabel = element("span", "layer-blend-label", catalog.layer_blends[0]);
  blend.append(blendLabel, icon("chevron-down"));
  bindCopy(blend,()=>copy.blend,"title"); bindCopy(blend,()=>copy.blend,"ariaLabel"); blend.setAttribute("aria-haspopup", "menu");
  const options = element("div", "layer-options");
  const opacity = numberField(catalog.layer_opacity, ()=>copy.opacity, value => dispatch({ type: "set_layer_opacity", opacity: value }), true);
  opacity.id = "layer-opacity"; options.append(blend, opacity); header.append(options);
  const glyphButton = (glyph, label, click, cls = "", getAction) => {
    const b = button("", e => { e.stopPropagation(); click(e); }, `layer-icon ${cls}`);
    if(typeof label==="function"){bindCopy(b,label,"title");bindCopy(b,label,"ariaLabel");}else{b.title=label;b.setAttribute("aria-label",label);} b.append(icon(glyph));
    if (getAction) b.onpointerenter = () => { b.title = app.action_tooltip(b.getAttribute("aria-label"), { type: "layer", action: getAction() }); };
    return b;
  };
  const flags = element("div", "layer-flags"), toggles = [];
  for (const [glyph, label, property, op, capability] of [
    ["alpha-lock", ()=>copy.alpha_lock, "alpha_locked", "alpha_lock", "alpha_lock"],
    ["lock", ()=>copy.lock_editing, "locked", "lock", "edit_lock"],
    ["reference", ()=>state().layer_tools.reference_action_label, "reference", "reference_selection", "reference"],
  ]) {
    const getAction = () => op === "reference_selection" ? { op } : { op, id: active().id, value: !active()[property] };
    const b = glyphButton(glyph, label, () => send(getAction()), "", getAction);
    if (capability === "reference") b.classList.add("layer-reference");
    toggles.push({ b, property, capability }); flags.append(b);
  }
  const attachment = glyphButton("clip", "", () => {
    const action = state().layer_tools.attachment.action;
    if (action) send(action);
  });
  attachment.classList.add("layer-attachment");
  attachment.onpointerenter = () => {
    const control = state().layer_tools.attachment;
    attachment.title = control.action ? app.action_tooltip(control.label, { type: "layer", action: control.action }) : control.label;
  };
  flags.insertBefore(attachment, flags.lastChild);
  header.append(flags);
  const rows = element("div", "layer-rows"); rows.id = "layer-rows"; rows.dataset.control = "layers";
  const list = element("div", "layer-list");
  const connections = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  connections.classList.add("layer-connections"); connections.setAttribute("aria-hidden", "true");
  list.append(rows, connections);
  const records = new Map();
  let documentEpoch, measurementKey;
  const menu = (node, getLayer, mask = false) => {
    node.dataset.context = "{}";
    node.layerMenu = () => { const id = getLayer().id, targetMask = typeof mask === "function" ? mask() : mask;
      send({ op: "context", id, mask: targetMask }); return app.layer_menu(id, targetMask); };
  };
  for (const group of [false, true]) {
    const getAction = () => ({ op: "new", group, clipped: false });
    footer.append(glyphButton(group ? "folder" : "add-layer", ()=>group ? copy.new_group : copy.new_layer, () => send(getAction()), "", getAction));
  }
  footer.append(glyphButton("selection-brush", ()=>copy.new_selection_layer, () => dispatch({type:"invoke",command:"new_selection_layer"})));
  const addMask = () => ({ op: "add_mask", id: active().id, replace: false });
  const maskButton = glyphButton("mask", ()=>copy.add_mask, () => send(addMask()), "", addMask); footer.append(maskButton);
  const addFilter = glyphButton("add-filter", ()=>copy.add_filter, () => openMenu(addFilter));
  addFilter.id = "layer-add-filter"; addFilter.menuModel = () => state().layer_tools.add_filter;
  addFilter.setAttribute("aria-haspopup", "menu"); footer.append(addFilter);
  footer.append(glyphButton("image", ()=>copy.import_image, () => dispatch({type:"invoke",command:"import_image"})));
  const deleteAction = () => ({ op: "delete_selected" });
  const deleteButton = glyphButton("delete", ()=>copy.delete_selected, () => send(deleteAction()), "", deleteAction);
  footer.append(deleteButton);
  const more = glyphButton("more-small", ()=>copy.actions, () => more.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true,
    clientX: more.getBoundingClientRect().left, clientY: more.getBoundingClientRect().top })));
  more.classList.add("layer-more"); menu(more, active, () => active()?.mask_selected ?? false); footer.append(more);
  panel.append(header, list, footer);
  const nameIcon = value => value.replace(/^layer-/, "").replace(/-symbolic$/, "");
  const connectionNodes = new Map();
  let connectionFrame = 0, connectionAnimationUntil = 0;
  const scheduleConnections = () => {
    if (!connectionFrame) connectionFrame = requestAnimationFrame(drawConnections);
  };
  function drawConnections(now) {
    connectionFrame = 0;
    const bounds = list.getBoundingClientRect(), geometry = new Map();
    const edges = state().layer_tools.connections;
    for (const edge of edges) for (const handle of [edge.from, edge.to]) {
      const id = String(handle), record = records.get(id);
      if (!record || geometry.has(id)) continue;
      const thumb = record.content.b.getBoundingClientRect();
      if (thumb.width && thumb.height) geometry.set(id, thumb);
    }
    const anchor = state().layers.find(row => geometry.has(String(row.id)));
    if (!anchor || !bounds.width || !bounds.height) { connections.replaceChildren(); connectionNodes.clear(); return; }
    const record = records.get(String(anchor.id)), rect = record.row.getBoundingClientRect();
    const column = geometry.get(String(anchor.id)).left - rect.left + record.root.getBoundingClientRect().left - bounds.left - Math.min(anchor.depth * 8, 24);
    connections.setAttribute("viewBox", `0 0 ${bounds.width} ${bounds.height}`);
    const keys = new Set();
    for (const edge of edges) {
      const from = geometry.get(String(edge.from)), to = geometry.get(String(edge.to));
      if (!from || !to) continue;
      const effect = edge.kind === "effect", top = (effect ? from.bottom : from.top) - bounds.top, bottom = (effect ? to.top : to.bottom) - bounds.top;
      if (bottom <= top || bottom < 0 || top > bounds.height) continue;
      const key = `${edge.kind}:${edge.from}:${edge.to}`, x = column + Math.min(edge.depth * 8, 24);
      keys.add(key);
      let node = connectionNodes.get(key);
      if (!node) {
        node = document.createElementNS(connections.namespaceURI, "g"); node.dataset.kind = edge.kind; node.dataset.from = String(edge.from); node.dataset.to = String(edge.to);
        const path = document.createElementNS(connections.namespaceURI, "path"); node.append(path);
        if (effect) { const glyph = icon("effect-link"); glyph.setAttribute("width", "12"); glyph.setAttribute("height", "12"); node.append(glyph); }
        connectionNodes.set(key, node); connections.append(node);
      }
      if (effect) {
        const center = x + 15, y = (top + bottom) * .5;
        node.firstChild.setAttribute("d", `${y - top > 6 ? `M${center} ${top}V${y - 6}` : ""}${bottom - y > 6 ? `M${center} ${y + 6}V${bottom}` : ""}`);
        node.lastChild.setAttribute("x", String(center - 6)); node.lastChild.setAttribute("y", String(y - 6));
      } else node.firstChild.setAttribute("d", `M${x - 3.5} ${top}V${bottom}`);
    }
    for (const [key, node] of connectionNodes) if (!keys.has(key)) { node.remove(); connectionNodes.delete(key); }
    if (now < connectionAnimationUntil) scheduleConnections();
  }
  rows.addEventListener("scroll", scheduleConnections, { passive: true });
  const connectionResize = new ResizeObserver(scheduleConnections); connectionResize.observe(list);
  function makeRow(layer) {
    const root = element("div", "layer-swipe"), row = element("div", "layer-row"), record = { root, row, layer }; row.dataset.layer = String(layer.id);
    const get = () => record.layer;
    const eye = glyphButton("eye", ()=>get().selection_layer?(get().visible?copy.hide_selection:copy.show_selection):(get().visible?copy.hide:copy.show), () => dispatch({ type: "set_layer_visibility", id: get().id, visible: !get().visible }));
    eye.onpointerenter = () => { eye.title = app.action_tooltip(eye.getAttribute("aria-label"), { type: "set_layer_visibility", id: get().id, visible: !get().visible }); };
    const selection = (e = {}) => ({ op: "select_row", id: get().id, extend: !!e.shiftKey, toggle: true });
    const check = glyphButton("selection-empty", ()=>copy.select_row_help, e => send(selection(e)), "", selection);
    const thumbnails = element("div", "layer-thumbnails"), gutter = element("span", "layer-connection-gutter");
    const thumb = (mask) => {
      const getAction = () => !mask && get().group ? { op: "collapse", id: get().id } : { op: "select", id: get().id, mask };
      const readLabel = ()=>mask?copy.edit_mask:get().group?(get().collapsed?copy.expand:copy.collapse):get().selection_layer?copy.edit_selection:copy.edit_content;
      const editFill = async () => {
        const { id, fill_color: fill } = get();
        if (mask || !fill) return;
        const selected = await chooseColor({ app, color: fill.color, element, button, opaque: fill.opaque });
        if (selected) dispatch({ type: "effect", action: { op: "set", layer: id, key: fill.key, value: { kind: "color", value: selected.color } } });
      };
      const b = glyphButton("mask", readLabel, () => { send(getAction()); editFill(); }, "layer-thumbnail", getAction);
      const image = element("canvas"); image.width = image.height = 32; image.setAttribute("aria-hidden", "true");
      // These are already-rasterized GPU preview bytes. Keep their tiny UI
      // bitmap in host memory rather than invoking another GPU canvas path.
      image.getContext("2d", { willReadFrequently: true });
      b.replaceChildren(image);
      b.addEventListener('click',e=>{
        if(!(e.ctrlKey||e.metaKey)||get().group)return;
        e.preventDefault();e.stopImmediatePropagation();
        dispatch({type:'selection',action:{op:'load_thumbnail',id:get().id,mask,shift:e.shiftKey,alt:e.altKey}});
      },{capture:true});
      return { b, image, readLabel };
    };
    const content = thumb(false), mask = thumb(true);
    const linkAction = () => ({ op: "link_mask", id: get().id, value: !get().mask_linked });
    const link = glyphButton("link", ()=>get().mask_linked?copy.unlink_mask:copy.link_mask_to_layer, () => send(linkAction()), "layer-link", linkAction);
    thumbnails.append(gutter, content.b, link, mask.b);
    const text = element("div", "layer-text"), name = element("span", "layer-name"), meta = element("span", "layer-meta"); text.append(name, meta);
    const lock = element("span", "layer-lock"), grip = element("span", "layer-grip"); grip.append(icon("grip"));
    const expand = glyphButton("chevron-down", ()=>get().expanded?copy.collapse_images:copy.expand_images, () => sendObject({ op: "expand", layer: get().id, expanded: !get().expanded }), "layer-expand");
    const load=button('',e=>{e.stopPropagation();dispatch({type:'selection',action:{op:'load_layer',id:get().id,mode:'new',inverted:false}});},'layer-icon selection-layer-load');
    load.append(icon('selection-load'));thumbnails.insertBefore(load,link);
    row.append(eye, check, thumbnails, text, expand, lock, grip);
    row.onclick = e => { if (!e.target.closest("button,input")) send({op:"select_row", id:get().id, extend:e.shiftKey, toggle:e.ctrlKey || e.metaKey}); };
    name.ondblclick = e => { e.stopPropagation(); if(get().can_rename) send({ op: "begin_rename", id: get().id }); };
    menu(row, get); menu(mask.b, get, true);
    Object.assign(record, { load, eye, check, thumbnails, content, mask, link, name, text, meta, expand, lock, grip });
    const remove = button(()=>liveCopy(app,"bootstrap_view").common.delete, e => { e.stopPropagation(); send({ op: "delete", id: get().id }); }, "layer-swipe-delete");
    root.append(remove, row);
    let offset = 0, drag, suppressClick;
    const position = (value, animate = false) => {
      offset = value; root.classList.toggle("swipe-animating", animate);
      root.style.setProperty("--swipe", `${offset}px`);
      remove.hidden = offset <= 0; remove.disabled = !get().can_delete;
      if (animate) connectionAnimationUntil = performance.now() + 180;
      scheduleConnections();
    };
    record.closeSwipe = () => position(0, true);
    position(0);
    row.addEventListener("pointerdown", e => {
      if (suppressClick) { row.removeEventListener("click", suppressClick, true); suppressClick = null; }
      if (e.button || !e.isPrimary || e.target.closest("input") || (!get().can_drop_below && !get().can_delete && !get().right_swipe)) return;
      drag = { x: e.clientX, y: e.clientY, top: row.getBoundingClientRect().top, pointer: e.pointerId,
        waitForHold: e.pointerType !== "mouse" && !grip.contains(e.target), held: false, origin: offset };
      // Follow fast mouse exits before pickup without retargeting ordinary
      // button clicks. Capture the row only once movement becomes a drag.
      window.addEventListener("pointermove", move, true);
      window.addEventListener("pointerup", finish, true);
      window.addEventListener("pointercancel", finish, true);
    });
    row.addEventListener("workspace-context-claimed", e => {
      if (drag?.ghost || drag?.swiping) e.preventDefault();
      else if (drag) { drag.held = true; row.setPointerCapture(drag.pointer); }
    });
    // Keep native panning until a hold wins. Pointer capture alone cannot stop
    // the browser taking the contact for scrolling after the menu opens.
    row.addEventListener("touchmove", e => {
      if ((drag?.held || drag?.swiping) && e.touches.length === 1) e.preventDefault();
    }, { passive: false });
    const move = e => {
      if (!drag || drag.pointer !== e.pointerId) return;
      const dx = e.clientX-drag.x, dy = e.clientY-drag.y;
      if (drag.waitForHold && !drag.held) {
        if (!drag.swiping && Math.hypot(dx,dy) > 8) {
          const allowed = dx < 0 ? get().can_delete : drag.origin > 0 || !!get().right_swipe;
          if (allowed && Math.abs(dx) > Math.abs(dy)) {
            drag.swiping = true; dismissContext(); row.setPointerCapture(e.pointerId);
          } else { finish({type:"pointercancel",pointerId:e.pointerId}); return; }
        }
        if (drag.swiping) {
          const minimum = drag.origin === 0 && get().right_swipe ? -72 : 0;
          e.preventDefault(); position(Math.max(minimum,Math.min(72,drag.origin-dx)));
        }
        return;
      }
      if (!get().can_drop_below) return;
      if (!drag.ghost && Math.hypot(e.clientX-drag.x, e.clientY-drag.y) > 6) {
        dismissContext();
        row.setPointerCapture(e.pointerId); drag.ghost = row.cloneNode(true); drag.ghost.classList.add("layer-drag-preview");
        drag.ghost.setAttribute("aria-hidden", "true"); drag.ghost.style.width = `${row.clientWidth}px`; drag.ghost.style.left = `${row.getBoundingClientRect().left}px`;
        document.body.append(drag.ghost); drag.frame = requestAnimationFrame(autoscroll);
        drag.ghost.querySelectorAll("canvas").forEach((c,i) => c.getContext("2d").drawImage(row.querySelectorAll("canvas")[i],0,0));
      }
      if (!drag.ghost) return;
      drag.point = [e.clientX, e.clientY];
      drag.ghost.style.top = `${drag.top + e.clientY-drag.y}px`;
      updateDrop(...drag.point);
    };
    function updateDrop(x, y) {
      rows.querySelectorAll(".layer-drop-before,.layer-drop-after,.layer-drop-into,.layer-drop-attach").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into","layer-drop-attach"));
      const hit = document.elementFromPoint(x,y), target = hit?.closest(".layer-row[data-layer]"); drag.target = null;
      if (target && target !== row && rows.contains(target)) {
        const record = records.get(target.dataset.layer), to = record.layer, rect = target.getBoundingClientRect();
        const fraction = to.can_drop_below ? (y-rect.top)/rect.height : 0;
        const surface = record.content.b.contains(hit) ? "thumbnail" : "row";
        const preview = app.layer_drop_preview(get().id, to.id, fraction, surface);
        if (preview) {
          drag.target = { op: "drop", id: get().id, target: to.id, fraction, surface };
          records.get(String(preview.effect_owner))?.content.b.classList.add("layer-drop-attach");
          const feedback = records.get(String(preview.target));
          if (feedback) (preview.position === "attach" ? feedback.content.b : feedback.row).classList.add(`layer-drop-${{above:"before",below:"after",into:"into",attach:"attach"}[preview.position]}`);
        }
      }
    };
    function autoscroll(now) {
      if (!drag?.ghost) return;
      const elapsed = drag.frameTime ? Math.min(50, now - drag.frameTime) / 1000 : 0;
      drag.frameTime = now;
      if (drag.point) {
        const bounds = rows.getBoundingClientRect(), [x, y] = drag.point;
        if (x >= bounds.left && x <= bounds.right) {
          const speed = y >= bounds.top - 12 && y < bounds.top + 20 ? -240 : y <= bounds.bottom + 12 && y > bounds.bottom - 20 ? 240 : 0;
          const before = rows.scrollTop; rows.scrollTop += speed * elapsed;
          if (before !== rows.scrollTop) updateDrop(x, y);
        }
      }
      drag.frame = requestAnimationFrame(autoscroll);
    }
    const finish = e => {
      if (!drag || (e.pointerId != null && drag.pointer !== e.pointerId)) return;
      const previous = drag; drag = null; cancelAnimationFrame(previous.frame);
      window.removeEventListener("pointermove", move, true);
      window.removeEventListener("pointerup", finish, true);
      window.removeEventListener("pointercancel", finish, true);
      if (row.hasPointerCapture(previous.pointer)) row.releasePointerCapture(previous.pointer);
      if ((previous.held || previous.ghost || previous.swiping) && e.type === "pointerup") {
        const suppress = e => { e.preventDefault(); e.stopImmediatePropagation(); };
        suppressClick = suppress;
        row.addEventListener("click", suppress, { once: true, capture: true });
        setTimeout(() => row.removeEventListener("click", suppress, true), 400);
      }
      if (previous.swiping) {
        const toggle = e.type === "pointerup" && offset <= -72*.4;
        position(e.type === "pointerup" && offset >= 72*.4 ? 72 : 0, true);
        if (toggle && get().right_swipe) send(get().right_swipe);
      }
      if (e.type !== "pointerup" && previous.held) dismissContext();
      if (previous.ghost) {
        previous.ghost.remove(); rows.querySelectorAll(".layer-drop-before,.layer-drop-after,.layer-drop-into,.layer-drop-attach").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into","layer-drop-attach"));
        if (e.type === "pointerup" && previous.target && app.layer_drop_preview(previous.target.id, previous.target.target, previous.target.fraction, previous.target.surface)) send(previous.target);
      }
    };
    record.cancelDrag = () => finish({type:"pointercancel"});
    row.addEventListener("pointerup", finish); row.addEventListener("pointercancel", finish);
    row.addEventListener("lostpointercapture", e => {
      // Transferring implicit touch/pen capture from a grip or child control
      // to its row releases that child without cancelling the reorder.
      if (e.target === row || !row.hasPointerCapture(e.pointerId)) finish(e);
    });
    return record;
  }
  const objectRecords = new Map();
  const objectDropClasses = () => rows.querySelectorAll(".layer-object-row.layer-drop-before,.layer-object-row.layer-drop-after").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after"));
  function makeObjectRow(object) {
    const root = element("div", "layer-swipe"), row = element("div", "layer-row layer-object-row"), record = { root, row, object };
    row.dataset.object = String(object.id); row.dataset.objectLayer = String(object.layer); root.append(row);
    const get = () => record.object;
    const eye = glyphButton("eye", ()=>get().visible?copy.hide_image:copy.show_image, () => sendObject({ op: "visibility", id: get().id, visible: !get().visible }));
    const spacer = element("span", "layer-icon layer-object-spacer"); spacer.setAttribute("aria-hidden", "true");
    const thumbnails = element("div", "layer-thumbnails"), gutter = element("span", "layer-connection-gutter");
    const select = e => sendObject({ op: "select", id: get().id, extend: !!(e.shiftKey || e.ctrlKey || e.metaKey) });
    const thumbnail = button("", e => { e.stopPropagation(); select(e); }, "layer-icon layer-thumbnail layer-object-thumbnail");
    thumbnail.addEventListener("keydown", e => { if (!composingKey(e) && (e.key === "Enter" || e.key === " ")) e.stopPropagation(); });
    const placeholder = icon("image"); placeholder.classList.add("layer-object-placeholder");
    const image = element("canvas"); image.width = image.height = 32; image.setAttribute("aria-hidden", "true");
    image.getContext("2d", { willReadFrequently: true });
    thumbnail.append(placeholder, image); thumbnails.append(gutter, thumbnail);
    const text = element("div", "layer-text"), name = element("span", "layer-name"); text.append(name);
    const grip = element("span", "layer-grip"); grip.append(icon("grip")); bindCopy(grip,()=>copy.move_image,"title");
    row.append(eye, spacer, thumbnails, text, grip);
    row.onclick = e => { if (!e.target.closest("button,input")) select(e); };
    row.dataset.context = "{}"; row.layerMenu = () => app.object_menu(get().id);
    Object.assign(record, { eye, thumbnails, thumbnail, image, name, grip });
    let drag, suppressClick;
    const movable = () => get().editable && (get().can_raise || get().can_lower);
    row.addEventListener("pointerdown", e => {
      if (suppressClick) { row.removeEventListener("click", suppressClick, true); suppressClick = null; }
      if (e.button || !e.isPrimary || e.target.closest("input") || (e.target.closest("button") && !grip.contains(e.target)) || !movable()) return;
      drag = { x: e.clientX, y: e.clientY, top: row.getBoundingClientRect().top, pointer: e.pointerId,
        waitForHold: e.pointerType !== "mouse" && !grip.contains(e.target), held: false };
      window.addEventListener("pointermove", move, true);
      window.addEventListener("pointerup", finish, true);
      window.addEventListener("pointercancel", finish, true);
    });
    row.addEventListener("workspace-context-claimed", e => {
      if (drag?.ghost) e.preventDefault();
      else if (drag) { drag.held = true; row.setPointerCapture(drag.pointer); }
    });
    row.addEventListener("touchmove", e => { if (drag?.held && e.touches.length === 1) e.preventDefault(); }, { passive: false });
    const move = e => {
      if (!drag || drag.pointer !== e.pointerId) return;
      const distance = Math.hypot(e.clientX-drag.x, e.clientY-drag.y);
      if (drag.waitForHold && !drag.held) { if (distance > 8) finish({ type: "pointercancel", pointerId: e.pointerId }); return; }
      if (!drag.ghost && distance > 6) {
        dismissContext(); row.setPointerCapture(e.pointerId);
        drag.ghost = row.cloneNode(true); drag.ghost.classList.add("layer-drag-preview"); drag.ghost.setAttribute("aria-hidden", "true");
        drag.ghost.style.width = `${row.clientWidth}px`; drag.ghost.style.left = `${row.getBoundingClientRect().left}px`;
        document.body.append(drag.ghost);
        drag.ghost.querySelector("canvas")?.getContext("2d").drawImage(image, 0, 0);
      }
      if (!drag.ghost) return;
      drag.ghost.style.top = `${drag.top + e.clientY-drag.y}px`;
      objectDropClasses(); drag.target = null;
      const target = document.elementFromPoint(e.clientX, e.clientY)?.closest(".layer-object-row");
      const to = target && target !== row && rows.contains(target) ? objectRecords.get(target.dataset.object)?.object : null;
      if (to && to.editable && String(to.layer) === String(get().layer)) {
        const rect = target.getBoundingClientRect(), below = e.clientY > rect.top + rect.height * .5;
        drag.target = { op: "drop", id: get().id, target: to.id, below };
        target.classList.add(below ? "layer-drop-after" : "layer-drop-before");
      }
    };
    const finish = e => {
      if (!drag || (e.pointerId != null && drag.pointer !== e.pointerId)) return;
      const previous = drag; drag = null;
      window.removeEventListener("pointermove", move, true);
      window.removeEventListener("pointerup", finish, true);
      window.removeEventListener("pointercancel", finish, true);
      if (row.hasPointerCapture(previous.pointer)) row.releasePointerCapture(previous.pointer);
      if ((previous.held || previous.ghost) && e.type === "pointerup") {
        const suppress = e => { e.preventDefault(); e.stopImmediatePropagation(); };
        suppressClick = suppress;
        row.addEventListener("click", suppress, { once: true, capture: true });
        setTimeout(() => row.removeEventListener("click", suppress, true), 400);
      }
      if (e.type !== "pointerup" && previous.held) dismissContext();
      if (previous.ghost) {
        previous.ghost.remove(); objectDropClasses();
        if (e.type === "pointerup" && previous.target) sendObject(previous.target);
      }
    };
    record.cancelDrag = () => finish({ type: "pointercancel" });
    row.addEventListener("lostpointercapture", e => { if (e.target === row || !row.hasPointerCapture(e.pointerId)) finish(e); });
    return record;
  }
  const cancelDrags = () => { for (const r of records.values()) { r.cancelDrag(); r.closeSwipe(); } for (const r of objectRecords.values()) r.cancelDrag(); };
  const closeSwipes = e => { for (const r of records.values()) if (!r.root.contains(e.target)) r.closeSwipe(); };
  document.addEventListener("pointerdown", closeSwipes, true);
  const closeOnClick = e => { if (!e.target.closest(".layer-swipe-delete")) for (const r of records.values()) r.closeSwipe(); };
  document.addEventListener("click", closeOnClick);
  window.addEventListener("blur", cancelDrags);
  const cancelOnEscape = e => { if (!composingKey(e) && e.key === "Escape") cancelDrags(); };
  window.addEventListener("keydown", cancelOnEscape);
  function refreshObject(r, depth) {
    const object = r.object;
    r.row.classList.toggle("selected", object.selected); r.row.classList.toggle("layer-object-hidden", !object.visible);
    r.thumbnails.style.marginLeft = `${Math.min(depth*8,32)}px`;
    r.name.textContent = object.label; r.name.title = object.label;
    r.thumbnail.title = r.thumbnail.ariaLabel = object.label; r.thumbnail.setAttribute("aria-pressed", String(object.selected));
    r.eye.replaceChildren(icon(object.visible ? "eye" : "eye-hidden")); r.eye.disabled = !object.editable;
    r.eye.title = r.eye.ariaLabel = object.visible ? copy.hide_image : copy.show_image;
    r.grip.hidden = !(object.editable && (object.can_raise || object.can_lower));
  }
  function refresh() {
    const epoch=String(state().document_file.epoch);
    if(epoch!==documentEpoch){cancelDrags();documentEpoch=epoch;revisions.clear();for(const id of owned)pending.delete(id);owned.clear();
      for(const r of records.values())for(const c of [r.content.image,r.mask.image])if(c)c.width=c.width;
      for(const r of objectRecords.values()){r.image.width=r.image.width;r.thumbnail.classList.remove("loaded");delete r.image.dataset.previewRevision;}
    }
    const view = state().layer_tools, current = view.editing_layer, controls = view.controls;
    if (current) { opacity.update(current.opacity); blendLabel.textContent = current.blend_label; }
    opacity.setDisabled(!controls.opacity); blend.disabled = !controls.blend; maskButton.disabled = !controls.mask; more.disabled = !current;
    deleteButton.disabled = !state().layer_tools.can_delete;
    addFilter.disabled = !view.add_filter;
    for (const { b, property, capability } of toggles) {
      const reference = capability === "reference";
      b.disabled = !(reference ? view.can_reference : controls[capability]);
      b.setAttribute("aria-pressed", reference ? view.references_selected : !!current?.[property]);
      if (reference) b.title = b.ariaLabel = view.reference_action_label;
    }
    const control = view.attachment;
    attachment.disabled = !control.action;
    attachment.ariaLabel = control.label;
    attachment.title = control.description;
    attachment.setAttribute("aria-description", control.description);
    attachment.setAttribute("aria-pressed", control.checked);
    if (attachment.firstChild?.dataset.asset !== nameIcon(control.icon)) attachment.replaceChildren(icon(nameIcon(control.icon)));
    const ids = new Set(state().layers.map(l => String(l.id)));
    for (const [id, record] of records) if (!ids.has(id)) {
      record.cancelDrag();
      record.root.remove(); records.delete(id); revisions.delete(`${id}:false`); revisions.delete(`${id}:true`);
    }
    const objectIds = new Set(state().layers.flatMap(l => l.objects.map(o => String(o.id))));
    for (const [id, record] of objectRecords) if (!objectIds.has(id)) {
      record.cancelDrag(); record.root.remove(); objectRecords.delete(id); revisions.delete(`${id}:object`);
    }
    let position = 0;
    const place = root => { if (rows.children[position] !== root) rows.insertBefore(root, rows.children[position] || null); position++; };
    for (const layer of state().layers) {
      const id = String(layer.id); if (!records.has(id)) records.set(id, makeRow(layer));
      place(records.get(id).root);
      for (const object of layer.objects) {
        const key = String(object.id); if (!objectRecords.has(key)) objectRecords.set(key, makeObjectRow(object));
        const r = objectRecords.get(key); r.object = object; place(r.root); refreshObject(r, layer.depth + 1);
      }
    }
    state().layers.forEach(layer => {
      const r = records.get(String(layer.id)); r.layer = layer;
      const {paint_revision,mask_revision,objects,...presentation}=layer;
      const key=JSON.stringify([presentation,view.rename_layer===layer.id],(_,value)=>typeof value==="bigint"?String(value):value);
      // Keep the latest thumbnail revisions above, but retain unchanged row
      // widgets, SVGs and text through canvas movement and raster publications.
      if(r.presentation===key)return;
      r.presentation=key;
      r.row.classList.toggle("selected", layer.selected);r.load.hidden=!layer.selection_layer;r.load.title=layer.load_selection_tooltip;r.load.setAttribute("aria-label",r.load.title);
      if (!layer.can_delete && !layer.right_swipe) r.closeSwipe();
      r.eye.replaceChildren(icon(layer.visible && !layer.visibility_blocked ? "eye" : "eye-hidden")); r.eye.style.opacity = layer.visibility_blocked ? .35 : 1;
      r.eye.title = r.eye.ariaLabel = layer.selection_layer?(layer.visible?copy.hide_selection:copy.show_selection):(layer.visible?copy.hide:copy.show);
      r.check.replaceChildren(icon(nameIcon(layer.selection_icon))); r.check.setAttribute("aria-pressed", String(layer.selected));
      r.thumbnails.style.marginLeft = `${Math.min(layer.depth*8,24)}px`;
      r.content.b.classList.toggle("editing-target", layer.content_selected);
      r.mask.b.classList.toggle("editing-target", layer.mask_selected);
      for(const thumbnail of [r.content,r.mask])thumbnail.b.title=thumbnail.b.ariaLabel=thumbnail.readLabel();
      r.content.b.classList.toggle("layer-folder", layer.group);
      r.content.b.classList.toggle("layer-adjustment", layer.adjustment_effect);
      if (layer.group) {
        r.content.b.replaceChildren(icon(layer.collapsed ? "folder" : "folder-open"));
        if (layer.pass_through) { const glyph = icon("group-pass-through"); glyph.classList.add("layer-group-pass-through"); r.content.b.append(glyph); }
      }
      else if(layer.content_icon && !layer.selection_layer) {
        const glyph = icon(nameIcon(layer.content_icon));
        if (layer.has_thumbnail) {
          glyph.classList.add("layer-type-symbol");
          r.content.b.replaceChildren(r.content.image, glyph);
        } else r.content.b.replaceChildren(glyph);
      }
      else if (r.content.b.childElementCount !== 1 || !r.content.image.isConnected) r.content.b.replaceChildren(r.content.image);
      r.mask.b.hidden = r.link.hidden = !layer.has_mask; r.mask.image.style.opacity = layer.mask_enabled ? 1 : .4;
      r.link.replaceChildren(icon(layer.mask_linked ? "link" : "unlink")); r.link.disabled = layer.locked; r.link.setAttribute("aria-pressed", String(layer.mask_linked)); r.link.title = r.link.ariaLabel = layer.mask_linked ? copy.unlink_mask : copy.link_mask_to_layer;
      r.name.textContent = layer.label; r.name.title = layer.label;
      r.meta.textContent = layer.description;
      r.meta.hidden = !r.meta.textContent; r.lock.replaceChildren(icon(layer.locked ? "lock" : "alpha-lock")); r.lock.style.opacity = layer.locked || layer.alpha_locked ? 1 : 0;
      r.grip.hidden = !layer.can_drop_below;
      r.expand.hidden = !layer.object_count; r.expand.classList.toggle("collapsed", !layer.expanded);
      r.expand.setAttribute("aria-expanded", String(layer.expanded));
      r.expand.title = r.expand.ariaLabel = layer.expanded ? copy.collapse_images : copy.expand_images;
      if (view.rename_layer !== layer.id && r.entry) r.closeRename();
      if (view.rename_layer === layer.id && !r.entry) {
        const input = element("input", "layer-name-entry"); input.value = layer.label; input.maxLength = 128; r.entry = input; r.name.hidden = true; r.text.prepend(input);
        let finished = false;
        r.closeRename = () => { finished = true; input.remove(); r.entry = null; r.name.hidden = false; };
        const done = cancel => { if (finished) return; r.closeRename();
          send(cancel || !input.value.trim() ? { op: "cancel_rename" } : { op: "rename", id: layer.id, name: input.value }); };
        input.onblur = () => done(false); input.onkeydown = e => { e.stopPropagation(); if (composingKey(e)) return; if (e.key === "Enter" || e.key === "Escape") { e.preventDefault(); done(e.key === "Escape"); } };
        input.focus(); input.select();
      }
    });
    // Bitmap revisions do not change row geometry. Native text, hierarchy and
    // controls do, including when this retained panel is currently offscreen.
    const next = JSON.stringify([view.quick_mask,current?.selection_layer,view.rename_layer?.toString(), state().layers.map(({paint_revision,mask_revision,objects,...row})=>[row,objects.map(({thumbnail_revision,...object})=>object)])],
      (_,value)=>typeof value==="bigint"?String(value):value);
    if (next !== measurementKey) { measurementKey=next; contentChanged("layers"); }
    scheduleConnections();
  }
  const pending = thumbnailPending, revisions = new Map();
  const owned = new Set();
  const thumbnailTimer = setInterval(() => {
    // A modal Settings session cannot edit layers. Defer thumbnail geometry
    // and uploads until it closes instead of forcing layout behind the modal.
    if (state().settings_open) return;
    if (!panel.isConnected || !panel.clientHeight) return;
    try {
      for (let image; (image = app.take_layer_thumbnail());) {
        const [id, width, height, bytes] = image, target = pending.get(id); pending.delete(id);target?.owned.delete(id);
        if (target && target.revision === target.revisions.get(target.key)) {
          target.canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(bytes),width,height),0,0);
          target.canvas.dataset.previewRevision = target.revision;
          target.loaded?.();
        }
      }
      const viewport = rows.getBoundingClientRect();
      const visible = rect => !(rect.bottom < Math.max(0,viewport.top) || rect.top > innerHeight || rect.height === 0);
      for (const [id, r] of objectRecords) {
        if (!visible(r.row.getBoundingClientRect())) continue;
        const key = `${id}:object`, revision = documentEpoch + ":" + String(r.object.thumbnail_revision);
        if (revisions.get(key) === revision || pending.size >= 8) continue;
        const token = ++thumbnailRequest;
        if (app.request_layer_thumbnail(token, r.object.id)) {
          owned.add(token); revisions.set(key, revision);
          pending.set(token, { key, revision, revisions, owned, canvas: r.image, loaded: () => r.thumbnail.classList.add("loaded") });
        } else if (app.shader_work_pending(true)) wake();
        return;
      }
      for (const [id, r] of records) {
        const rect = r.row.getBoundingClientRect(); if (rect.bottom < Math.max(0,viewport.top) || rect.top > innerHeight || rect.height === 0) continue;
        for (const mask of [false,true]) {
          if (mask ? !r.layer.has_mask : !r.layer.has_thumbnail) continue;
          const key = `${id}:${mask}`, revision = documentEpoch + ":" + String(mask ? r.layer.mask_revision : r.layer.paint_revision);
          if (revisions.get(key) === revision || pending.size >= 8) continue;
          const token = ++thumbnailRequest;
          if (app.request_layer_thumbnail(token, mask ? r.layer.mask_id : r.layer.id)) {
            owned.add(token);revisions.set(key,revision); pending.set(token,{key,revision,revisions,owned,canvas: (mask ? r.mask : r.content).image});
          }
          if (app.canvas_work_pending() || app.shader_work_pending(true)) wake();
          return;
        }
      }
    } catch (error) { message(error); }
  }, 120);
  return { refresh, dispose(){cancelDrags();cancelAnimationFrame(connectionFrame);connectionResize.disconnect();document.removeEventListener("pointerdown",closeSwipes,true);document.removeEventListener("click",closeOnClick);window.removeEventListener("blur",cancelDrags);window.removeEventListener("keydown",cancelOnEscape);clearInterval(thumbnailTimer); for(const id of owned)pending.delete(id);} };
}
