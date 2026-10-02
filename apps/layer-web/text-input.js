const preedit = new WeakSet();
let compositionRoot;

export function captureTextComposition(root) {
  compositionRoot = root;
  root.addEventListener("cancel", event => {
    if (!composingKey(event)) return;
    event.preventDefault(); event.stopImmediatePropagation();
  }, true);
  root.addEventListener("compositionstart", event => preedit.add(event.target), true);
  root.addEventListener("compositionend", event => preedit.delete(event.target), true);
  root.addEventListener("focusout", event => preedit.delete(event.target), true);
}

export function composingKey(event) {
  return event.isComposing || event.keyCode === 229 || preedit.has(event.target) || preedit.has(compositionRoot?.activeElement);
}
