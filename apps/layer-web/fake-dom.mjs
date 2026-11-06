export class FakeElement {
  showPopover() { if(this.popoverOpen)return;this.popoverOpen=true;this.dispatchEvent({type:'toggle',oldState:'closed',newState:'open'}); }
  hidePopover() { if(!this.popoverOpen)return;this.popoverOpen=false;this.dispatchEvent({type:'toggle',oldState:'open',newState:'closed'}); }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(type, listener) { (this.listeners[type] ??= []).push(listener); }
  removeEventListener(type, listener) { this.listeners[type] = (this.listeners[type] ?? []).filter(current => current !== listener); }
  dispatchEvent(event) { for (const listener of this.listeners[event.type] ?? []) listener(event); return !event.defaultPrevented; }
  append(...nodes) { for (const node of nodes) { node.parentNode = this; this.children.push(node); } }
  replaceChildren(...nodes) { for (const node of this.children) node.parentNode = null; this.children = []; this.append(...nodes); }
}
