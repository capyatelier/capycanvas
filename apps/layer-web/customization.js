// DOM presentation of the shared Rust customization models. This module owns
// widgets and animation, not catalogs, validation, selection or docking policy.
export function createCustomization({ app, catalog, state, workspace, panels, groups,
  element, button, icon, numberField, panelFrame,
  dispatch, draggable, grip, place, updateZen }) {
  const send = (action) => dispatch({ type: "customize", action });
  const views = new Map(), fields = new Map();
  const tileResize = new ResizeObserver(entries => {
    for (const { target: strip } of entries) {
      if (!strip.isConnected || !strip.dataset.axis) continue;
      layoutTiles(strip, app.panel_tiles(strip.dataset.panel, strip.clientWidth, strip.clientHeight,
        strip.dataset.axis, strip.dataset.standalone === "true"));
    }
  });
  let anchor = [320, 120], expanded = null, animation = 0, popupControl = null;
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const context = element("div", "panel-context-menu");
  // A context menu opens during a press/hold, before release. Automatic
  // popovers light-dismiss that same release as an outside click in Chromium.
  context.popover = "manual"; context.setAttribute("role", "menu"); workspace.append(context);
  const popup = element("div", "tile-popover"); popup.popover = "auto"; workspace.append(popup);
  context.addEventListener("toggle", updateZen);
  window.addEventListener("pointerdown", (e) => {
    if (context.matches(":popover-open") && !context.contains(e.target)) context.hidePopover();
  }, { capture: true });
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && context.matches(":popover-open")) {
      context.hidePopover(); e.preventDefault(); e.stopPropagation();
    }
  });
  popup.addEventListener("toggle", () => {
    if (!popup.matches(":popover-open") && state().customization.control) send({ type: "close_control" });
    updateZen();
  });
  const target = (node, value) => { node.dataset.context = JSON.stringify(value); return node; };
  // The same recursive Rust menu drives both the header and contextual menus.
  // Submenus replace their parent page, as in GTK's sliding popover menus.
  function renderMenu(container, model, close, parents = []) {
    container.classList.add("workspace-menu-items");
    container.setAttribute("aria-label", model.title);
    container.replaceChildren();
    if (parents.length) {
      const back = button(model.title, () => {
        const previous = parents.at(-1);
        renderMenu(container, previous, close, parents.slice(0, -1));
      }, "submenu-back");
      back.prepend(icon("down")); container.append(back, element("hr"));
    }
    model.sections.filter(section => section.length).forEach((section, index) => {
      if (index) container.append(element("hr"));
      for (const item of section) {
        const submenu = item.sections?.some(section => section.length);
        const row = button("", () => {
          if (submenu) renderMenu(container, { title: item.label, sections: item.sections }, close, [...parents, model]);
          else { close(); if (item.action) dispatch(item.action); }
        });
        row.disabled = !item.enabled;
        row.setAttribute("role", item.selected == null ? "menuitem" : "menuitemcheckbox");
        if (item.selected != null) row.setAttribute("aria-checked", item.selected);
        const mark = element("span", "menu-check");
        if (item.selected) mark.append(icon("check"));
        row.append(mark, element("span", "menu-label", item.label));
        if (item.hint) row.append(element("span", "shortcut-hint", item.hint));
        if (submenu) { const arrow = icon("down"); arrow.classList.add("submenu-arrow"); row.append(arrow); }
        container.append(row);
      }
    });
    if (container === context && context.matches(":popover-open")) positionPopup(context);
  }
  function showContext(node, point) {
    const model = node.layerMenu ? node.layerMenu() : app.context_menu(JSON.parse(node.dataset.context));
    anchor = point;
    renderMenu(context, model, () => context.hidePopover());
    context.showPopover(); positionPopup(context);
  }
  function positionPopup(node) {
    const r = node.getBoundingClientRect();
    node.style.left = `${Math.max(6, Math.min(anchor[0], innerWidth - r.width - 6))}px`;
    node.style.top = `${Math.max(6, Math.min(anchor[1], innerHeight - r.height - 6))}px`;
  }
  function contextTarget(node) {
    if (node.closest("input,select,textarea,[contenteditable=true],.scroll-thumb")) return null;
    return node.closest("[data-context]");
  }
  workspace.addEventListener("contextmenu", (e) => {
    const node = contextTarget(e.target);
    if (!node) return;
    e.preventDefault(); showContext(node, [e.clientX, e.clientY]);
  });
  // Touch long press supplies the browser's missing context-menu gesture. A
  // scroll, drag or cancelled contact cancels it; never claim input editing.
  let hold, heldPointer = null;
  const cancelHold = () => { if (hold) clearTimeout(hold.timer); hold = null; };
  workspace.addEventListener("pointerdown", (e) => {
    cancelHold(); heldPointer = null;
    const node = contextTarget(e.target);
    if (e.pointerType !== "touch" || !node) return;
    hold = { x: e.clientX, y: e.clientY, timer: setTimeout(() => {
      heldPointer = e.pointerId; cancelHold();
      node.dispatchEvent(new Event("workspace-context-claimed", { bubbles: true }));
      showContext(node, [e.clientX, e.clientY]);
    }, 500) };
  });
  workspace.addEventListener("pointermove", (e) => {
    if (hold && Math.hypot(e.clientX - hold.x, e.clientY - hold.y) > 8) cancelHold();
  }, { capture: true });
  for (const event of ["pointerup", "pointercancel", "dragstart", "scroll"])
    workspace.addEventListener(event, cancelHold, { capture: true });
  window.addEventListener("blur", cancelHold);
  workspace.addEventListener("click", (e) => {
    if (heldPointer === e.pointerId) { heldPointer = null; e.preventDefault(); e.stopImmediatePropagation(); }
  }, { capture: true });

  // One field factory for extra compact controls, the drawer and scalar popup.
  function field(control, label) {
    const row = element("div", "panel-field"); row.dataset.control = control;
    let input, sync = () => {};
    const range = (value, action) => {
      input = numberField(catalog.opacity, label, value => dispatch(action(value)));
      sync = () => input.update(value()); row.append(input);
    };
    switch (control) {
      case "brush_size":
        input = numberField(catalog.brush_size, label, value => dispatch({ type: "set_brush_size", value }));
        row.append(input);
        sync = () => input.update(state().brush.diameter); break;
      case "brush_opacity":
        range(() => state().brush.opacity, (value) => ({ type: "set_brush_opacity", value })); break;
      case "layer_opacity":
        range(() => state().layers.find((l) => l.selected).opacity,
          (opacity) => ({ type: "set_layer_opacity", opacity })); break;
      case "brush_color":
        input = element("input"); input.type = "color";
        input.addEventListener("input", () => dispatch({ type: "set_color",
          rgba: [1, 3, 5].map((i) => parseInt(input.value.slice(i, i + 2), 16) / 255).concat(1) }));
        sync = () => { input.value = hexColor(state().brush.color); }; row.append(input); break;
      case "brushes":
      case "layers": {
        input = element("select");
        input.addEventListener("change", () => dispatch(control === "brushes"
          ? { type: "select_brush", id: Number(input.value) }
          : { type: "select_layer", id: state().layers[Number(input.value)].id }));
        sync = () => {
          const choices = control === "brushes"
            ? catalog.brush_categories.flatMap((c) => c.brushes).map((b) => [b.id, b.label])
            : state().layers.map((l, i) => [i, l.label]);
          const key = JSON.stringify(choices);
          if (input.dataset.key !== key) {
            input.dataset.key = key;
            input.replaceChildren(...choices.map(([id, label]) => {
              const option = element("option", "", label); option.value = id; return option;
            }));
          }
          input.value = control === "brushes" ? state().brush.preset : state().layers.findIndex((l) => l.selected);
        }; row.append(input); break;
      }
      case "size_presets":
        row.classList.add("configuration-presets");
        for (const value of catalog.brush_sizes) row.append(button(String(value), () => dispatch({ type: "set_brush_size", value })));
        break;
      case "layer_actions":
        row.classList.add("configuration-presets");
        for (const id of catalog.layer_commands) {
          const node = button("", () => dispatch({ type: "invoke", command: id }));
          node.dataset.command = id; row.append(node);
        }
        sync = () => { for (const node of row.children) {
          const command = state().commands.find((c) => c.id === node.dataset.command);
          node.textContent = command.label; node.disabled = !command.enabled;
        } }; break;
      case "adjustments":
      case "properties":
      case "stats":
        // These are complete schema-driven views, not duplicated scalar fields.
        row.append(element("span", "", label)); break;
      default: throw new Error(`Unknown panel control: ${control}`);
    }
    if (input) input.setAttribute("aria-label", label);
    fields.set(row, sync); sync(); return row;
  }
  function discardFields(node) {
    for (const row of fields.keys()) if (node.contains(row)) fields.delete(row);
  }
  function refreshPanels() {
    views.clear();
    for (const config of state().workspace.layout.panels) {
      const view = app.panel_view(config.id); views.set(config.id, view);
      let panel = panels.get(config.id);
      if (!panel) {
        panel = element("div", "panel tile-panel"); panelFrame(panel, false); panels.set(config.id, panel);
      }
      const toolbar = config.content.kind === "toolbar";
      target(panel, { kind: toolbar ? "ribbon" : "panel", panel: config.id });
      if (toolbar) {
        const key = JSON.stringify([config.content.tiles, view.tile_style]);
        if (panel.dataset.tiles !== key) {
          panel.dataset.tiles = key;
          const old = panel.querySelector(".toolbar-controls"); if (old) tileResize.unobserve(old);
          const strip = element("div", "toolbar-controls");
          strip.dataset.panel = config.id;
          strip.dataset.tileStyle = view.tile_style;
          for (const tile of view.tiles) {
            // Like GTK, drag/context target surrounds the command button. A
            // disabled command remains movable and removable.
            const tileRoot = element("div", "tile-button tool-tile");
            tileRoot.dataset.tile = tile.id;
            const node = button("", () => {
              const r = tileRoot.getBoundingClientRect(); anchor = [r.x, r.bottom + 6];
              dispatch({ type: "activate_tile", panel: config.id, tile: tile.id });
            });
            if (tile.control.kind === "command") { node.dataset.command = tile.control.command; node.dataset.icon = "true"; }
            const glyph = icon(tile.icon);
            glyph.style.width = glyph.style.height = `${view.tile_style === "large" ? 32 : 16}px`;
            node.append(glyph);
            if (view.tile_style === "labeled") node.append(element("span", "tile-label", tile.label));
            tileRoot.append(node);
            target(tileRoot, { kind: "tile", panel: config.id, tile: tile.id });
            strip.append(draggable(tileRoot, { kind: "tile", panel: config.id, tile: tile.id }));
          }
          strip.append(grip({ kind: "panel", panel: config.id }));
          panel.replaceChildren(strip);
          tileResize.observe(strip);
        }
        for (const tile of view.tiles) {
          const node = panel.querySelector(`[data-tile="${tile.id}"] > button`);
          node.disabled = !tile.enabled; node.title = tile.tooltip;
          node.setAttribute("aria-label", tile.label); node.setAttribute("aria-pressed", tile.selected);
        }
      } else {
        for (const control of view.controls) {
          let row = panel.querySelector(`[data-control="${control.control}"]`);
          if (!row) { row = field(control.control, control.label); if (!row.querySelector(".number-control")) row.prepend(element("label", "", control.label)); panel.append(row); }
          row.hidden = !control.visible_in_panel;
        }
      }
    }
    for (const [id, panel] of panels) if (!views.has(id)) {
      const strip = panel.querySelector(".toolbar-controls"); if (strip) tileResize.unobserve(strip);
      discardFields(panel); panel.parentElement.remove(); panels.delete(id);
    }
  }

  const picker = element("dialog", "tool-picker"); picker.id = "tool-picker";
  const pickerHeader = element("header", "dialog-header"), pickerTitle = element("h2");
  const cancel = () => send({ type: "cancel_tools" });
  const confirm = button("", () => send({ type: "confirm_tools" }), "suggested-action");
  confirm.id = "confirm-tools"; pickerHeader.append(button("Cancel", cancel), pickerTitle, confirm);
  const pickerBody = element("div", "tool-picker-body"), nameRow = element("label", "toolbar-name");
  const nameLabel = element("span"), name = element("input"); name.id = "toolbar-name";
  name.addEventListener("input", () => send({ type: "picker_name", name: name.value })); nameRow.append(nameLabel, name);
  const search = element("input", "preferences-search"); search.type = "search"; search.id = "tool-search";
  search.addEventListener("input", () => send({ type: "picker_search", query: search.value }));
  const searchRow = element("div", "tool-search"); searchRow.append(icon("search"), search);
  const choices = element("div", "tool-choices"), error = element("p", "preferences-error"), count = element("span", "tool-count");
  pickerBody.append(nameRow, searchRow, panelFrame(choices), error, count); picker.append(pickerHeader, pickerBody); workspace.append(picker);
  picker.addEventListener("cancel", (e) => { e.preventDefault(); cancel(); });
  picker.addEventListener("close", () => { if (app.tool_picker()) cancel(); });
  function refreshPicker() {
    const model = app.tool_picker();
    if (!model) { if (picker.open) picker.close(); return; }
    pickerTitle.textContent = model.title; picker.setAttribute("aria-label", model.title);
    nameRow.hidden = model.name == null; nameLabel.textContent = model.name_label;
    name.setAttribute("aria-label", model.name_label);
    if (model.name != null && name.value !== model.name) name.value = model.name;
    search.placeholder = model.search_hint; search.setAttribute("aria-label", model.search_hint);
    if (search.value !== model.query) search.value = model.query;
    count.textContent = `${model.selected_count} selected`;
    error.textContent = model.error || ""; error.hidden = !model.error;
    confirm.textContent = model.confirm_label; confirm.disabled = !model.can_confirm;
    const key = JSON.stringify(model.choices.map(({ selected, ...c }) => c));
    if (choices.dataset.key !== key) {
      choices.dataset.key = key;
      choices.replaceChildren(...model.choices.map((choice) => {
        const row = element("label", "tool-choice"), text = element("span");
        text.append(element("span", "", choice.label), element("small", "", choice.description));
        const check = element("input", "panel-check"); check.type = "checkbox";
        check.addEventListener("change", () => send({ type: "picker_select", control: choice.control, selected: check.checked }));
        row.append(icon(choice.icon), text, check); return row;
      }));
    }
    model.choices.forEach((choice, index) => { choices.children[index].querySelector("input").checked = choice.selected; });
    if (!picker.open) { picker.showModal(); (model.name == null ? search : name).focus(); }
  }

  const manager = element("dialog", "toolbar-manager"); manager.id = "toolbar-manager";
  const managerHeader = element("header", "dialog-header"), managerTitle = element("h2");
  const closeManager = () => send({ type: "close_toolbar_manager" });
  const managerClose = button("×", closeManager, "dialog-close");
  managerHeader.append(managerTitle, managerClose);
  const managerBody = element("div", "toolbar-manager-body"), managerDescription = element("p");
  const managerScroll = element("div", "toolbar-manager-scroll"), managerList = element("div", "managed-toolbars"), managerEmpty = element("p", "toolbar-manager-empty");
  const managerDelete = button("", () => { const action = app.toolbar_manager()?.delete_action; if (action) send(action); }, "destructive-action");
  managerDelete.id = "delete-managed-toolbar";
  managerScroll.append(managerList, managerEmpty);
  managerBody.append(managerDescription, managerScroll, managerDelete);
  manager.append(managerHeader, managerBody); workspace.append(manager);
  manager.addEventListener("cancel", e => { e.preventDefault(); closeManager(); });
  manager.addEventListener("close", () => { if (app.toolbar_manager()) closeManager(); });
  function refreshManager() {
    const model = app.toolbar_manager();
    if (!model) { if (manager.open) manager.close(); return; }
    managerTitle.textContent = model.title; manager.setAttribute("aria-label", model.title);
    managerClose.setAttribute("aria-label", model.close_label);
    managerDescription.textContent = model.description;
    managerEmpty.textContent = model.empty_label; managerEmpty.hidden = model.toolbars.length > 0;
    managerList.hidden = !model.toolbars.length;
    managerDelete.textContent = model.delete_label; managerDelete.disabled = !model.delete_action;
    const key = JSON.stringify(model.toolbars);
    if (managerList.dataset.key !== key) {
      managerList.dataset.key = key;
      managerList.replaceChildren(...model.toolbars.map(toolbar => {
        const row = button("", () => send({ type: "select_managed_toolbar", panel: toolbar.panel }));
        row.dataset.panel = toolbar.panel;
        const text = element("span"); text.append(element("span", "", toolbar.title), element("small", "", toolbar.subtitle));
        row.append(icon(toolbar.icon), text); return row;
      }));
    }
    for (const row of managerList.children) row.setAttribute("aria-pressed", String(row.dataset.panel === model.selected));
    if (!manager.open) { manager.showModal(); managerClose.focus(); }
  }

  const prompt = element("dialog", "toolbar-prompt"), promptTitle = element("h2"), promptMessage = element("p");
  prompt.id = "toolbar-prompt";
  const promptNameRow = element("label", "toolbar-name"), promptNameLabel = element("span"), promptName = element("input");
  promptName.id = "toolbar-prompt-name";
  promptName.addEventListener("input", () => send({ type: "toolbar_name", name: promptName.value }));
  promptNameRow.append(promptNameLabel, promptName);
  const promptError = element("p", "preferences-error"), promptActions = element("div", "prompt-actions");
  const cancelPrompt = () => send({ type: "cancel_toolbar" });
  const promptCancel = button("", cancelPrompt), promptConfirm = button("", () => send({ type: "confirm_toolbar" }));
  promptConfirm.id = "confirm-toolbar"; promptActions.append(promptCancel, promptConfirm);
  prompt.append(promptTitle, promptMessage, promptNameRow, promptError, promptActions); workspace.append(prompt);
  prompt.addEventListener("cancel", e => { e.preventDefault(); cancelPrompt(); });
  prompt.addEventListener("close", () => { if (app.toolbar_prompt()) cancelPrompt(); });
  promptName.addEventListener("keydown", e => {
    if (e.key === "Enter" && !promptConfirm.disabled) { e.preventDefault(); promptConfirm.click(); }
  });
  function refreshPrompt() {
    const model = app.toolbar_prompt();
    if (!model) { if (prompt.open) prompt.close(); return; }
    promptTitle.textContent = model.title; prompt.setAttribute("aria-label", model.title);
    promptMessage.textContent = model.message; promptMessage.hidden = !model.message;
    promptNameRow.hidden = model.name == null;
    promptNameLabel.textContent = model.name_label; promptName.setAttribute("aria-label", model.name_label);
    if (model.name != null && promptName.value !== model.name) promptName.value = model.name;
    promptError.textContent = model.error || ""; promptError.hidden = !model.error;
    promptCancel.textContent = model.cancel_label; promptConfirm.textContent = model.confirm_label;
    promptConfirm.disabled = !model.can_confirm;
    promptConfirm.className = model.destructive ? "destructive-action" : "suggested-action";
    if (!prompt.open) { prompt.showModal(); if (model.name != null) { promptName.focus(); promptName.select(); } }
  }

  function clearExpansion() {
    cancelAnimationFrame(animation); animation = 0;
    if (!expanded) return;
    discardFields(expanded.configuration);
    expanded.configuration.remove(); expanded.join.remove();
    expanded.root.classList.remove("expanded-panel", "configuration-left", "configuration-right", "configuration-flush");
    expanded.root.style.zIndex = expanded.root.dataset.zIndex;
    place(expanded.root, expanded.docked);
    const preview = expanded.root.querySelector(".panel-preview");
    preview.removeAttribute("style"); expanded = null;
  }
  function measureExpansion() {
    const sizing = app.expanded_panel({ viewport: [workspace.clientWidth, workspace.clientHeight], panel: expanded.panel,
      heights: [0, 0], open: true, progress: 1 });
    // Measure wrapping at final column widths. These are presentation facts;
    // Rust computes the bounds, clamps and animation interpolation.
    const panel = panels.get(expanded.panel);
    panel.style.width = `${sizing.preview.width}px`; panel.style.height = "auto";
    const previewHeight = panel.scrollHeight + expanded.headerHeight;
    panel.style.width = ""; panel.style.height = "";
    expanded.body.style.width = `${sizing.configuration.width}px`;
    expanded.body.style.height = "auto";
    const configurationHeight = expanded.body.scrollHeight;
    expanded.body.style.height = "";
    expanded.heights = [previewHeight, configurationHeight];
  }
  function renderExpansion() {
    if (!expanded) return;
    const p = reducedMotion.matches ? 1 : Math.min(1, (performance.now() - expanded.started) / catalog.panel_expansion_ms);
    const eased = 1 - (1 - p) ** 3;
    const e = app.expanded_panel({ viewport: [workspace.clientWidth, workspace.clientHeight], panel: expanded.panel,
      heights: expanded.heights, open: expanded.open, progress: eased, from: expanded.from });
    expanded.placement = e;
    place(expanded.root, e.bounds);
    const preview = expanded.root.querySelector(".panel-preview"); place(preview, e.preview);
    place(expanded.configuration, e.configuration);
    expanded.root.classList.toggle("configuration-left", e.configuration.x < e.preview.x);
    expanded.root.classList.toggle("configuration-right", e.configuration.x >= e.preview.x);
    expanded.root.classList.toggle("configuration-flush", !expanded.headerHeight);
    expanded.join.hidden = !e.concave_join;
    place(expanded.join, { x: e.configuration.x + e.configuration.width - 8, y: e.configuration.y - 8, width: 8, height: 8 });
    const strip = panels.get(expanded.panel).querySelector(".toolbar-controls");
    if (strip) layoutTiles(strip, app.panel_tiles(expanded.panel, e.preview.width, e.preview.height - expanded.headerHeight, expanded.axis, !expanded.headerHeight));
    if (p < 1) animation = requestAnimationFrame(renderExpansion);
    else if (!expanded.open) clearExpansion();
    else expanded.from = null;
  }
  function arrange(resolved) {
    const id = state().customization.expanded;
    const group = resolved.groups.find((g) => g.panels.includes(id));
    if (!id || !group) {
      if (expanded && expanded.open) { expanded.from = expanded.placement; expanded.started = performance.now(); expanded.open = false; }
      if (expanded && !groups.has(expanded.group)) clearExpansion();
    } else if (!expanded || expanded.panel !== id || expanded.group !== group.id || expanded.root !== groups.get(group.id)) {
      const from = expanded?.group === group.id ? expanded.placement : null;
      clearExpansion();
      const view = views.get(id), root = groups.get(group.id);
      const body = element("div", "panel-configuration-body panel");
      body.append(element("h3", "", view.configuration_title), element("p", "", view.configuration_hint));
      for (const control of view.controls) {
        const row = element("div", "configuration-row"), label = element("label");
        const check = element("input", "panel-check"); check.type = "checkbox"; check.dataset.visible = control.control;
        check.checked = control.visible_in_panel;
        check.addEventListener("change", () => send({ type: "set_control_visible", panel: id, control: control.control, visible: check.checked }));
        label.append(check, document.createTextNode(control.label)); row.append(label, field(control.control, control.label)); body.append(row);
      }
      if (view.toolbar_options.length) {
        const options = element("div", "toolbar-options"); body.append(options);
        renderToolbarOptions(options, view.toolbar_options);
      }
      const configuration = panelFrame(body), join = element("div", "panel-column-join");
      configuration.classList.add("panel-configuration"); join.setAttribute("aria-hidden", "true");
      root.querySelector(".panel-columns").append(configuration, join);
      root.classList.add("expanded-panel"); root.style.zIndex = "2000";
      expanded = { panel: id, group: group.id, root, configuration, body, join, docked: group.bounds,
        open: true, headerHeight: group.tabs_visible ? resolved.tab_bar_height : 0, axis: group.axis,
        from, started: performance.now(), placement: from };
    } else {
      expanded.docked = group.bounds;
      if (!expanded.open) { expanded.from = expanded.placement; expanded.started = performance.now(); expanded.open = true; }
    }
    // Content measurement belongs to layout updates, not every animation frame.
    if (expanded) measureExpansion();
    cancelAnimationFrame(animation); renderExpansion();
  }
  function layoutTiles(strip, geometry) {
    const handle = strip.querySelector(":scope > .panel-grip"); handle.hidden = !geometry.grip;
    if (geometry.grip) place(handle, geometry.grip);
    [...strip.querySelectorAll(":scope > .tile-button")].forEach((tile, i) => place(tile, geometry.tiles[i]));
  }
  function refresh() {
    refreshPanels(); refreshPicker(); refreshManager(); refreshPrompt();
    for (const update of fields.values()) update();
    if (expanded) for (const check of expanded.body.querySelectorAll("[data-visible]")) {
      check.checked = views.get(expanded.panel)?.controls.find((c) => c.control === check.dataset.visible)?.visible_in_panel || false;
    }
    if (expanded) {
      const options = expanded.body.querySelector(".toolbar-options");
      if (options) renderToolbarOptions(options, views.get(expanded.panel)?.toolbar_options || []);
    }
    const control = state().customization.control;
    if (popupControl !== control) {
      popupControl = control; discardFields(popup); popup.replaceChildren();
      if (control) {
        const label = [...views.values()].flatMap((v) => v.controls).find((c) => c.control === control).label;
        const content = field(control, label);
        if (!content.querySelector(".number-control")) popup.append(element("label", "", label));
        popup.append(content); popup.showPopover(); positionPopup(popup);
      } else if (popup.matches(":popover-open")) popup.hidePopover();
    }
    workspace.style.setProperty("--paint-color", hexColor(state().brush.color));
  }
  function renderToolbarOptions(container, sections) {
    const key = JSON.stringify(sections);
    if (container.dataset.key === key) return;
    container.dataset.key = key;
    container.replaceChildren(...sections.filter(s => s.length).map(section => {
      const list = element("div", "toolbar-option-group");
      for (const item of section) {
        const row = button("", () => dispatch(item.action)); row.disabled = !item.enabled;
        if (item.selected != null) {
          row.setAttribute("role", "radio"); row.setAttribute("aria-checked", item.selected);
          row.append(element("span", "option-radio"));
        }
        row.append(element("span", "menu-label", item.label));
        if (item.hint) row.append(element("span", "shortcut-hint", item.hint));
        list.append(row);
      }
      return list;
    }));
  }
  return { refresh, arrange, target, renderMenu, view: (id) => views.get(id), layoutTiles,
    placement: () => expanded?.placement ?? null };
}

function hexColor(rgba) {
  return `#${rgba.slice(0, 3).map((v) => Math.round(v * 255).toString(16).padStart(2, "0")).join("")}`;
}
