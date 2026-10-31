import {liveCopy,bindCopy} from './localization.js';
import { chooseColor } from "./color-editor.js";
import { chooseTexture, paperThumbnail } from "./paper-picker.js";
import { addScrollbar } from "./scrollbars.js";
import { layerTouchInput } from './layer-touch.js';
let thumbnailRequest=0n;
const thumbnailPending=new Map();
// Layer widgets only. Selection, references, hierarchy and menu policy are Rust.
export function createLayerPanel({ app, catalog, state, panel, element, button, icon, dropdown, dispatch, applyChange, message, numberField, wake, dismissContext, openMenu, contentChanged = () => {} }) {
  const copy=liveCopy(app,"catalog").native_copy.layers,paperCopy=liveCopy(app,"catalog").native_copy.paper;
  const send = action => dispatch({ type: "layer", action });
  const header = element("div", "layer-header"), footer = element("div", "layer-footer");
  header.dataset.control = "layer_opacity"; footer.dataset.control = "layer_actions";
  const active = () => state().layer_tools.editing_layer;
  const blend = dropdown(()=>copy.blend);blend.classList.add('layer-blend');
  const options = element("div", "layer-options");
  const opacity = numberField(catalog.layer_opacity, ()=>copy.opacity, value => dispatch({ type: "set_layer_opacity", opacity: value }), true);
  opacity.id = "layer-opacity"; options.append(blend, opacity); header.append(options);
  const glyphButton = (glyph, label, click, cls = "", getAction) => {
    const b = button("", e => { e.stopPropagation(); click(e); }, `layer-icon ${cls}`);
    if(typeof label==="function"){bindCopy(b,label,"title");bindCopy(b,label,"ariaLabel");}else{b.title=label;b.setAttribute("aria-label",label);} b.append(icon(glyph));
    if (getAction) b.onpointerenter = () => { b.title = app.action_tooltip(b.getAttribute("aria-label"), { type: "layer", action: getAction() }); };
    return b;
  };
  const rowFrame = () => element("div", "layer-row");
  const rowName = () => element("span", "layer-name");
  const rowThumbnails = () => element("div", "layer-thumbnails");
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
  const addPaper = button("", () => {
    const action = state().layer_tools.add_paper?.action;
    if (action) dispatch({ type: "effect", action });
  }, "layer-row");
  addPaper.id = "layer-add-paper";
  const list = element("div", "layer-list");
  const connections = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  connections.classList.add("layer-connections"); connections.setAttribute("aria-hidden", "true");
  const unit = rowFrame(), unitIcons = rowThumbnails(), unitText = element("div", "layer-text"), unitName = rowName();
  unit.classList.add("layer-row-unit");
  unit.style.cssText = "position:absolute;inset:0 0 auto;visibility:hidden;pointer-events:none";
  unit.inert = true; unit.setAttribute("aria-hidden", "true");
  bindCopy(unitName, () => copy.show, "textContent"); unitText.append(unitName);
  unitIcons.append(glyphButton("eye", () => copy.show, () => {}),
    glyphButton("mask", () => copy.edit_content, () => {}, "layer-thumbnail"));
  unit.append(unitIcons, unitText);
  list.append(rows, connections, unit);
  addScrollbar(rows, list);
  const records = new Map();
  let documentEpoch, measurementKey;
  const menu = (node, getLayer, mask = false) => {
    node.dataset.context = "{}";
    const targetMask = () => typeof mask === "function" ? mask() : mask;
    node.prepareMenu = () => send({ op: "context", id: getLayer().id, mask: targetMask() });
    node.menuModel = () => app.layer_menu(getLayer().id, targetMask());
  };
  const footerButtons = new Map();
  const place = (id, node) => { node.dataset.footerButton = id; footerButtons.set(id, node); footer.append(node); return node; };
  for (const group of [false, true]) {
    const getAction = () => ({ op: "new", group, clipped: false });
    place(group ? "new_group" : "new_layer", glyphButton(group ? "folder" : "add-layer", ()=>group ? copy.new_group : copy.new_layer, () => send(getAction()), "", getAction));
    if (!group) place("new_vector_layer", glyphButton("add-vector-layer", ()=>copy.new_vector_layer, () => dispatch({type:"invoke",command:"new_vector_layer"})));
  }
  place("new_selection_layer", glyphButton("selection-brush", ()=>copy.new_selection_layer, () => dispatch({type:"invoke",command:"new_selection_layer"})));
  const addMask = () => ({ op: "add_mask", id: active().id, replace: false });
  const maskButton = place("add_mask", glyphButton("mask", ()=>copy.add_mask, () => send(addMask()), "", addMask));
  const addFilter = glyphButton("add-filter", ()=>copy.add_filter, () => openMenu(addFilter));
  addFilter.id = "layer-add-filter"; addFilter.menuModel = () => state().layer_tools.add_filter;
  addFilter.setAttribute("aria-haspopup", "menu"); place("add_filter", addFilter);
  place("import_image", glyphButton("image", ()=>copy.import_image, () => dispatch({type:"invoke",command:"import_image"})));
  const deleteAction = () => ({ op: "delete_selected" });
  const deleteButton = place("delete", glyphButton("delete", ()=>copy.delete_selected, () => send(deleteAction()), "", deleteAction));
  const more = glyphButton("more-small", ()=>copy.actions, () => more.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true,
    clientX: more.getBoundingClientRect().left, clientY: more.getBoundingClientRect().top })));
  const footerWidth = () => footer.getBoundingClientRect().width;
  const overflow = () => (state().layer_tools.footer ?? []).filter(slot => footerWidth() + 0.5 < slot.min_width).map(slot => slot.item);
  more.classList.add("layer-more"); more.dataset.context = "{}";
  more.prepareMenu = () => {
    const layer = active();
    if (layer) send({ op: "context", id: layer.id, mask: layer.mask_selected });
  };
  more.menuModel = () => {
    const layer = active(), mask = layer?.mask_selected ?? false;
    const model = layer ? app.layer_menu(layer.id, mask) : { title: copy.actions, sections: [] };
    const extra = overflow();
    return extra.length ? { ...model, sections: [extra, ...model.sections] } : model;
  };
  footer.append(more);
  const fitFooter = () => {
    const width = footerWidth();
    for (const slot of state().layer_tools.footer ?? []) {
      const node = footerButtons.get(slot.button);
      if (node?.parentNode===footer) node.style.display = width + 0.5 < slot.min_width ? "none" : "";
    }
  };
  const footerResize = new ResizeObserver(fitFooter); footerResize.observe(footer);
  panel.append(header, list, footer);
  const opacityPopup=element('div','panel layer-opacity-popup');opacityPopup.popover='auto';
  opacityPopup.style.cssText='position:fixed;inset:auto;margin:0;width:220px;padding:12px;';
  const opacityEditor=numberField(catalog.layer_opacity,()=>copy.opacity,value=>{
    const id=state().layer_tools.opacity_popup;if(id!=null)dispatch({type:'set_layer_opacity',id,opacity:value});
  });
  opacityEditor.id='layer-gesture-opacity';opacityPopup.append(opacityEditor);panel.append(opacityPopup);
  opacityPopup.addEventListener('toggle',event=>{
    if(event.newState==='closed'&&state().layer_tools.opacity_popup!=null&&Number(state().layer_tools.opacity_surface)===touchInput.surface)send({op:'close_opacity'});
  });
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
    const root = element("div", "layer-swipe"), row = rowFrame(), record = { root, row, layer }; row.dataset.layer = String(layer.id);
    const get = () => record.layer;
    const eye = glyphButton("eye", ()=>get().selection_layer?(get().visible?copy.hide_selection:copy.show_selection):(get().visible?copy.hide:copy.show), () => dispatch({ type: "set_layer_visibility", id: get().id, visible: !get().visible }));
    eye.onpointerenter = () => { eye.title = app.action_tooltip(eye.getAttribute("aria-label"), { type: "set_layer_visibility", id: get().id, visible: !get().visible }); };
    const selection = (e = {}) => ({ op: "select_row", id: get().id, extend: !!e.shiftKey, toggle: true });
    const check = glyphButton("selection-empty", ()=>copy.select_row_help, e => send(selection(e)), "", selection);
    const thumbnails = rowThumbnails(), gutter = element("span", "layer-connection-gutter");
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
    const texture = glyphButton("paper", ()=>paperCopy.choose, () => {
      send({ op: "select", id: get().id, mask: false });
      if (get().paper?.editable) chooseTexture({ app, layer: get().id, state, dispatch, element, button, icon, message });
    }, "layer-thumbnail layer-texture");
    texture.id = `layer-texture-${layer.id}`;
    const texturePreview = paperThumbnail({ app, element, icon, size: 28 }); texture.replaceChildren(texturePreview.node);
    thumbnails.append(gutter, content.b, link, mask.b, texture);
    const text = element("div", "layer-text"), name = rowName(), meta = element("span", "layer-meta"); text.append(name, meta);
    const lock = element("span", "layer-lock"), grip = element("span", "layer-grip"); grip.append(icon("grip"));
    const load=button('',e=>{e.stopPropagation();dispatch({type:'selection',action:{op:'load_layer',id:get().id,mode:'new',inverted:false}});},'layer-icon selection-layer-load');
    load.append(icon('selection-load'));thumbnails.insertBefore(load,link);
    row.append(eye, check, thumbnails, text, lock, grip);
    row.onclick = e => { if (!e.target.closest("button,input")) send({op:"select_row", id:get().id, extend:e.shiftKey, toggle:e.ctrlKey || e.metaKey}); };
    name.ondblclick = e => { e.stopPropagation(); if(get().can_rename) send({ op: "begin_rename", id: get().id }); };
    menu(row, get); menu(mask.b, get, true);
    Object.assign(record, { load, eye, check, thumbnails, content, mask, link, texture, texturePreview, name, text, meta, lock, grip });
    const actions = element("div", "layer-swipe-actions"), actionButtons = new Map();
    root.append(actions, row);
    record.refreshSwipe = () => {
      const ids = new Set(get().swipe_actions.map(action => action.id));
      for (const [id, b] of actionButtons) if (!ids.has(id)) { b.remove(); actionButtons.delete(id); }
      for (const model of get().swipe_actions) {
        let b = actionButtons.get(model.id);
        if (!b) {
          b = button("", e => { e.stopPropagation(); const action = get().swipe_actions.find(action => action.id === model.id); if (action?.enabled) { record.closeSwipe(); send(action.action); } }, `layer-swipe-${model.id}`);
          actionButtons.set(model.id, b);
        }
        b.textContent = b.title = model.label; b.setAttribute("aria-label", model.label); b.disabled = !model.enabled;
        actions.append(b);
      }
      actions.style.width = `${revealWidth()}px`;
    };
    const revealWidth = () => Math.min(root.clientWidth || 216, get().swipe_actions.length * 72);
    let offset = 0, drag, suppressClick, settling = false;
    const atRest = () => offset === 0 && !settling;
    const pickup = (handle, device) => handle ? get().drag_from_handle : get().drag_from_body.includes(device);
    record.refreshGrip = (maximum = offset > 0 ? revealWidth() : 72) => {
      grip.hidden = !get().can_drop_below;
      grip.style.opacity = String(.6 * (1 - Math.min(1, Math.abs(offset) / Math.max(1, maximum))));
      grip.ariaDisabled = String(!get().drag_from_handle);
    };
    const settled = e => {
      if (e.target !== row || e.propertyName !== "transform" || row.getAnimations().some(animation =>
        animation.transitionProperty === "transform" && !["finished", "idle"].includes(animation.playState))) return;
      settling = false; record.refreshGrip();
    };
    for (const type of ["transitionend", "transitioncancel"]) row.addEventListener(type, settled);
    const position = (value, animate = false) => {
      const maximum = value > 0 ? revealWidth() : 72;
      settling = animate && (offset !== value || settling);
      offset = value; root.classList.toggle("swipe-animating", animate);
      root.style.setProperty("--swipe", `${offset}px`);
      actions.hidden = offset <= 0; actions.inert = offset <= 0;
      if (animate) connectionAnimationUntil = performance.now() + 180;
      record.refreshGrip(maximum);
      scheduleConnections();
    };
    record.closeSwipe = () => position(0, true);
    position(0);
    record.refreshSwipe();
    root.addEventListener("pointerdown", e => {
      if (suppressClick) { row.removeEventListener("click", suppressClick, true); suppressClick = null; }
      if (e.button || !e.isPrimary || e.target.closest("input") || (!get().can_drop_below && !get().swipe_actions.some(action => action.enabled) && !get().right_swipe)) return;
      drag = { x: e.clientX, y: e.clientY, top: row.getBoundingClientRect().top, pointer: e.pointerId,
        handle: grip.contains(e.target), device: e.pointerType,
        canDrag: atRest() && pickup(grip.contains(e.target), e.pointerType),
        direct: e.pointerType !== "mouse" && !grip.contains(e.target), held: false, origin: offset };
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
      if (drag.direct) {
        if (drag.held) return;
        if (!drag.swiping && Math.hypot(dx,dy) > 8) {
          const allowed = dx < 0 ? get().swipe_actions.some(action => action.enabled) : drag.origin > 0 || !!get().right_swipe;
          if (allowed && Math.abs(dx) > Math.abs(dy)) {
            drag.swiping = true; dismissContext(); row.setPointerCapture(e.pointerId);
          } else { finish({type:"pointercancel",pointerId:e.pointerId}); return; }
        }
        if (drag.swiping) {
          const minimum = drag.origin === 0 && get().right_swipe ? -72 : 0;
          e.preventDefault(); position(Math.max(minimum,Math.min(revealWidth(),drag.origin-dx)));
        }
        return;
      }
      if (!drag.canDrag || !atRest()) return;
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
        const threshold = previous.origin > 0 ? previous.origin - 72*.4 : 72*.4;
        position(e.type === "pointerup" && offset >= threshold ? revealWidth() : 0, true);
        if (toggle && get().right_swipe) send(get().right_swipe);
      }
      if (e.type !== "pointerup" && previous.held) dismissContext();
      if (previous.ghost) {
        previous.ghost.remove(); rows.querySelectorAll(".layer-drop-before,.layer-drop-after,.layer-drop-into,.layer-drop-attach").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into","layer-drop-attach"));
        if (e.type === "pointerup" && previous.target && app.layer_drop_preview(previous.target.id, previous.target.target, previous.target.fraction, previous.target.surface)) send(previous.target);
      }
    };
    record.cancelDrag = () => finish({type:"pointercancel"});
    record.retireIneligibleDrag = () => {
      if (!drag?.canDrag || pickup(drag.handle, drag.device)) return;
      if (drag.ghost) finish({type:"pointercancel"}); else drag.canDrag = false;
    };
    row.addEventListener("pointerup", finish); row.addEventListener("pointercancel", finish);
    row.addEventListener("lostpointercapture", e => {
      // Transferring implicit touch/pen capture from a grip or child control
      // to its row releases that child without cancelling the reorder.
      if (e.target === row || !row.hasPointerCapture(e.pointerId)) finish(e);
    });
    return record;
  }
  const cancelDrags = () => { for (const r of records.values()) { r.cancelDrag(); r.closeSwipe(); } };
  const touchInput=layerTouchInput({list,app,applyChange,wake,cancelNative:cancelDrags,
    rowAt:target=>{const row=target.closest('.layer-row');return row&&list.contains(row)?records.get(row.dataset.layer)?.layer.id:null;}});
  const closeSwipes = e => { for (const r of records.values()) if (!r.root.contains(e.target)) r.closeSwipe(); };
  document.addEventListener("pointerdown", closeSwipes, true);
  const closeOnClick = e => { if (!e.target.closest(".layer-swipe-actions")) for (const r of records.values()) r.closeSwipe(); };
  document.addEventListener("click", closeOnClick);
  window.addEventListener("blur", cancelDrags);
  const cancelOnEscape = e => { if (e.key === "Escape") cancelDrags(); };
  window.addEventListener("keydown", cancelOnEscape);
  function refresh() {
    const epoch=String(state().document_file.epoch);
    if(epoch!==documentEpoch){touchInput.cancel();cancelDrags();documentEpoch=epoch;thumbnails.dispose();
      for(const r of records.values())for(const c of [r.content.image,r.mask.image])if(c)c.width=c.width;
    }
    const view = state().layer_tools, current = view.editing_layer, controls = view.controls;
    if(view.opacity_popup!=null&&Number(view.opacity_surface)===touchInput.surface&&current){
      opacityEditor.update(current.opacity);opacityEditor.setDisabled(!controls.opacity);
      if(!opacityPopup.matches(':popover-open')){
        const rect=records.get(String(view.opacity_popup))?.row.getBoundingClientRect()??list.getBoundingClientRect();
        opacityPopup.style.left=`${Math.max(8,Math.min(rect.x,innerWidth-236))}px`;
        opacityPopup.style.top=`${Math.max(8,Math.min(rect.bottom,innerHeight-110))}px`;
        opacityPopup.showPopover();
      }
    }else if(opacityPopup.matches(':popover-open'))opacityPopup.hidePopover();
    if (current) opacity.update(current.opacity);
    blend.update({text:current?.blend_label??'',disabled:!controls.blend,menu:()=>app.layer_blend_menu(active().id)});
    opacity.setDisabled(!controls.opacity); maskButton.disabled = !controls.mask; more.disabled = !current && !overflow().length;
    fitFooter();
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
      record.root.remove(); records.delete(id); thumbnails.forget(`${id}:false`); thumbnails.forget(`${id}:true`);
    }
    let position = 0;
    const place = root => { if (rows.children[position] !== root) rows.insertBefore(root, rows.children[position] || null); position++; };
    for (const layer of state().layers) {
      const id = String(layer.id); if (!records.has(id)) records.set(id, makeRow(layer));
      place(records.get(id).root);
    }
    if (view.add_paper) {
      const action = view.add_paper;
      if (addPaper.ariaLabel !== action.label) {
        addPaper.ariaLabel = action.label;
        addPaper.replaceChildren(icon(nameIcon(action.icon)), element("span", "layer-name", action.label));
      }
      place(addPaper);
    } else addPaper.remove();
    state().layers.forEach(layer => {
      const r = records.get(String(layer.id)); r.layer = layer; r.retireIneligibleDrag();
      const {paint_revision,mask_revision,...presentation}=layer;
      const key=JSON.stringify([presentation,view.rename_layer===layer.id],(_,value)=>typeof value==="bigint"?String(value):value);
      // Keep the latest thumbnail revisions above, but retain unchanged row
      // widgets, SVGs and text through canvas movement and raster publications.
      if(r.presentation===key)return;
      r.presentation=key;
      r.row.classList.toggle("selected", layer.selected);r.load.hidden=!layer.selection_layer;r.load.title=layer.load_selection_tooltip;r.load.setAttribute("aria-label",r.load.title);
      r.refreshSwipe();
      if (!layer.swipe_actions.some(action => action.enabled) && !layer.right_swipe) r.closeSwipe();
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
      r.texture.hidden = !layer.paper;
      if (layer.paper) { r.texturePreview.show(layer.paper.id); r.texture.setAttribute("aria-description", layer.paper.label); }
      r.name.textContent = layer.label; r.name.title = layer.label;
      r.meta.textContent = layer.description;
      r.meta.hidden = !r.meta.textContent; r.lock.replaceChildren(icon(layer.locked ? "lock" : "alpha-lock")); r.lock.style.opacity = layer.locked || layer.alpha_locked ? 1 : 0;
      r.refreshGrip();
      if (view.rename_layer !== layer.id && r.entry) r.closeRename();
      if (view.rename_layer === layer.id && !r.entry) {
        const input = element("input", "layer-name-entry"); input.value = layer.label; input.maxLength = 128; r.entry = input; r.name.hidden = true; r.text.prepend(input);
        let finished = false;
        r.closeRename = () => { finished = true; input.remove(); r.entry = null; r.name.hidden = false; };
        const done = cancel => { if (finished) return; r.closeRename();
          send(cancel || !input.value.trim() ? { op: "cancel_rename" } : { op: "rename", id: layer.id, name: input.value }); };
        input.onblur = () => done(false); input.onkeydown = e => { e.stopPropagation(); if (e.key === "Enter" || e.key === "Escape") { e.preventDefault(); done(e.key === "Escape"); } };
        input.focus(); input.select();
      }
    });
    // Bitmap revisions do not change row geometry. Native text, hierarchy and
    // controls do, including when this retained panel is currently offscreen.
    const next = JSON.stringify([view.quick_mask,current?.selection_layer,view.rename_layer?.toString(),view.add_paper, state().layers.map(({paint_revision,mask_revision,...row})=>row)],
      (_,value)=>typeof value==="bigint"?String(value):value);
    if (next !== measurementKey) { measurementKey=next; contentChanged("layers"); }
    scheduleConnections();
  }
  const thumbnails = createLayerThumbnails(app, wake);
  const thumbnailTimer = setInterval(() => {
    // A modal Settings session cannot edit layers. Defer thumbnail geometry
    // and uploads until it closes instead of forcing layout behind the modal.
    if (state().settings_open) return;
    if (!panel.isConnected || !panel.clientHeight) return;
    try {
      thumbnails.poll();
      const viewport = rows.getBoundingClientRect();
      for (const [id, r] of records) {
        const rect = r.row.getBoundingClientRect(); if (rect.bottom < Math.max(0,viewport.top) || rect.top > innerHeight || rect.height === 0) continue;
        for (const mask of [false,true]) {
          if (mask ? !r.layer.has_mask : !r.layer.has_thumbnail) continue;
          const key = `${id}:${mask}`, revision = documentEpoch + ":" + String(mask ? r.layer.mask_revision : r.layer.paint_revision);
          if (!thumbnails.request(key, revision, mask ? r.layer.mask_id : r.layer.id, (mask ? r.mask : r.content).image)) continue;
          return;
        }
      }
    } catch (error) { message(error); }
  }, 120);
  return { refresh, measureUnit: () => unit.getBoundingClientRect().height, dispose(){touchInput.dispose();cancelDrags();opacityEditor.dispose?.();opacityPopup.remove();cancelAnimationFrame(connectionFrame);connectionResize.disconnect();footerResize.disconnect();document.removeEventListener("pointerdown",closeSwipes,true);document.removeEventListener("click",closeOnClick);window.removeEventListener("blur",cancelDrags);window.removeEventListener("keydown",cancelOnEscape);clearInterval(thumbnailTimer); thumbnails.dispose();} };
}

export function createLayerThumbnails(app, wake) {
  const revisions=new Map(),owned=new Set(),pending=thumbnailPending;
  return {
    poll() {
      for(let image;(image=app.take_layer_thumbnail());) {
        const [id,width,height,bytes]=image,target=pending.get(id);pending.delete(id);target?.owned.delete(id);
        if(target&&target.revisions.get(target.key)===target) {
          target.canvas.width=width;target.canvas.height=height;
          target.canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(bytes),width,height),0,0);
          target.canvas.dataset.previewRevision=target.revision;
          target.loaded?.();
        }
      }
    },
    request(key,revision,id,canvas,loaded) {
      const previous=revisions.get(key);
      if(previous?.revision===revision&&previous.canvas===canvas||pending.size>=8)return false;
      const token=++thumbnailRequest;
      if(app.request_layer_thumbnail(token,id)) {
        const target={key,revision,revisions,owned,canvas,loaded};
        owned.add(token);revisions.set(key,target);pending.set(token,target);
      }
      if(app.canvas_work_pending()||app.shader_work_pending(true))wake();
      return true;
    },
    forget(key){revisions.delete(key);for(const token of owned)if(pending.get(token)?.key===key){pending.delete(token);owned.delete(token);}},
    dispose(){for(const id of owned)pending.delete(id);owned.clear();revisions.clear();},
  };
}
