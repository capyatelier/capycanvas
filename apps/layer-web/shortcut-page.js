// DOM adapter for the shortcut, modifier key and pen button views. Grouping,
// filtering, recording, conflicts and defaults are all decided in Rust.
export function createShortcutPage({ element, button, icon, send, view, scroller, settingsGroup: group, dropdown }) {
  const panes = new Map(), active = new Map(), rows = new Map();
  let keymapChoice, keymapOutdated, contextChoice, showChoice, shortcutSearch, categoryList, emptyStatus;
  let rootResults, categoryResults, modifierMain, modifierCategory, inputRoot;
  let categorySignature = "", resultsSignature = "", modifierSignature = "", triggerSignature = "";
  const categoryRows = new Map(), triggerRows = new Map(), triggerGroups = new Map(), savedScroll = new Map();
  const chevron = () => { const glyph = icon("go-next"); glyph.classList.add("row-chevron"); return glyph; };
  const flatButton = (glyph, label, onClick, id) => {
    const node = button("", onClick, "icon-button"); node.append(icon(glyph));
    node.setAttribute("aria-label", label); node.title = label; if (id) node.id = id; return node;
  };
  const navRow = (label, subtitle, value, onClick) => {
    const row = button("", onClick, "preference-row nav-row");
    const text = element("span", "preference-text"); text.append(element("span", "", label));
    const detail = element("p", "", subtitle); detail.hidden = !subtitle; text.append(detail);
    const hint = element("span", "shortcut-hint", value);
    row.append(text, hint, chevron());
    row.update = (nextLabel, nextSubtitle, nextValue) => {
      text.firstChild.textContent = nextLabel; detail.textContent = nextSubtitle; detail.hidden = !nextSubtitle; hint.textContent = nextValue;
    };
    return row;
  };

  // Slide pages: only the visible pane is laid out, and it slides in from the
  // side it was opened toward.
  function pane(pageId, name, node) {
    const element_ = element("div", "settings-pane"); element_.dataset.pane = name;
    node.append(element_);
    if (!panes.has(pageId)) panes.set(pageId, new Map());
    panes.get(pageId).set(name, element_);
    return element_;
  }
  function showPane(pageId, name, depth) {
    const current = active.get(pageId);
    if (current?.name === name) return;
    const previous = current && panes.get(pageId).get(current.name);
    if (previous) savedScroll.set(`${pageId}:${current.name}`, scroller.scrollTop);
    for (const [key, node] of panes.get(pageId)) if (node.hidden !== (key !== name)) node.hidden = key !== name;
    const next = panes.get(pageId).get(name);
    active.set(pageId, { name, depth });
    if (!current) return;
    const forward = depth > current.depth;
    scroller.scrollTop = forward ? 0 : savedScroll.get(`${pageId}:${name}`) ?? 0;
    if (!matchMedia("(prefers-reduced-motion: reduce)").matches)
      next.animate([{ transform: `translateX(${forward ? 24 : -24}%)`, opacity: 0 }, { transform: "none", opacity: 1 }], { duration: 200, easing: "cubic-bezier(.2,.8,.2,1)" });
  }

  function recordingRow(capture) {
    const row = element("div", "preference-row recording");
    row.id = "shortcut-recording"; row.tabIndex = -1;
    const glyph = icon(capture.existing ? "info" : capture.notice ? "info" : "keyboard");
    if (capture.notice && !capture.existing) glyph.classList.add("warning");
    const text = element("span", "preference-text"); text.append(element("span", "", capture.shortcut));
    if (capture.notice) text.append(element("p", "", capture.notice));
    const cancel = button("Cancel", () => send({ type: "cancel_shortcut" })); cancel.id = "cancel-shortcut";
    const replace = !!capture.conflict;
    const confirm = button(capture.existing ? "Open" : replace ? "Reassign" : "Add", () => send({ type: "confirm_shortcut", replace }), "suggested-action");
    confirm.id = "confirm-shortcut"; confirm.disabled = !capture.chord || !!capture.error;
    row.append(glyph, text, cancel, confirm);
    return row;
  }

  function sheet(id, onDismiss) {
    const node = element("dialog", "sheet"); node.id = id;
    const header = element("header", "dialog-header"), title = element("h2");
    const exit = button("", () => onDismiss(), "dialog-close"); exit.append(icon("window-close"));
    exit.id = `${id}-close`; exit.setAttribute("aria-label", "Close"); exit.title = "Close";
    header.append(title, exit);
    const body = element("div", "sheet-body");
    node.append(header, body); document.body.append(node);
    node.addEventListener("cancel", e => { e.preventDefault(); onDismiss(); });
    node.addEventListener("click", e => { if (e.target === node) onDismiss(); });
    node.addEventListener("close", () => { node.signature = ""; });
    return { node, header, title, body, signature: "" };
  }
  const dismissRecording = close => () => (view()?.capture ? send({ type: "cancel_shortcut" }) : close());
  const editor = sheet("shortcut-editor", dismissRecording(() => send({ type: "close_shortcut_editor" })));
  const modifierSheet = sheet("modifier-key", () => send({ type: "cancel_shortcut" }));
  const picker = sheet("action-picker", () => send({ type: "close_action_picker" }));
  picker.node.classList.add("picker");
  const pickerReset = button("Reset", () => { const trigger = view()?.shortcut_page.picker?.trigger; if (trigger) send({ type: "reset_trigger", trigger }); });
  pickerReset.id = "action-picker-reset"; pickerReset.classList.add("sheet-start");
  picker.header.prepend(pickerReset);
  const pickerDescription = element("p", "settings-description"); pickerDescription.id = "action-picker-description";
  const pickerSearch = element("input", "preferences-search"); pickerSearch.type = "search"; pickerSearch.id = "action-picker-search";
  pickerSearch.placeholder = "Search actions"; pickerSearch.setAttribute("aria-label", "Search actions");
  pickerSearch.addEventListener("input", () => send({ type: "search_action_picker", query: pickerSearch.value }));
  pickerSearch.addEventListener("keydown", e => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); send({ type: "close_action_picker" }); } });
  const pickerList = element("div", "picker-list");
  picker.body.append(pickerDescription, pickerSearch, pickerList);
  const details = sheet("keymap-details", () => send({ type: "keymap_details", open: false }));
  details.node.classList.add("picker");
  const importDialog = element("dialog", "sheet"); importDialog.id = "keymap-import";
  importDialog.addEventListener("cancel", e => { e.preventDefault(); send({ type: "cancel_keymap_import" }); });
  document.body.append(importDialog);
  let importSignature = "";

  function present(target, open) {
    if (open && !target.node.open) { target.node.showModal(); target.presented = true; }
    else if (!open && target.node.open) target.node.close();
  }

  function buildShortcuts(node) {
    const root = pane("shortcuts", "root", node), category = pane("shortcuts", "category", node), modifier = pane("shortcuts", "modifier", node);
    category.hidden = modifier.hidden = true;
    active.set("shortcuts", { name: "root", depth: 0 });
    const keymap = group("Keymap"); keymap.section.id = "keymap";
    const presetRow = element("div", "preference-row");
    const presetText = element("span", "preference-text"); presetText.append(element("span", "", "Preset"));
    keymapOutdated = element("p", "", "Updated since you chose it"); keymapOutdated.hidden = true; presetText.append(keymapOutdated);
    keymapChoice = dropdown("Keymap preset", index => send({ type: "select_keymap", id: view().keymap.presets[index].id }));
    keymapChoice.id = "keymap-preset";
    const menu = element("details", "preference-choice keymap-menu"); menu.id = "keymap-menu";
    const summary = element("summary"); summary.setAttribute("aria-label", "Keymap options"); summary.title = "Keymap options"; summary.append(icon("more"));
    const options = element("div", "preference-options"); options.setAttribute("role", "menu");
    for (const [label, action, id] of [
      ["Import…", { type: "choose_keymap_file" }, "keymap-import-button"],
      ["Export…", { type: "export_keymap" }, "keymap-export-button"],
      ["Differences…", { type: "keymap_details", open: true }, "keymap-details-button"],
      ["Reset All Shortcuts", { type: "reset_all_shortcuts" }, "reset-all-shortcuts"],
    ]) {
      const item = button(label, () => { menu.open = false; send(action); }); item.id = id; item.setAttribute("role", "menuitem"); options.append(item);
    }
    menu.append(summary, options);
    presetRow.append(presetText, keymapChoice, menu); keymap.list.append(presetRow);
    root.append(keymap.section);

    const shortcuts = element("section", "settings-group"); shortcuts.id = "shortcuts";
    shortcuts.append(element("h3", "", "Shortcuts"));
    const filters = element("div", "shortcut-filters"); filters.id = "shortcut-filters";
    const searchBox = element("div", "shortcut-search");
    shortcutSearch = element("input", "preferences-search"); shortcutSearch.type = "search"; shortcutSearch.id = "shortcuts-search";
    shortcutSearch.placeholder = "Search or press a shortcut"; shortcutSearch.setAttribute("aria-label", "Search shortcuts");
    shortcutSearch.addEventListener("input", () => send({ type: "search_shortcuts", query: shortcutSearch.value }));
    shortcutSearch.addEventListener("keydown", e => {
      const editing = e.ctrlKey && !e.altKey && !e.metaKey && ["a", "c", "v", "x", "Backspace", "Delete", "ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key.length === 1 ? e.key.toLowerCase() : e.key);
      const named = /^F\d+$/.test(e.key) || e.key.startsWith("Audio") || e.key.startsWith("Media");
      if (editing || !(e.ctrlKey || e.altKey || e.metaKey || named) || ["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      e.preventDefault(); e.stopPropagation();
      send({ type: "search_shortcut_key", chord: { key: e.key, command: e.ctrlKey || e.metaKey, shift: e.shiftKey, alt: e.altKey } });
      shortcutSearch.select();
    });
    searchBox.append(icon("search"), shortcutSearch);
    contextChoice = dropdown("Show what shortcuts do with a kind of tool", index => send({ type: "shortcut_context", category: view().shortcut_page.contexts[index].category }));
    contextChoice.id = "shortcut-context"; contextChoice.title = "Show what shortcuts do with a kind of tool";
    showChoice = dropdown("Choose which actions to list", index => send({ type: "shortcut_show", show: view().shortcut_page.shows[index].show }));
    showChoice.id = "shortcut-show"; showChoice.title = "Choose which actions to list";
    filters.append(searchBox, contextChoice, showChoice);
    categoryList = element("div", "preference-group"); categoryList.id = "shortcut-categories";
    emptyStatus = element("div", "status-page"); emptyStatus.id = "shortcut-empty"; emptyStatus.hidden = true;
    emptyStatus.append(icon("search"), element("strong"), element("p"));
    shortcuts.append(filters, categoryList, emptyStatus);
    root.append(shortcuts);
    modifierMain = group("Modifier keys"); modifierMain.section.id = "modifier-results"; modifierMain.section.hidden = true;
    rootResults = element("div", "shortcut-results");
    root.append(modifierMain.section, rootResults);
    modifierCategory = group("", "Hold a key to use a tool or mode until you let go."); modifierCategory.section.id = "modifier-keys";
    categoryResults = element("div", "shortcut-results");
    category.append(modifierCategory.section, categoryResults);
    modifier.dataset.prefix = "modifier";
    return root;
  }

  function buildInput(node) {
    inputRoot = pane("input", "root", node);
    const pen = pane("input", "pen", node); pen.hidden = true; pen.dataset.prefix = "pen-button";
    active.set("input", { name: "root", depth: 0 });
    return inputRoot;
  }

  function shortcutRow(spec) {
    let entry = rows.get(spec.id);
    if (!entry) {
      const row = element("div", "preference-row shortcut-row"); row.dataset.shortcut = spec.id;
      const choose = button("", () => send({ type: "edit_shortcut", id: spec.id }), "shortcut-choose");
      const text = element("span", "preference-text"), detail = element("p");
      text.append(element("span", "", spec.label), detail);
      const binding = element("span", "shortcut-hint"); choose.append(text, binding);
      const reset = flatButton("reset", "Reset to default", () => send({ type: "reset_shortcut", id: spec.id }), `shortcut-reset-${spec.id}`);
      row.append(choose, reset);
      entry = { row, detail, binding, reset, signature: "" };
      rows.set(spec.id, entry);
    }
    const scope = spec.scope === "" ? "" : spec.scope === "Canvas" ? "On the canvas" : `With ${spec.scope.toLowerCase()} tools`;
    const subtitle = [spec.detail, scope].filter(Boolean).join(" · ");
    const signature = JSON.stringify([subtitle, spec.shortcut, spec.modified]);
    if (entry.signature !== signature) {
      entry.signature = signature;
      entry.detail.textContent = subtitle; entry.detail.hidden = !subtitle;
      entry.binding.textContent = spec.shortcut; entry.reset.hidden = !spec.modified;
      entry.row.classList.toggle("modified", spec.modified);
    }
    return entry.row;
  }

  function refreshResults(model) {
    const page = model.shortcut_page, layout = [];
    for (const spec of model.shortcuts.filter(r => r.visible)) {
      const title = page.category && !page.filtering ? spec.subgroup : spec.group;
      const last = layout.at(-1);
      if (last?.title === title) last.specs.push(spec); else layout.push({ title, specs: [spec] });
    }
    const target = page.category ? categoryResults : rootResults, other = page.category ? rootResults : categoryResults;
    const signature = JSON.stringify([page.category, layout.map(g => [g.title, g.specs.map(s => s.id)])]);
    if (signature !== resultsSignature) {
      resultsSignature = signature;
      target.replaceChildren(...layout.map(({ title, specs }) => {
        const section = group(title);
        section.list.append(...specs.map(shortcutRow));
        return section.section;
      }));
      if (page.category || page.filtering) other.replaceChildren();
    } else for (const spec of layout.flatMap(g => g.specs)) shortcutRow(spec);
  }

  function refreshModifiers(model) {
    const page = model.shortcut_page, visible = page.modifiers.filter(m => m.visible);
    const onCategory = page.category === "Modifier keys";
    modifierMain.section.hidden = !(page.filtering && visible.length);
    modifierCategory.section.hidden = !onCategory;
    const signature = JSON.stringify([visible, onCategory]);
    if (signature === modifierSignature) return;
    modifierSignature = signature;
    const list = onCategory ? modifierCategory.list : modifierMain.list;
    (onCategory ? modifierMain.list : modifierCategory.list).replaceChildren();
    list.replaceChildren(...visible.map(modifier => {
      const row = navRow(modifier.label, modifier.detail, modifier.action, () => send({ type: "edit_modifier_key", key: modifier.key }));
      row.dataset.modifier = modifier.label; return row;
    }));
    if (onCategory) {
      const add = button("", () => send({ type: "add_modifier_key" }), "preference-row button-row"); add.id = "add-modifier-key";
      add.append(icon("plus"), element("span", "", "Add Modifier Key")); list.append(add);
    }
  }

  function refreshCategories(model) {
    const page = model.shortcut_page;
    const signature = JSON.stringify(page.categories.map(c => c.id));
    if (signature !== categorySignature) {
      categorySignature = signature; categoryRows.clear();
      categoryList.replaceChildren(...page.categories.map(category => {
        const row = navRow(category.id, "", String(category.count), () => send({ type: "shortcut_category", id: category.id }));
        row.dataset.category = category.id; categoryRows.set(category.id, row); return row;
      }));
    }
    for (const category of page.categories) categoryRows.get(category.id)?.update(category.id, "", String(category.count));
    categoryList.hidden = page.filtering;
    emptyStatus.hidden = !page.empty;
    if (page.empty) { emptyStatus.querySelector("strong").textContent = page.empty.title; emptyStatus.querySelector("p").textContent = page.empty.description; }
  }

  function renderPerTool(paneNode, editor, { summary, reset, same, pick, remove }) {
    const signature = JSON.stringify([editor, summary, !!remove]);
    if (paneNode.signature === signature) return;
    paneNode.signature = signature;
    const prefix = paneNode.dataset.prefix;
    const section = group("", summary);
    if (editor.modified) section.section.querySelector(".settings-heading").append(flatButton("reset", "Reset to default", () => send(reset), `${prefix}-reset`));
    const sameRow = element("label", "preference-row");
    const toggle = element("input", "settings-switch"); toggle.type = "checkbox"; toggle.setAttribute("role", "switch");
    toggle.id = `${prefix}-same`; toggle.checked = !editor.per_tool;
    toggle.addEventListener("change", () => send(same(!toggle.checked)));
    sameRow.append(element("span", "preference-text", "Same for every tool"), toggle);
    section.list.append(sameRow);
    for (const action of editor.actions) {
      const row = navRow(action.label, "", action.action, () => send(pick(action.category)));
      row.id = `${prefix}-action-${action.category ?? "all"}`; section.list.append(row);
    }
    const nodes = [section.section];
    if (remove) {
      const removal = element("div", "preference-group");
      const row = button("", () => send(remove.action), "preference-row button-row destructive-action"); row.id = `${prefix}-remove`;
      row.append(icon("delete"), element("span", "", remove.label)); removal.append(row);
      const wrapper = element("section", "settings-group"); wrapper.append(removal); nodes.push(wrapper);
    }
    paneNode.replaceChildren(...nodes);
  }

  function refreshTriggers(model) {
    if (!inputRoot) return;
    const triggers = model.shortcut_page.triggers;
    const signature = JSON.stringify(triggers.map(t => [t.id, t.section]));
    if (signature !== triggerSignature) {
      triggerSignature = signature;
      for (const section of triggerGroups.values()) section.section.remove();
      triggerGroups.clear(); triggerRows.clear();
      for (const trigger of triggers) {
        if (!triggerGroups.has(trigger.section)) {
          const section = group(trigger.section); section.section.id = `triggers-${trigger.section.toLowerCase().replaceAll(" ", "-")}`;
          triggerGroups.set(trigger.section, section); inputRoot.append(section.section);
        }
        const pen = trigger.section === "Pen buttons";
        const row = navRow(trigger.label, trigger.detail, trigger.action, () => send(pen ? { type: "edit_pen_button", trigger: trigger.id } : { type: "open_action_picker", trigger: trigger.id }));
        row.dataset.trigger = trigger.id; triggerGroups.get(trigger.section).list.append(row); triggerRows.set(trigger.id, row);
      }
    }
    for (const trigger of triggers) triggerRows.get(trigger.id)?.update(trigger.label, trigger.detail, trigger.action);
  }

  function refreshEditor(model) {
    const spec = model.shortcut_editor;
    if (!spec) return present(editor, false);
    const capture = model.capture?.id === spec.id ? model.capture : null;
    const error = capture ? null : model.error;
    const signature = JSON.stringify([spec, capture, error]);
    if (editor.signature !== signature) {
      editor.signature = signature;
      editor.title.textContent = spec.label;
      const description = element("p", "settings-description sheet-summary", spec.description);
      const section = group("", [`Default: ${spec.defaults.join(" / ") || "none"}`, ...spec.overlaps].join("\n"));
      if (spec.modified) section.section.querySelector(".settings-heading").append(flatButton("reset", "Reset to default", () => send({ type: "reset_shortcut", id: spec.id }), "shortcut-editor-reset"));
      spec.bindings.forEach((binding, index) => {
        const row = element("div", "preference-row");
        row.append(element("span", "preference-text", binding), flatButton("delete", "Remove shortcut", () => send({ type: "remove_shortcut", id: spec.id, index }), `remove-shortcut-${index}`));
        section.list.append(row);
      });
      if (capture) section.list.append(recordingRow(capture));
      else if (spec.can_add) {
        const add = button("", () => send({ type: "begin_shortcut", id: spec.id }), "preference-row button-row"); add.id = "add-shortcut";
        add.append(icon("plus"), element("span", "", "Add Shortcut")); section.list.append(add);
      }
      const nodes = [description, element("p", "preferences-error", error || ""), section.section];
      if (spec.gestures.length) {
        const gestures = group("Pen and Touch", "Change these on the Pen & Input page.");
        gestures.list.append(...spec.gestures.map(g => { const row = element("div", "preference-row"); row.append(element("span", "preference-text", g)); return row; }));
        nodes.push(gestures.section);
      }
      editor.body.replaceChildren(...nodes);
    }
    present(editor, true);
    focusRecording(editor, capture);
  }

  function focusRecording(target, capture) {
    const row = target.node.querySelector("#shortcut-recording");
    if (capture && row && !target.node.contains(document.activeElement?.closest?.("#shortcut-recording"))) row.focus({ preventScroll: true });
    else if (!target.node.contains(document.activeElement)) target.node.querySelector("button:not([hidden])")?.focus({ preventScroll: true });
  }

  function refreshModifierSheet(model) {
    const capture = model.capture?.id === "modifier" ? model.capture : null;
    if (!capture) return present(modifierSheet, false);
    const signature = JSON.stringify(capture);
    if (modifierSheet.signature !== signature) {
      modifierSheet.signature = signature;
      modifierSheet.title.textContent = "New Modifier Key";
      const section = group(); section.list.append(recordingRow(capture));
      modifierSheet.body.replaceChildren(element("p", "settings-description sheet-summary", "Press the key or button to hold."), section.section);
    }
    present(modifierSheet, true);
    focusRecording(modifierSheet, capture);
  }

  function refreshPicker(model) {
    const spec = model.shortcut_page.picker;
    if (!spec) return present(picker, false);
    picker.title.textContent = spec.title;
    pickerDescription.textContent = spec.description;
    pickerReset.hidden = !spec.modified;
    if (pickerSearch.value !== spec.query) pickerSearch.value = spec.query;
    const signature = JSON.stringify([spec.nothing, spec.query, spec.sections]);
    if (picker.signature !== signature) {
      picker.signature = signature;
      const choice = (id, label, detail, selected) => {
        const row = button("", () => send({ type: "choose_action", id }), "preference-row picker-row"); row.id = `action-${id}`;
        const text = element("span", "preference-text"); text.append(element("span", "", label));
        if (detail) text.append(element("p", "", detail));
        const check = icon("check"); check.style.visibility = selected ? "visible" : "hidden";
        row.append(text, check); return row;
      };
      const sections = [];
      if ("nothing".includes(spec.query.trim().toLowerCase())) { const none = group(); none.list.append(choice("", "Nothing", "", spec.nothing)); sections.push(none.section); }
      for (const section of spec.sections) {
        const list = group(section.title);
        list.list.append(...section.actions.map(a => choice(a.id, a.label, a.detail, a.selected)));
        sections.push(list.section);
      }
      if (!sections.length) { const none = element("div", "status-page"); none.append(icon("search"), element("strong", "", "No Results Found"), element("p", "", "Try a different search.")); sections.push(none); }
      pickerList.replaceChildren(...sections);
    }
    const opening = !picker.node.open;
    present(picker, true);
    if (opening) pickerSearch.focus();
  }

  function refreshKeymap(model) {
    const keymap = model.keymap;
    keymapChoice.update(keymap.presets.map(p => p.title), keymap.presets.findIndex(p => p.id === keymap.selected));
    keymapOutdated.hidden = !keymap.outdated;
    const signature = JSON.stringify([keymap.selected, keymap.differences, keymap.source, keymap.links]);
    if (details.signature !== signature) {
      details.signature = signature;
      details.title.textContent = keymap.title;
      const links = element("div", "keymap-links");
      for (const link of keymap.links) {
        const name = (link.replace(/\/$/, "").split("/").at(-1) || link).split(".")[0].replaceAll(/[_-]/g, " ");
        const anchor = element("a", "settings-link", name); anchor.href = link; anchor.target = "_blank"; anchor.rel = "noopener noreferrer"; links.append(anchor);
      }
      const list = group();
      const rowsOrEmpty = keymap.differences.length ? keymap.differences : [{ trigger: "No differences", note: "This keymap uses the CapyCanvas defaults." }];
      list.list.append(...rowsOrEmpty.map(d => { const row = element("div", "preference-row"); const text = element("span", "preference-text"); text.append(element("span", "", d.trigger), element("p", "", d.note)); row.append(text); return row; }));
      details.body.replaceChildren(element("p", "settings-description", keymap.source), links, list.section);
    }
    present(details, keymap.details);
    const preview = keymap.import, signatureImport = JSON.stringify(preview);
    if (preview && signatureImport !== importSignature) {
      const header = element("header", "dialog-header"); header.append(element("h2", "", `Import ${preview.title}?`));
      const body = element("div", "sheet-body");
      for (const [title, items] of [["Added", preview.added], ["Changed", preview.changed], ["Removed", preview.removed], ["Not available", preview.unavailable]]) {
        if (!items.length) continue;
        body.append(element("h3", "", `${title} (${items.length})`));
        const list = element("ul", "keymap-preview"); list.append(...items.map(item => element("li", "", item))); body.append(list);
      }
      if (!body.children.length) body.append(element("p", "", "No shortcuts change."));
      const footer = element("footer");
      const confirmImport = button("Import", () => send({ type: "confirm_keymap_import" }), "suggested-action"); confirmImport.id = "confirm-keymap-import";
      footer.append(button("Cancel", () => send({ type: "cancel_keymap_import" })), confirmImport);
      importDialog.replaceChildren(header, body, footer);
    }
    importSignature = preview ? signatureImport : "";
    if (preview && !importDialog.open) importDialog.showModal();
    else if (!preview && importDialog.open) importDialog.close();
  }

  function refreshFilters(model) {
    const page = model.shortcut_page, context = JSON.stringify(page.context);
    contextChoice.update(page.contexts.map(c => c.label), page.contexts.findIndex(c => JSON.stringify(c.category) === context));
    showChoice.update(page.shows.map(s => s.label), page.shows.findIndex(s => s.show === page.show));
    if (shortcutSearch.value !== model.shortcut_query) shortcutSearch.value = model.shortcut_query;
  }

  return {
    container(pageId, node) {
      if (pageId === "shortcuts") return buildShortcuts(node);
      if (pageId === "input") return buildInput(node);
      return node;
    },
    title(model) {
      if (model.page === "shortcuts") return model.modifier_editor?.label ?? model.shortcut_page.category ?? null;
      if (model.page === "input") return model.pen_button_editor?.label ?? null;
      return null;
    },
    back(model) {
      if (model.page === "input" && model.pen_button_editor) send({ type: "close_pen_button" });
      else if (model.modifier_editor) send({ type: "close_modifier_key" });
      else send({ type: "shortcut_category", id: null });
    },
    refresh(model) {
      if (!keymapChoice) return;
      refreshKeymap(model);
      refreshFilters(model);
      refreshCategories(model);
      refreshResults(model);
      refreshModifiers(model);
      refreshTriggers(model);
      const shortcutPanes = panes.get("shortcuts");
      const modifierEditor = model.modifier_editor;
      if (modifierEditor) renderPerTool(shortcutPanes.get("modifier"), modifierEditor, {
        summary: `Hold ${modifierEditor.label} to use an action until you let go.`,
        reset: { type: "reset_modifier_key", key: modifierEditor.key },
        same: per_tool => ({ type: "modifier_key_per_tool", key: modifierEditor.key, per_tool }),
        pick: category => ({ type: "open_modifier_picker", key: modifierEditor.key, category }),
        remove: { label: "Remove Modifier Key", action: { type: "remove_modifier_key", key: modifierEditor.key } },
      });
      if (model.page === "shortcuts") showPane("shortcuts", modifierEditor ? "modifier" : model.shortcut_page.category ? "category" : "root", modifierEditor ? 2 : model.shortcut_page.category ? 1 : 0);
      const pen = model.pen_button_editor;
      if (pen && panes.has("input")) renderPerTool(panes.get("input").get("pen"), pen, {
        summary: "Tools, brushes and modes last while the button is held. Other actions run once.",
        reset: { type: "reset_trigger", trigger: pen.trigger },
        same: per_tool => ({ type: "pen_button_per_tool", trigger: pen.trigger, per_tool }),
        pick: category => ({ type: "open_pen_button_picker", trigger: pen.trigger, category }),
      });
      if (model.page === "input" && panes.has("input")) showPane("input", pen ? "pen" : "root", pen ? 1 : 0);
      refreshEditor(model);
      refreshModifierSheet(model);
      refreshPicker(model);
    },
    close() {
      for (const target of [editor, modifierSheet, picker, details]) present(target, false);
      if (importDialog.open) importDialog.close();
    },
  };
}
