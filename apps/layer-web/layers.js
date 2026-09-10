// Layer widgets only. Selection, references, hierarchy and menu policy are Rust.
export function createLayerPanel({ app, catalog, state, panel, element, button, icon, dispatch, applyChange, message, numberField }) {
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
  const menu = (node, getLayer, mask = false) => {
    node.dataset.context = "{}";
    node.layerMenu = () => { const id = getLayer().id, targetMask = typeof mask === "function" ? mask() : mask;
      send({ op: "context", id, mask: targetMask }); return app.layer_menu(id, targetMask); };
  };
  for (const group of [false, true]) {
    const getAction = () => ({ op: "new", group, clipped: false });
    footer.append(glyphButton(group ? "folder" : "plus", group ? "New group" : "New layer", () => send(getAction()), "", getAction));
  }
  const addMask = () => ({ op: "add_mask", id: active().id, replace: false });
  const maskButton = glyphButton("mask", "Add layer mask", () => send(addMask()), "", addMask); footer.append(maskButton);
  const file = element("input"); file.type = "file"; file.accept = "image/*"; file.hidden = true;
  file.onchange = async () => {
    const source = file.files[0]; if (!source) return;
    try {
      const image = await createImageBitmap(source);
      // Decode an imported file only; this is not a canvas raster fallback.
      const buffer = new OffscreenCanvas(image.width, image.height), context = buffer.getContext("2d");
      context.drawImage(image, 0, 0); const rgba = context.getImageData(0, 0, image.width, image.height);
      applyChange(app.import_layer_image(source.name, image.width, image.height, rgba.data)); image.close();
    } catch (error) { message(error); }
    file.value = "";
  };
  footer.append(glyphButton("image", "Import image as layer", () => file.click()), file);
  const more = glyphButton("more", "Layer actions", () => more.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true,
    clientX: more.getBoundingClientRect().left, clientY: more.getBoundingClientRect().top })));
  more.classList.add("layer-more"); menu(more, active, () => active()?.mask_selected ?? false); footer.append(more);
  panel.append(header, rows, footer);
  const nameIcon = value => value.replace(/^layer-/, "").replace(/-symbolic$/, "");
  function makeRow(layer) {
    const row = element("div", "layer-row"), record = { row, layer }; row.dataset.layer = String(layer.id);
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
      b.replaceChildren(image); return { b, image };
    };
    const content = thumb(false), mask = thumb(true);
    const linkAction = () => ({ op: "link_mask", id: get().id, value: !get().mask_linked });
    const link = glyphButton("link", "Link mask to layer", () => send(linkAction()), "layer-link", linkAction);
    thumbnails.append(clipping, content.b, link, mask.b);
    const text = element("div", "layer-text"), name = element("span", "layer-name"), meta = element("span", "layer-meta"); text.append(name, meta);
    const lock = element("span", "layer-lock"), grip = element("span", "layer-grip"); grip.append(icon("grip"));
    row.append(eye, check, thumbnails, text, lock, grip);
    row.onclick = e => { if (!e.target.closest("button,input")) select(false); };
    name.ondblclick = () => send({ op: "begin_rename", id: get().id });
    menu(row, get); menu(mask.b, get, true);
    Object.assign(record, { eye, check, thumbnails, clipping, content, mask, link, name, text, meta, lock, grip });
    let drag;
    row.addEventListener("pointerdown", e => {
      if (e.button || e.target.closest("input") || (e.pointerType === "touch" && !grip.contains(e.target)) || !get().can_drop_below) return;
      drag = { x: e.clientX, y: e.clientY, top: row.getBoundingClientRect().top, pointer: e.pointerId };
    });
    row.addEventListener("workspace-context-claimed", () => { drag = null; });
    row.addEventListener("pointermove", e => {
      if (!drag) return;
      if (!drag.ghost && Math.hypot(e.clientX-drag.x, e.clientY-drag.y) > 6) {
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
    });
    const finish = e => {
      if (!drag) return; const previous = drag; drag = null;
      if (previous.ghost) {
        previous.ghost.remove(); rows.querySelectorAll(".layer-row").forEach(n => n.classList.remove("layer-drop-before","layer-drop-after","layer-drop-into"));
        if (e.type === "pointerup") {
          const suppress = e => { e.preventDefault(); e.stopImmediatePropagation(); };
          row.addEventListener("click", suppress, { once: true, capture: true });
          setTimeout(() => row.removeEventListener("click", suppress, true), 0);
        }
        if (e.type === "pointerup" && previous.target) send(previous.target);
      }
    };
    row.addEventListener("pointerup", finish); row.addEventListener("pointercancel", finish);
    return record;
  }
  function refresh() {
    const view = state().layer_tools, current = view.editing_layer, controls = view.controls;
    if (current) { opacity.update(current.opacity); blend.value = current.blend; }
    opacity.setDisabled(!controls.opacity); blend.disabled = !controls.blend; maskButton.disabled = !controls.mask;
    for (const { b, property, capability } of toggles) {
      const reference = capability === "reference";
      b.disabled = !(reference ? view.can_reference : controls[capability]);
      b.setAttribute("aria-pressed", reference ? view.references_selected : !!current?.[property]);
      if (reference) b.title = b.ariaLabel = view.reference_action_label;
    }
    const ids = new Set(state().layers.map(l => String(l.id)));
    for (const [id, record] of records) if (!ids.has(id)) {
      record.row.remove(); records.delete(id); revisions.delete(`${id}:false`); revisions.delete(`${id}:true`);
    }
    state().layers.forEach((layer, index) => {
      const id = String(layer.id); if (!records.has(id)) records.set(id, makeRow(layer));
      const r = records.get(id); r.layer = layer;
      if (rows.children[index] !== r.row) rows.insertBefore(r.row, rows.children[index] || null);
      r.row.classList.toggle("selected", layer.selected);
      r.eye.replaceChildren(icon(layer.visible ? "eye" : "eye-hidden")); r.eye.title = r.eye.ariaLabel = layer.visible ? "Hide layer" : "Show layer";
      r.check.replaceChildren(icon(nameIcon(layer.selection_icon)));
      r.thumbnails.style.marginLeft = `${Math.min(layer.depth*8,24)}px`; r.clipping.style.opacity = layer.clipped ? 1 : 0;
      r.content.b.classList.toggle("editing-target", layer.editing && !layer.mask_selected);
      r.mask.b.classList.toggle("editing-target", layer.mask_selected);
      r.content.b.classList.toggle("layer-folder", layer.group);
      if (layer.group) r.content.b.replaceChildren(icon(layer.collapsed ? "folder" : "folder-open"));
      else if(layer.content_icon) r.content.b.replaceChildren(icon(nameIcon(layer.content_icon)));
      else if (!r.content.image.isConnected) r.content.b.replaceChildren(r.content.image);
      r.mask.b.hidden = r.link.hidden = !layer.has_mask; r.mask.image.style.opacity = layer.mask_enabled ? 1 : .4;
      r.link.style.opacity = layer.mask_linked ? 1 : .35; r.link.title = r.link.ariaLabel = layer.mask_linked ? "Unlink mask from layer" : "Link mask to layer";
      r.name.textContent = layer.label; r.name.title = layer.label;
      r.meta.textContent = [layer.blend ? layer.blend_label : "", layer.opacity < 1 ? `${Math.round(layer.opacity*100)}%` : ""].filter(Boolean).join(" · ");
      r.meta.hidden = !r.meta.textContent; r.lock.replaceChildren(icon(layer.locked ? "lock" : "alpha-lock")); r.lock.style.opacity = layer.locked || layer.alpha_locked ? 1 : 0;
      r.grip.hidden = !layer.can_drop_below;
      if (view.rename_layer === layer.id && !r.entry) {
        const input = element("input", "layer-name-entry"); input.value = layer.label; input.maxLength = 128; r.entry = input; r.name.hidden = true; r.text.prepend(input);
        let finished = false;
        const done = cancel => { if (finished) return; finished = true; input.remove(); r.entry = null; r.name.hidden = false;
          send(cancel || !input.value.trim() ? { op: "cancel_rename" } : { op: "rename", id: layer.id, name: input.value }); };
        input.onblur = () => done(false); input.onkeydown = e => { e.stopPropagation(); if (e.key === "Enter" || e.key === "Escape") { e.preventDefault(); done(e.key === "Escape"); } };
        input.focus(); input.select();
      }
    });
  }
  let request = 0n; const pending = new Map(), revisions = new Map();
  setInterval(() => {
    if (!panel.isConnected || !panel.clientHeight) return;
    try {
      for (let image; (image = app.take_layer_thumbnail());) {
        const [id, width, height, bytes] = image, target = pending.get(id); pending.delete(id);
        if (target && target.revision === revisions.get(target.key)) target.canvas.getContext("2d").putImageData(new ImageData(new Uint8ClampedArray(bytes),width,height),0,0);
      }
      const viewport = rows.getBoundingClientRect();
      for (const [id, r] of records) {
        const rect = r.row.getBoundingClientRect(); if (rect.bottom < Math.max(0,viewport.top) || rect.top > innerHeight || rect.height === 0) continue;
        for (const mask of [false,true]) {
          if (mask ? !r.layer.has_mask : r.layer.group || r.layer.content_icon) continue;
          const key = `${id}:${mask}`, revision = String(mask ? r.layer.mask_revision : r.layer.paint_revision);
          if (revisions.get(key) === revision || pending.size >= 8) continue;
          const token = ++request;
          if (app.request_layer_thumbnail(token, mask ? r.layer.mask_id : r.layer.id)) {
            revisions.set(key,revision); pending.set(token,{key,revision,canvas: (mask ? r.mask : r.content).image});
          }
        }
      }
    } catch (error) { message(error); }
  }, 120);
  return { refresh };
}
