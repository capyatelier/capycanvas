import { composingKey } from "./text-input.js";
// Native text/range controls around Rust's numeric policy. No expression,
// range-mapping, unit-formatting or rounding rules are duplicated here.
export function createNumberField({ control, label, labels: captions, resolve, onChange, icon, inline = false, widthSamples, valueOnly = false }) {
  const node = (tag, cls) => { const el = document.createElement(tag); el.className = cls; return el; };
  const initialCaptions = typeof captions === "function" ? captions(label) : captions;
  const root = node("div", `number-control number-${control.kind}`);
  const header = node("div", "number-header"), labels = node("div", "number-labels");
  const title = node("span", "number-title"); title.textContent = label; title.title = label; labels.append(title);
  const valueButton = node("button", "number-value"); valueButton.type = "button";
  valueButton.setAttribute("aria-label", initialCaptions.edit);
  const entry = node("input", "number-entry"); entry.type = "text"; entry.inputMode = "decimal";
  entry.spellcheck = false; entry.autocomplete = "off"; entry.setAttribute("aria-label", label);
  const valueBox = node("div", "number-value-box"); valueBox.append(valueButton, entry);
  header.append(labels, valueBox); root.append(header);
  const track = node("div", "number-track");
  const slider = node("input", "number-slider"); slider.type = "range"; slider.min = 0; slider.max = 1; slider.step = "any";
  slider.setAttribute("aria-label", label);
  const step = (steps, name, caption) => {
    const button = node("button", "number-step"); button.type = "button"; button.append(icon(name));
    button.setAttribute("aria-label", caption);
    button.addEventListener("click", () => { if (finish()) apply({ type: "step", steps }); });
    return button;
  };
  const minus = step(-1, "minus", initialCaptions.decrease), plus = step(1, "plus", initialCaptions.increase);
  const ranged = control.kind === "slider";
  const buttonValue = ranged || valueOnly;
  if (ranged) { track.append(minus, slider, plus); root.append(track); }
  else { valueBox.classList.add("number-spin"); valueBox.append(minus, plus); }
  if (inline) {
    root.classList.add("number-inline"); root.title = label;
    labels.remove(); minus.remove(); plus.remove();
    root.replaceChildren(track, valueBox);
    const measure = node("span", "number-measure");
    measure.textContent = (widthSamples || [control.min, control.max].map(value => resolve({ control, value, operation: { type: "format" } }).text))
      .sort((a, b) => b.length - a.length)[0].replace(/\d/g, "8");
    valueBox.append(measure);
    if (valueOnly) { root.classList.add('number-value-only'); entry.size = 1; }
  }
  let value = control.min, display, presented, editing = false, disabled = false;
  let gesture = false, cancelled = false, heldKey, releaseTimer, releasePointer;
  const finishGesture = (phase = 'up') => {
    clearTimeout(releaseTimer); releaseTimer = null;
    releasePointer?.(); releasePointer = null;
    window.removeEventListener('blur', blurGesture);
    const active=gesture&&!cancelled;
    gesture = cancelled = false; heldKey = null;
    if(active)root.onEditPhase?.(phase);
  };
  const blurGesture = () => finishGesture();
  const beginGesture = () => {
    if(releaseTimer)finishGesture();
    if(!root.onEditPhase || disabled || gesture)return;
    gesture = true;root.onEditPhase('down');window.addEventListener('blur',blurGesture);
  };
  root.addEventListener('pointerdown', e => {
    if(e.button || !(e.target===slider || e.target.closest('.number-step')))return;
    document.activeElement?.blur?.();
    beginGesture();if(!gesture)return;
    const end = event => {
      if(event.pointerId!==e.pointerId)return;
      if(event.type==='pointercancel')finishGesture('cancel');
      else releaseTimer=setTimeout(()=>finishGesture(),0);
    };
    window.addEventListener('pointerup',end,true);window.addEventListener('pointercancel',end,true);
    releasePointer=()=>{window.removeEventListener('pointerup',end,true);window.removeEventListener('pointercancel',end,true);};
  },true);
  root.addEventListener('lostpointercapture',()=>{if(gesture&&!releaseTimer)finishGesture('cancel');});
  root.addEventListener('keydown',e=>{
    if(composingKey(e))return;
    if(e.key==='Escape'&&gesture){
      e.preventDefault();e.stopPropagation();
      if(!cancelled){root.onEditPhase?.('cancel');cancelled=true;}finish(true);return;
    }
    if((e.target===entry&&['ArrowUp','ArrowDown'].includes(e.key)) || (e.target===slider&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','PageUp','PageDown','Home','End'].includes(e.key))){
      if(heldKey&&heldKey!==e.key)finishGesture();
      beginGesture();heldKey=e.key;
    }
  },true);
  root.addEventListener('keyup',e=>{if(e.key===heldKey)finishGesture();},true);
  root.addEventListener('focusout',e=>{if(gesture&&!root.contains(e.relatedTarget))finishGesture();});
  function show(next) {
    value = next.value; display = next; valueButton.textContent = next.text;
    if (valueOnly) valueBox.querySelector('.number-measure').textContent = next.text;
    if (!editing) entry.value = ranged ? next.edit : next.text;
    entry.setAttribute("aria-valuenow", value * control.scale);
    slider.value = next.fill; slider.style.setProperty("--fill", `${next.fill * 100}%`);
    slider.setAttribute("aria-valuetext", next.text);
    minus.disabled = disabled || value <= control.min; plus.disabled = disabled || value >= control.max;
  }
  function apply(operation) {
    if(cancelled || (operation.type==='expression' && presented!=null && operation.text===presented))return true;
    try {
      const next = resolve({ control, value, operation });
      root.classList.remove("error"); entry.removeAttribute("aria-invalid"); entry.title = "";
      const changed = next.value !== value; show(next); if (changed) onChange(next.value);
      return true;
    } catch (error) { root.classList.add("error"); entry.setAttribute("aria-invalid", "true"); entry.title = String(error); return false; }
  }
  function begin() {
    if (disabled) return;
    editing = true; entry.value = display.edit; entry.size = valueOnly ? 1 : Math.min(10, Math.max(3, display.edit.length));
    valueButton.hidden = true; entry.hidden = false; entry.focus(); entry.select();
  }
  function finish(cancel = false) {
    if (!editing) return true;
    if (!cancel && (composingKey({target:entry}) || !apply({ type: "expression", text: entry.value }))) return false;
    editing = false; root.classList.remove("error"); entry.removeAttribute("aria-invalid"); entry.title = "";
    entry.value = ranged ? display.edit : display.text;
    if (buttonValue) { entry.hidden = true; valueButton.hidden = false; }
    return true;
  }
  valueButton.addEventListener("click", begin);
  entry.addEventListener("focus", () => { editing = true; });
  entry.addEventListener("input", () => { editing = true; });
  entry.addEventListener("blur", () => finish());
  entry.addEventListener("keydown", e => {
    if (composingKey(e)) return;
    if (e.key === "Enter" || e.key === "Escape") {
      e.preventDefault(); e.stopPropagation();
      if (finish(e.key === "Escape")) { entry.blur(); if (buttonValue) valueButton.focus(); }
    } else if (e.key === "Tab" && !finish()) e.preventDefault();
    else if (!ranged && ["ArrowUp", "ArrowDown"].includes(e.key)) {
      e.preventDefault(); if (finish()) apply({ type: "step", steps: e.key === "ArrowUp" ? 1 : -1 });
    }
  });
  slider.addEventListener("input", () => { if (finish()) apply({ type: "position", position: Number(slider.value) }); else show(display); });
  root.update = (next,text) => {
    if(!display || next!==value || text!==presented){presented=text;const result=resolve({control,value:next,operation:{type:'format'}});if(text!=null)result.text=result.edit=text;show(result);}
  };
  root.getValue = () => value;
  root.setDisabled = next => { if (disabled === next) return; if(next)finishGesture('cancel');disabled = next; entry.disabled = next; valueButton.disabled = next; slider.disabled = next; show(display); };
  root.setDescription = text => {
    labels.querySelector('.number-description')?.remove();
    if (text) { const p = node("p", "number-description"); p.textContent = text; labels.append(p); }
  };
  root.relabel = next => {
    label = next;
    const text = typeof captions === "function" ? captions(next) : captions;
    title.textContent = title.title = next;
    if (inline) root.title = next;
    entry.setAttribute("aria-label", next); slider.setAttribute("aria-label", next);
    valueButton.setAttribute("aria-label", text.edit);
    minus.setAttribute("aria-label", text.decrease); plus.setAttribute("aria-label", text.increase);
    if (root.classList.contains("error")) {
      try { resolve({control, value, operation:{type:"expression",text:entry.value}}); }
      catch (error) { entry.title = String(error); }
    }
  };
  root.entry = entry;
  root.valueButton = valueButton;
  root.slider = slider;
  root.format = () => {const result=resolve({control,value,operation:{type:'format'}});if(presented!=null)result.text=result.edit=presented;show(result);};
  root.apply = apply;
  root.cancelEditing = () => finish(true);
  root.commit = () => finish();
  root.dispose = () => {finishGesture('cancel');finish(true);};
  entry.hidden = buttonValue; valueButton.hidden = !buttonValue;
  if (!ranged) { entry.setAttribute("role", "spinbutton"); entry.setAttribute("aria-valuemin", control.min * control.scale); entry.setAttribute("aria-valuemax", control.max * control.scale); }
  root.update(value);
  return root;
}

// Chromium's native range drag does not consistently consume tablet contacts.
// Keep the native keyboard control and capture pointer input on the track,
// mapping positions to the thumb's centre as the native control does.
export function captureSliderContacts(number) {
  const { slider } = number;
  let contact = null;
  const pick = e => {
    const b = slider.getBoundingClientRect(), thumb = parseFloat(getComputedStyle(slider).getPropertyValue("--thumb-size")) || 0;
    if (!number.commit()) return false;
    number.apply({ type: "position", position: (e.clientX - b.x - thumb / 2) / Math.max(1, b.width - thumb) });
    return true;
  };
  slider.addEventListener("pointerdown", e => {
    if (e.button || slider.disabled) return;
    e.preventDefault(); e.stopPropagation();
    if (!pick(e)) return;
    slider.focus({preventScroll:true});
    contact = e.pointerId; slider.setPointerCapture(e.pointerId);
  });
  slider.addEventListener("pointermove", e => { if (contact === e.pointerId) { e.preventDefault(); pick(e); } });
  for (const type of ["pointerup", "pointercancel", "lostpointercapture"]) slider.addEventListener(type, () => { contact = null; });
}
