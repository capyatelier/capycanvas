import {liveCopy,bindCopy} from './localization.js';
import {chooseColor} from './color-editor.js';
// Color numbers, parsing and display transforms come from the shared Rust model.
export const colorCss = preview => `rgba(${preview.rgba.slice(0, 3).map(v => v * 255).join(',')},${preview.rgba[3]})`;

let paintPairId = 0;
const paintPairIcons = new WeakSet();
export function createPaintPairIcon(svg, view) {
  svg.dataset.paintPair = '';
  const circles = [...svg.children], document = svg.ownerDocument;
  const create = tag => document.createElementNS('http://www.w3.org/2000/svg', tag);
  const defs = create('defs'); svg.prepend(defs);
  for (const [slot, offset] of [['background', 0], ['foreground', 2]]) {
    const group = create('g'), pattern = create('pattern');
    group.dataset.paintSlot = slot;
    pattern.dataset.paintSlot = slot; pattern.setAttribute('patternUnits', 'userSpaceOnUse');
    for (let i = 0; i < 3; i++) pattern.append(create('rect'));
    circles[offset].removeAttribute('class');
    group.append(circles[offset], circles[offset + 1]); defs.append(pattern); svg.append(group);
  }
  updatePaintPairIcon(svg, view);
  return svg;
}

export function updatePaintPairIcon(svg, view) {
  if (!paintPairIcons.has(svg)) {
    paintPairIcons.add(svg); delete svg.dataset.paintKey;
    for (const pattern of svg.querySelectorAll('pattern[data-paint-slot]')) {
      pattern.id = `paint-pair-${paintPairId++}`;
      svg.querySelector(`g[data-paint-slot="${pattern.dataset.paintSlot}"]`).firstChild.setAttribute('fill', `url(#${pattern.id})`);
    }
  }
  const key = JSON.stringify([view.front_swatch, view.checker_cell, view.swatches.map(s => s.checker)]);
  if (svg.dataset.paintKey === key) return;
  svg.dataset.paintKey = key;
  for (const swatch of view.swatches) {
    const group = svg.querySelector(`g[data-paint-slot="${swatch.slot}"]`), circle = group.firstChild;
    const pattern = svg.querySelector(`pattern[data-paint-slot="${swatch.slot}"]`);
    const cell = view.checker_cell;
    if (pattern.dataset.checkerCell !== String(cell)) {
      pattern.dataset.checkerCell = String(cell);
      for (const name of ['width', 'height']) pattern.setAttribute(name, 2 * cell);
      pattern.setAttribute('x', Number(circle.getAttribute('cx')) - Number(circle.getAttribute('r')));
      pattern.setAttribute('y', Number(circle.getAttribute('cy')) - Number(circle.getAttribute('r')));
      [...pattern.children].forEach((rect, i) => {
        for (const [name, value] of Object.entries({x: i === 2 ? cell : 0, y: i === 1 ? cell : 0, width: i ? cell : 2 * cell, height: i ? cell : 2 * cell})) rect.setAttribute(name, value);
      });
    }
    const colors = JSON.stringify(swatch.checker);
    if (pattern.dataset.paintKey === colors) continue;
    pattern.dataset.paintKey = colors;
    [...pattern.children].forEach((rect, i) => rect.setAttribute('fill', colorCss({rgba: swatch.checker[i ? 1 : 0]})));
  }
  const front = svg.querySelector(`g[data-paint-slot="${view.front_swatch}"]`);
  if (svg.lastElementChild !== front) svg.append(front);
}

export function colorButton({app, label, element, button, change, current = () => '', opaque = false}) {
  let color, previewKey, inGamut=true,disposed=false;
  const node = button(label, async () => {
    const context = current(), selected = await chooseColor({app, color, element, button, opaque});
    if (selected && !disposed && current() === context) change(selected.color);
  }, 'property-color');
  const read=()=>typeof label==='function'?label():label;bindCopy(node,read,'ariaLabel');
  const update = (value, mapped) => {
    color = value;
    const key = JSON.stringify([value, mapped]);
    if (key === previewKey) return;
    previewKey = key;
    const preview = mapped ?? app.color_ui({type: 'preview', colors: [color]})[0];
    node.style.background = colorCss(preview);
    inGamut=preview.in_gamut;node.title = read();
  };
  bindCopy(node,()=>inGamut?read():`${read()} · ${liveCopy(app,'catalog').native_copy.color.outside_srgb}`,'title');
  return {node, update, disable: disabled => node.disabled = disabled,dispose:()=>{disposed=true;}};
}

// Browser-native clicks preserve keyboard activation and hold-to-drag arbitration.
export function pickerButtonAction(control,anchor,dispatch,activate) {
  let last=0,device=null;
  return event=>{
    const current=typeof control==='function'?control():control;
    if(current?.kind!=='color_picker' && !(current?.kind==='command'&&current.command==='eyedropper')){last=0;return activate(event);}
    const now=performance.now(),type=event?.pointerType||'keyboard';
    const double=event?.detail!==0 && now-last<400 && device===type;
    last=double?0:now;device=type;
    if(double)dispatch({type:'color_picker',action:{kind:'settings',anchor}});else activate(event);
  };
}
