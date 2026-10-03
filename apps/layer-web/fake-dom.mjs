export class FakeElement {
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(type, listener) { (this.listeners[type] ??= []).push(listener); }
  dispatchEvent(event) { for (const listener of this.listeners[event.type] ?? []) listener(event); return !event.defaultPrevented; }
  append(...nodes) { for (const node of nodes) { node.parentNode = this; this.children.push(node); } }
}
