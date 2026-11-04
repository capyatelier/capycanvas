import {createDialogHost} from './dialog-host.js';
import { bindCopy } from "./localization.js";
// DOM focus/IME and modal capture; Rust owns search, selection and execution.
export function createCommandBar({app,element, button, icon, dispatch, style, canvas, layoutChanged, nativeCopy}) {
  const copy = nativeCopy.search;
  let view = null, signature = "", focus = "canvas";
  const dialog = element("dialog", "command-bar");
  dialog.id = "command-bar";
  bindCopy(dialog, () => copy.search_commands, "ariaLabel");
  for (const [key, value] of Object.entries(style)) dialog.style.setProperty(`--command-${key.replaceAll("_", "-")}`, `${value}px`);
  const header = element("div", "command-search-header");
  const field = element("div", "command-search-field");
  field.append(icon("search"));
  const input = element("input", "command-search-input");
  input.id = "command-search"; input.type = "search";
  bindCopy(input, () => view?.parameter ? copy.enter_value : copy.search_commands, "placeholder");
  input.autocomplete = "off"; input.spellcheck = false;
  input.setAttribute("role", "combobox"); input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-controls", "command-results");
  bindCopy(input, () => view?.parameter?.label ?? copy.search_commands, "ariaLabel");
  field.append(input);
  const unit = element("span", "command-unit");
  const close = button("", () => send({type:"close"}), "command-search-close");close.append(icon("window-close"));
  close.id = "command-search-close"; bindCopy(close, () => copy.close_search, "ariaLabel");
  header.append(field, unit, close);
  const list = element("div", "command-results"); list.id = "command-results";
  list.setAttribute("role", "listbox"); bindCopy(list, () => copy.commands, "ariaLabel");
  const empty = element("div", "command-empty"); bindCopy(empty, () => copy.no_matches);
  const detail = element("div", "command-detail"); detail.id = "command-detail";
  detail.setAttribute("aria-live", "polite");
  input.setAttribute("aria-describedby", detail.id);
  dialog.append(header, list, empty, detail); document.body.append(dialog);
  const send = action => dispatch({type:"command_search", action});
  const shell=createDialogHost({app,dialog,header,cancel:close,primary:close,icon,copy:app.bootstrap_view().common,onCancel:()=>send({type:'close'}),
    onDismiss:()=>send({type:'dismiss',search_focused:document.activeElement===input,text:input.value}),
    placement:visible=>{const bounds=app.command_search_placement(visible.width,visible.height);return {x:bounds.x+visible.x,y:bounds.y+visible.y,width:bounds.width,maxHeight:bounds.height};}});
  function fitViewport() {
    if (!view) return;
    shell.layout();
    layoutChanged();
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
  dialog.addEventListener("keydown", e => {
    if (!view) return;
    e.stopPropagation();
    if (["ArrowDown", "ArrowUp"].includes(e.key) && !view?.parameter) {
      e.preventDefault(); send({type:"move", delta:e.key === "ArrowDown" ? 1 : -1});
    } else if (e.key === "Enter" && e.target !== close) {
      e.preventDefault(); send({type:"commit", text:input.value});
    }
  });
  return {
    refresh(next) {
      const wasOpen = !!view, previousParameter = view?.parameter?.id;
      view = next ? {...next, selected:Number(next.selected)} : null;
      if (!view) {
        shell.hide();if(wasOpen){layoutChanged();if(document.activeElement===document.body)canvas.focus({preventScroll:true});}
        return;
      }
      const parameter = view.parameter;
      if (!wasOpen || previousParameter !== parameter?.id) {
        input.value = parameter?.parameter.text ?? view.query;
        if (parameter) input.select();
      }
      if(!parameter&&input.value!==view.query)input.value=view.query;
      input.placeholder = parameter ? copy.enter_value : copy.search_commands;
      input.setAttribute("aria-label", parameter?.label ?? copy.search_commands);
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
          if (command.selected) { const check = element("span", "command-check", "✓"); bindCopy(check, () => copy.selected, "ariaLabel"); row.append(check); }
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
      detail.textContent = view.detail;
      if (!wasOpen) {
        shell.present(view.dialog); input.focus({preventScroll:true});
        if (!matchMedia("(prefers-reduced-motion: reduce)").matches)
          dialog.animate([{opacity:0, transform:"translateY(-4px)"}, {opacity:1, transform:"translateY(0)"}], {duration:120, easing:"cubic-bezier(.2,.8,.2,1)"}).onfinish = layoutChanged;
      }
      fitViewport();
      if (!parameter) list.children[view.selected]?.scrollIntoView({block:"nearest"});
    },
  };
}
