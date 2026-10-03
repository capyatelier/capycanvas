import {liveCopy,bindCopy} from './localization.js';
// Color numbers, parsing and display transforms come from the shared Rust model.
export const colorCss = preview => `rgba(${preview.rgba.slice(0, 3).map(v => v * 255).join(',')},${preview.rgba[3]})`;

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
    let result = null, view;
    const apply = button(()=>copy.use_color, () => {
      if (!apply.disabled && view.value && app.state().document_file.epoch === epoch) { result = view.value; onIntensity?.(view.draft.intensity); root.close(); }
    }, 'suggested-action');
    footer.append(cancel, apply);
    const baseFigure=element('figure'),adjustedFigure=element('figure');baseFigure.append(baseLabel,basePreview);adjustedFigure.append(adjustedLabel,preview);comparison.append(baseFigure,adjustedFigure);
    const modelRow=element('label','document-size',()=>copy.model);modelRow.append(model);
    const group=element('div','color-entry-group');group.append(modelRow,intensityRow,...fields.map(f=>f.label));
    form.append(title, description, comparison, group, warning, error, footer);
    root.append(form); document.body.append(root);
    const render = (next, preserve=false) => {
      view = next; intensityRow.hidden=view.draft.intensity==null;if(document.activeElement!==intensityInput)intensityInput.value=view.draft.intensity??0; if(view.draft.intensity==null)form.insertBefore(comparison,warning);description.textContent = view.description;
      if (!model.options.length) for (const [id, name] of view.models) {
        const option = element('option', '', name); option.value = id; model.append(option);
      }
      model.value = view.draft.model;
      fields.forEach(({label, text, input}, i) => {
        label.hidden = !view.labels[i]; text.textContent = view.labels[i];
        input.setAttribute('aria-label', view.labels[i]);
        if (!preserve && input.value !== view.draft.fields[i]) input.value = view.draft.fields[i];
      });
      if (view.preview) preview.style.background = colorCss(view.preview);
      baseFigure.hidden=!view.base_preview;adjustedLabel.hidden=!view.base_preview;if(view.base_preview)basePreview.style.background=colorCss(view.base_preview);
      warning.textContent = view.validation??'';
      error.textContent = view.error ?? ''; apply.disabled = !view.value || !!view.error;
    };
    const query = request => {
      try {
        if(view?.draft.intensity!=null){
          const text=intensityInput.value.trim(),stops=Number(text);
          if(!text||!Number.isFinite(stops))throw Error('Enter a finite EV value');
          request={...request,change_intensity:stops};
        }
        render(app.color_ui({type: 'form', request}));
      }
      catch (e) { error.textContent = String(e); apply.disabled = true; }
    };
    fields.forEach(({input}) => input.oninput = () => query({...view.draft, fields: fields.map(f => f.input.value)}));
    intensityInput.oninput=()=>query({...view.draft,fields:fields.map(f=>f.input.value)});
    model.onchange = () => query({...view.draft, change_model: model.value});
    form.onsubmit = e => { e.preventDefault(); apply.click(); };
    root.addEventListener('close', () => { root.remove(); resolve(result); }, {once: true});
    const panel=app.color_panel();query({color,document_depth:(app.state().layer_tools.mask_editing?.colors??app.state().colors).hdr_depth, document_space: panel.rgb_space, display_space:'Srgb',model:panel.hdr?'linear_rgb':'document_rgb',intensity:panel.hdr?(intensity??null):null,rendition:panel.rendition});
    Object.defineProperty(form,'language',{set(){if(view)render(app.color_ui({type:'form',request:{...view.draft,fields:fields.map(f=>f.input.value)}}),true);}});
    bindCopy(form,()=>app.language_tag(),'language');
    root.showModal(); fields[0].input.focus();
  });
}

export function colorButton({app, label, element, button, change, current = () => ''}) {
  let color, previewKey, inGamut=true;
  const node = button(label, async () => {
    const context = current(), selected = await chooseColor({app, color, element, button});
    if (selected && current() === context) change(selected);
  }, 'property-color');
  const read=()=>typeof label==='function'?label():label;bindCopy(node,read,'ariaLabel');
  const update = value => {
    color = value;
    const key = JSON.stringify(value);
    if (key === previewKey) return;
    previewKey = key;
    const preview = app.color_ui({type: 'preview', colors: [color]})[0];
    node.style.background = colorCss(preview);
    inGamut=preview.in_gamut;node.title = read();
  };
  bindCopy(node,()=>inGamut?read():`${read()} · ${liveCopy(app,'catalog').native_copy.color.outside_srgb}`,'title');
  return {node, update, disable: disabled => node.disabled = disabled};
}

// Browser-native clicks preserve keyboard activation and hold-to-drag arbitration.
export function pickerButtonAction(control,anchor,dispatch,activate) {
  if(control?.kind!=='color_picker' && !(control?.kind==='command'&&control.command==='eyedropper'))return activate;
  let last=0,device=null;
  return event=>{
    const now=performance.now(),type=event?.pointerType||'keyboard';
    const double=event?.detail!==0 && now-last<400 && device===type;
    last=double?0:now;device=type;
    if(double)dispatch({type:'color_picker',action:{kind:'settings',anchor}});else activate(event);
  };
}
