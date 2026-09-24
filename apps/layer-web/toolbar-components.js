import { createNumberField } from './numeric.js';
const key = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? String(v) : v);

// Retained DOM controls. Rust owns the field schema, edit context, numeric math,
// and fitting; this adapter reports native font/control measurements.
export function createToolbarComponent({ app, tile, view, element, button, icon, dispatch, draggable, target, place, panel }) {
  const root = element('div', 'tile-button tool-tile toolbar-component');
  root.dataset.tile = tile.id; root.dataset.toolbarComponent = tile.control.kind;
  const item = { kind: 'tile', panel, tile: tile.id };
  target(root, item);
  let model, schema = '', fields = [], vertical = false, extent = [0, 0], measured = '', popup;
  const style = app.toolbar_ui({ type: 'style', style: view.tile_style });
  const preferences = tile.control.style || { text: true, sliders: true };
  const standalone = tile.control.kind !== 'tool_options';
  const more = button('', () => dispatch({ type: 'activate_tile', panel, tile: tile.id }), 'toolbar-more');
  more.append(icon('more')); more.title = 'More tool options'; more.setAttribute('aria-label', more.title);
  target(more, item); draggable(more, item); root.append(more);
  function send(context, action) { dispatch({ type: 'toolbar_edit', context, action }); }
  function closePopup() { if (popup) { popup.remove(); popup = null; } }
  function openPopup(anchor, content) {
    closePopup(); popup = element('div', 'toolbar-editor-popover panel'); popup.popover = 'auto';
    popup.append(content); root.append(popup); popup.showPopover();
    const a = anchor.getBoundingClientRect(), b = popup.getBoundingClientRect();
    popup.style.left = `${Math.max(6, Math.min(a.right + 6, innerWidth - b.width - 6))}px`;
    popup.style.top = `${Math.max(6, Math.min(a.y, innerHeight - b.height - 6))}px`;
  }
  function numeric(field, context, slider = false) {
    const row = element('div', slider ? 'toolbar-slider' : 'toolbar-option toolbar-numeric');
    row.dataset.toolbarSetting = field.id; if (!slider) row.dataset.toolbarField = '';
    const info = app.toolbar_ui({ type: 'numeric_info', id: field.id, control: field.numeric, compact: true, units: true });
    let units = true;
    const change = value => send(context, { type: 'set_tool_setting', id: field.id, value });
    const number = createNumberField({ control: field.numeric, label: field.label, icon, inline: true,
      widthSamples: info.samples, onChange: change,
      resolve: request => app.toolbar_ui({ type: 'number', request, compact: true, units }) });
    // Chromium's native range drag does not consistently consume tablet
    // contacts. Keep the native keyboard control and capture pointer input here.
    let trackContact;
    const pick = e => {
      const b = number.slider.getBoundingClientRect();
      const position = slider && vertical ? 1 - (e.clientY - b.y) / b.height : (e.clientX - b.x) / b.width;
      number.cancelEditing(); number.apply({ type: 'position', position });
    };
    number.slider.addEventListener('pointerdown', e => {
      if (e.button || number.slider.disabled) return;
      e.preventDefault(); e.stopPropagation(); trackContact = e.pointerId;
      number.slider.setPointerCapture(e.pointerId); pick(e);
    });
    number.slider.addEventListener('pointermove', e => { if (trackContact === e.pointerId) { e.preventDefault(); pick(e); } });
    for (const type of ['pointerup', 'pointercancel', 'lostpointercapture']) number.slider.addEventListener(type, () => { trackContact = null; });
    const label = element('span', 'toolbar-option-label', field.label), glyph = icon(info.icon);
    const face = button('', () => {}, 'toolbar-number-face');
    const faceIcon = icon(info.icon), faceLabel = element('span', 'toolbar-face-label', field.label), faceValue = element('span', 'toolbar-face-value');
    face.append(faceIcon, faceLabel, faceValue); face.title = field.label; face.setAttribute('aria-label', field.label);
    for (const node of [label, glyph]) node.ondblclick = () => send(context, { type: 'reset_tool_setting', id: field.id });
    let current = field.value;
    function update(value) {
      current = value; number.update(value);
      updateFace();
    }
    const textMeasure = document.createElement('canvas').getContext('2d');
    let faceFont;
    function updateFace() {
      if (!faceFont && row.isConnected) { const f = getComputedStyle(faceValue); faceFont = `${f.fontWeight} ${f.fontSize} ${f.fontFamily}`; }
      if (faceFont) textMeasure.font = faceFont;
      const format = units => app.toolbar_ui({ type: 'number', request: { control: field.numeric, value: current, operation: { type: 'format' } }, compact: true, units }).text;
      let text = format(view.tile_style !== 'small');
      const available = Math.min(style.size[0], extent[0] || style.size[0]) - (style.labeled ? 38 : 4);
      if (textMeasure.measureText(text.replace(/\d/g, '8')).width + 2 > available) text = format(false);
      faceValue.textContent = text;
      faceValue.toggleAttribute('data-wide', text.length >= 4);
    }
    function scrub(node) {
      let contact, scrubbed = false;
      node.addEventListener('pointerdown', e => {
        if (e.button || e.pointerType === 'mouse') return;
        scrubbed = false;
        contact = { id: e.pointerId, y: e.clientY, fill: app.number_input({ control: field.numeric, value: current, operation: { type: 'format' } }).fill, moved: false };
      });
      node.addEventListener('pointermove', e => {
        if (contact?.id !== e.pointerId || (!contact.moved && Math.abs(e.clientY - contact.y) < 8)) return;
        contact.moved = true; node.setPointerCapture(e.pointerId); e.preventDefault();
        number.cancelEditing(); number.apply({ type: 'position', position: contact.fill + (contact.y - e.clientY) / 200 });
      });
      node.addEventListener('pointerup', e => { if (contact?.id === e.pointerId) { scrubbed = contact.moved; contact = null; } });
      node.addEventListener('click', e => { if (scrubbed) { e.stopImmediatePropagation(); e.preventDefault(); scrubbed = false; } }, true);
      node.addEventListener('lostpointercapture', () => { contact = null; });
      for (const type of ['workspace-drag-held', 'workspace-context-claimed']) row.addEventListener(type, () => { contact = null; });
      node.addEventListener('pointercancel', () => { contact = null; });
      node.addEventListener('wheel', e => { e.preventDefault(); number.cancelEditing(); number.apply({ type: 'step', steps: e.deltaY < 0 ? 1 : -1 }); }, { passive: false });
    }
    scrub(number.valueButton); scrub(face);
    face.addEventListener('click', () => {
      const editor = createNumberField({ control: field.numeric, label: field.label, icon, resolve: request => app.number_input(request), onChange: change });
      editor.update(current); openPopup(face, editor);
    });
    if (slider) {
      row.classList.add(tile.control.kind === 'brush_opacity_slider' ? 'opacity-track' : 'size-track');
      target(number.valueButton.parentElement, item); draggable(number.valueButton.parentElement, item);
      number.slider.style.touchAction = 'none';
      row.append(number); more.hidden = true;
    } else row.append(label, glyph, number, face);
    function orient() {
      units = !vertical || (!slider && view.tile_style !== 'small'); number.format(); updateFace();
      label.hidden = vertical || !preferences.text; glyph.style.display = !vertical && !preferences.text ? '' : 'none';
      face.hidden = !vertical; number.hidden = !slider && vertical;
      face.classList.toggle('labeled', style.labeled); faceLabel.hidden = !style.labeled;
      number.querySelector('.number-track').hidden = !slider && !preferences.sliders;
      number.slider.style.writingMode = slider && vertical ? 'vertical-lr' : '';
      number.slider.style.direction = slider && vertical ? 'rtl' : '';
      number.slider.setAttribute('aria-orientation', slider && vertical ? 'vertical' : 'horizontal');
      if (slider) number.querySelector('.number-measure').hidden = vertical;
    }
    update(field.value);
    return { row, update: option => update(option.Numeric?.value ?? option.value), orient, dispose: () => number.cancelEditing(), number };
  }
  function choice(spec, context) {
    const row = element('div', `toolbar-option ${spec.segmented ? 'toolbar-segments selection-modes' : 'toolbar-choice'}`);
    row.dataset.toolbarField = ''; row.dataset.toolbarChoice = spec.id;
    let selected = spec.items.findIndex(i => i.selected);
    const buttons = spec.segmented ? spec.items.map((item, i) => {
      const b = button('', () => send(context, item.action)); b.append(icon(item.icon)); b.title = item.label;
      b.setAttribute('aria-label', item.label); b.dataset.toolbarSegment = `${spec.id}-${i}`; row.append(b); return b;
    }) : [];
    const b = spec.segmented ? null : button('', () => {
      const menu = element('div', 'toolbar-choice-menu');
      spec.items.forEach((item, i) => {
        const entry = button('', () => { closePopup(); send(context, item.action); });
        entry.append(icon(item.icon), element('span', '', item.label)); entry.setAttribute('role', 'menuitemradio'); entry.setAttribute('aria-checked', i === selected); menu.append(entry);
      });
      openPopup(b, menu);
    });
    if (b) { b.setAttribute('aria-label', spec.label); b.title = spec.label; row.append(b); }
    row.setAttribute('role', spec.segmented ? 'radiogroup' : 'group'); row.setAttribute('aria-label', spec.label);
    function update(option) {
      const items = option.Choice.items; selected = items.findIndex(i => i.selected);
      if (b) { const item = items[selected] || items[0]; b.replaceChildren(icon(item.icon), element('span', 'toolbar-choice-label', item.label), icon('chevron-down')); }
      buttons.forEach((button, i) => { button.setAttribute('role', 'radio'); button.setAttribute('aria-checked', items[i].selected); button.setAttribute('aria-pressed', items[i].selected); });
    }
    update({ Choice: spec });
    return { row, update, segmented: buttons.length, orient() {
      row.classList.toggle('labeled', style.labeled); row.classList.toggle('stacked', vertical && extent[0] < style.size[0] * buttons.length);
    } };
  }
  function action(spec, context) {
    const row = element('div', 'toolbar-option toolbar-action'); row.dataset.toolbarField = '';
    const b = button('', () => send(context, { type: 'invoke', command: spec.state.id }));
    b.append(icon(spec.state.icon || 'settings')); row.append(b); b.title = spec.state.tooltip; b.setAttribute('aria-label', spec.state.label);
    return { row, action: true, update(option) { b.disabled = !option.Action.state.enabled; if (spec.checkable) b.setAttribute('aria-pressed', option.Action.state.selected); } };
  }
  function layout() {
    if (!model || !root.isConnected || !extent[0] || !extent[1]) return;
    root.classList.toggle('vertical-component', vertical); root.dataset.tileStyle = view.tile_style;
    fields.forEach(f => f.orient?.());
    if (standalone) {
      const f = fields[0]; if (!f) return;
      const box = f.number.querySelector('.number-value-box'), track = f.number.querySelector('.number-track');
      const cap = vertical ? 36 : f.number.querySelector('.number-measure').scrollWidth;
      const [valueBounds, trackBounds] = app.toolbar_ui({ type: 'slider_layout', width: extent[0], height: extent[1], axis: vertical ? 'vertical' : 'horizontal', cap });
      place(box, valueBounds);
      // The core allocates the whole cross axis; the native track is 24px wide.
      place(track, vertical ? { ...trackBounds, x: (extent[0] - 24) / 2, width: 24, height: Math.max(0, trackBounds.height - 8) }
        : { ...trackBounds, x: trackBounds.x + 8, y: (extent[1] - 24) / 2, width: Math.max(0, trackBounds.width - 16), height: 24 });
      f.number.setDisabled(!model.numeric); return;
    }
    const sizes = fields.map(f => {
      if (f.segmented) return vertical ? [extent[0], style.size[1] * (extent[0] < style.size[0] * f.segmented ? f.segmented : 1)] : [style.size[0] * f.segmented, style.size[1]];
      if (vertical || f.action) return style.size;
      f.row.style.width = 'max-content'; f.row.style.height = 'auto'; f.row.hidden = false;
      return [f.row.scrollWidth, Math.max(24, f.row.scrollHeight)];
    });
    const geometry = app.toolbar_ui({ type: 'options_layout', width: extent[0], height: extent[1], axis: vertical ? 'vertical' : 'horizontal', sizes, button: style.size, gap: vertical ? 2 : 10 });
    place(more, geometry.more);
    fields.forEach((f, i) => {
      const bounds = geometry.fields[i];
      if (!bounds && f.row.contains(document.activeElement)) more.focus();
      f.row.hidden = !bounds; if (bounds) place(f.row, bounds);
    });
  }
  root.updateComponent = next => {
    const value = next.component;
    const nextSchema = key([value.context, value.numeric && { ...value.numeric, value: 0 }, value.options.map(o => o.Numeric ? { Numeric: { ...o.Numeric, value: 0 } } : o.Choice ? { Choice: { ...o.Choice, items: o.Choice.items.map(i => ({ ...i, selected: false })) } } : { Action: { ...o.Action, state: { ...o.Action.state, selected: false, enabled: true } } })]);
    model = value;
    if (nextSchema !== schema) {
      closePopup(); fields.forEach(f => { f.dispose?.(); f.row.remove(); }); fields = []; schema = nextSchema;
      if (standalone) {
        const field = model.numeric || { id: tile.control.kind === 'brush_size_slider' ? 'size' : 'opacity', label: tile.label, numeric: app.toolbar_ui({ type: 'slider_spec', control: tile.control }), value: 0.5 };
        fields.push(numeric(field, value.context, true));
      } else fields = value.options.map(o => o.Numeric ? numeric(o.Numeric, value.context) : o.Choice ? choice(o.Choice, value.context) : action(o.Action, value.context));
      fields.forEach(f => root.append(f.row)); measured = '';
    }
    if (standalone) { if (value.numeric) fields[0].update(value.numeric); }
    else fields.forEach((f, i) => f.update(value.options[i]));
    if (!measured) { layout(); measured = schema; }
  };
  root.layoutComponent = (bounds, axis) => {
    const next = key([bounds.width, bounds.height, axis]);
    if (next === root.dataset.componentBounds) return;
    root.dataset.componentBounds = next; extent = [bounds.width, bounds.height]; vertical = axis === 'vertical'; closePopup(); layout();
  };
  root.disposeComponent = () => { closePopup(); fields.forEach(f => f.dispose?.()); };
  root.updateComponent(tile);
  return root;
}
