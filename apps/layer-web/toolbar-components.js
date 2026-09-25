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
  function brushSlider(field, context) {
    // The disabled slider's schema crosses the JSON-valued toolbar query;
    // restore the u32 precision before returning it to the typed numeric API.
    field = { ...field, numeric: { ...field.numeric, digits: Number(field.numeric.digits) } };
    const row = element('div', `toolbar-slider ${field.id === 'opacity' ? 'opacity-track' : 'size-track'}`);
    const cap = button('', show, 'toolbar-slider-cap'); cap.title = field.label; cap.setAttribute('aria-label', field.label);
    target(cap, item); draggable(cap, item);
    const track = element('div', 'number-track'), slider = element('input', 'number-slider');
    slider.type = 'range'; slider.min = 0; slider.max = 1; slider.step = 'any'; slider.setAttribute('aria-label', field.label);
    const marks = element('div', 'toolbar-slider-marks'), thumb = element('div', 'toolbar-slider-thumb');
    track.append(slider, thumb, marks); row.append(cap, track); more.hidden = true;
    let current = field.value, fill = 0, contact, stamp, canvas, caption, bookmark, preview;
    const format = () => app.number_input({ control: field.numeric, value: current, operation: { type: 'format' } });
    function change(value) { send(context, { type: 'set_tool_setting', id: field.id, value }); }
    function position(e) {
      const b = slider.getBoundingClientRect(), travel = Math.max(1, (vertical ? b.height : b.width) - 12);
      return Math.max(0, Math.min(1, vertical ? 1 - (e.clientY - b.y - 6) / travel : (e.clientX - b.x - 6) / travel));
    }
    function pick(e, snap) {
      const positionValue = position(e);
      change(snap ? app.toolbar_ui({ type: 'slider_bookmark_value', control: tile.control, values: model.bookmarks.map(m => m.value), position: positionValue, travel: Math.max(1, (vertical ? slider.clientHeight : slider.clientWidth) - 12) })
        : app.number_input({ control: field.numeric, value: current, operation: { type: 'position', position: positionValue } }).value);
    }
    function show() {
      if (!model.numeric) return;
      if (!popup || popup !== preview || !popup.isConnected) {
        stamp = app.toolbar_stamp(context);
        const image = document.createElement('canvas'); image.width = image.height = stamp.size;
        const pixels = image.getContext('2d', { willReadFrequently: true }).createImageData(stamp.size, stamp.size);
        // A neutral ink stamp stays legible in both themes, independent of paint color.
        stamp.alpha.forEach((alpha, i) => { pixels.data[i * 4] = pixels.data[i * 4 + 1] = pixels.data[i * 4 + 2] = 255; pixels.data[i * 4 + 3] = alpha; });
        image.getContext('2d', { willReadFrequently: true }).putImageData(pixels, 0, 0); stamp.image = image;
        closePopup(); preview = popup = element('div', 'toolbar-brush-preview panel'); popup.popover = 'manual';
        caption = element('span', 'toolbar-preview-caption');
        bookmark = button('', () => { send(context, { type: 'toggle_slider_bookmark', control: tile.control }); }, 'toolbar-preview-bookmark');
        canvas = element('canvas', 'toolbar-preview-stamp'); popup.append(canvas, caption, bookmark); root.append(popup); popup.showPopover();
        const opened = popup;
        opened.addEventListener('toggle', e => {
          if (e.newState === 'closed' && popup === opened && !opened.matches(':popover-open')) { contact = null; closePopup(); }
        });
      }
      if (!popup.matches(':popover-open')) popup.showPopover();
      paintPreview();
    }
    function paintPreview() {
      if (!popup || popup !== preview) return;
      const geometry = app.toolbar_ui({ type: 'slider_preview', control: tile.control, style: view.tile_style, value: current, length: Math.max(...extent), extent: stamp.extent });
      popup.style.width = popup.style.height = `${geometry.side}px`; popup.style.setProperty('--tile-radius', `${geometry.radius}px`);
      for (const [node, b] of [[caption, geometry.caption], [bookmark, geometry.bookmark]]) Object.assign(node.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.width}px`, height: `${b.height}px` });
      bookmark.style.setProperty('--preview-icon', `${geometry.icon}px`);
      const a = root.getBoundingClientRect(), side = geometry.side;
      const x = vertical ? (a.right + side + 8 <= innerWidth ? a.right + 8 : a.x - side - 8) : a.x;
      const y = vertical ? a.y + (a.height - side) / 2 : (a.bottom + side + 8 <= innerHeight ? a.bottom + 8 : a.y - side - 8);
      popup.style.left = `${Math.max(6, Math.min(x, innerWidth - side - 6))}px`;
      popup.style.top = `${Math.max(6, Math.min(y, innerHeight - side - 6))}px`;
      caption.textContent = geometry.text;
      const selected = model.bookmarks.some(m => m.selected);
      if (bookmark.dataset.selected !== String(selected)) { bookmark.replaceChildren(icon(selected ? 'minus' : 'plus')); bookmark.dataset.selected = selected; }
      bookmark.title = selected ? 'Remove bookmark' : 'Bookmark this value'; bookmark.setAttribute('aria-label', bookmark.title);
      const ratio = devicePixelRatio || 1;
      const pixels = Math.round(side * ratio);
      if (canvas.width !== pixels || canvas.height !== pixels) canvas.width = canvas.height = pixels;
      const ctx = canvas.getContext('2d', { willReadFrequently: true }); ctx.resetTransform(); ctx.clearRect(0, 0, pixels, pixels); ctx.scale(ratio, ratio);
      ctx.save(); ctx.beginPath(); const viewport = geometry.viewport; ctx.rect(viewport.x, viewport.y, viewport.width, viewport.height); ctx.clip();
      ctx.globalAlpha = geometry.opacity; const b = geometry.stamp; ctx.drawImage(stamp.image, b.x, b.y, b.width, b.height);
      ctx.globalAlpha = 1; ctx.globalCompositeOperation = 'source-in'; ctx.fillStyle = getComputedStyle(root).color; ctx.fillRect(0, 0, side, side); ctx.restore();
      if (geometry.header_fade) {
        const fade = ctx.createLinearGradient(0, 0, 0, geometry.header_fade), background = getComputedStyle(popup).backgroundColor;
        fade.addColorStop(0, background); fade.addColorStop(1, 'transparent');
        ctx.save(); ctx.globalAlpha = geometry.header_fade_opacity; ctx.fillStyle = fade; ctx.fillRect(0, 0, side, geometry.header_fade); ctx.restore();
      }
    }
    function update(option) {
      if (option) current = option.value;
      const shown = format(); fill = shown.fill; slider.value = fill; slider.setAttribute('aria-valuetext', shown.text);
      slider.disabled = !model.numeric; cap.disabled = !model.numeric;
      marks.replaceChildren(...model.bookmarks.map(mark => {
        const line = element('span', 'toolbar-slider-mark'); line.classList.toggle('selected', mark.selected);
        line.style.setProperty('--position', mark.selected ? fill : mark.fill); return line;
      }));
      track.style.setProperty('--position', fill); paintPreview();
    }
    slider.addEventListener('pointerdown', e => {
      if (e.button || slider.disabled || contact) return;
      e.preventDefault(); e.stopPropagation();
      contact = { id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
      slider.setPointerCapture(e.pointerId); pick(e, true); show();
    });
    slider.addEventListener('pointermove', e => {
      if (contact?.id !== e.pointerId) return;
      if (!contact.moved && Math.hypot(e.clientX - contact.x, e.clientY - contact.y) < 3) return;
      contact.moved = true; pick(e, false);
    });
    slider.addEventListener('pointerup', e => { if (contact?.id === e.pointerId) { if (contact.moved) closePopup(); contact = null; } });
    for (const type of ['pointercancel', 'lostpointercapture']) slider.addEventListener(type, () => { if (contact) closePopup(); contact = null; });
    // Chromium's native touch range edit runs after pointerdown and can replace
    // a snapped value. Pointer capture above owns touch; retain keyboard input.
    slider.addEventListener('touchstart', e => e.preventDefault(), { passive: false });
    slider.addEventListener('input', () => { change(app.number_input({ control: field.numeric, value: current, operation: { type: 'position', position: Number(slider.value) } }).value); show(); });
    const outside = e => { if (popup && popup === preview && !row.contains(e.target) && !popup.contains(e.target)) closePopup(); };
    const escape = e => { if (e.key === 'Escape') { closePopup(); contact = null; } };
    document.addEventListener('pointerdown', outside, true); document.addEventListener('keydown', escape);
    const blur = () => { closePopup(); contact = null; }; window.addEventListener('blur', blur);
    function orient() {
      slider.style.writingMode = vertical ? 'vertical-lr' : ''; slider.style.direction = vertical ? 'rtl' : '';
      slider.setAttribute('aria-orientation', vertical ? 'vertical' : 'horizontal');
      const [capBounds, trackBounds] = app.toolbar_ui({ type: 'slider_layout', width: extent[0], height: extent[1], axis: vertical ? 'vertical' : 'horizontal' });
      place(cap, capBounds);
      const along = Math.max(0, (vertical ? trackBounds.height : trackBounds.width) - 4);
      place(track, vertical ? { ...trackBounds, x: (extent[0] - 28) / 2, y: trackBounds.y + 2, width: 28, height: along }
        : { ...trackBounds, x: trackBounds.x + 2, y: (extent[1] - 28) / 2, width: along, height: 28 });
      track.style.setProperty('--track-shape', trackShape(along - 12, field.id === 'opacity' ? 8 : 2.5));
    }
    function trackShape(length, narrow) {
      if (length < 4) return 'none';
      const point = (a, b) => vertical ? `${14 + b} ${length + 6 - a}` : `${6 + a} ${14 + b}`;
      return `path('M ${point(0, -narrow)} L ${point(length - 3, -8)} C ${point(length + 1, -8)} ${point(length + 1, 8)} ${point(length - 3, 8)} `
        + `L ${point(0, narrow)} C ${point(-3, narrow)} ${point(-3, -narrow)} ${point(0, -narrow)} Z')`;
    }
    update(); return { row, update, orient, dispose() { closePopup(); document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', escape); window.removeEventListener('blur', blur); } };
  }
  function numeric(field, context) {
    const row = element('div', 'toolbar-option toolbar-numeric');
    row.dataset.toolbarSetting = field.id; row.dataset.toolbarField = '';
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
      const position = (e.clientX - b.x) / b.width;
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
    row.append(label, glyph, number, face);
    function orient() {
      units = !vertical || view.tile_style !== 'small'; number.format(); updateFace();
      label.hidden = vertical || !preferences.text; glyph.style.display = !vertical && !preferences.text ? '' : 'none';
      face.hidden = !vertical; number.hidden = vertical;
      face.classList.toggle('labeled', style.labeled); faceLabel.hidden = !style.labeled;
      number.querySelector('.number-track').hidden = !preferences.sliders;
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
    if (standalone) return;
    const fieldHeight = 24;
    const sizes = fields.map(f => {
      if (f.segmented) return vertical ? [extent[0], style.size[1] * (extent[0] < style.size[0] * f.segmented ? f.segmented : 1)] : [style.size[0] * f.segmented, fieldHeight];
      if (vertical || f.action) return style.size;
      f.row.style.width = 'max-content'; f.row.style.height = 'auto'; f.row.hidden = false;
      return [f.row.scrollWidth, Math.max(fieldHeight, f.row.scrollHeight)];
    });
    const geometry = app.toolbar_ui({ type: 'options_layout', width: extent[0], height: extent[1], axis: vertical ? 'vertical' : 'horizontal', sizes, button: style.size, gap: vertical ? 2 : 10 });
    place(more, geometry.more);
    fields.forEach((f, i) => {
      const bounds = geometry.fields[i];
      if (!bounds && f.row.contains(document.activeElement)) more.focus();
      f.row.hidden = !bounds;
      if (bounds) place(f.row, f.segmented && !vertical ? { ...bounds, y: bounds.y + (bounds.height - fieldHeight) / 2, height: fieldHeight } : bounds);
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
        fields.push(brushSlider(field, value.context));
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
