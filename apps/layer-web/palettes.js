const GAP = 4, CELL = 44, SLOP = 8;
const plain = (_, value) => typeof value === "bigint" ? Number(value) : value;
const css = rgba => `rgba(${rgba.slice(0, 3).map(v => Math.round(Math.min(1, Math.max(0, v)) * 255)).join(",")},${rgba[3] ?? 1})`;
const paint = (node, rgba) => { node.style.background = `linear-gradient(${css(rgba)},${css(rgba)}),repeating-conic-gradient(#ccc 0 25%,#8c8c8c 0 50%) 0 0 / 10px 10px`; };

export function createPalettes({ app, state, workspace, element, button, icon, panelFrame, applyChange, rasterWorker, dismissContext, contentChanged }) {
  const chevron = up => { const glyph = icon("chevron-down"); glyph.classList.toggle("palette-chevron-up", up); return glyph; };
  const instances = new Set();
  let view = null, viewKey = "", seen = [];
  const library = (action, report) => {
    try { applyChange(app.dispatch({ type: "color", action: { op: "library", action } })); report?.(null); return true; }
    catch (error) { report?.(String(error)); return false; }
  };
  const load = () => {
    const next = app.palette_panel(), key = JSON.stringify(next, plain);
    if (key === viewKey) return false;
    view = next; viewKey = key; return true;
  };
  const worker = async (request, bytes = new Uint8Array()) =>
    rasterWorker({ operation: "palette-file", metadata: JSON.stringify(request, plain), buffers: [bytes] });
  let limits;
  function importFile(report, done) {
    limits ??= worker({ type: "limits" }).then(r => r.metadata);
    limits.then(({ read_bytes, extensions }) => {
      const input = element("input"); input.type = "file"; input.hidden = true;
      input.accept = extensions.map(e => `.${e}`).join(",");
      input.onchange = async () => {
        const file = input.files[0]; input.remove();
        if (!file) return;
        try {
          const bytes = new Uint8Array(await file.slice(0, read_bytes).arrayBuffer());
          const { metadata } = await worker({ type: "import", file_name: file.name }, bytes);
          if (library(metadata.action, report)) done();
        } catch (error) { report(String(error).replace(/^Error: /, "")); }
      };
      input.oncancel = () => input.remove();
      document.body.append(input); input.click();
    }, error => report(String(error)));
  }
  async function exportPalette(id, format, description, report) {
    const palette = state().colors.library.palettes.find(p => p.id === id);
    if (!palette) { report("Palette no longer exists"); return; }
    try {
      const { metadata, bytes } = await worker({ type: "export", palette, format });
      const extension = metadata.file_name.slice(metadata.file_name.lastIndexOf("."));
      if (window.showSaveFilePicker) {
        let handle;
        try {
          handle = await showSaveFilePicker({ suggestedName: metadata.file_name,
            types: [{ description: description || extension, accept: { "application/octet-stream": [extension] } }] });
        } catch (error) { if (error.name === "AbortError") return; throw error; }
        const writable = await handle.createWritable(); await writable.write(bytes); await writable.close();
      } else {
        const link = element("a"), url = URL.createObjectURL(new Blob([bytes], { type: "application/octet-stream" }));
        link.href = url; link.download = metadata.file_name; document.body.append(link); link.click(); link.remove();
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
      report(metadata.notice, true);
    } catch (error) { report(String(error).replace(/^Error: /, "")); }
  }
  function form({ title, text, value, confirm, destructive, check, submit }) {
    const root = element("dialog", "workspace-form palette-form"), error = element("p", "workspace-error"), footer = element("footer");
    root.append(element("h2", "", title)); root.setAttribute("aria-label", title);
    if (text) root.append(element("p", "", text));
    let input;
    if (value != null) {
      const label = element("label", "", "Name"); input = element("input"); input.value = value; input.maxLength = 64;
      input.setAttribute("aria-label", "Name"); input.dataset.paletteName = ""; label.append(input); root.append(label);
    }
    const ok = button(confirm, () => {
      const message = submit(input?.value ?? "");
      if (message) { error.textContent = message; input?.focus(); } else root.close();
    }, destructive ? "destructive-action" : "suggested-action");
    const validate = () => { const message = check?.(input.value); ok.disabled = !!message; error.textContent = message || ""; };
    if (input) { input.oninput = validate; validate(); input.onkeydown = e => { if (e.key === "Enter") { e.preventDefault(); if (!ok.disabled) ok.click(); } }; }
    footer.append(button("Cancel", () => root.close()), ok); root.append(error, footer);
    root.addEventListener("close", () => root.remove());
    document.body.append(root); root.showModal(); input?.select();
  }

  function mount(root) {
    const body = element("div", "palette-body"), normal = element("div", "palette-normal");
    const divider = () => element("hr", "palette-divider");
    const history = element("div", "palette-grid palette-history"), scroll = element("div", "palette-scroll"), grid = element("div", "palette-grid palette-swatches");
    history.title = "Recent colors — added only when used in artwork";
    scroll.append(grid); normal.append(history, divider(), panelFrame(scroll));
    const expanded = element("div", "palette-grid palette-expanded palette-cover"), chooser = element("div", "palette-chooser palette-cover");
    expanded.hidden = chooser.hidden = true;
    const search = element("input", "palette-entry palette-search"); search.type = "search"; search.placeholder = "Find a palette";
    search.setAttribute("aria-label", "Find a palette");
    const more = button("", () => {
      const r = more.getBoundingClientRect();
      more.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: r.left, clientY: r.bottom + 2 }));
    }, "palette-icon palette-library-add");
    more.append(icon("plus")); more.title = "New or import palette"; more.setAttribute("aria-label", more.title);
    more.dataset.context = "{}"; more.menuModel = () => ({ title: "Palettes", sections: app.palette_menu({ kind: "library" }) }); more.menuCommand = run;
    const searchRow = element("div", "palette-search-row"); searchRow.append(search, more);
    const list = element("div", "palette-list"), empty = element("p", "palette-no-results", "No matching palettes");
    list.setAttribute("role", "listbox"); list.setAttribute("aria-label", "Palettes"); empty.hidden = true;
    const results = panelFrame(list); results.append(empty); chooser.append(searchRow, results);
    chooser.addEventListener("mousedown", e => { if (e.target.closest("button")) e.preventDefault(); });
    body.append(normal, expanded, chooser);
    const selector = button("", e => browse(chooser.hidden, !["touch", "pen"].includes(e.pointerType)), "palette-selector"), selectorLabel = element("span", "palette-selector-label");
    selector.append(selectorLabel, chevron(true));
    const info = element("div", "palette-info"), name = button("", () => editName(), "palette-name"), nameLabel = element("span");
    name.append(nameLabel);
    const editor = element("input", "palette-entry palette-editor"); editor.maxLength = 64; editor.hidden = true; editor.setAttribute("aria-label", "Color name");
    const detail = element("span", "palette-detail");
    detail.title = "sRGB hex preview; saved colors retain their original color space, alpha and HDR intensity";
    info.append(name, editor, detail);
    const footer = element("div", "palette-footer"); footer.append(selector, info);
    const note = element("p", "palette-message"); note.hidden = true; note.setAttribute("role", "status");
    const add = button("", addColor, "palette-tile palette-add");
    add.append(icon("plus")); add.title = "Add current color to this palette"; add.setAttribute("aria-label", add.title);
    root.classList.add("palettes-panel");
    root.replaceChildren(body, divider(), footer, note);

    const tiles = new Map();
    let selected = null, editing = false, drag = null, columns = 6, historyKey, choicesKey, currentKey, swatchCount = -1, rendered = false, layoutFrame = 0;
    const current = () => app.color_panel().definition;
    const selectedSwatch = () => view.swatches.find(s => s.id === selected);
    function report(text, notice = false) {
      note.textContent = text || ""; note.hidden = !text; note.classList.toggle("notice", !!notice);
      editor.classList.toggle("error", !!text && !notice);
    }
    function browse(show, typing = true) {
      if (show && editing) { commitName(); if (editing) return; }
      expanded.hidden = true; chooser.hidden = !show; normal.inert = show;
      if (!show) { selector.focus(); return; }
      filter();
      (typing ? search : list.querySelector(".palette-choice[aria-selected=true]") ?? search).focus();
    }
    function expand(show) {
      chooser.hidden = true; expanded.hidden = !show; normal.inert = show; layoutGrids();
      (show ? expanded : history).lastElementChild?.focus();
    }
    function filter() {
      const query = search.value.trim().toLowerCase();
      let visible = 0;
      for (const row of list.children) { row.hidden = !row.dataset.name.toLowerCase().includes(query); visible += !row.hidden; }
      empty.hidden = visible > 0;
    }
    search.addEventListener("input", filter);
    function showLabel() { editor.hidden = true; name.hidden = false; }
    function editName() {
      editing = true;
      const pending = state().colors.library.pending_name;
      editor.value = selectedSwatch()?.name ?? (pending?.[1] === view.color_name ? pending[1] : "");
      name.hidden = true; editor.hidden = false; editor.focus(); editor.select();
    }
    function commitName() {
      if (!editing) return;
      editing = false;
      const action = selected != null ? { op: "rename", id: selected, name: editor.value } : { op: "name_current", color: current(), name: editor.value };
      if (library(action, report)) showLabel(); else editing = true;
    }
    function cancelName() { editing = false; showLabel(); report(null); name.focus(); }
    editor.addEventListener("keydown", e => {
      if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); commitName(); if (!editing) name.focus(); }
    });
    editor.addEventListener("focusout", () => { if (editing && !editor.hidden) commitName(); });
    function addColor() {
      if (editing) { commitName(); if (editing) return; }
      if (library({ op: "store", palette: view.palette, name: "", color: current() }, report)) {
        selected = view.swatches.at(-1)?.id ?? null; sync();
      }
    }
    function run(command, item) {
      const paletteName = id => view.palettes.find(p => p.id === id)?.name;
      switch (command.command) {
        case "new_palette": case "rename_palette": {
          const id = command.command === "rename_palette" ? command.id : null;
          if (id != null && paletteName(id) == null) return;
          const action = value => id != null ? { op: "rename_palette", id, name: value } : { op: "create_palette", name: value };
          form({ title: id != null ? "Rename Palette" : "New Palette", value: id != null ? paletteName(id) : "", confirm: "Save",
            check: value => app.palette_action_error(action(value)),
            submit: value => { let failure = null; if (library(action(value), text => { failure = text; })) browse(false); return failure; } });
          return;
        }
        case "import_palette": importFile(report, () => browse(false)); return;
        case "export_palette": exportPalette(command.id, command.format, item?.label, report); return;
        case "remove_palette": {
          const title = paletteName(command.id); if (title == null) return;
          form({ title: "Remove Palette?", text: `Remove “${title}” and its saved colors?`, confirm: "Remove", destructive: true,
            submit: () => { let failure = null; library({ op: "remove_palette", id: command.id }, text => { failure = text; }); return failure; } });
          return;
        }
        case "rename_color":
          if (!view.swatches.some(s => s.id === command.id)) return;
          selected = command.id; library({ op: "use", id: command.id }, report); editName(); return;
        case "library": library(command.action, report); return;
      }
    }
    function tile(className = "palette-tile") {
      const node = button("", () => {}, className), patch = element("span", "palette-paint");
      node.append(patch); return { node, patch };
    }
    function swatchTile(id) {
      const record = tile(), { node } = record;
      node.dataset.id = String(id); node.dataset.context = "{}";
      node.menuModel = () => ({ title: record.swatch?.name ?? "", sections: app.palette_menu({ kind: "color", id }) });
      node.menuCommand = run;
      node.addEventListener("click", e => {
        if (node.suppressClick) { node.suppressClick = false; e.preventDefault(); return; }
        if (editing) { commitName(); if (editing) return; }
        selected = id; library({ op: "use", id }, report);
      });
      node.addEventListener("pointerdown", e => press(e, record, id));
      node.addEventListener("workspace-context-claimed", e => {
        if (drag?.node !== node) return;
        if (drag.started) e.preventDefault(); else drag.held = true;
      });
      return record;
    }
    function historyTiles(target, expandedGrid) {
      const nodes = view.history.map(entry => {
        const { node, patch } = tile(); paint(patch, entry.rgba); node.title = entry.detail; node.setAttribute("aria-label", entry.detail);
        node.classList.add("palette-recent");
        node.onclick = () => {
          if (editing) { commitName(); if (editing) return; }
          selected = null;
          try { applyChange(app.dispatch({ type: "color", action: { op: "definition", color: entry.color } })); report(null); } catch (error) { report(String(error)); }
        };
        return node;
      });
      if (!nodes.length) for (let i = 0; i < 5; i++) { const node = element("div", "palette-empty"); node.title = "Colors appear here after painting"; nodes.push(node); }
      const toggle = button("", () => expand(!expandedGrid), "palette-toggle");
      toggle.append(chevron(expandedGrid));
      toggle.title = expandedGrid ? "Collapse color history" : "Expand color history"; toggle.setAttribute("aria-label", toggle.title);
      toggle.dataset.paletteHistory = expandedGrid ? "collapse" : "expand";
      target.replaceChildren(...nodes, toggle);
    }
    function layoutGrids() {
      if (!body.clientWidth) return;
      columns = Math.max(1, getComputedStyle(grid).gridTemplateColumns.split(" ").length);
      for (const [target, rows] of [[history, 1], [expanded, Math.min(4, Math.max(1, Math.floor((expanded.clientHeight + GAP) / CELL)))]]) {
        if (target === expanded && target.hidden) continue;
        const children = [...target.children], capacity = rows * columns;
        children.forEach((node, index) => {
          const last = index === children.length - 1;
          if (last) node.style.gridArea = `${rows} / -2 / ${rows} / -1`;
          else if (node.hidden !== index >= capacity - 1) node.hidden = index >= capacity - 1;
        });
      }
    }
    const resize = new ResizeObserver(() => { cancelAnimationFrame(layoutFrame); layoutFrame = requestAnimationFrame(layoutGrids); });
    resize.observe(body);
    function choices() {
      list.replaceChildren(...view.palettes.map(palette => {
        const row = button("", () => {
          if (editing) { commitName(); if (editing) return; }
          if (library({ op: "select_palette", id: palette.id }, report)) browse(false);
        }, "palette-choice");
        row.dataset.paletteId = String(palette.id); row.dataset.name = palette.name; row.title = palette.name;
        row.setAttribute("role", "option"); row.setAttribute("aria-selected", String(palette.active));
        row.dataset.context = "{}"; row.menuModel = () => ({ title: palette.name, sections: app.palette_menu({ kind: "palette", id: palette.id }) }); row.menuCommand = run;
        const strip = element("span", "palette-strip");
        for (const rgba of palette.preview) { const cell = element("span"); paint(cell, rgba); strip.append(cell); }
        row.append(element("span", "palette-choice-name", palette.name), strip);
        return row;
      }));
      filter();
    }
    function sync() {
      const s = selectedSwatch();
      for (const [key, record] of tiles) record.node.classList.toggle("selected", key === String(selected));
      const label = s ? s.name : view.color_name;
      if (nameLabel.textContent !== label) nameLabel.textContent = label;
      name.title = `${label} · Click to rename`;
    }
    function render(changed) {
      if (!view || (!changed && rendered)) return;
      rendered = true;
      if (drag && (view.palette !== drag.palette || !view.swatches.some(s => s.id === drag.id))) finish(true);
      const nextCurrent = JSON.stringify([view.color_detail, view.swatches.map(s => s.current)]);
      const nextChoices = JSON.stringify(view.palettes, plain);
      if (editing && (nextCurrent !== currentKey || nextChoices !== choicesKey)) { editing = false; showLabel(); report(null); }
      currentKey = nextCurrent;
      if (!view.swatches.some(s => s.id === selected && s.current)) selected = view.swatches.find(s => s.current)?.id ?? null;
      const live = new Set(), order = [];
      for (const swatch of view.swatches) {
        const key = String(swatch.id); live.add(key);
        let record = tiles.get(key);
        if (!record) { record = swatchTile(swatch.id); tiles.set(key, record); }
        if (record.swatch?.detail !== swatch.detail || record.swatch?.rgba.join() !== swatch.rgba.join()) {
          paint(record.patch, swatch.rgba); record.node.title = swatch.detail; record.node.setAttribute("aria-label", swatch.detail);
        }
        record.swatch = swatch; order.push(record.node);
      }
      for (const [key, record] of tiles) if (!live.has(key)) { record.node.remove(); tiles.delete(key); }
      order.push(add);
      const focused = grid.contains(document.activeElement) ? document.activeElement : null;
      order.forEach((node, index) => { if (grid.children[index] !== node) grid.insertBefore(node, grid.children[index] ?? null); });
      if (focused?.isConnected && document.activeElement !== focused) focused.focus({ preventScroll: true });
      add.disabled = name.disabled = !view.can_name;
      const nextHistory = JSON.stringify(view.history, plain);
      if (nextHistory !== historyKey) { historyKey = nextHistory; historyTiles(history, false); historyTiles(expanded, true); }
      if (nextChoices !== choicesKey) { choicesKey = nextChoices; choices(); }
      if (selectorLabel.textContent !== view.name) selectorLabel.textContent = view.name;
      selector.title = `Choose a palette · ${view.name}`;
      if (detail.textContent !== view.color_detail) detail.textContent = view.color_detail;
      sync(); layoutGrids();
      if (swatchCount !== view.swatches.length) { swatchCount = view.swatches.length; contentChanged("palettes"); }
    }

    function hit([x, y]) {
      if (!chooser.hidden || !expanded.hidden) return null;
      const clip = scroll.getBoundingClientRect(), g = grid.getBoundingClientRect();
      if (x < clip.left || x >= clip.right || y < clip.top || y >= clip.bottom || x < g.left || x >= g.right || y < g.top) return null;
      const index = Math.floor((y - g.top) / CELL) * columns + Math.floor((x - g.left) * columns / (g.width + GAP));
      return index < grid.children.length ? index : null;
    }
    function update() {
      const d = drag;
      d.ghost.style.left = `${d.point[0] - d.grab[0]}px`; d.ghost.style.top = `${d.point[1] - d.grab[1]}px`;
      const slot = hit(d.point);
      if (slot === d.slot) return;
      d.slot = slot;
      const original = view.swatches.findIndex(s => s.id === d.id);
      const preview = app.palette_reorder_preview(d.palette, d.id, slot ?? original);
      if (!preview) return;
      d.action = slot == null ? null : preview.action;
      const cells = [...grid.children], cell = (grid.clientWidth + GAP) / columns;
      const position = index => [Math.round((index % columns) * cell), Math.floor(index / columns) * CELL];
      preview.order.forEach((id, target) => {
        const node = tiles.get(String(id))?.node;
        if (!node) return;
        const [x0, y0] = position(cells.indexOf(node)), [x1, y1] = position(target);
        node.style.transform = x1 !== x0 || y1 !== y0 ? `translate(${x1 - x0}px,${y1 - y0}px)` : "";
      });
    }
    function autoscroll(now) {
      const d = drag;
      if (!d?.started) return;
      const elapsed = d.frameTime ? Math.min(50, now - d.frameTime) / 1000 : 0, clip = scroll.getBoundingClientRect(), [x, y] = d.point;
      d.frameTime = now;
      if (x >= clip.left && x <= clip.right) {
        const delta = y < clip.top + 20 && y >= clip.top - 12 ? -240 * elapsed : y > clip.bottom - 20 && y <= clip.bottom + 12 ? 240 * elapsed : 0;
        d.scroll += delta;
        const step = Math.trunc(d.scroll);
        if (step) {
          d.scroll -= step;
          const before = scroll.scrollTop; scroll.scrollTop += step;
          if (scroll.scrollTop !== before) { d.slot = undefined; update(); }
        }
      }
      d.frame = requestAnimationFrame(autoscroll);
    }
    function press(e, record, id) {
      if (e.button !== 0 || !e.isPrimary || drag) return;
      record.node.suppressClick = false;
      const r = record.node.getBoundingClientRect();
      drag = { node: record.node, record, id, palette: view.palette, pointer: e.pointerId, origin: [e.clientX, e.clientY], point: [e.clientX, e.clientY],
        grab: [e.clientX - r.left, e.clientY - r.top], started: false, held: false, slot: undefined, action: null, scroll: 0 };
      for (const type of ["pointermove", "pointerup", "pointercancel"]) window.addEventListener(type, contact, true);
      record.node.addEventListener("lostpointercapture", lost);
    }
    function lost(e) { if (drag?.started && e.pointerId === drag.pointer) finish(true); }
    function contact(e) {
      const d = drag;
      if (!d || e.pointerId !== d.pointer) return;
      if (e.type === "pointerup") { d.point = [e.clientX, e.clientY]; if (d.started) update(); finish(false); return; }
      if (e.type === "pointercancel") { finish(true); return; }
      d.point = [e.clientX, e.clientY];
      if (!d.started) {
        if (Math.hypot(d.point[0] - d.origin[0], d.point[1] - d.origin[1]) <= SLOP) return;
        start(d);
      }
      e.preventDefault(); update();
    }
    function start(d) {
      d.started = true; dismissContext();
      try { d.node.setPointerCapture(d.pointer); } catch {}
      const r = d.node.getBoundingClientRect();
      d.ghost = element("div", "palette-drag-ghost"); d.ghost.classList.toggle("selected", d.node.classList.contains("selected"));
      d.ghost.style.width = `${r.width}px`; d.ghost.style.height = `${r.height}px`;
      const patch = element("span", "palette-paint"); patch.style.background = d.record.patch.style.background; d.ghost.append(patch);
      workspace.append(d.ghost);
      d.node.classList.add("palette-drag-source"); grid.classList.add("palette-reordering"); root.classList.add("palette-dragging");
      d.frame = requestAnimationFrame(autoscroll);
    }
    function finish(cancel) {
      const d = drag;
      if (!d) return;
      drag = null;
      for (const type of ["pointermove", "pointerup", "pointercancel"]) window.removeEventListener(type, contact, true);
      d.node.removeEventListener("lostpointercapture", lost);
      cancelAnimationFrame(d.frame);
      if (d.started) {
        d.ghost.remove(); grid.classList.remove("palette-reordering"); root.classList.remove("palette-dragging");
        d.node.classList.remove("palette-drag-source");
        for (const record of tiles.values()) record.node.style.transform = "";
        d.node.suppressClick = true;
      }
      try { if (d.node.hasPointerCapture(d.pointer)) d.node.releasePointerCapture(d.pointer); } catch {}
      if (d.started || cancel) dismissContext();
      if (!cancel && d.started && d.action) { library(d.action, report); d.node.focus({ preventScroll: true }); }
    }
    const escape = e => { if (e.key === "Escape" && drag) { finish(true); e.preventDefault(); e.stopPropagation(); } };
    const blur = () => finish(true);
    window.addEventListener("keydown", escape, true);
    window.addEventListener("blur", blur);
    root.addEventListener("keydown", e => {
      if (e.key === "Escape") {
        if (editing) cancelName();
        else if (!chooser.hidden) browse(false);
        else if (!expanded.hidden) expand(false);
        else return;
        e.preventDefault(); e.stopPropagation(); return;
      }
      if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
        const target = e.target.closest("[data-context]");
        if (!target) return;
        const r = target.getBoundingClientRect();
        e.preventDefault(); e.stopPropagation();
        target.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: r.left + 4, clientY: r.bottom }));
        return;
      }
      if ((e.ctrlKey || e.metaKey) && ["z", "Z", "y"].includes(e.key) && !e.target.matches("input,textarea")) {
        const redo = e.key === "y" || e.shiftKey;
        if (redo ? view.can_redo : view.can_undo) library({ op: redo ? "redo_reorder" : "undo_reorder", palette: view.palette }, report);
        e.preventDefault(); e.stopPropagation(); return;
      }
      if ([" ", "Enter"].includes(e.key)) e.stopPropagation();
    });
    root.addEventListener("keyup", e => { if ([" ", "Enter"].includes(e.key)) e.stopPropagation(); });
    const instance = {
      render,
      refresh() { render(false); },
      dispose() {
        finish(true); instances.delete(instance); resize.disconnect(); cancelAnimationFrame(layoutFrame);
        window.removeEventListener("keydown", escape, true); window.removeEventListener("blur", blur);
      },
    };
    instances.add(instance);
    const changed = load();
    for (const other of instances) other.render(changed);
    return instance;
  }
  return {
    mount,
    refresh(regions) {
      const { colors, layer_tools } = state();
      if (!instances.size || (!(regions & 20) && colors === seen[0] && layer_tools === seen[1])) return;
      seen = [colors, layer_tools];
      const changed = load();
      for (const instance of instances) instance.render(changed);
    },
  };
}
