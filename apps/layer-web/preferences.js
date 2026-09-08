// DOM adapter for the same PreferencesView as GTK. Definitions, dependencies,
// validation, search, recording and conflicts are all resolved in Rust.
export function createPreferences({ element, button, icon, spin, setNumber, numericControl, panelFrame, dispatch, view }) {
  const dialog = document.getElementById("settings");
  const send = (action) => dispatch({ type: "preferences", action });
  const close = () => dispatch({ type: "cancel_settings" });
  const root = element("div", "preferences-layout");
  const sidebar = element("aside", "preferences-sidebar");
  const sidebarHeader = element("header", "dialog-header");
  sidebarHeader.append(element("h2", "", "Preferences"));
  const navigation = element("nav", "preferences-navigation");
  navigation.setAttribute("aria-label", "Preferences categories");
  sidebar.append(sidebarHeader, navigation);
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
  const searchIcon = icon("search"); searchIcon.classList.add("preferences-search-icon");
  const pages = element("div", "preferences-pages");
  const empty = element("p", "preferences-empty", "No matching preferences"); pages.append(empty);
  content.append(header, search, searchIcon, panelFrame(pages));
  const footer = element("footer");
  const error = element("span", "preferences-error"); error.setAttribute("role", "status");
  const cancel = button("Cancel", close); cancel.value = "cancel";
  const apply = button("Apply", () => dispatch({ type: "apply_settings" }), "suggested-action"); apply.id = "apply-settings";
  footer.append(error, cancel, apply); root.append(sidebar, content, footer); dialog.append(root);
  dialog.addEventListener("close", () => { if (view()) close(); });
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
  captureFooter.append(button("Cancel", stop), button("Clear", () => send({ type: "clear_shortcut" })), confirm);
  capture.append(captureHeader, captureLabel, captureKey, captureError, captureFooter); document.body.append(capture);
  capture.addEventListener("cancel", (e) => { e.preventDefault(); stop(); });
  capture.addEventListener("close", () => { if (view()?.capture) stop(); });

  const fields = new Map(), pageNodes = new Map(), tabs = new Map(), groups = [];
  const shortcuts = new Map();
  let shortcutList;
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
            case "choice":
              input = element("select");
              row.kind.options.forEach((name, index) => { const option = element("option", "", name); option.value = index; input.append(option); });
              widget = input; break;
            case "number":
              input = element("input"); input.type = "number"; numericControl(input, row.kind.control);
              input.setAttribute("aria-label", row.title); widget = spin(input, row.kind.control.digits); break;
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
          input.addEventListener("input", () => {
            if (input.type === "number" && input.value === "") return;
            send({ type: "edit", id: row.id, value: row.kind.type === "switch" ? input.checked : Number(input.value) });
          });
          if (row.kind.type === "number") input.addEventListener("blur", () => {
            const current = view()?.pages.flatMap((p) => p.groups.flatMap((g) => g.rows)).find((r) => r.id === row.id);
            if (current) setNumber(input, current.kind.value);
          });
          line.append(widget); list.append(line); fields.set(row.id, { line, input, widget });
        }
      }
      if (page.id === "shortcuts") {
        const section = element("section", "settings-group");
        const heading = element("div", "shortcut-heading");
        const labels = element("div");
        labels.append(element("h3", "", "Shortcuts"), element("p", "settings-description", "Select an action to record a shortcut. Escape cancels recording. Unassigned actions show Disabled."));
        heading.append(labels, button("Reset All", () => send({ type: "reset_all_shortcuts" })));
        shortcutList = element("div", "preference-group");
        section.append(heading, shortcutList);
        node.append(section);
      }
    }
  }
  return function refresh(model) {
    if (!model) {
      if (capture.open) capture.close();
      if (dialog.open) dialog.close();
      return;
    }
    if (!fields.size) build(model);
    empty.hidden = !model.empty;
    title.textContent = model.pages.find((p) => p.id === model.page).title;
    if (search.value !== model.query) search.value = model.query;
    for (const [id, node] of pageNodes) node.hidden = id !== model.page || model.empty;
    for (const [id, tab] of tabs) tab.setAttribute("aria-selected", String(id === model.page));
    const visible = new Set();
    for (const row of model.pages.flatMap((p) => p.groups.flatMap((g) => g.rows))) {
      const { line, input, widget } = fields.get(row.id);
      line.hidden = !row.visible; if (row.visible) visible.add(row.id);
      line.classList.toggle("disabled", !row.enabled);
      for (const control of [input, ...widget.querySelectorAll("button")]) control.disabled = !row.enabled;
      if (row.kind.type === "number") setNumber(input, row.kind.value);
      else if (row.kind.type === "choice") input.value = row.kind.selected;
      else if (row.kind.type === "switch") input.checked = row.kind.active;
    }
    for (const [ids, section] of groups) section.hidden = !ids.some((id) => visible.has(id));
    const shortcutIds = new Set(model.shortcuts.map((spec) => spec.id));
    for (const [id, { row }] of shortcuts) {
      if (!shortcutIds.has(id)) { row.remove(); shortcuts.delete(id); }
    }
    for (const spec of model.shortcuts) {
      if (!shortcuts.has(spec.id)) {
        const row = element("div", "preference-row shortcut-row"); row.dataset.shortcut = spec.id;
        const choose = button("", () => send({ type: "begin_shortcut", id: spec.id }), "shortcut-choose");
        const text = element("span", "preference-text"); text.append(element("span", "", spec.label), element("p", "", spec.group));
        const binding = element("span", "shortcut-hint"); choose.append(text, binding);
        const reset = button("Reset", () => send({ type: "reset_shortcut", id: spec.id }));
        row.append(choose, reset); shortcutList.append(row); shortcuts.set(spec.id, { row, text, binding, reset });
      }
      const { row, binding, reset } = shortcuts.get(spec.id);
      row.hidden = !spec.visible; binding.textContent = spec.shortcut || "Disabled"; reset.disabled = !spec.modified;
    }
    error.textContent = model.error || ""; apply.disabled = !model.dirty || !!model.capture;
    if (!dialog.open) { dialog.showModal(); root.classList.add("show-content"); }
    if (model.capture) {
      const c = model.capture;
      captureLabel.textContent = c.label; captureKey.textContent = c.shortcut;
      captureError.textContent = c.error || (c.conflict ? `Already assigned to ${c.conflict}. Replace its shortcut?` : "");
      confirm.disabled = !c.chord || !!c.error; confirm.textContent = c.conflict ? "Replace Shortcut" : "Set Shortcut";
      if (!capture.open) capture.showModal();
    } else if (capture.open) capture.close();
    pages.dispatchEvent(new Event("scroll"));
  };
}
