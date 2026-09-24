let thumbnailRequest=0n;
const thumbnailPending=new Map();
// Layer widgets only. Selection, references, hierarchy and menu policy are Rust.
export function createLayerPanel({ app, catalog, state, panel, element, button, icon, dispatch, applyChange, message, numberField, wake, dismissContext, contentChanged = () => {} }) {
  const send = action => dispatch({ type: "layer", action });
  const header = element("div", "layer-header"), footer = element("div", "layer-footer");
  header.dataset.control = "layer_opacity"; footer.dataset.control = "layer_actions";
  const options = element("div", "layer-options"), blend = element("select");
  blend.title = "Layer blend mode"; blend.setAttribute("aria-label", blend.title);
  catalog.layer_blends.forEach((label, i) => { const option = element("option", "", label); option.value = i; blend.append(option); });
  const active = () => state().layer_tools.editing_layer;
  blend.onchange = () => send({ op: "blend", id: active().id, value: Number(blend.value) });
  const opacity = numberField(catalog.layer_opacity, "Layer opacity", value => dispatch({ type: "set_layer_opacity", opacity: value }), true);
  opacity.id = "layer-opacity"; options.append(blend, opacity); header.append(options);
  const glyphButton = (glyph, label, click, cls = "", getAction) => {
    const b = button("", e => { e.stopPropagation(); click(); }, `layer-icon ${cls}`);
    b.title = label; b.setAttribute("aria-label", label); b.append(icon(glyph));
    if (getAction) b.onpointerenter = () => { b.title = app.action_tooltip(b.getAttribute("aria-label"), { type: "layer", action: getAction() }); };
    return b;
  };
  const flags = element("div", "layer-flags"), toggles = [];
  for (const [glyph, label, property, op, capability] of [
    ["alpha-lock", "Alpha lock", "alpha_locked", "alpha_lock", "alpha_lock"],
    ["lock", "Lock editing", "locked", "lock", "edit_lock"],
    ["clip", "Clip to layer below", "clipped", "clip", "clip"],
    ["reference", "Use selected layers as references", "reference", "reference_selection", "reference"],
  ]) {
    const getAction = () => op === "reference_selection" ? { op } : { op, id: active().id, value: !active()[property] };
    const b = glyphButton(glyph, label, () => send(getAction()), "", getAction);
    if (capability === "reference") b.classList.add("layer-reference");
    toggles.push({ b, property, capability }); flags.append(b);
  }
  header.append(flags);
  const rows = element("div", "layer-rows"); rows.id = "layer-rows"; rows.dataset.control = "layers";
  const records = new Map();
  let documentEpoch, measurementKey;
  const menu = (node, getLayer, mask = false) => {
    node.dataset.context = "{}";
    node.layerMenu = () => { const id = getLayer().id, targetMask = typeof mask === "function" ? mask() : mask;
      send({ op: "context", id, mask: targetMask }); return app.layer_menu(id, targetMask); };
  };
  for (const group of [false, true]) {
    const getAction = () => ({ op: "new", group, clipped: false });
    footer.append(glyphButton(group ? "folder" : "add-layer", group ? "New group" : "New layer", () => send(getAction()), "", getAction));
  }
  footer.append(glyphButton("selection-brush", "New Selection Layer", () => dispatch({type:"invoke",command:"new_selection_layer"})));
  const addMask = () => ({ op: "add_mask", id: active().id, replace: false });
  const maskButton = glyphButton("mask", "Add layer mask", () => send(addMask()), "", addMask); footer.append(maskButton);
  footer.append(glyphButton("image", "Import image as layer", () => dispatch({type:"invoke",command:"import_image"})));
  const deleteAction = () => ({ op: "delete_selected" });
  const deleteButton = glyphButton("delete", "Delete selected layers", () => send(deleteAction()), "", deleteAction);
  footer.append(deleteButton);
  const more = glyphButton("more", "Layer actions", () => more.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true,
    clientX: more.getBoundingClientRect().left, clientY: more.getBoundingClientRect().top })));
  more.classList.add("layer-more"); menu(more, active, () => active()?.mask_selected ?? false); footer.append(more);
  panel.append(header, rows, footer);
  const nameIcon = value => value.replace(/^layer-/, "").replace(/-symbolic$/, "");
  function makeRow(layer) {
    const root = element("div", "layer-swipe"), row = element("div", "layer-row"), record = { root, row, layer }; row.dataset.layer = String(layer.id);
    const get = () => record.layer, select = mask => send({ op: "select", id: get().id, mask });
    const eye = glyphButton("eye", "Hide layer", () => dispatch({ type: "set_layer_visibility", id: get().id, visible: !get().visible }));
    eye.onpointerenter = () => { eye.title = app.action_tooltip(eye.getAttribute("aria-label"), { type: "set_layer_visibility", id: get().id, visible: !get().visible }); };
    const selection = () => ({ op: "toggle_selection", id: get().id });
    const check = glyphButton("selection-empty", "Select layer without changing drawing target", () => send(selection()), "", selection);
    const thumbnails = element("div", "layer-thumbnails"), clipping = element("span", "layer-clipping");
    const thumb = (mask) => {
      const getAction = () => !mask && get().group ? { op: "collapse", id: get().id } : { op: "select", id: get().id, mask };
      const b = glyphButton("mask", mask ? "Edit layer mask" : "Edit layer content", () => send(getAction()), "layer-thumbnail", getAction);
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
      return { b, image };
    };
    const content = thumb(false), mask = thumb(true);
    const linkAction = () => ({ op: "link_mask", id: get().id, value: !get().mask_linked });
    const link = glyphButton("link", "Link mask to layer", () => send(linkAction()), "layer-link", linkAction);
    thumbnails.append(clipping, content.b, link, mask.b);
    const text = element("div", "layer-text"), name = element("span", "layer-name"), meta = element("span", "layer-meta"); text.append(name, meta);
    const lock = element("span", "layer-lock"), grip = element("span", "layer-grip"); grip.append(icon("grip"));
    const load=button('',e=>{e.stopPropagation();dispatch({type:'selection',action:{op:'load_layer',id:get().id,mode:'new',inverted:false}});},'selection-layer-load');
    load.append(icon('selection-load'));thumbnails.insertBefore(load,link);
    row.append(eye, check, thumbnails, text, lock, grip);
    row.onclick = e => { if (!e.target.closest("button,input")) select(false); };
    name.ondblclick = e => { e.stopPropagation(); if(get().can_rename) send({ op: "begin_rename", id: get().id }); };
    menu(row, get); menu(mask.b, get, true);
    Object.assign(record, { load, eye, check, thumbnails, clipping, content, mask, link, name, text, meta, lock, grip });
    const remove = button("Delete", e => { e.stopPropagation(); send({ op: "delete", id: get().id }); }, "layer-swipe-delete");
    root.append(remove, row);
    let offset = 0, drag, suppressClick;
    const position = (value, animate = false) => {
      offset = value; root.classList.toggle("swipe-animating", animate);
      root.style.setProperty("--swipe", `${offset}px`);
      remove.hidden = offset === 0; remove.disabled = !get().can_delete;
    };
    record.closeSwipe = () => position(0, true);
    position(0);
    row.addEventListener("pointerdown", e => {
      if (suppressClick) { row.removeEventListener("click", suppressClick, true); suppressClick = null; }
      if (e.button || !e.isPrimary || e.target.closest("input") || (!get().can_drop_below && !get().can_delete)) return;
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
          if (get().can_delete && Math.abs(dx) > Math.abs(dy) && (dx < 0 || drag.origin > 0)) {
            drag.swiping = true; dismissContext(); row.setPointerCapture(e.pointerId);
          } else { finish({type:"pointercancel",pointerId:e.pointerId}); return; }
        }
        if (drag.swiping) {
          e.preventDefault(); position(Math.max(0,Math.min(72,drag.origin-dx)));
        }
        return;
      }
      if (!get().can_drop_below) return;
      if (!drag.ghost && Math.hypot(e.clientX-drag.x, e.clientY-drag.y) > 6) {
        dismissContext();
        row.setPointerCapture(e.pointerId); drag.ghost = row.cloneNode(true); drag.ghost.classList.add("layer-drag-preview");
        drag.ghost.style.width = `${row.clientWidth}px`; drag.ghost.style.left = `${row.getBoundingClientRect().left}px`;
        document.body.append(drag.ghost);
        drag.ghost.querySelectorAll("canvas").forEach((c,i) => c.getContext("2d").drawImage(row.querySelectorAll("canvas")[i],0,0));
      }
      if (!drag.ghost) return;
      drag.ghost.style.top = `${drag.top + e.clientY-drag.y}px`;
      rows.querySelectorAll(".layer-drop-before,.layer-drop-after,.layer-drop-into").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into"));
      const target = document.elementFromPoint(e.clientX,e.clientY)?.closest(".layer-row"); drag.target = null;
      if (target && target !== row && rows.contains(target)) {
        const to = records.get(target.dataset.layer).layer, rect = target.getBoundingClientRect();
        const fraction = to.can_drop_below ? (e.clientY-rect.top)/rect.height : 0;
        drag.target = { op: "drop", id: get().id, target: to.id, fraction };
        target.classList.add(to.group && fraction > .25 && fraction < .75 ? "layer-drop-into" : fraction < .5 ? "layer-drop-before" : "layer-drop-after");
      }
    };
    const finish = e => {
      if (!drag || (e.pointerId != null && drag.pointer !== e.pointerId)) return;
      const previous = drag; drag = null;
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
      if (previous.swiping) position(e.type === "pointerup" && offset >= 72*.4 ? 72 : 0, true);
      if (e.type !== "pointerup" && previous.held) dismissContext();
      if (previous.ghost) {
        previous.ghost.remove(); rows.querySelectorAll(".layer-row").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into"));
        if (e.type === "pointerup" && previous.target) send(previous.target);
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
  const cancelDrags = () => { for (const r of records.values()) { r.cancelDrag(); r.closeSwipe(); } };
  const closeSwipes = e => { for (const r of records.values()) if (!r.root.contains(e.target)) r.closeSwipe(); };
  document.addEventListener("pointerdown", closeSwipes, true);
  const closeOnClick = e => { if (!e.target.closest(".layer-swipe-delete")) for (const r of records.values()) r.closeSwipe(); };
  document.addEventListener("click", closeOnClick);
  window.addEventListener("blur", cancelDrags);
  const cancelOnEscape = e => { if (e.key === "Escape") cancelDrags(); };
  window.addEventListener("keydown", cancelOnEscape);
  function refresh() {
    const epoch=String(state().document_file.epoch);
    if(epoch!==documentEpoch){cancelDrags();documentEpoch=epoch;revisions.clear();for(const id of owned)pending.delete(id);owned.clear();
      for(const r of records.values())for(const c of [r.content.image,r.mask.image])if(c)c.width=c.width;
    }
    const view = state().layer_tools, current = view.editing_layer, controls = view.controls;
    if (current) { opacity.update(current.opacity); blend.value = current.blend; }
    opacity.setDisabled(!controls.opacity); blend.disabled = !controls.blend; maskButton.disabled = !controls.mask; more.disabled = !current;
    deleteButton.disabled = !state().layer_tools.can_delete;
    for (const { b, property, capability } of toggles) {
      const reference = capability === "reference";
      b.disabled = !(reference ? view.can_reference : controls[capability]);
      b.setAttribute("aria-pressed", reference ? view.references_selected : !!current?.[property]);
      if (reference) b.title = b.ariaLabel = view.reference_action_label;
    }
    const ids = new Set(state().layers.map(l => String(l.id)));
    for (const [id, record] of records) if (!ids.has(id)) {
      record.cancelDrag();
      record.root.remove(); records.delete(id); revisions.delete(`${id}:false`); revisions.delete(`${id}:true`);
    }
    state().layers.forEach((layer, index) => {
      const id = String(layer.id); if (!records.has(id)) records.set(id, makeRow(layer));
      const r = records.get(id); r.layer = layer;
      if (rows.children[index] !== r.root) rows.insertBefore(r.root, rows.children[index] || null);
      const {paint_revision,mask_revision,...presentation}=layer;
      const key=JSON.stringify([presentation,view.rename_layer===layer.id],(_,value)=>typeof value==="bigint"?String(value):value);
      // Keep the latest thumbnail revisions above, but retain unchanged row
      // widgets, SVGs and text through canvas movement and raster publications.
      if(r.presentation===key)return;
      r.presentation=key;
      r.row.classList.toggle("selected", layer.selected);r.load.hidden=!layer.selection_layer;r.load.title=layer.load_selection_tooltip;r.load.setAttribute("aria-label",r.load.title);
      if (!layer.can_delete) r.closeSwipe();
      r.eye.replaceChildren(icon(layer.visible ? "eye" : "eye-hidden")); r.eye.title = r.eye.ariaLabel = layer.selection_layer?(layer.visible?"Hide selection overlay":"Show selection overlay"):(layer.visible?"Hide layer":"Show layer");
      r.check.replaceChildren(icon(nameIcon(layer.selection_icon)));
      r.thumbnails.style.marginLeft = `${Math.min(layer.depth*8,24)}px`; r.clipping.style.opacity = layer.clipped ? 1 : 0;
      r.content.b.classList.toggle("editing-target", layer.editing && !layer.mask_selected);
      r.mask.b.classList.toggle("editing-target", layer.mask_selected);
      r.content.b.classList.toggle("layer-folder", layer.group);
      if (layer.group) r.content.b.replaceChildren(icon(layer.collapsed ? "folder" : "folder-open"));
      else if(layer.content_icon && !layer.selection_layer) {
        const glyph = icon(nameIcon(layer.content_icon));
        if (layer.content_icon_color) {
          glyph.style.color = layer.content_icon_color; glyph.classList.add("paper-thumbnail-icon");
          r.content.b.replaceChildren(r.content.image, glyph);
        } else r.content.b.replaceChildren(glyph);
      }
      else if (!r.content.image.isConnected) r.content.b.replaceChildren(r.content.image);
      r.mask.b.hidden = r.link.hidden = !layer.has_mask; r.mask.image.style.opacity = layer.mask_enabled ? 1 : .4;
      r.link.style.opacity = layer.mask_linked ? 1 : .35; r.link.title = r.link.ariaLabel = layer.mask_linked ? "Unlink mask from layer" : "Link mask to layer";
      r.name.textContent = layer.label; r.name.title = layer.label;
      r.meta.textContent = layer.description;
      r.meta.hidden = !r.meta.textContent; r.lock.replaceChildren(icon(layer.locked ? "lock" : "alpha-lock")); r.lock.style.opacity = layer.locked || layer.alpha_locked ? 1 : 0;
      r.grip.hidden = !layer.can_drop_below;
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
    const next = JSON.stringify([view.quick_mask,current?.selection_layer,view.rename_layer?.toString(), state().layers.map(({paint_revision,mask_revision,...row})=>row)],
      (_,value)=>typeof value==="bigint"?String(value):value);
    if (next !== measurementKey) { measurementKey=next; contentChanged("layers"); }
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
        }
      }
      const viewport = rows.getBoundingClientRect();
      for (const [id, r] of records) {
        const rect = r.row.getBoundingClientRect(); if (rect.bottom < Math.max(0,viewport.top) || rect.top > innerHeight || rect.height === 0) continue;
        for (const mask of [false,true]) {
          if (mask ? !r.layer.has_mask : r.layer.group || (r.layer.content_icon && !r.layer.selection_layer && !r.layer.content_icon_color)) continue;
          const key = `${id}:${mask}`, revision = documentEpoch + ":" + String(mask ? r.layer.mask_revision : r.layer.paint_revision);
          if (revisions.get(key) === revision || pending.size >= 8) continue;
          const token = ++thumbnailRequest;
          if (app.request_layer_thumbnail(token, mask ? r.layer.mask_id : r.layer.id)) {
            owned.add(token);revisions.set(key,revision); pending.set(token,{key,revision,revisions,owned,canvas: (mask ? r.mask : r.content).image});
          } else if (app.shader_work_pending()) {
            // A drawer can request its first mask preview while the canvas is
            // idle. Start deferred compilation without waiting for canvas input.
            wake();
          }
        }
      }
    } catch (error) { message(error); }
  }, 120);
  return { refresh, dispose(){cancelDrags();document.removeEventListener("pointerdown",closeSwipes,true);document.removeEventListener("click",closeOnClick);window.removeEventListener("blur",cancelDrags);window.removeEventListener("keydown",cancelOnEscape);clearInterval(thumbnailTimer); for(const id of owned)pending.delete(id);} };
}
