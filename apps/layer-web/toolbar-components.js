import { captureSliderContacts, createNumberField } from './numeric.js';
import { createRangeControl } from './range-control.js';
import { bindCopy, liveCopy } from './localization.js';
import {gradientButton} from './gradient.js';
const key = value => JSON.stringify(value, (_, v) => typeof v === 'bigint' ? String(v) : v);
const rectangle = ({x,y,width,height}) => ({x,y,width,height});
const semantic = ({label, tooltip, disabled_reason, ...value}) => value;
const fieldSemantic = ({group, numeric, ...value}) => {
  const {endpoint_labels, ...rules} = numeric;
  return {...semantic(value), numeric: rules};
};

export function actionField({ element, button, icon }, spec, send, { label, ariaDisabled = false, explain, checkbox = false } = {}) {
  const check = spec.checkable && (checkbox || !spec.state.icon);
  const row = element(check ? 'label' : 'div', `toolbar-option ${check ? 'toolbar-check' : 'toolbar-action'}`); row.dataset.toolbarField = '';
  const invoke = () => {
    if (b.getAttribute('aria-disabled') !== 'true') send(spec.action ?? { type: 'invoke', command: spec.state.id });
    else { if (check) b.checked = spec.state.selected; explain?.(b); }
  };
  const b = check ? element('input', 'panel-check') : button('', invoke);
  if (check) { b.type = 'checkbox'; b.addEventListener('change', invoke); }
  else if (spec.state.icon) b.append(icon(spec.state.icon));
  row.append(b);
  const caption = check || label != null || !spec.state.icon ? element('span', 'toolbar-action-label', label ?? spec.state.label) : null;
  if (caption) (check ? row : b).append(caption);
  let tooltip, disabled, pressed;
  function update(option) {
    spec = option.Action;
    const state = option.Action.state;
    if (caption) caption.textContent = label ?? state.label;
    b.setAttribute('aria-label', label ?? state.label);
    if (disabled !== !state.enabled) {
      disabled = !state.enabled;
      if (ariaDisabled) b.setAttribute('aria-disabled', String(disabled)); else b.disabled = disabled;
    }
    if (check) b.checked = state.selected;
    else if (spec.checkable && pressed !== state.selected) b.setAttribute('aria-pressed', String(pressed = state.selected));
    const text = state.enabled ? state.tooltip : state.disabled_reason ?? state.tooltip;
    if (tooltip !== text) row.title = b.title = tooltip = text;
  }
  update({ Action: spec });
  return { row, button: b, action: true, intrinsic: check, update };
}

export function checklistField(helpers, spec, send) {
  const row=helpers.element('div','toolbar-checklist'),caption=helpers.element('h3','',spec.label);
  row.dataset.toolbarField='';row.dataset.toolChecklist=spec.id;row.setAttribute('role','group');row.append(caption);
  const option=item=>({action:item.action,checkable:true,state:{...item,tooltip:item.label}});
  const fields=spec.items.map(item=>{const field=actionField(helpers,option(item),send,{checkbox:true});row.append(field.row);return field;});
  const update=next=>{spec=next.Checklist;caption.textContent=spec.label;row.setAttribute('aria-label',spec.label);fields.forEach((field,index)=>field.update({Action:option(spec.items[index])}));};
  update({Checklist:spec});
  return {row,intrinsic:true,update};
}

export function choiceField({ app, element, button, icon, openPopup, closePopup }, spec, send, { labels = false } = {}) {
  const row = element('div', `toolbar-option ${spec.labeled ? 'toolbar-labeled-choice' : spec.segmented ? 'toolbar-segments selection-modes' : 'toolbar-choice'}`);
  const group = spec.labeled ? element('div', 'toolbar-segments selection-modes') : row;
  const label = spec.labeled ? element('span', 'toolbar-option-label', spec.label) : null;
  if (label) row.append(label, group);
  if (spec.labeled) {
    const metrics = app.toolbar_ui({ type: 'choice_style', labeled: true });
    row.style.setProperty('--choice-width', `${metrics.size[0]}px`);
    row.style.setProperty('--choice-height', `${metrics.size[1]}px`);
    row.style.setProperty('--choice-icon', `${metrics.icon}px`);
  }
  row.dataset.toolbarField = ''; row.dataset.toolbarChoice = spec.id;
  if (spec.columns) { row.classList.add('choice-grid'); row.style.setProperty('--choice-columns', spec.columns); row.title = spec.label; }
  let selected = spec.items.findIndex(i => i.selected), entries = [];
  const captions = [];
  const buttons = spec.segmented ? spec.items.map((item, i) => {
    const b = button('', () => send(spec.items[i].action)); b.append(icon(item.icon));
    if (labels && !spec.labeled && !spec.columns) b.append(captions[i] = element('span', 'toolbar-segment-label', item.label));
    b.title = item.label; b.setAttribute('aria-label', item.label); b.dataset.toolbarSegment = `${spec.id}-${i}`; group.append(b); return b;
  }) : [];
  const b = spec.segmented ? null : button('', () => {
    const menu = element('div', 'toolbar-choice-menu');
    entries = spec.items.map((item, i) => {
      const entry = button('', () => { closePopup(); send(spec.items[i].action); }), caption = element('span', '', item.label);
      entry.append(icon(item.icon), caption); entry.setAttribute('role', 'menuitemradio'); entry.setAttribute('aria-checked', i === selected); menu.append(entry);
      return {entry,caption};
    });
    openPopup(b, menu);
  });
  const chosen = b ? element('span', 'toolbar-choice-label') : null, arrow = b ? icon('chevron-down') : null;
  let shownIcon;
  if (b) { b.setAttribute('aria-label', spec.label); b.title = spec.label; row.append(b); }
  group.setAttribute('role', spec.segmented ? 'radiogroup' : 'group'); group.setAttribute('aria-label', spec.label);
  function update(option) {
    spec = option.Choice;
    const items = spec.items; selected = items.findIndex(i => i.selected);
    group.setAttribute('aria-label', spec.label);
    if (label) label.textContent = spec.label;
    if (spec.columns) row.title = spec.label;
    if (b) { b.setAttribute('aria-label', spec.label); b.title = spec.label; }
    if (b) { const item = items[selected] || items[0]; if(shownIcon!==item.icon){shownIcon=item.icon;b.replaceChildren(icon(item.icon),chosen,arrow);} chosen.textContent=item.label; }
    buttons.forEach((button, i) => { button.title = items[i].disabled_reason ?? items[i].label; button.setAttribute('aria-label', items[i].label); if (captions[i]) captions[i].textContent = items[i].label; button.setAttribute('role', 'radio'); button.setAttribute('aria-checked', items[i].selected); button.setAttribute('aria-pressed', items[i].selected); });
    entries.forEach(({entry,caption},i) => { caption.textContent=items[i].label; entry.setAttribute('aria-checked', i === selected); });
  }
  update({ Choice: spec });
  return { row, update, segmented: spec.labeled ? 0 : buttons.length, intrinsic: !!spec.labeled };
}

// Retained DOM controls. Rust owns the field schema, edit context, numeric math,
// and fitting; this adapter reports native font/control measurements.
export function createToolbarComponent({ app, tile, control = tile?.control, presentation = tile, view, element, button, icon, dropdown, dispatch, draggable, target, place, panel, rasterWorker }) {
  const root = element('div', 'tile-button tool-tile toolbar-component');
  root.dataset.toolbarComponent = control.kind;
  const item = tile && { kind: 'tile', panel, tile: tile.id };
  if (item) { root.dataset.tile = tile.id; target(root, item); }
  let model, schema = '', fields = [], vertical = false, extent = [0, 0], stretchDistance = 0, measured = '', selections = '', popup;
  let tileStyle = view.tile_style, style = app.toolbar_ui({ type: 'style', style: tileStyle });
  const controlHeight = liveCopy(app, 'catalog').panel_metrics.control_height;
  const clear = (app.toolbar_ui({ type: 'style', style: 'small' }).size[1] - controlHeight) / 2;
  root.style.setProperty('--field-clear', `${clear}px`);
  const preferences = control.style || { text: true, sliders: true };
  let sliderPreferences = presentation.slider_style;
  const standalone = control.kind !== 'tool_options';
  const paint = standalone ? app.toolbar_ui({type:'slider_paint'}) : null;
  let paintScale = 1;
  const copy = liveCopy(app, 'catalog').native_copy.tool_controls;
  let copyKey = copy.more_options, copyChanged = false;
  const more = button('', () => dispatch({ type: 'activate_tile', panel, tile: tile.id }), 'toolbar-more');
  more.append(icon('more')); bindCopy(more, () => copy.more_options, 'title'); bindCopy(more, () => copy.more_options, 'ariaLabel');
  if (item) { target(more, item); draggable(more, item); } root.append(more);
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
    const row = element('div', `toolbar-slider ${field.id !== 'size' ? 'opacity-track' : 'size-track'}`);
    const cap = button('', show, 'toolbar-slider-cap'); cap.title = field.label; cap.setAttribute('aria-label', field.label);
    if (item) { target(cap, item); draggable(cap, item); }
    const readout = element('span', 'toolbar-slider-readout'), title = element('span', 'toolbar-slider-title');
    const valueRow = element('span', 'toolbar-slider-value-row'), valueLabel = element('span', 'toolbar-slider-value'), unit = element('span', 'toolbar-slider-unit');
    valueRow.append(valueLabel, unit); readout.append(title, valueRow); cap.append(readout); readout.hidden = true;
    const track = element('div', 'number-track'), slider = element('input', 'number-slider');
    slider.type = 'range'; slider.min = 0; slider.max = 1; slider.step = 'any'; slider.value = 0; slider.setAttribute('aria-label', field.label);
    const marks = element('div', 'toolbar-slider-marks'), thumb = element('div', 'toolbar-slider-thumb');
    track.style.setProperty('--position', 0);
    track.append(slider, thumb, marks); row.append(cap, track); more.hidden = true;
    let current = field.value, fill = 0, contact, stamp, canvas, caption, bookmark, preview, previewFrame, previewBusy=false, previewDirty=false;
    const format = () => app.number_input({ control: field.numeric, value: current, operation: { type: 'format' } });
    function change(value) { send(context, { type: 'set_tool_setting', id: field.id, value }); }
    function position(e) {
      const b = slider.getBoundingClientRect(), band = root.getBoundingClientRect();
      return app.toolbar_ui({type:'slider_stretch', geometry:{
        track:vertical?[b.bottom-paint.thumb[0]*paintScale/2,b.y+paint.thumb[0]*paintScale/2]:[b.x+paint.thumb[0]*paintScale/2,b.right-paint.thumb[0]*paintScale/2],
        band:vertical?[band.x,band.right]:[band.y,band.bottom],
        extent:vertical?innerHeight:innerWidth,
        transition:app.phone_slider_stretch_distance()??stretchDistance,
      },point:vertical?[e.clientY,e.clientX]:[e.clientX,e.clientY]});
    }
    function pick(e, snap) {
      const positionValue = position(e),bounds=slider.getBoundingClientRect();
      change(snap ? app.toolbar_ui({ type: 'slider_bookmark_value', control, values: model.bookmarks.map(m => m.value), position: positionValue, travel: Math.max(1, (vertical ? bounds.height : bounds.width) - paint.thumb[0]*paintScale) })
        : app.number_input({ control: field.numeric, value: current, operation: { type: 'position', position: positionValue } }).value);
    }
    function show() {
      if (!model.numeric) return;
      if (!popup || popup !== preview || !popup.isConnected) {
        stamp?.image.close(); stamp = undefined;
        closePopup(); preview = popup = element('div', 'toolbar-brush-preview panel'); popup.popover = 'manual';
        caption = element('span', 'toolbar-preview-caption');
        bookmark = button('', () => { send(context, { type: 'toggle_slider_bookmark', control }); }, 'toolbar-preview-bookmark');
        canvas = element('canvas', 'toolbar-preview-stamp'); popup.append(canvas, caption, bookmark); root.append(popup); popup.showPopover();
        const opened = popup;
        opened.addEventListener('toggle', e => {
          if (e.newState === 'closed' && popup === opened && !opened.matches(':popover-open')) { contact = null; closePopup(); }
        });
      }
      if (!popup.matches(':popover-open')) popup.showPopover();
      paintPreview(); refreshPreview();
    }
    async function refreshPreview() {
      cancelAnimationFrame(previewFrame);
      if (!popup || popup !== preview || !popup.isConnected) return;
      if (previewBusy) { previewDirty=true; return; }
      previewBusy=true; previewDirty=false;
      const opened=preview;
      try {
        const next = app.toolbar_stamp(context, stamp?.request_id);
        if (next?.alpha.length && next.request_id !== stamp?.request_id) {
          const image=await rasterWorker({operation:'alpha-image',metadata:{size:next.size},buffers:[next.alpha]});
          if (popup!==opened || !opened.isConnected) { image.close(); return; }
          stamp?.image.close(); stamp = {...next, image};
          canvas.dataset.previewRequest = String(next.request_id);
          paintPreview();
        }
        if (popup!==opened || !opened.isConnected) return;
        previewFrame = requestAnimationFrame(refreshPreview);
      } catch (error) { console.error(error); if(popup===opened)closePopup(); }
      finally {
        previewBusy=false;
        if (previewDirty && popup && popup===preview && popup.isConnected) {
          cancelAnimationFrame(previewFrame); previewFrame=requestAnimationFrame(refreshPreview);
        }
      }
    }
    function paintPreview() {
      if (!popup || popup !== preview) return;
      const geometry = app.toolbar_ui({ type: 'slider_preview', control, style: tileStyle, value: current, length: Math.max(...extent), extent: stamp?.extent || 1 });
      popup.style.width = popup.style.height = `${geometry.side}px`; popup.style.setProperty('--tile-radius', `${geometry.radius}px`);
      for (const [node, b] of [[caption, geometry.caption], [bookmark, geometry.bookmark]]) Object.assign(node.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.width}px`, height: `${b.height}px` });
      bookmark.style.setProperty('--preview-icon', `${geometry.icon}px`);
      const a = root.getBoundingClientRect(), handle = thumb.getBoundingClientRect(), side=geometry.side;
      const [,bounds] = app.toolbar_ui({type:'slider_placement',handle:rectangle(handle),strip:rectangle(a),viewport:{x:0,y:0,width:innerWidth,height:innerHeight},axis:vertical?'vertical':'horizontal',side:geometry.side});
      popup.style.left = `${bounds.x}px`; popup.style.top = `${bounds.y}px`;
      caption.textContent = geometry.text;
      const selected = model.bookmarks.some(m => m.selected);
      if (bookmark.dataset.selected !== String(selected)) { bookmark.replaceChildren(icon(selected ? 'minus' : 'plus')); bookmark.dataset.selected = selected; }
      bindCopy(bookmark, () => model.bookmarks.some(m => m.selected) ? copy.remove_bookmark : copy.bookmark_value, 'title');
      bindCopy(bookmark, () => model.bookmarks.some(m => m.selected) ? copy.remove_bookmark : copy.bookmark_value, 'ariaLabel');
      const ratio = devicePixelRatio || 1;
      const pixels = Math.round(side * ratio);
      if (canvas.width !== pixels || canvas.height !== pixels) canvas.width = canvas.height = pixels;
      const ctx = canvas.getContext('2d', { willReadFrequently: true }); ctx.resetTransform(); ctx.clearRect(0, 0, pixels, pixels); ctx.scale(ratio, ratio);
      ctx.save(); ctx.beginPath(); const viewport = geometry.viewport; ctx.rect(viewport.x, viewport.y, viewport.width, viewport.height); ctx.clip();
      ctx.globalAlpha = geometry.opacity; const b = geometry.stamp; if (stamp) ctx.drawImage(stamp.image, b.x, b.y, b.width, b.height);
      ctx.globalAlpha = 1; ctx.globalCompositeOperation = 'source-in'; ctx.fillStyle = getComputedStyle(root).color; ctx.fillRect(0, 0, side, side); ctx.restore();
      if (geometry.header_fade) {
        const fade = ctx.createLinearGradient(0, 0, 0, geometry.header_fade), background = getComputedStyle(popup).backgroundColor;
        fade.addColorStop(0, background); fade.addColorStop(1, 'transparent');
        ctx.save(); ctx.globalAlpha = geometry.header_fade_opacity; ctx.fillStyle = fade; ctx.fillRect(0, 0, side, geometry.header_fade); ctx.restore();
      }
    }
    function update(option) {
      slider.disabled = !model.numeric; cap.disabled = !model.numeric;
      if (!model.numeric) { updateReadout(); return; }
      if (option) { current = option.value; field = { ...option, numeric: { ...option.numeric, digits: Number(option.numeric.digits) } }; }
      cap.title = field.label; cap.setAttribute('aria-label', field.label); slider.setAttribute('aria-label', field.label);
      const shown = format(); fill = shown.fill; slider.value = fill; slider.setAttribute('aria-valuetext', shown.text);
      marks.replaceChildren(...model.bookmarks.map(mark => {
        const line = element('span', 'toolbar-slider-mark'); line.classList.toggle('selected', mark.selected);
        line.style.setProperty('--position', mark.selected ? fill : mark.fill); return line;
      }));
      track.style.setProperty('--position', fill); updateReadout(); paintPreview(); refreshPreview();
    }
    function finish(cancelled) {
      if(!contact)return;
      const moved=contact.moved;contact=null;
      dispatch({type:'phone',action:{kind:'slider',control,phase:cancelled?'cancel':'up'}});
      if(cancelled||moved)closePopup();
    }
    slider.addEventListener('pointerdown', e => {
      if (e.button || slider.disabled || contact) return;
      e.preventDefault(); e.stopPropagation();
      contact = { id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
      slider.setPointerCapture(e.pointerId); dispatch({type:'phone',action:{kind:'slider',control,phase:'down'}}); pick(e, true); show();
    });
    slider.addEventListener('pointermove', e => {
      if (contact?.id !== e.pointerId) return;
      if (!app.toolbar_ui({type:'slider_dragging',moved:contact.moved,delta:[(e.clientX-contact.x)/paintScale,(e.clientY-contact.y)/paintScale]})) return;
      contact.moved = true; pick(e, false);
    });
    slider.addEventListener('pointerup', e => { if (contact?.id === e.pointerId) finish(false); });
    for (const type of ['pointercancel', 'lostpointercapture']) slider.addEventListener(type, () => finish(true));
    // Chromium's native touch range edit runs after pointerdown and can replace
    // a snapped value. Pointer capture above owns touch; retain keyboard input.
    slider.addEventListener('touchstart', e => { if (e.cancelable) e.preventDefault(); }, { passive: false });
    slider.addEventListener('input', () => { change(app.number_input({ control: field.numeric, value: current, operation: { type: 'position', position: Number(slider.value) } }).value); show(); });
    const outside = e => { if (popup && popup === preview && !row.contains(e.target) && !popup.contains(e.target)) { finish(true); closePopup(); } };
    const escape = e => { if (e.key === 'Escape') { finish(true); closePopup(); } };
    document.addEventListener('pointerdown', outside, true); document.addEventListener('keydown', escape);
    const blur = () => { finish(true); closePopup(); }; window.addEventListener('blur', blur);
    function orient() {
      slider.style.writingMode = vertical ? 'vertical-lr' : ''; slider.style.direction = vertical ? 'rtl' : '';
      slider.setAttribute('aria-orientation', vertical ? 'vertical' : 'horizontal');
      paintScale = tile ? 1 : app.toolbar_ui({type:'slider_scale',cross:vertical?extent[0]:extent[1]});
      const {cap:capBounds, track:trackBounds, show_value, stretch_distance} = app.toolbar_ui({ type: 'slider_layout', width: extent[0]/paintScale, height: extent[1]/paintScale, axis: vertical ? 'vertical' : 'horizontal', style: tileStyle, slider_style: sliderPreferences, grip: !!tile });
      stretchDistance = stretch_distance*paintScale; cap.hidden = !tile; readout.hidden = !show_value;
      row.style.width=`${extent[0]/paintScale}px`;row.style.height=`${extent[1]/paintScale}px`;
      row.style.transformOrigin='0 0';row.style.transform=`scale(${paintScale})`;
      place(cap, capBounds);
      const inset=tile?paint.inset:0,cross=paint.cross,along=Math.max(0,(vertical?trackBounds.height:trackBounds.width)-2*inset);
      place(track, vertical ? {x:(extent[0]/paintScale-cross)/2,y:trackBounds.y+inset,width:cross,height:along}
        : {x:trackBounds.x+inset,y:(extent[1]/paintScale-cross)/2,width:along,height:cross});
      const inputCross=(vertical?extent[0]:extent[1])/paintScale;
      Object.assign(slider.style,vertical?{left:`${(cross-inputCross)/2}px`,top:'0',width:`${inputCross}px`,height:'100%'}
        :{left:'0',top:`${(cross-inputCross)/2}px`,width:'100%',height:`${inputCross}px`});
      for(const [name,value]of Object.entries({'thumb-length':paint.thumb[0],'thumb-cross':paint.thumb[1],'thumb-radius':paint.radius,'mark-length':paint.mark[0],'mark-thickness':paint.mark[1]}))
        row.style.setProperty(`--slider-${name}`,`${value}px`);
      track.style.setProperty('--track-shape', trackShape(along-paint.thumb[0], field.id!=='size'?paint.wide:paint.narrow));
      updateReadout();
    }
    function updateReadout() {
      if (readout.hidden) return;
      const presentation = app.toolbar_ui({type:'slider_readout', style:tileStyle, axis:vertical?'vertical':'horizontal', control:field.numeric, value:current});
      const {inset, gap, units_below} = presentation.metrics;
      title.hidden = !style.labeled; title.textContent = model.value_label;
      valueLabel.textContent = model.numeric ? presentation.value : ''; unit.textContent = model.numeric ? presentation.unit : '';
      readout.style.gap = valueRow.style.gap = `${gap}px`;
      valueRow.style.flexDirection = units_below ? 'column' : 'row';
      if (!extent[0] || !extent[1]) return;
      const standard = parseFloat(getComputedStyle(root).fontSize);
      valueRow.style.fontSize = `${standard}px`;
      const available = [Math.max(1, cap.clientWidth - 2*inset), Math.max(1, cap.clientHeight - 2*inset - (title.hidden ? 0 : title.getBoundingClientRect().height + gap))];
      const natural = valueRow.getBoundingClientRect();
      let scale = Math.min(1, available[0]/Math.max(1,natural.width), available[1]/Math.max(1,natural.height));
      for (let pass=0; pass<4; pass++) {
        valueRow.style.fontSize = `${standard*scale}px`;
        const measured = valueRow.getBoundingClientRect();
        const fit = Math.min(1, available[0]/Math.max(1,measured.width), available[1]/Math.max(1,measured.height));
        if (fit >= 1) break;
        scale *= fit;
      }
    }
    function trackShape(length, narrow) {
      if (length < 4) return 'none';
      const half=paint.thumb[0]/2,center=paint.cross/2,wide=paint.wide,[tip,bend]=paint.tip;
      const point = (a, b) => vertical ? `${center + b} ${length + half - a}` : `${half + a} ${center + b}`;
      return `path('M ${point(0, -narrow)} L ${point(length - tip, -wide)} C ${point(length + bend, -wide)} ${point(length + bend, wide)} ${point(length - tip, wide)} `
        + `L ${point(0, narrow)} C ${point(-tip, narrow)} ${point(-tip, -narrow)} ${point(0, -narrow)} Z')`;
    }
    update(); return { row, update, orient, cancel:()=>finish(true), dispose() { finish(true); closePopup(); stamp?.image.close(); document.removeEventListener('pointerdown', outside, true); document.removeEventListener('keydown', escape); window.removeEventListener('blur', blur); } };
  }
  function numeric(field, context) {
    const row = element('div', 'toolbar-option toolbar-numeric');
    row.dataset.toolbarSetting = field.id; row.dataset.toolbarField = '';
    const info = app.toolbar_ui({ type: 'numeric_info', id: field.id, control: field.numeric, compact: true, units: true });
    let units = true;
    const change = value => send(context, { type: 'set_tool_setting', id: field.id, value });
    const errorCaption = reason => app.native_caption({type:'numeric_error',reason});
    const number = createNumberField({ readoutGestures:false, control: field.numeric, label: field.label, labels:next=>app.numeric_labels(next), errorCaption, icon, inline: true,
      widthSamples: info.samples, onChange: change,
      resolve: request => app.toolbar_ui({ type: 'number', request, compact: true, units }) });
    if(field.preview)number.onEditPhase=phase=>send(context,{type:'tool_setting_contact',id:field.id,phase});
    captureSliderContacts(number);
    const label = element('span', 'toolbar-option-label', field.label), glyph = icon(info.icon);
    label.title = field.label;
    const face = button('', () => {}, 'toolbar-number-face');
    const faceIcon = icon(info.icon), faceLabel = element('span', 'toolbar-face-label', field.label), faceValue = element('span', 'toolbar-face-value');
    face.append(faceIcon, faceLabel, faceValue); face.title = field.label; face.setAttribute('aria-label', field.label);
    for (const node of [label, glyph]) node.ondblclick = () => send(context, { type: 'reset_tool_setting', id: field.id });
    let current = field.value, editor;
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
      let text = format(tileStyle !== 'small');
      const available = Math.min(style.size[0], extent[0] || style.size[0]) - (style.labeled ? 38 : 4);
      if (textMeasure.measureText(text.replace(/\d/g, '8')).width + 2 > available) text = format(false);
      faceValue.textContent = text;
      faceValue.toggleAttribute('data-wide', text.length >= 4);
    }
    function scrub(node) {
      let contact, scrubbed = false;
      const finish=phase=>{const moved=contact?.moved;contact=null;if(moved&&field.preview)send(context,{type:'tool_setting_contact',id:field.id,phase});};
      node.addEventListener('pointerdown', e => {
        if (e.button || e.pointerType === 'mouse' || !number.commit()) return;
        scrubbed = false;
        contact = { id: e.pointerId, y: e.clientY, fill: app.number_input({ control: field.numeric, value: current, operation: { type: 'format' } }).fill, moved: false };
      });
      node.addEventListener('pointermove', e => {
        if (contact?.id !== e.pointerId || (!contact.moved && Math.abs(e.clientY - contact.y) < 8)) return;
        if (!number.commit()) { finish('cancel'); return; }
        if(!contact.moved&&field.preview)send(context,{type:'tool_setting_contact',id:field.id,phase:'down'});
        contact.moved = true; node.setPointerCapture(e.pointerId); e.preventDefault();
        number.apply({ type: 'position', position: contact.fill + (contact.y - e.clientY) / 200 });
      });
      node.addEventListener('pointerup', e => { if (contact?.id === e.pointerId) { scrubbed = contact.moved; finish('up'); } });
      node.addEventListener('click', e => { if (scrubbed) { e.stopImmediatePropagation(); e.preventDefault(); scrubbed = false; } }, true);
      node.addEventListener('lostpointercapture', () => finish('cancel'));
      for (const type of ['workspace-drag-held', 'workspace-context-claimed']) row.addEventListener(type, () => finish('cancel'));
      node.addEventListener('pointercancel', () => finish('cancel'));
      node.addEventListener('wheel', e => { e.preventDefault(); if (!number.commit()) return; number.apply({ type: 'step', steps: e.deltaY < 0 ? 1 : -1 }); }, { passive: false });
    }
    scrub(number.valueButton); scrub(face);
    face.addEventListener('click', () => {
      editor = createNumberField({ control: field.numeric, label: field.label, labels:next=>app.numeric_labels(next), errorCaption, icon, resolve: request => app.number_input(request), onChange: change });
      editor.onReset=()=>send(context,{type:'reset_tool_setting',id:field.id});
      if(field.preview)editor.onEditPhase=phase=>send(context,{type:'tool_setting_contact',id:field.id,phase});
      editor.update(current); openPopup(face, editor);
    });
    row.append(label, glyph, number, face);
    function orient() {
      units = !vertical || tileStyle !== 'small'; number.format(); updateFace();
      label.hidden = vertical || !preferences.text; glyph.style.display = !vertical && !preferences.text ? '' : 'none';
      face.hidden = !vertical; number.hidden = vertical;
      face.classList.toggle('labeled', style.labeled); faceLabel.hidden = !style.labeled;
      number.querySelector('.number-track').hidden = !preferences.sliders;
    }
    update(field.value);
    return { row, update(option) { const next=option.Numeric??option; if(copyChanged||field.label!==next.label){field=next;number.relabel(next.label);if(editor?.isConnected)editor.relabel(next.label);label.textContent=faceLabel.textContent=next.label;label.title=face.title=next.label;face.setAttribute('aria-label',next.label);} update(next.value); }, orient, dispose: () => number.cancelEditing(), number };
  }
  function choice(spec, context) {
    const field = choiceField({ app, element, button, icon, openPopup, closePopup }, spec, action => send(context, action));
    return { ...field, orient() {
      field.row.classList.toggle('labeled', style.labeled); field.row.classList.toggle('stacked', vertical && extent[0] < style.size[0] * field.segmented);
    } };
  }
  function range(spec, context) {
    const row = element('div', 'toolbar-option toolbar-range'); row.dataset.toolbarField = '';
    const control = createRangeControl({app, ...spec, icon, prefix:'toolbar', showSlider:preferences.sliders,
      onChange:(index,value)=>send(context,{type:'set_tool_setting',id:spec.bounds[index].id,value})});
    row.style.minWidth = preferences.sliders ? '280px' : '0'; row.append(control);
    return {row, interval:true, update(option){const next=option.Range;if(copyChanged||spec.label!==next.label||spec.bounds.some((field,i)=>field.label!==next.bounds[i].label)){control.relabel(next.bounds,next.label);spec=next;}control.update(next.bounds.map(f=>f.value));}, dispose:()=>control.dispose()};
  }
  function action(spec, context) {
    return actionField({ element, button, icon }, spec, action => send(context, action));
  }
  function layout() {
    if (!model || !root.isConnected || !extent[0] || !extent[1]) return;
    root.classList.toggle('vertical-component', vertical); root.dataset.tileStyle = tileStyle;
    fields.forEach(f => { f.row.classList.remove('allocated'); f.orient?.(); });
    if (standalone) return;
    const sizes = fields.map(f => {
      if (f.gradient) return vertical ? [extent[0], controlHeight] : [120 + 2 * clear, style.size[1]];
      if (f.segmented) return vertical ? [extent[0], style.size[1] * (extent[0] < style.size[0] * f.segmented ? f.segmented : 1)] : [style.size[0] * f.segmented, style.size[1]];
      if (!f.interval && !f.intrinsic && (vertical || f.action)) return style.size;
      f.row.style.width = 'max-content'; f.row.style.height = 'auto'; f.row.hidden = false;
      const bounds=f.row.getBoundingClientRect();
      return [Math.ceil(bounds.width), Math.max(controlHeight, bounds.height)];
    });
    const geometry = app.toolbar_ui({ type: 'options_layout', width: extent[0], height: extent[1], axis: vertical ? 'vertical' : 'horizontal', sizes, button: style.size, gap: style.gap });
    place(more, geometry.more);
    fields.forEach((f, i) => {
      const bounds = geometry.fields[i];
      if (!bounds && f.row.contains(document.activeElement)) more.focus();
      f.row.hidden = !bounds;
      f.row.classList.toggle('allocated', !!bounds && !vertical);
      if (bounds) place(f.row, (f.segmented || f.gradient) && !vertical ? { x: bounds.x + clear, y: bounds.y + (bounds.height - controlHeight) / 2, width: bounds.width - 2 * clear, height: controlHeight } : bounds);
    });
  }
  root.updateComponent = next => {
    const value = next.component;
    sliderPreferences = next.slider_style;
    copyChanged = copyKey !== copy.more_options; copyKey = copy.more_options;
    const nextSchema = key([sliderPreferences, value.context, value.numeric && { ...fieldSemantic(value.numeric), value: 0 }, value.options.map(o => o.Gradient ? {Gradient:o.Gradient.gradient.destination} : o.Range ? {Range:{...semantic(o.Range),bounds:o.Range.bounds.map(f=>({...fieldSemantic(f),value:0}))}} : o.Numeric ? { Numeric: { ...fieldSemantic(o.Numeric), value: 0 } } : o.Checklist ? {Checklist:{...semantic(o.Checklist),items:o.Checklist.items.map(i=>({...semantic(i),selected:false,enabled:true}))}} : o.Choice ? { Choice: { ...semantic(o.Choice), items: o.Choice.items.map(i => ({ ...semantic(i), selected: false })) } } : { Action: { ...o.Action, state: { ...semantic(o.Action.state), selected: false, enabled: true } } })]);
    const sliderField = () => {
      if (value.numeric) return value.numeric;
      const numeric = app.toolbar_ui({ type: 'slider_spec', control });
      return { id: control.kind === 'brush_size_slider' ? 'size' : 'strength', label: next.label, numeric, value: numeric.soft_min };
    };
    model = value;
    if (standalone && !value.numeric && fields.length) {
      closePopup(); fields[0].cancel();
      if (model !== value) return;
      fields[0].update(); layout(); return;
    }
    if (nextSchema !== schema) {
      closePopup(); const previous=fields; fields=[]; schema=nextSchema;
      if (standalone) {
        fields.push(brushSlider(sliderField(), value.context));
      } else fields = value.options.map(o => o.Gradient ? gradientButton({app,element,button,icon,dropdown,dispatch:action=>send(value.context,action)},o.Gradient,openPopup) : o.Range ? range(o.Range, value.context) : o.Numeric ? numeric(o.Numeric, value.context) : o.Checklist ? checklistField({element,button,icon},o.Checklist,action=>send(value.context,action)) : o.Choice ? choice(o.Choice, value.context) : action(o.Action, value.context));
      fields.forEach(f => root.append(f.row)); measured = '';
      previous.forEach(f=>{f.row.remove();f.dispose?.();});
      if(model!==value)return;
    }
    if (standalone) fields[0].update(sliderField());
    else fields.forEach((f, i) => f.update(value.options[i]));
    const selected = key(value.options.map(o => o.Choice && !o.Choice.segmented ? o.Choice.items.findIndex(i => i.selected) : null));
    if (selected !== selections) { selections = selected; measured = ''; }
    if (!measured) { layout(); measured = schema; }
  };
  root.layoutComponent = (bounds, axis, presented = tileStyle) => {
    const next = key([bounds.width, bounds.height, axis, presented]);
    if (next === root.dataset.componentBounds) return;
    if (presented !== tileStyle) { tileStyle = presented; style = app.toolbar_ui({ type: 'style', style: tileStyle }); }
    fields.forEach(f=>f.cancel?.()); root.dataset.componentBounds = next; extent = [bounds.width, bounds.height]; vertical = axis === 'vertical'; closePopup(); layout();
  };
  root.disposeComponent = () => { closePopup(); fields.forEach(f => f.dispose?.()); };
  root.closeComponent = () => { fields.forEach(f=>f.cancel?.()); closePopup(); };
  root.updateComponent(presentation);
  return root;
}
