// DOM adapter for the same PreferencesView as GTK. Definitions, dependencies,
// validation, search, recording and conflicts are all resolved in Rust.
export function createPreferences({ element, button, icon, numberField, panelFrame, dispatch, view }) {
  const dialog = document.getElementById("settings");
  const send = (action) => dispatch({ type: "preferences", action });
  const close = () => dispatch({ type: "close_settings" });
  const root = element("div", "preferences-layout");
  const sidebar = element("aside", "preferences-sidebar");
  const sidebarHeader = element("header", "dialog-header");
  sidebarHeader.append(element("h2", "", "Preferences"));
  const searchToggle = button("", () => send({ type: "toggle_search", open: !view()?.searching }), "preferences-search-toggle");
  searchToggle.append(icon("search")); searchToggle.setAttribute("aria-label", "Search preferences");
  sidebarHeader.append(searchToggle);
  const navigation = element("nav", "preferences-navigation");
  navigation.setAttribute("aria-label", "Preferences categories");
  const content = element("div", "preferences-content");
  const header = element("header", "dialog-header");
  const title = element("h2"); title.id = "settings-title";
  const back = button("‹", () => root.classList.remove("show-content"), "preferences-back");
  back.setAttribute("aria-label", "Preferences categories");
  const exit = button("×", close, "dialog-close"); exit.setAttribute("aria-label", "Close preferences");
  header.append(back, title, exit);
  const search = element("input", "preferences-search");
  search.type = "search"; search.placeholder = "Search preferences"; search.id = "settings-search";
  search.setAttribute("aria-label", "Search preferences");
  search.addEventListener("input", () => send({ type: "search", query: search.value }));
  search.addEventListener("keydown", e => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); send({ type: "toggle_search", open: false }); } });
  const searchResults = element("div", "preferences-search-results");
  const empty = element("p", "preferences-empty", "No matching preferences");
  sidebar.append(sidebarHeader, search, navigation, searchResults, empty);
  const pages = element("div", "preferences-pages");
  content.append(header, panelFrame(pages));
  const footer = element("footer");
  const error = element("span", "preferences-error"); error.setAttribute("role", "status");
  const done = button("Done", close); done.id = "close-settings";
  footer.append(error, done); content.append(footer); root.append(sidebar, content); dialog.append(root);
  dialog.addEventListener("close", () => { if (!dialog.open && view()) close(); });
  dialog.addEventListener("cancel", (e) => { e.preventDefault(); close(); });

  const capture = element("dialog", "shortcut-capture"); capture.id = "shortcut-capture";
  capture.setAttribute("aria-label", "Set Shortcut");
  const captureHeader = element("header", "dialog-header"); captureHeader.append(element("h2", "", "Set Shortcut"));
  const captureClose = button("×", () => send({ type: "cancel_shortcut" }), "dialog-close");
  captureClose.setAttribute("aria-label", "Cancel shortcut recording"); captureHeader.append(captureClose);
  const captureLabel = element("p"), captureKey = element("p", "shortcut-key"), captureError = element("p", "preferences-error");
  const captureFooter = element("footer");
  const stop = () => send({ type: "cancel_shortcut" });
  const confirm = button("Set Shortcut", () => send({ type: "confirm_shortcut", replace: !!view()?.capture?.conflict }), "suggested-action");
  confirm.id = "confirm-shortcut";
  captureFooter.append(button("Cancel", stop), confirm);
  capture.append(captureHeader, captureLabel, captureKey, captureError, captureFooter); document.body.append(capture);
  capture.addEventListener("cancel", (e) => { e.preventDefault(); stop(); });
  capture.addEventListener("close", () => { if (!capture.open && view()?.capture) stop(); });
  const editor = element("dialog", "shortcut-editor"); editor.id = "shortcut-editor";
  const closeEditor = () => send({ type: "close_shortcut_editor" });
  editor.addEventListener("cancel", e => { e.preventDefault(); closeEditor(); });
  editor.addEventListener("close", () => { if (!editor.open && view()?.shortcut_editor) closeEditor(); });
  document.body.append(editor);
  let editorSignature = "", searchSignature = "", searchFocus = 0;

  const fields = new Map(), pageNodes = new Map(), tabs = new Map(), groups = [];
  const shortcuts = new Map();
  let shortcutList, shortcutSearch;
  function build(model) {
    for (const page of model.pages) {
      const tab = button("", () => { send({ type: "page", page: page.id }); root.classList.add("show-content"); });
      tab.dataset.settingsPage = page.id;
      tab.append(icon(page.icon), element("span", "", page.title));
      navigation.append(tab); tabs.set(page.id, tab);
      const node = element("section", "preferences-page"); node.dataset.page = page.id;
      node.setAttribute("aria-label", page.title); pages.append(node); pageNodes.set(page.id, node);
      for (const group of page.groups) {
        const section = element("section", "settings-group");
        section.append(element("h3", "", group.title));
        const list = element("div", "preference-group"); section.append(list); node.append(section);
        groups.push([group.rows.map((r) => r.id), section]);
        for (const row of group.rows) {
          const line = element("div", "preference-row"), text = element("div", "preference-text");
          const label = element("label", "", row.title); text.append(label, element("p", "", row.description)); line.append(text);
          const id = `setting-${row.id.replaceAll("_", "-")}`;
          label.htmlFor = id;
          let input, widget;
          switch (row.kind.type) {
            case "text":
              input = element("input", "preference-entry"); input.type = "text";
              input.maxLength = row.kind.max_length; input.placeholder = row.kind.placeholder;
              input.spellcheck = false; input.autocomplete = "off"; input.setAttribute("autocapitalize", "off");
              input.value = row.kind.value;
              input.addEventListener("change", () => send({ type: "edit", id: row.id, value: input.value }));
              input.addEventListener("keydown", e => {
                if (e.key === "Enter") { e.preventDefault(); send({ type: "edit", id: row.id, value: input.value }); }
              });
              widget = input; break;
            case "choice":
              if (row.kind.icons.length) {
                input = element("input"); input.type = "hidden";
                widget = element("details", "preference-choice");
                const summary = element("summary"); summary.setAttribute("aria-label", row.title);
                const choices = element("div", "preference-options"); choices.setAttribute("role", "listbox");
                row.kind.options.forEach((name, index) => {
                  const choice = button("", () => { input.value = index; input.dispatchEvent(new Event("input")); widget.open = false; });
                  choice.dataset.choice = index; choice.setAttribute("role", "option");
                  choice.append(icon(row.kind.icons[index]), element("span", "", name)); choices.append(choice);
                });
                widget.append(summary, choices, input);
              } else {
                input = element("select");
                row.kind.options.forEach((name, index) => { const option = element("option", "", name); option.value = index; input.append(option); });
                widget = input;
              }
              break;
            case "number":
              input = numberField(row.kind.control, row.title, value => send({ type: "edit", id: row.id, value }));
              input.setDescription(row.description); text.remove();
              line.classList.add("number-preference"); widget = input; break;
            case "switch":
              input = element("input", "settings-switch"); input.type = "checkbox"; input.setAttribute("role", "switch"); widget = input; break;
            case "info":
              input = element("span", "settings-info", row.kind.value); widget = input; break;
            case "link":
              input = element("a", "settings-link", row.kind.label);
              input.href = row.kind.url; input.target = "_blank"; input.rel = "noopener noreferrer";
              widget = input; break;
          }
          input.id = id; input.setAttribute("aria-label", row.title);
          if (!["number", "text"].includes(row.kind.type)) input.addEventListener("input", () => {
            if (input.type === "number" && input.value === "") return;
            send({ type: "edit", id: row.id, value: row.kind.type === "switch" ? input.checked : Number(input.value) });
          });
          line.append(widget); list.append(line); fields.set(row.id, { line, input, widget });
        }
      }
      if (page.id === "shortcuts") {
        shortcutSearch = element("input", "preferences-search"); shortcutSearch.type = "search";
        shortcutSearch.id = "shortcuts-search"; shortcutSearch.placeholder = "Search shortcuts";
        shortcutSearch.setAttribute("aria-label", "Search shortcuts");
        shortcutSearch.addEventListener("input", () => send({ type: "search_shortcuts", query: shortcutSearch.value }));
        node.append(shortcutSearch);
        const section = element("section", "settings-group");
        const heading = element("div", "shortcut-heading");
        const labels = element("div");
        labels.append(element("h3", "", "Shortcuts"), element("p", "settings-description", "Select an action to edit its shortcuts."));
        heading.append(labels, button("Reset All", () => send({ type: "reset_all_shortcuts" })));
        shortcutList = element("div", "preference-group");
        section.append(heading, shortcutList);
        node.append(section);
      }
    }
  }
  return function refresh(model) {
    if (!model) {
      searchFocus = 0;
      if (capture.open) capture.close();
      if (editor.open) editor.close();
      if (dialog.open) dialog.close();
      return;
    }
    if (!fields.size) build(model);
    empty.hidden = !model.empty;
    title.textContent = model.pages.find((p) => p.id === model.page).title;
    if (search.value !== model.query) search.value = model.query;
    const openingSearch = search.hidden && model.searching;
    search.hidden = !model.searching;
    searchToggle.setAttribute("aria-pressed", String(model.searching));
    navigation.hidden = !!model.query;
    if (openingSearch) search.focus();
    if (searchFocus !== model.search_focus) {
      searchFocus = model.search_focus;
      root.classList.remove("show-content");
      search.focus();
      search.setSelectionRange(search.value.length, search.value.length);
    }
    const resultsSignature = JSON.stringify(model.search_results);
    if (searchSignature !== resultsSignature) {
      searchResults.replaceChildren();
      for (const result of model.search_results) {
        const row = button("", () => { send(result.action); root.classList.add("show-content"); });
        row.append(element("span", "", result.title), element("small", "", result.description));
        searchResults.append(row);
      }
      searchSignature = resultsSignature;
    }
    if (shortcutSearch.value !== model.shortcut_query) shortcutSearch.value = model.shortcut_query;
    for (const [id, node] of pageNodes) node.hidden = id !== model.page;
    for (const [id, tab] of tabs) tab.setAttribute("aria-selected", String(id === model.page));
    const visible = new Set();
    for (const row of model.pages.flatMap((p) => p.groups.flatMap((g) => g.rows))) {
      const { line, input, widget } = fields.get(row.id);
      line.hidden = !row.visible; if (row.visible) visible.add(row.id);
      line.classList.toggle("disabled", !row.enabled);
      for (const control of [input, ...widget.querySelectorAll("button")]) control.disabled = !row.enabled;
      if (row.kind.type === "number") { input.setDisabled(!row.enabled); input.update(row.kind.value); }
      else if (row.kind.type === "choice") {
        input.value = row.kind.selected;
        if (row.kind.icons.length) {
          widget.querySelector("summary").replaceChildren(icon(row.kind.icons[row.kind.selected]), element("span", "", row.kind.options[row.kind.selected]));
          for (const choice of widget.querySelectorAll("[data-choice]")) choice.setAttribute("aria-selected", String(Number(choice.dataset.choice) === row.kind.selected));
        }
      }
      else if (row.kind.type === "switch") input.checked = row.kind.active;
      else if (row.kind.type === "text" && document.activeElement !== input) input.value = row.kind.value;
    }
    for (const [ids, section] of groups) section.hidden = !ids.some((id) => visible.has(id));
    const shortcutIds = new Set(model.shortcuts.map((spec) => spec.id));
    for (const [id, { row }] of shortcuts) {
      if (!shortcutIds.has(id)) { row.remove(); shortcuts.delete(id); }
    }
    for (const spec of model.shortcuts) {
      if (!shortcuts.has(spec.id)) {
        const row = element("div", "preference-row shortcut-row"); row.dataset.shortcut = spec.id;
        const choose = button("", () => send({ type: "edit_shortcut", id: spec.id }), "shortcut-choose");
        const text = element("span", "preference-text"); text.append(element("span", "", spec.label), element("p", "", spec.group));
        const binding = element("span", "shortcut-hint"); choose.append(text, binding);
        row.append(choose); shortcutList.append(row); shortcuts.set(spec.id, { row, text, binding });
      }
      const { row, binding } = shortcuts.get(spec.id);
      row.hidden = !spec.visible; binding.textContent = spec.shortcut || "Disabled";
    }
    error.textContent = model.error || "";
    if (!dialog.open) { dialog.showModal(); root.classList.add("show-content"); }
    if (model.shortcut_editor) {
      const spec = model.shortcut_editor, signature = JSON.stringify([spec, model.error]);
      if (editorSignature !== signature) {
        editor.replaceChildren();
        const header = element("header", "dialog-header"); header.append(element("h2", "", spec.label));
        const exit = button("×", closeEditor, "dialog-close"); exit.setAttribute("aria-label", "Close shortcut editor"); header.append(exit);
        const body = element("div", "shortcut-editor-body");
        body.append(element("p", "settings-description", spec.group));
        const list = element("div", "preference-group");
        spec.bindings.forEach((binding, index) => {
          const row = element("div", "preference-row");
          row.append(element("span", "", binding), button("Remove", () => send({ type: "remove_shortcut", id: spec.id, index })));
          list.append(row);
        });
        body.append(list, element("p", "settings-description", `Default: ${spec.defaults.join(" / ") || "Disabled"}`), element("p", "preferences-error", model.error || ""));
        const footer = element("footer");
        const reset = button("Reset", () => send({ type: "reset_shortcut", id: spec.id })); reset.disabled = !spec.modified;
        const add = button("Add Shortcut", () => send({ type: "begin_shortcut", id: spec.id })); add.id = "add-shortcut"; add.disabled = !spec.can_add;
        const done = button("Done", closeEditor); done.id = "close-shortcut-editor";
        footer.append(reset, add, done); body.append(footer); editor.append(header, body);
        editorSignature = signature;
      }
      if (!editor.open) editor.showModal();
    } else if (editor.open) editor.close();
    if (model.capture) {
      const c = model.capture;
      captureLabel.textContent = c.label; captureKey.textContent = c.shortcut;
      captureError.textContent = c.notice;
      confirm.disabled = !c.chord || !!c.error; confirm.textContent = c.conflict ? "Replace Shortcut" : "Set Shortcut";
      if (!capture.open) capture.showModal();
    } else if (capture.open) capture.close();
    pages.dispatchEvent(new Event("scroll"));
  };
}
