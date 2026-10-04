import {liveCopy,bindCopy} from './localization.js';
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

export function chooseColor({app, color, element, button, intensity, onIntensity}) {
  const copy=liveCopy(app,"catalog").native_copy.color,common=liveCopy(app,"bootstrap_view").common;
  const epoch = app.state().document_file.epoch;
  return new Promise(resolve => {
    const root = element('dialog', 'document-dialog color-dialog'), form = element('form');
    form.method = 'dialog'; bindCopy(root,()=>copy.edit,"ariaLabel");
    const title = element('h2', '', ()=>copy.edit), description = element('p','color-description'), model = element('select');
    bindCopy(model,()=>copy.model,"ariaLabel");
    const intensityInput=element('input');intensityInput.type='text';intensityInput.inputMode='decimal';intensityInput.autocomplete='off';bindCopy(intensityInput,()=>copy.intensity_ev,"ariaLabel");
    const intensityRow=element('label','color-entry',()=>copy.intensity_ev);intensityRow.append(intensityInput);
    const preview = element('div', 'color-form-preview'), basePreview=element('div','color-form-preview'), comparison=element('div','color-comparison'),baseLabel=element('figcaption','',()=>copy.base),adjustedLabel=element('figcaption','',()=>copy.adjusted), warning = element('p'), error = element('p');
    error.setAttribute('role', 'status');
    const fields = Array.from({length: 4}, (_, i) => {
      const label = element('label', 'color-entry'), text = element('span'), input = element('input');
      input.type = 'text'; input.autocomplete = 'off'; input.spellcheck = false;
      input.dataset.colorField = i; label.append(text, input); return {label, text, input};
    });
    const footer = element('footer'), cancel = button(()=>common.cancel, () => root.close());
    let result = null, view, failure;
    const apply = button(()=>copy.use_color, () => {
      if (!apply.disabled && view.value && app.state().document_file.epoch === epoch) { result = view.value; onIntensity?.(view.draft.intensity); root.close(); }
    }, 'suggested-action');
    footer.append(cancel, apply);
    const baseFigure=element('figure'),adjustedFigure=element('figure');baseFigure.append(baseLabel,basePreview);adjustedFigure.append(adjustedLabel,preview);comparison.append(baseFigure,adjustedFigure);
    const modelRow=element('label','document-size',()=>copy.model);modelRow.append(model);
    const group=element('div','color-entry-group');group.append(modelRow,intensityRow,...fields.map(f=>f.label));
    form.append(title, description, comparison, group, warning, error, footer);
    root.append(form); document.body.append(root);
    const project = next => {
      description.textContent = next.description;
      for (const [id, name] of next.models) {
        let option = [...model.options].find(option=>option.value===id);
        if (!option) { option=element('option');option.value=id;model.append(option); }
        option.textContent = name;
      }
      fields.forEach(({label, text, input}, i) => {
        label.hidden = !next.labels[i]; text.textContent = next.labels[i];
        input.setAttribute('aria-label', next.labels[i]);
      });
      warning.textContent = next.validation??'';
      error.textContent = failure??next.error??'';
      apply.disabled = !view.value || !!next.error || failure!=null;
    };
    const render = next => {
      view = next; intensityRow.hidden=view.draft.intensity==null;
      if(document.activeElement!==intensityInput)intensityInput.value=view.draft.change_intensity_text??view.draft.intensity??0;
      if(view.draft.intensity==null)form.insertBefore(comparison,warning);
      project(view);model.value = view.draft.model;
      fields.forEach(({input}, i) => { if (input.value !== view.draft.fields[i]) input.value = view.draft.fields[i]; });
      if (view.preview) preview.style.background = colorCss(view.preview);
      baseFigure.hidden=!view.base_preview;adjustedLabel.hidden=!view.base_preview;if(view.base_preview)basePreview.style.background=colorCss(view.base_preview);
    };
    const query = request => {
      try {
        if(view?.draft.intensity!=null)request={...request,change_intensity_text:intensityInput.value};
        failure=null;render(app.color_ui({type: 'form', request}));
      }
      catch (e) { failure=String(e);error.textContent=failure;apply.disabled=true; }
    };
    fields.forEach(({input}) => input.oninput = () => query({...view.draft, fields: fields.map(f => f.input.value)}));
    intensityInput.oninput=()=>query({...view.draft,fields:fields.map(f=>f.input.value)});
    model.onchange = () => query({...view.draft, change_model: model.value});
    form.onsubmit = e => { e.preventDefault(); apply.click(); };
    root.addEventListener('close', () => { root.remove(); resolve(result); }, {once: true});
    const panel=app.color_panel();query({color,document_depth:(app.state().layer_tools.mask_editing?.colors??app.state().colors).hdr_depth, document_space: panel.rgb_space, display_space:'Srgb',model:panel.hdr?'linear_rgb':'document_rgb',intensity:panel.hdr?(intensity??null):null,rendition:panel.rendition});
    Object.defineProperty(form,'language',{set(){if(view)project(app.color_ui({type:'form_copy',copy:view.copy}));}});
    bindCopy(form,()=>app.language_tag(),'language');
    root.showModal(); fields[0].input.focus();
  });
}

export function colorButton({app, label, element, button, change, current = () => ''}) {
  let color, previewKey, inGamut=true,disposed=false;
  const node = button(label, async () => {
    const context = current(), selected = await chooseColor({app, color, element, button});
    if (selected && !disposed && current() === context) change(selected);
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
