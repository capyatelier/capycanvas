const copies = new WeakMap(), bindings = new Set(), nodeBindings = Symbol("localized DOM bindings"), textBinding = Symbol("localized text");

export function updateCopy(target, source) {
  for (const key of Object.keys(target)) if (!Object.hasOwn(source,key)) delete target[key];
  for (const [key, value] of Object.entries(source)) {
    if (value && typeof value === 'object' && target[key] && typeof target[key] === 'object' && Array.isArray(value) === Array.isArray(target[key])) updateCopy(target[key], value);
    else target[key] = value;
  }
  if (Array.isArray(source)) target.length = source.length;
  return target;
}

export function liveCopy(app, method) {
  let owned = copies.get(app);
  if (!owned) copies.set(app, owned = new Map());
  if (!owned.has(method)) owned.set(method, app[method]());
  return owned.get(method);
}

export function bindCopy(node, read, property = 'textContent') {
  const owner = node;
  if(property === 'textContent' && node.ownerDocument) {
    node = node[textBinding] ??= node.firstChild?.nodeType === 3 ? node.firstChild : node.insertBefore(node.ownerDocument.createTextNode(''),node.firstChild);
    property = 'nodeValue';
  }
  if (!node[nodeBindings]) node[nodeBindings] = new Map();
  if (!node[nodeBindings].has(property)) bindings.add({node:new WeakRef(node),property});
  node[nodeBindings].set(property,read);
  node[property] = read();
  return owner;
}

export function refreshCopy(app) {
  for (const [method, copy] of copies.get(app) ?? []) updateCopy(copy, app[method]());
  refreshBindings();
}

export function refreshBindings() {
  for (const binding of bindings) {
    const node = binding.node.deref();
    if (!node) { bindings.delete(binding); continue; }
    const value = node[nodeBindings].get(binding.property)();
    if (node[binding.property] !== value) node[binding.property] = value;
  }
}
