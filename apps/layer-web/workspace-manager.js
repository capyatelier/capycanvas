export function createWorkspaceManager({ app, store, applyChange, element, button, message, dispatch, hasLegacy, legacyError }) {
  const dialog = element("dialog", "workspace-manager"), formDialog = element("dialog", "workspace-form");
  const heading = element("h2"), header = element("header", "dialog-header");
  heading.id = "workspace-manager-title"; dialog.setAttribute("aria-labelledby", heading.id);
  const add = button("+", () => send({ type: "form", kind: "new" }), "workspace-add");
  const close = button("×", cancel, "dialog-close"); close.setAttribute("aria-label", "Close");
  header.append(heading, add, close);
  const intro = element("p", "workspace-intro"), filter = element("input", "workspace-filter");
  filter.type = "search"; filter.placeholder = "Search"; filter.setAttribute("aria-label", "Search workspaces and layouts");
  filter.addEventListener("input", () => send({ type: "filter", query: filter.value }));
  const list = element("div", "workspace-list"); list.setAttribute("role", "listbox");
  const error = element("p", "workspace-error"), footer = element("footer");
  const primary = button("", () => send({ type: "confirm" }), "suggested-action");
  const retry = button("Retry", () => send({type:"retry"})); retry.hidden = true;
  footer.append(button("Cancel", cancel), retry, primary); dialog.append(header, intro, filter, list, error, footer);
  document.body.append(dialog, formDialog);
  const switcher = element("div", "workspace-switcher"); switcher.setAttribute("role", "group"); switcher.setAttribute("aria-label", "Workspaces");
  document.querySelector("#document-title").after(switcher);
  const recoveryButton = button("Workspace save failed…", () => { dismissedError = null; showRecovery(view.error); }, "workspace-recovery");
  recoveryButton.hidden = true; switcher.after(recoveryButton);
  const switches = new Map();
  let view, lastView, pageKey, rowsKey, formKey, timer, formName, formSource, formError, formSubmit, previousFocus, observePending = false;
  const expectedCloses = new WeakMap();
  const channel = typeof BroadcastChannel === "function" ? new BroadcastChannel("capycanvas.workspace.windows") : null;
  channel?.addEventListener("message", e => {
    if (e.data?.focus === view?.id) { window.focus(); document.title = `CapyCanvas — ${view.name}`; }
  });
  function send(input) {
    try { applyChange(app.workspace_input(JSON.stringify(input))); render(); tick(); }
    catch (e) { message(e); }
  }
  function cancel() { send({ type: "cancel" }); }
  for (const node of [dialog, formDialog]) {
    node.addEventListener("cancel", e => { e.preventDefault(); cancel(); });
    node.addEventListener("close", () => {
      const expected = expectedCloses.get(node) || 0;
      if (expected) expectedCloses.set(node, expected - 1);
      else if (node === formDialog ? view?.form : view?.page) cancel();
    });
    node.addEventListener("click", e => { if (e.target === node) { const r = node.getBoundingClientRect(); if (e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom) cancel(); } });
  }
  function show(node, open) {
    if (open && !node.open) { if (!dialog.open && !formDialog.open) previousFocus = document.activeElement; node.showModal(); }
    if (!open && node.open) { expectedCloses.set(node,(expectedCloses.get(node)||0)+1); node.close(); if (!dialog.open && !formDialog.open) previousFocus?.focus?.(); }
  }
  function render() {
    const json = app.workspace_view(); if (json === lastView) return; lastView = json; view = JSON.parse(json);
    if (!view) return;
    recoveryButton.hidden = !view.error;
    if (!view.error) { dismissedError = null; if (recovery?.open) recovery.close(); }
    for (const row of view.defaults) {
      let node = switches.get(row.id);
      if (!node) { node = button(row.title, () => send({type:"switch",id:row.id})); node.dataset.workspaceId = row.id; switches.set(row.id,node); switcher.append(node); }
      node.textContent = row.title; node.title = `Switch to ${row.title} workspace`;
      node.setAttribute("aria-pressed",String(row.id === view.id)); node.disabled = !view.ready || view.busy || !!view.page || !!view.form;
    }
    if (view.focus_window) { channel?.postMessage({ focus: view.focus_window }); message("The workspace is open in another tab or window. Switch to that window to continue."); }
    if (view.page !== pageKey) { pageKey = view.page; filter.value = ""; list.scrollTop = 0; }
    heading.textContent = view.title; intro.textContent = view.intro; intro.hidden = !view.intro;
    add.hidden = view.page === "history"; add.disabled = view.busy;
    add.setAttribute("aria-label", "New Workspace");
    primary.textContent = view.primary; primary.disabled = view.busy || !view.enabled;
    retry.hidden = !view.retry; retry.disabled = view.busy;
    error.textContent = view.error || ""; error.hidden = !view.error;
    if (JSON.stringify(view.rows) !== rowsKey) {
      const focusedId = list.contains(document.activeElement) ? document.activeElement.dataset.id : null;
      const scroll = list.scrollTop;
      rowsKey = JSON.stringify(view.rows); list.replaceChildren();
      for (const row of view.rows) {
        const container = element("div", "workspace-row"), select = button("", () => send({ type: "select", id: row.id }), "workspace-choice");
        select.dataset.id = row.id; select.setAttribute("role", "option");
        select.append(element("span", "workspace-row-title", row.title));
        if (row.subtitle) select.append(element("span", "workspace-row-subtitle", row.subtitle));
        // Enter/double-click invoke only selection, never the confirmation button.
        container.append(select);
        if (row.options) {
          const options = element("details", "workspace-options"), summary = element("summary", "", "⋮");
          summary.setAttribute("aria-label", `Options for ${row.title}`);
          const menu = element("div", "workspace-row-menu");
          for (const [kind, label] of [["rename", "Rename…"], ...(row.delete ? [["delete", "Delete…"]] : [])]) menu.append(button(label, () => {
            options.open = false; send({ type: "form", kind, id: row.id });
          }));
          options.append(summary, menu); container.append(options);
        }
        list.append(container);
      }
      if (focusedId) [...list.querySelectorAll('.workspace-choice')].find(node=>node.dataset.id===focusedId)?.focus({preventScroll:true});
      list.scrollTop = scroll;
    }
    for (const row of list.querySelectorAll(".workspace-choice")) { row.setAttribute("aria-selected", String(row.dataset.id === view.selected)); row.parentElement.dataset.selected = String(row.dataset.id === view.selected); row.disabled = view.busy; }
    const form = view.form;
    const key = form ? JSON.stringify([form.kind, form.id, form.name]) : null;
    if (key !== formKey) {
      formKey = key; formDialog.replaceChildren();
      if (form) {
        const heading = element("h2", "", form.title); heading.id = "workspace-form-title";
        formDialog.setAttribute("aria-labelledby", heading.id); formDialog.append(heading);
        if (form.message) formDialog.append(element("p", "", form.message));
        if (!["delete","reset","reset_brushes"].includes(form.kind)) {
          const label = element("label", "", "Name"); formName = element("input"); formName.value = form.name; formName.maxLength = 100;
          formName.setAttribute("aria-label", "Name"); label.append(formName); formDialog.append(label);
        }
        formSource = null;
        formError = element("p", "workspace-error"); const footer = element("footer");
        formSubmit = button(form.confirm, () => send({ type: "submit", name: formName?.value || "", source: formSource?.value || null }), "suggested-action");
        if (form.kind === "delete") formSubmit.classList.add("destructive-action");
        footer.append(button("Cancel", cancel), formSubmit); formDialog.append(formError, footer);
      }
    }
    if (form) { formError.textContent = view.error || ""; formSubmit.disabled = view.busy; formSubmit.textContent = view.retry && form.kind !== "recover" ? "Retry" : form.confirm; }
    show(dialog, !!view.page); show(formDialog, !!form);
    if (!view.page && !form && view.error) showRecovery(view.error);
  }
  let recovery, recoveryText, dismissedError;
  function showRecovery(text) {
    if (text === dismissedError) return;
    if (!recovery) {
      recovery = element("dialog", "workspace-form"); recovery.setAttribute("aria-label", "Workspace could not be saved");
      recoveryText = element("p"); const footer = element("footer");
      const hide = () => { recovery.close(); };
      recovery.addEventListener("cancel", e => { e.preventDefault(); dismissedError = view.error; hide(); send({type:"resume"}); });
      footer.append(button("Keep Open", () => { dismissedError = view.error; hide(); send({type:"resume"}); }), button("Retry", () => { dismissedError = null; hide(); send({ type: "retry" }); }),
        button("Save as New Workspace…", () => { hide(); send({ type: "form", kind: "recover" }); }, "suggested-action"));
      recovery.append(element("h2", "", "Workspace could not be saved"), recoveryText, footer); document.body.append(recovery);
    }
    recoveryText.textContent = text; if (!recovery.open) recovery.showModal();
  }
  function tick() { if (observePending) { observePending = false; app.workspace_observe(); } applyChange(app.workspace_tick()); render(); }
  // Reload replaces this document in the same browsing context. Reusing its
  // fenced identity avoids treating the old document's short lease as another
  // window. A new navigation/tab always receives an independent identity, even
  // when the browser copies its opener's sessionStorage.
  let owner;
  try { if (performance.getEntriesByType("navigation")[0]?.type === "reload") owner = JSON.parse(sessionStorage.getItem("capy.workspace.owner")); } catch {}
  if (!owner?.id || !owner?.epoch) owner = {id:crypto.randomUUID(),epoch:crypto.randomUUID()};
  try { sessionStorage.setItem("capy.workspace.owner",JSON.stringify(owner)); } catch {}
  app.workspace_start(store.execute, JSON.stringify(owner), hasLegacy, legacyError);
  timer = setInterval(tick, 100); tick();
  document.addEventListener("visibilitychange", () => send({ type: document.hidden ? "suspend" : "resume" }));
  window.addEventListener("pagehide", () => send({ type: "suspend" }));
  window.addEventListener("pageshow", () => send({ type: "resume" }));
  window.addEventListener("beforeunload", e => { if (view?.dirty || view?.busy) { e.preventDefault(); e.returnValue = ""; } });
  return {
    observe() { observePending = true; },
    send,
    handle(request) {
      const c = request.kind.command;
      const input = ({ manage: { type: "open", page: "workspaces" }, 
        layout_history: { type: "open", page: "history" }, new: { type: "form", kind: "new" },
        reset_layout: { type: "form", kind: "reset" }, reset_brushes: {type:"form",kind:"reset_brushes"} })[c.type];
      if (input) send(input); else if (c.type === "switch") send({ type: "switch", id: c.id });
      else if (c.type === "manage_toolbars") dispatch({ type: "customize", action: { type: "manage_toolbars" } });
      else if (c.type === "new_toolbar") dispatch({ type: "customize", action: { type: "new_toolbar", group: c.group } });
      else message("This toolbar command is unavailable.");
    },
  };
}
