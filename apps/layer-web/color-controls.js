// Color numbers, parsing and display transforms come from the shared Rust model.
export const colorCss = preview => `rgba(${preview.rgba.slice(0, 3).map(v => v * 255).join(',')},${preview.rgba[3]})`;

export function chooseColor({app, color, element, button, intensity, onIntensity}) {
  const epoch = app.state().document_file.epoch;
  return new Promise(resolve => {
    const root = element('dialog', 'document-dialog color-dialog'), form = element('form');
    form.method = 'dialog'; root.setAttribute('aria-label', 'Edit Color');
    const title = element('h2', '', 'Edit Color'), description = element('p'), model = element('select');
    model.setAttribute('aria-label', 'Color model');
    const intensityInput=element('input');intensityInput.type='number';intensityInput.step='.1';intensityInput.min='-16';intensityInput.max='15.999';intensityInput.setAttribute('aria-label','Intensity (EV)');
    const intensityRow=element('label','document-size','Intensity (EV)');intensityRow.append(intensityInput);
    const preview = element('div', 'color-form-preview'), basePreview=element('div','color-form-preview'), comparison=element('div','color-comparison'),baseLabel=element('figcaption','','Base'),adjustedLabel=element('figcaption','','Adjusted'), warning = element('p'), error = element('p');
    error.setAttribute('role', 'status');
    const fields = Array.from({length: 4}, (_, i) => {
      const label = element('label', 'document-size'), text = element('span'), input = element('input');
      input.type = 'text'; input.autocomplete = 'off'; input.spellcheck = false;
      input.dataset.colorField = i; label.append(text, input); return {label, text, input};
    });
    const footer = element('footer'), cancel = button('Cancel', () => root.close());
    let result = null, view;
    const apply = button('Use Color', () => {
      if (view.value && app.state().document_file.epoch === epoch) { result = view.value; onIntensity?.(view.draft.intensity); root.close(); }
    }, 'suggested-action');
    footer.append(cancel, apply);
    const baseFigure=element('figure'),adjustedFigure=element('figure');baseFigure.append(basePreview,baseLabel);adjustedFigure.append(preview,adjustedLabel);comparison.append(baseFigure,adjustedFigure);
    form.append(title, description, model, intensityRow, comparison, ...fields.map(f => f.label), warning, error, footer);
    root.append(form); document.body.append(root);
    const render = next => {
      view = next; intensityRow.hidden=view.draft.intensity==null;intensityInput.value=view.draft.intensity??0; description.textContent = view.description;
      if (!model.options.length) for (const [id, name] of view.models) {
        const option = element('option', '', name); option.value = id; model.append(option);
      }
      model.value = view.draft.model;
      fields.forEach(({label, text, input}, i) => {
        label.hidden = !view.labels[i]; text.textContent = view.labels[i];
        input.setAttribute('aria-label', view.labels[i]);
        if (input.value !== view.draft.fields[i]) input.value = view.draft.fields[i];
      });
      if (view.preview) preview.style.background = colorCss(view.preview);
      baseFigure.hidden=!view.base_preview;adjustedLabel.hidden=!view.base_preview;if(view.base_preview)basePreview.style.background=colorCss(view.base_preview);
      warning.textContent = view.preview && !view.preview.in_gamut ? 'Outside the sRGB preview gamut. The stored color is preserved.' : '';
      error.textContent = view.error ?? ''; apply.disabled = !view.value;
    };
    const query = request => {
      try { render(app.color_ui({type: 'form', request})); }
      catch (e) { error.textContent = String(e); apply.disabled = true; }
    };
    fields.forEach(({input}) => input.oninput = () => query({...view.draft, fields: fields.map(f => f.input.value)}));
    intensityInput.oninput=()=>query({...view.draft,change_intensity:Number(intensityInput.value)});
    model.onchange = () => query({...view.draft, change_model: model.value});
    form.onsubmit = e => { e.preventDefault(); apply.click(); };
    root.addEventListener('close', () => { root.remove(); resolve(result); }, {once: true});
    const panel=app.color_panel();query({color, document_space: app.state().colors.rgb_space, display_space:'Srgb',model:panel.hdr?'linear_rgb':'document_rgb',intensity:panel.hdr?(intensity??0):null,rendition:panel.rendition});
    root.showModal(); fields[0].input.focus();
  });
}

export function colorButton({app, label, element, button, change, current = () => ''}) {
  let color, previewKey;
  const node = button(label, async () => {
    const context = current(), selected = await chooseColor({app, color, element, button});
    if (selected && current() === context) change(selected);
  }, 'property-color');
  node.setAttribute('aria-label', label);
  const update = value => {
    color = value;
    const key = JSON.stringify(value);
    if (key === previewKey) return;
    previewKey = key;
    const preview = app.color_ui({type: 'preview', colors: [color]})[0];
    node.style.background = colorCss(preview);
    node.title = preview.in_gamut ? label : `${label} · outside the sRGB preview gamut`;
  };
  return {node, update, disable: disabled => node.disabled = disabled};
}
