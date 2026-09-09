// Native text/range controls around Rust's numeric policy. No expression,
// range-mapping, unit-formatting or rounding rules are duplicated here.
export function createNumberField({ control, label, resolve, onChange, icon }) {
  const node = (tag, cls) => { const el = document.createElement(tag); el.className = cls; return el; };
  const root = node("div", `number-control number-${control.kind}`);
  const header = node("div", "number-header"), labels = node("div", "number-labels");
  const title = node("span", "number-title"); title.textContent = label; title.title = label; labels.append(title);
  const valueButton = node("button", "number-value"); valueButton.type = "button";
  valueButton.setAttribute("aria-label", `Edit ${label}`);
  const entry = node("input", "number-entry"); entry.type = "text"; entry.inputMode = "decimal";
  entry.spellcheck = false; entry.autocomplete = "off"; entry.setAttribute("aria-label", label);
  const valueBox = node("div", "number-value-box"); valueBox.append(valueButton, entry);
  header.append(labels, valueBox); root.append(header);
  const track = node("div", "number-track");
  const slider = node("input", "number-slider"); slider.type = "range"; slider.min = 0; slider.max = 1; slider.step = "any";
  slider.setAttribute("aria-label", label);
  const step = (steps, name, verb) => {
    const button = node("button", "number-step"); button.type = "button"; button.append(icon(name));
    button.setAttribute("aria-label", `${verb} ${label}`);
    button.addEventListener("click", () => { if (finish()) apply({ type: "step", steps }); });
    return button;
  };
  const minus = step(-1, "minus", "Decrease"), plus = step(1, "plus", "Increase");
  const ranged = control.kind === "slider";
  if (ranged) { track.append(minus, slider, plus); root.append(track); }
  else { valueBox.classList.add("number-spin"); valueBox.append(minus, plus); }
  let value = control.min, display, editing = false, disabled = false;
  function show(next) {
    value = next.value; display = next; valueButton.textContent = next.text;
    if (!editing) entry.value = ranged ? next.edit : next.text;
    entry.setAttribute("aria-valuenow", value * control.scale);
    slider.value = next.fill; slider.style.setProperty("--fill", `${next.fill * 100}%`);
    slider.setAttribute("aria-valuetext", next.text);
    minus.disabled = disabled || value <= control.min; plus.disabled = disabled || value >= control.max;
  }
  function apply(operation) {
    try {
      const next = resolve({ control, value, operation });
      root.classList.remove("error"); entry.removeAttribute("aria-invalid"); entry.title = "";
      const changed = next.value !== value; show(next); if (changed) onChange(next.value);
      return true;
    } catch (error) { root.classList.add("error"); entry.setAttribute("aria-invalid", "true"); entry.title = String(error); return false; }
  }
  function begin() {
    if (disabled) return;
    editing = true; entry.value = display.edit; entry.size = Math.min(10, Math.max(3, display.edit.length));
    valueButton.hidden = true; entry.hidden = false; entry.focus(); entry.select();
  }
  function finish(cancel = false) {
    if (!editing) return true;
    if (!cancel && !apply({ type: "expression", text: entry.value })) return false;
    editing = false; root.classList.remove("error"); entry.removeAttribute("aria-invalid"); entry.title = "";
    entry.value = ranged ? display.edit : display.text;
    if (ranged) { entry.hidden = true; valueButton.hidden = false; }
    return true;
  }
  valueButton.addEventListener("click", begin);
  entry.addEventListener("focus", () => { editing = true; });
  entry.addEventListener("input", () => { editing = true; });
  entry.addEventListener("blur", () => finish());
  entry.addEventListener("keydown", e => {
    if (e.isComposing) return;
    if (e.key === "Enter" || e.key === "Escape") {
      e.preventDefault(); e.stopPropagation();
      if (finish(e.key === "Escape")) { entry.blur(); if (ranged) valueButton.focus(); }
    } else if (e.key === "Tab" && !finish()) e.preventDefault();
    else if (!ranged && ["ArrowUp", "ArrowDown"].includes(e.key)) {
      e.preventDefault(); if (finish()) apply({ type: "step", steps: e.key === "ArrowUp" ? 1 : -1 });
    }
  });
  slider.addEventListener("input", () => { finish(true); apply({ type: "position", position: Number(slider.value) }); });
  root.update = next => show(resolve({ control, value: next, operation: { type: "format" } }));
  root.setDisabled = next => { disabled = next; entry.disabled = next; valueButton.disabled = next; slider.disabled = next; show(display); };
  root.setDescription = text => { const p = node("p", "number-description"); p.textContent = text; labels.append(p); };
  root.entry = entry;
  entry.hidden = ranged; valueButton.hidden = !ranged;
  if (!ranged) { entry.setAttribute("role", "spinbutton"); entry.setAttribute("aria-valuemin", control.min * control.scale); entry.setAttribute("aria-valuemax", control.max * control.scale); }
  root.update(value);
  return root;
}
