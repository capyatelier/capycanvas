// DOM focus/IME and modal capture; Rust owns search, selection and execution.
export function createCommandBar({element, button, icon, dispatch, style, canvas}) {
  const dialog = element("dialog", "command-bar");
  dialog.id = "command-bar";
  dialog.setAttribute("aria-label", "Search commands");
  for (const [key, value] of Object.entries(style)) dialog.style.setProperty(`--command-${key.replaceAll("_", "-")}`, `${value}px`);
  const header = element("div", "command-search-header");
  const field = element("div", "command-search-field");
  field.append(icon("search"));
  const input = element("input", "command-search-input");
  input.id = "command-search"; input.type = "search"; input.placeholder = "Search commands";
  input.autocomplete = "off"; input.spellcheck = false;
  input.setAttribute("role", "combobox"); input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-controls", "command-results"); input.setAttribute("aria-label", "Search commands");
  field.append(input);
  const unit = element("span", "command-unit");
  const close = button("×", () => send({type:"close"}), "command-search-close");
  close.id = "command-search-close"; close.setAttribute("aria-label", "Close command search");
  header.append(field, unit, close);
  const list = element("div", "command-results"); list.id = "command-results";
  list.setAttribute("role", "listbox"); list.setAttribute("aria-label", "Commands");
  const empty = element("div", "command-empty", "No matching commands");
  const detail = element("div", "command-detail"); detail.id = "command-detail";
  detail.setAttribute("aria-live", "polite");
  input.setAttribute("aria-describedby", detail.id);
  dialog.append(header, list, empty, detail); document.body.append(dialog);
  let view = null, signature = "", previousFocus, focus = "canvas", outside = false;
  const send = action => dispatch({type:"command_search", action});
  function fitViewport() {
    if (!view) return;
    const viewport = window.visualViewport, height = viewport?.height ?? innerHeight;
    const top = innerWidth <= 600 ? 16 : Math.min(72, height * .12);
    dialog.style.top = `${top + (viewport?.offsetTop ?? 0)}px`;
    dialog.style.maxHeight = `${height - top - 16}px`;
  }
  window.visualViewport?.addEventListener("resize", fitViewport);
  window.addEventListener("resize", fitViewport);
  function rememberFocus(target) {
    if (view || !(target instanceof Element) || target.closest("dialog, .window-bar, .popover, [popover]")) return;
    const next = target.matches("input,textarea,[contenteditable=true]") ? "text" : target.closest(".palettes-panel") ? "palette" : "canvas";
    if (next !== focus) { focus = next; send({type:"focus", focus}); }
  }
  document.addEventListener("focusin", e => rememberFocus(e.target));
  document.addEventListener("pointerdown", e => rememberFocus(e.target), {capture:true});
  input.addEventListener("input", () => { if (view && !view.parameter) send({type:"query", text:input.value}); });
  function back() { send({type:view?.parameter ? "back" : "close"}); }
  dialog.addEventListener("cancel", e => { e.preventDefault(); back(); });
  dialog.addEventListener("keydown", e => {
    if (!view) return;
    e.stopPropagation();
    if (e.isComposing) return;
    if (e.key === "Escape") { e.preventDefault(); back(); }
    else if (["ArrowDown", "ArrowUp"].includes(e.key) && !view?.parameter) {
      e.preventDefault(); send({type:"move", delta:e.key === "ArrowDown" ? 1 : -1});
    } else if (e.key === "Enter" && e.target !== close) {
      e.preventDefault(); send({type:"commit", text:input.value});
    }
  });
  const outsideBounds = e => { const r = dialog.getBoundingClientRect(); return e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom; };
  dialog.addEventListener("pointerdown", e => { outside = e.target === dialog && outsideBounds(e); });
  // Keep the modal through the complete dismissal click so its release cannot paint.
  dialog.addEventListener("click", e => {
    if (outside && e.target === dialog && outsideBounds(e)) { e.preventDefault(); e.stopPropagation(); send({type:"close"}); }
    outside = false;
  });
  dialog.addEventListener("pointercancel", () => { outside = false; });
  return {
    refresh(next) {
      const wasOpen = !!view, previousParameter = view?.parameter?.id;
      view = next ? {...next, selected:Number(next.selected)} : null;
      if (!view) {
        if (dialog.open) dialog.close();
        if (wasOpen && !document.querySelector("dialog[open]")) {
          if (previousFocus?.isConnected && !dialog.contains(previousFocus)) previousFocus.focus({preventScroll:true});
          if (document.activeElement === document.body || dialog.contains(document.activeElement)) canvas.focus({preventScroll:true});
        }
        return;
      }
      const parameter = view.parameter;
      if (!wasOpen || previousParameter !== parameter?.id) {
        input.value = parameter?.parameter.text ?? view.query;
        if (parameter) input.select();
      }
      input.placeholder = parameter ? "Enter a value" : "Search commands";
      input.setAttribute("aria-label", parameter?.label ?? "Search commands");
      input.setAttribute("aria-expanded", String(!parameter));
      unit.textContent = parameter?.parameter.numeric.unit ?? "";
      unit.hidden = !unit.textContent;
      const nextSignature = JSON.stringify(view.results);
      if (signature !== nextSignature) {
        signature = nextSignature;
        list.replaceChildren(...view.results.map((command, index) => {
          const row = button("", () => send({type:"execute", id:command.id, value:null}), "command-result");
          row.id = `command-result-${index}`; row.tabIndex = -1;
          row.setAttribute("role", "option"); row.setAttribute("aria-disabled", String(!command.enabled));
          row.append(element("span", "command-name", command.label));
          if (command.selected) { const check = element("span", "command-check", "✓"); check.setAttribute("aria-label", "On"); row.append(check); }
          row.append(element("span", "command-shortcut", command.shortcut));
          row.addEventListener("pointerdown", e => e.preventDefault());
          row.addEventListener("pointermove", () => {
            if (view?.results[view.selected]?.id !== command.id) send({type:"select", id:command.id});
          });
          return row;
        }));
      }
      list.hidden = !!parameter; empty.hidden = !!parameter || view.results.length !== 0;
      [...list.children].forEach((row, index) => {
        row.dataset.selected = String(index === view.selected);
        row.setAttribute("aria-selected", String(index === view.selected));
      });
      const selected = parameter ?? view.results[view.selected];
      if (selected && !parameter) input.setAttribute("aria-activedescendant", `command-result-${view.selected}`);
      else input.removeAttribute("aria-activedescendant");
      detail.textContent = view.error ?? selected?.disabled_reason ?? (parameter ? parameter.label : selected?.category) ?? "";
      if (!wasOpen) {
        previousFocus = document.activeElement;
        dialog.showModal(); input.focus({preventScroll:true});
        if (!matchMedia("(prefers-reduced-motion: reduce)").matches)
          dialog.animate([{opacity:0, transform:"translateY(-4px)"}, {opacity:1, transform:"translateY(0)"}], {duration:120, easing:"cubic-bezier(.2,.8,.2,1)"});
      }
      fitViewport();
      if (!parameter) list.children[view.selected]?.scrollIntoView({block:"nearest"});
    },
  };
}
