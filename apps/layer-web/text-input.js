const preedit = new WeakSet(), ended = new WeakMap(), held = new WeakMap(), owned = new WeakSet();
let compositionRoot;

const latinPairs = {Backquote:'`~', Minus:'-_', Equal:'=+', BracketLeft:'[{', BracketRight:']}',
  Backslash:'\\|', IntlBackslash:'\\|', Semicolon:';:', Quote:'\'"', Comma:',<', Period:'.>', Slash:'/?'};
export function latinKey({key, code, shiftKey}) {
  const pair = /^Key[A-Z]$/.test(code) ? code[3].toLowerCase() + code[3]
    : /^Digit[0-9]$/.test(code) ? code[5] + ')!@#$%^&*('[Number(code[5])] : latinPairs[code];
  const latin = pair?.[shiftKey ? 1 : 0];
  return latin && latin !== key ? latin : null;
}

export function captureTextComposition(root, releaseKey = () => {}) {
  compositionRoot = root;
  root.addEventListener("cancel", event => {
    if (!composingKey(event)) return;
    event.preventDefault(); event.stopImmediatePropagation();
  }, true);
  root.addEventListener("compositionstart", event => preedit.add(event.target), true);
  root.addEventListener("compositionend", event => {
    preedit.delete(event.target);
    ended.set(event.target, event.timeStamp);
  }, true);
  root.addEventListener("keydown", event => {
    const key = event.code || event.key, keys = held.get(event.target) ?? new Map();
    if (!event.repeat && keys.get(key) !== event.timeStamp) keys.delete(key);
    if (!composingKey(event)) return;
    owned.add(event);
    if (event.key !== "Process" && event.keyCode !== 229) keys.set(key, event.timeStamp);
    held.set(event.target, keys);
    event.stopImmediatePropagation();
  }, true);
  root.addEventListener("keyup", event => {
    if (composingKey(event)) { owned.add(event); releaseKey(event); event.stopImmediatePropagation(); }
    held.get(event.target)?.delete(event.code || event.key);
  }, true);
  root.addEventListener("pointerdown", () => held.delete(root.activeElement), true);
  root.addEventListener("focusout", event => {
    preedit.delete(event.target); ended.delete(event.target); held.delete(event.target);
  }, true);
}

export function composingKey(event) {
  const active = compositionRoot?.activeElement;
  const keys = held.get(event.target) ?? held.get(active);
  return event.isComposing || event.keyCode === 229 || owned.has(event)
    || preedit.has(event.target) || preedit.has(active)
    || event.timeStamp <= ended.get(event.target)
    || (event.code || event.key ? keys?.has(event.code || event.key) : !!keys?.size);
}
