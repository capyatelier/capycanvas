import {FakeElement} from './fake-dom.mjs';
import {bindCopy} from './localization.js';
import {configureColorEditor} from './color-editor.js';

export class Element extends FakeElement {
  constructor(tag, doc) {
    super();
    Object.assign(this, {tagName:tag.toUpperCase(), ownerDocument:doc, children:[], attributes:new Map(), listeners:{}, value:'', style:{}, dataset:{}, hidden:false, selectionStart:0, selectionEnd:0, clientWidth:0, offsetHeight:0});
    const classes = new Set();
    this.classList = {add:name => classes.add(name), remove:name => classes.delete(name), toggle:(name, on = !classes.has(name)) => (on ? classes.add(name) : classes.delete(name), on), contains:name => classes.has(name)};
  }
  get firstChild() { return this.children[0]; }
  get textContent() { return this.children.map(node => node.nodeType === 3 ? node.nodeValue : node.textContent).join(''); }
  set textContent(value) { this.children.forEach(node => node.parentNode = null); this.children = []; if (value !== '') this.append(this.ownerDocument.createTextNode(value)); }
  get ariaLabel() { return this.getAttribute('aria-label'); }
  set ariaLabel(value) { this.setAttribute('aria-label', value); }
  get placeholder() { return this.getAttribute('placeholder'); }
  set placeholder(value) { this.setAttribute('placeholder', value); }
  hasAttribute(name) { return this.attributes.has(name); }
  removeAttribute(name) { this.attributes.delete(name); }
  toggleAttribute(name, on) { if (on) this.attributes.set(name, ''); else this.attributes.delete(name); }
  insertBefore(node, before) { if (node.parentNode) node.parentNode.children.splice(node.parentNode.children.indexOf(node), 1); node.parentNode = this; this.children.splice(before ? this.children.indexOf(before) : this.children.length, 0, node); return node; }
  remove() { this.parentNode?.children.splice(this.parentNode.children.indexOf(this), 1); this.parentNode = null; }
  replaceChildren(...nodes) { this.children.forEach(node => node.parentNode = null); this.children = []; this.append(...nodes); }
  contains(node) { for (let current = node; current; current = current.parentNode) if (current === this) return true; return false; }
  closest() { return null; }
  matches(selector) { return selector === 'input' && this.tagName === 'INPUT'; }
  focus() { this.ownerDocument.activeElement = this; }
  blur() {}
  select() { this.selectionStart = 0; this.selectionEnd = String(this.value).length; }
  setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; }
  setPointerCapture() {}
  getContext() { return new Proxy({}, {get:() => () => ({addColorStop() {}})}); }
  getBoundingClientRect() { return {x:0, y:0, left:0, top:0, right:0, bottom:0, width:0, height:0}; }
  click() { this.dispatchEvent({type:'click', preventDefault() {}}); }
  showModal() { this.open = true; }
  close() { this.open = false; this.dispatchEvent({type:'close'}); }
  descendants() { return this.children.flatMap(node => node.tagName ? [node, ...node.descendants()] : []); }
}

export function colorDialogDom(t) {
  const previous = Object.fromEntries(['document', 'requestAnimationFrame', 'cancelAnimationFrame', 'ResizeObserver', 'devicePixelRatio'].map(key => [key, globalThis[key]]));
  const doc = {createTextNode:nodeValue => ({nodeType:3, nodeValue:String(nodeValue)}), createElementNS:(_, tag) => new Element(tag, doc), createElement:tag => new Element(tag, doc), activeElement:null,
    querySelector:selector => selector === 'dialog[open]' ? doc.body.descendants().find(node => node.tagName === 'DIALOG' && node.open) ?? null : null};
  doc.body = new Element('body', doc);
  Object.assign(globalThis, {document:doc, requestAnimationFrame:() => 0, cancelAnimationFrame() {}, ResizeObserver:class { observe() {} disconnect() {} }, devicePixelRatio:1});
  t.after(() => Object.assign(globalThis, previous));
  const element = (tag, cls, text) => { const node = new Element(tag, doc); node.className = cls ?? ''; if (text != null) { if (typeof text === 'function') bindCopy(node, text); else node.textContent = text; } return node; };
  const button = (text, action, cls) => { const node = element('button', cls, text); node.addEventListener('click', action); return node; };
  const icon = asset => { const node = element('svg'); node.dataset.asset = asset; return node; };
  const actions = [];
  configureColorEditor({dispatch:action => actions.push(action), icon});
  return {doc, element, button, icon, actions};
}

const row = (label, values, language) => ({form:label.toLowerCase(), label, forms:[{form:label.toLowerCase(), label}], space:label === 'RGB' ? 'sRGB' : null,
  values:values.map((text, i) => ({text, edit:text.replace(/[°%]/g, ''), name:`${language}:value${i}`})), copy:`${label.toLowerCase()}(${values.join(' ')})`});

export function fakeColorApp({language = () => 'en', selected = {space:'Srgb', rgba:[.25, .5, .75, 1]}, hdr = false} = {}) {
  const stats = {requests:0, actions:[]};
  const view = editor => ({
    panel:{hdr, intensity:0, rgb_space:'Srgb', shape:'circle', rendition:null}, shapes:['circle', 'square', 'triangle'].map(shape => ({shape, label:shape, name:shape, selected:shape === 'circle'})),
    current:{rgba:[1, 1, 1, 1]}, new:{rgba:editor.value.rgba}, hex:editor.hex, hex_note:null,
    rows:[row('RGB', ['64', '128', '191'], language()), row('HSB', ['210°', '67%', '75%'], language()), row('OKLCH', ['58.1%', '0.112', '247°'], language())],
    intensity:hdr ? {text:'+1.00 EV', edit:'1', name:`${language()}:intensity`} : null, value:editor.value, current_value:{space:'Srgb', rgba:[1, 1, 1, 1]}, stops:hdr ? 1 : null, changed:editor.hex !== '#FFFFFF', search:editor.picker.editor.search});
  const app = {
    state:() => ({document_file:{epoch:1}, colors:{}, layer_tools:{}, color_library:{history:[]}}), language_tag:language,
    color_panel:() => ({rendition:null}), color_preview:() => ({picker:{editor:false, preview:null, picked:null}}), swatch_sheet:() => ({sections:[], empty:null}),
    catalog:() => ({native_copy:{color:Object.fromEntries(['edit', 'wheel', 'intensity', 'circle', 'square', 'triangle', 'current', 'new', 'pick_canvas', 'copy', 'hex', 'format', 'intensity_ev', 'swatch_search', 'close_swatches', 'all_swatches', 'use_color', 'picking_strip', 'copied'].map(key => [key, `${language()}:${key}`])), palettes:{add_current:`${language()}:add`}}}),
    bootstrap_view:() => ({common:{cancel:`${language()}:cancel`}}),
    color_ui(request) {
      if (request.type === 'editor_open') { const editor = {picker:{editor:{forms:['rgb', 'hsb', 'oklch'], search:''}}, value:selected, hex:'#4080BF'}; return {editor, view:view(editor), error:null}; }
      if (request.type === 'editor') {
        stats.requests++;
        if (request.action) stats.actions.push(request.action);
        const refused = request.action?.op === 'value' && !Number.isFinite(Number(request.action.text));
        return {editor:request.editor, view:view(request.editor), error:refused ? `${language()}:refused` : null};
      }
      throw Error(`Unexpected request ${request.type}`);
    }};
  return {app, stats};
}
