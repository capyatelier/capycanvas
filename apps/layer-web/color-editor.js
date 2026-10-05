import {liveCopy,bindCopy} from './localization.js';
import {createRasterWorker} from './raster-worker-client.js';
import {wheelPainter,wheelPicker,hueStopCache,wheelHit,rgba,json} from './color-wheel.js';

let services = null, worker = null;
const editors = new Set();
const fieldWorker = request => (worker ??= createRasterWorker())(request);

export function configureColorEditor(next) { services = next; }
export function refreshColorEditors() { for (const editor of [...editors]) editor(); }

const SHAPES = [['circle','OKLCH'],['square','HSB'],['triangle','HLS']];
const speed = e => e.shiftKey ? 'fast' : (e.altKey || e.ctrlKey) ? 'fine' : 'normal';

export function chooseColor({app, color, slot, element, button, opaque = false}) {
  const {dispatch, icon} = services;
  const catalog = () => liveCopy(app,"catalog").native_copy, copy = liveCopy(app,"catalog").native_copy.color, common = liveCopy(app,"bootstrap_view").common;
  const epoch = app.state().document_file.epoch;
  const colors = app.state().layer_tools.mask_editing?.colors ?? app.state().colors;
  const rendition = app.color_panel().rendition ?? null;
  const canPick = !document.querySelector('dialog[open]');
  let response;
  try { response = app.color_ui({type:'editor_open', colors, ...(slot ? {slot} : {color}), opaque, display_space:'Srgb', rendition}); }
  catch (error) { console.error('Edit Color', error); return Promise.resolve(null); }
  let editor = response.editor, view = response.view;
  const memory = json(editor.picker.editor);
  const request = action => {
    const next = app.color_ui({type:'editor', editor, action, display_space:'Srgb', rendition});
    editor = next.editor; view = next.view; return next.error;
  };
  return new Promise(resolve => {
    const root = element('dialog', 'color-dialog'), body = element('div', 'color-editor-body');
    bindCopy(root, () => copy.edit, 'ariaLabel');
    const wheelBox = element('div', 'color-editor-wheel'), stage = element('div', 'color-editor-stage'), canvas = element('canvas', 'color-wheel');
    bindCopy(canvas, () => copy.wheel, 'ariaLabel');
    const arc = document.createElementNS('http://www.w3.org/2000/svg', 'svg'), ramp = document.createElementNS(arc.namespaceURI, 'g'), track = document.createElementNS(arc.namespaceURI, 'path');
    const markerShadow = document.createElementNS(arc.namespaceURI, 'circle'), marker = document.createElementNS(arc.namespaceURI, 'circle');
    arc.classList.add('color-intensity'); track.setAttribute('fill','none'); track.setAttribute('stroke','transparent'); track.setAttribute('stroke-linecap','round');
    track.setAttribute('tabindex','0'); track.setAttribute('role','slider'); bindCopy(track, () => copy.intensity, 'ariaLabel');
    markerShadow.setAttribute('fill','none'); markerShadow.setAttribute('stroke','rgba(0,0,0,.5)'); markerShadow.setAttribute('stroke-width','4');
    marker.setAttribute('stroke','white'); marker.setAttribute('stroke-width','2');
    arc.append(ramp, track, markerShadow, marker); stage.append(canvas, arc); wheelBox.append(stage);
    const shapes = element('div', 'color-shapes'); shapes.setAttribute('role','radiogroup');
    const shapeButtons = SHAPES.map(([shape, label], i) => {
      const node = button('', () => act({op:'wheel', action:{op:'shape', shape}}), 'color-shape-choice');
      node.dataset.shape = shape; node.setAttribute('role','radio');
      node.append(icon(`color-${shape}`), element('span', '', label));
      bindCopy(node, () => [copy.circle, copy.square, copy.triangle][i], 'title');
      shapes.append(node); return node;
    });
    const left = element('div', 'color-editor-left'); left.append(wheelBox, shapes);
    const title = element('h2', 'color-editor-title', () => copy.edit);
    const current = button('', () => act({op:'revert'}), 'color-current'), fresh = element('span', 'color-new');
    bindCopy(current, () => copy.current, 'title'); bindCopy(current, () => copy.current, 'ariaLabel'); bindCopy(fresh, () => copy.new, 'ariaLabel');
    const pair = element('div', 'color-pair'); pair.append(current, fresh);
    const captions = element('div', 'color-pair-captions'); captions.append(element('span', '', () => copy.current), element('span', '', () => copy.new));
    const pick = button('', () => startPick(), 'color-pick'); pick.append(icon('eyedropper')); pick.hidden = !canPick;
    bindCopy(pick, () => copy.pick_canvas, 'title'); bindCopy(pick, () => copy.pick_canvas, 'ariaLabel');
    const fields = [];
    const field = (target, name) => {
      const cell = element('span', 'color-value-cell'), show = element('button', 'color-value'), input = element('input', 'color-value-input');
      show.type = 'button'; input.type = 'text'; input.autocomplete = 'off'; input.spellcheck = false; input.hidden = true;
      show.dataset.colorValue = name; input.dataset.colorValue = name; cell.append(show, input);
      const entry = {target, name, cell, show, input, edit:'', label:''}; fields.push(entry); return entry;
    };
    const copyButton = name => { const node = button('', () => copyText(node, name === 'hex' ? view.hex : view.rows[name].copy), 'color-copy'); node.dataset.colorCopy = name; node.append(icon('copy')); bindCopy(node, () => copy.copy, 'title'); bindCopy(node, () => copy.copy, 'ariaLabel'); return node; };
    const hexNote = element('span', 'color-badge');
    const hex = field('hex', 'hex'), hexCopy = copyButton('hex');
    const hexBlock = element('div', 'color-hex'); hexBlock.append(hexNote, hex.cell, hexCopy);
    const head = element('div', 'color-editor-head'); head.append(pair, pick, hexBlock, captions);
    const grid = element('div', 'color-rows');
    const rows = [0, 1, 2].map(row => {
      const format = button('', () => toggleMenu(row), 'color-format'), label = element('span', 'color-format-names'), menu = element('div', 'color-format-menu'), space = element('span', 'color-badge');
      format.dataset.colorFormat = row; format.setAttribute('aria-haspopup','menu'); format.append(label, icon('chevron-down')); bindCopy(format, () => copy.format, 'title');
      menu.setAttribute('role','menu'); menu.hidden = true;
      const name = element('span', 'color-row-name'), spaceCell = element('span', 'color-row-space'); name.append(format, menu); spaceCell.append(space); grid.append(name, spaceCell);
      const values = [0, 1, 2].map(index => { const entry = field({kind:'value', row, index}, `${row}-${index}`); grid.append(entry.cell); return entry; });
      const rowCopy = copyButton(row); grid.append(rowCopy);
      return {format, label, menu, space, values, copy:rowCopy};
    });
    const intensityName = element('span', 'color-row-name', () => copy.intensity_ev), intensity = field({kind:'intensity'}, 'ev');
    intensity.cell.classList.add('color-intensity-cell'); grid.append(intensityName, intensity.cell);
    const status = element('p', 'color-editor-error'); status.setAttribute('role','status');
    const values = element('div', 'color-editor-values'); values.append(grid, status);
    body.append(title, left, head, values);
    const sheet = element('div', 'color-sheet'), search = element('input'), sheetClose = button('', () => showSheet(false), 'color-sheet-close'), sheetBody = element('div', 'color-sheet-body');
    search.type = 'search'; search.autocomplete = 'off'; search.spellcheck = false; bindCopy(search, () => copy.swatch_search, 'placeholder'); bindCopy(search, () => copy.swatch_search, 'ariaLabel');
    sheetClose.append(icon('chevron-down')); bindCopy(sheetClose, () => copy.close_swatches, 'title'); bindCopy(sheetClose, () => copy.close_swatches, 'ariaLabel');
    const sheetHead = element('div', 'color-sheet-head'); sheetHead.append(search, sheetClose); sheet.append(sheetHead, sheetBody); sheet.hidden = true;
    const recent = element('div', 'color-recent'), mini = element('div', 'color-mini'), miniPair = element('div', 'color-pair'), miniCurrent = element('span'), miniNew = element('span'), miniHex = element('span', 'color-mini-hex');
    miniPair.append(miniCurrent, miniNew); mini.append(miniPair, miniHex); mini.hidden = true;
    const more = button('', () => showSheet(sheet.hidden), 'color-swatches'); more.append(icon('chevron-down'));
    const cancel = button(() => common.cancel, () => root.close(), 'color-cancel');
    const apply = button(() => copy.use_color, () => {
      if (apply.disabled) return;
      if (app.state().document_file.epoch === epoch) result = {color: view.value, intensity: view.stops ?? null};
      root.close();
    }, 'suggested-action');
    const footer = element('footer', 'color-editor-bottom'); footer.append(recent, mini, more, cancel, apply);
    const page = element('div', 'color-editor-page'); page.append(body, sheet);
    const content = element('div', 'color-editor'); content.append(page, footer);
    root.append(content); document.body.append(root);
    const strip = button('', () => { if (app.color_preview().picker.editor) dispatch({type:'color_picker', action:{kind:'toggle'}}); }, 'color-strip');
    const stripPair = element('span', 'color-pair'), stripOriginal = element('span'), stripSample = element('span'), stripHex = element('span', 'color-strip-hex'), stripEv = element('span', 'color-strip-ev'), stripSpace = element('span', 'color-strip-space'), stripValues = element('span', 'color-strip-values');
    const stripTop = element('span', 'color-strip-line'), stripLine = element('span', 'color-strip-line'), stripText = element('span', 'color-strip-text');
    stripPair.append(stripOriginal, stripSample); stripTop.append(stripHex, stripEv); stripLine.append(stripSpace, stripValues); stripText.append(stripTop, stripLine); strip.append(stripPair, stripText);
    bindCopy(strip, () => copy.picking_strip, 'title'); bindCopy(strip, () => copy.picking_strip, 'ariaLabel'); strip.hidden = true; document.body.append(strip);
    let result = null, error = null, errorTarget = null, refused = null, picking = false, closed = false, scrub = null, layoutKey = '', paintKey = '', frame = 0, sheetOpen = false;
    const painter = wheelPainter({canvas, worker:fieldWorker, request:pixels => json({side:pixels, colors:editor.picker, rendition:view.panel.rendition ?? null}), hueStops:hueStopCache(app), repaint:() => { paintKey = ''; queueDraw(); }});
    const hit = wheelHit(app);
    wheelPicker(canvas, (size, point) => hit(size, point, view.panel.shape), action => act({op:'wheel', action}));
    const same = (a, b) => json(a) === json(b);
    function act(action, target = null) {
      const failure = request(action);
      if (failure || same(errorTarget, target) || errorTarget == null) { error = failure; errorTarget = failure ? target : null; refused = failure ? action : null; }
      render(); return !failure;
    }
    function queueDraw() { if (!frame) frame = requestAnimationFrame(() => { frame = 0; draw(); }); }
    function draw() {
      const width = Math.round(wheelBox.clientWidth); if (width < 128) return;
      const panel = view.panel, side = width, size = side + 28, hdr = panel.hdr;
      const layout = app.color_ui({type:'layout', size}), inset = layout.wheel[1];
      const nextLayout = JSON.stringify([size, hdr]);
      if (nextLayout !== layoutKey) {
        layoutKey = nextLayout;
        const geometry = hdr ? app.color_ui({type:'arc', size, fraction:0}).geometry : null;
        const bottom = hdr ? geometry.center[1] + geometry.radius + Math.max(geometry.width / 2, geometry.marker_radius) + 2 : inset + side;
        wheelBox.style.height = `${bottom - inset}px`;
        Object.assign(stage.style, {left:`${-inset}px`, top:`${-inset}px`, width:`${size}px`, height:`${bottom}px`});
        Object.assign(canvas.style, {left:`${layout.wheel[0]}px`, top:`${layout.wheel[1]}px`, width:`${side}px`, height:`${side}px`});
        arc.setAttribute('width', size); arc.setAttribute('height', bottom);
      }
      arc.style.display = hdr ? '' : 'none';
      if (hdr) {
        const g = app.color_ui({type:'arc', size, fraction:(panel.intensity + 2) / 8}), a = g.geometry;
        const start = app.color_ui({type:'arc', size, fraction:0}).point, end = app.color_ui({type:'arc', size, fraction:1}).point;
        track.setAttribute('d', `M${start} A${a.radius} ${a.radius} 0 0 0 ${end}`); track.setAttribute('stroke-width', a.width);
        for (const node of [markerShadow, marker]) { node.setAttribute('cx', g.point[0]); node.setAttribute('cy', g.point[1]); node.setAttribute('r', a.marker_radius); }
        marker.setAttribute('fill', rgba(panel.marker_color));
        track.setAttribute('aria-valuenow', panel.intensity); track.setAttribute('aria-valuetext', `${panel.intensity.toFixed(2)} EV`);
        g.path.slice(1).forEach((p, i) => {
          let segment = ramp.children[i];
          if (!segment) { segment = document.createElementNS(arc.namespaceURI, 'path'); segment.setAttribute('stroke-linecap','round'); ramp.append(segment); }
          segment.setAttribute('d', `M${g.path[i]} L${p}`); segment.setAttribute('stroke', rgba(panel.intensity_ramp[i])); segment.setAttribute('stroke-width', a.width);
        });
      }
      const pixels = Math.ceil(side * Math.min(devicePixelRatio || 1, 2)), next = json([panel, pixels]);
      if (next !== paintKey) { paintKey = next; painter.paint(panel, side, pixels); }
    }
    let arcContact = null;
    const arcPick = e => { const r = stage.getBoundingClientRect(), hitArc = app.color_ui({type:'arc', size:r.width, point:[e.clientX - r.x, e.clientY - r.y]}); act({op:'wheel', action:{op:'hdr_intensity', stops:Math.round((-2 + 8 * hitArc.fraction) * 100) / 100}}); };
    track.onpointerdown = e => { if (e.button !== 0) return; arcContact = e.pointerId; track.setPointerCapture(e.pointerId); e.preventDefault(); arcPick(e); };
    track.onpointermove = e => { if (e.pointerId === arcContact) arcPick(e); };
    for (const name of ['pointerup','pointercancel','lostpointercapture']) track.addEventListener(name, e => { if (e.pointerId === arcContact) arcContact = null; });
    track.onkeydown = e => {
      if (!['ArrowLeft','ArrowDown','ArrowRight','ArrowUp'].includes(e.key)) return;
      e.preventDefault(); act({op:'wheel', action:{op:'hdr_intensity', stops:Math.max(-2, Math.min(6, view.panel.intensity + (['ArrowLeft','ArrowDown'].includes(e.key) ? -.1 : .1)))}});
    };
    function render() {
      const editingAny = fields.find(entry => !entry.input.hidden);
      shapeButtons.forEach((node, i) => { const choice = view.shapes[i]; node.setAttribute('aria-checked', String(choice.selected)); node.classList.toggle('selected', choice.selected); });
      for (const [node, preview] of [[current, view.current], [fresh, view.new], [miniCurrent, view.current], [miniNew, view.new]]) node.style.background = rgba(preview.rgba);
      const setField = (entry, text, edit, name) => {
        entry.edit = edit; entry.label = name;
        if (entry.show.textContent !== text) entry.show.textContent = text;
        entry.show.setAttribute('aria-label', `${name} ${text}`); entry.input.setAttribute('aria-label', name);
      };
      setField(hex, view.hex, view.hex, copy.hex); miniHex.textContent = view.hex;
      hexNote.hidden = !view.hex_note; hexNote.textContent = view.hex_note?.text ?? ''; hexNote.title = view.hex_note?.tip ?? '';
      view.rows.forEach((shown, row) => {
        const r = rows[row]; r.format.setAttribute('aria-label', shown.label);
        while (r.label.children.length < shown.forms.length) r.label.append(element('span'));
        shown.forms.forEach((choice, i) => { const name = r.label.children[i]; if (name.textContent !== choice.label) name.textContent = choice.label; name.classList.toggle('current', choice.form === shown.form); });
        r.space.hidden = !shown.space; r.space.textContent = shown.space ?? '';
        while (r.menu.children.length < shown.forms.length) {
          const item = button('', () => { r.menu.hidden = true; act({op:'form', row, form:item.dataset.form}); r.format.focus(); }, 'color-format-item');
          item.setAttribute('role','menuitemradio'); r.menu.append(item);
        }
        shown.forms.forEach((choice, i) => {
          const item = r.menu.children[i];
          item.dataset.form = choice.form; if (item.textContent !== choice.label) item.textContent = choice.label; item.setAttribute('aria-checked', String(choice.form === shown.form));
        });
        shown.values.forEach((value, index) => setField(r.values[index], value.text, value.edit, value.name));
      });
      intensityName.hidden = intensity.cell.hidden = !view.intensity;
      if (view.intensity) setField(intensity, view.intensity.text, view.intensity.edit, view.intensity.name);
      status.textContent = error ?? '';
      for (const entry of fields) entry.input.toggleAttribute('aria-invalid', !!error && same(errorTarget, entry.target));
      apply.disabled = fields.some(entry => !entry.input.hidden && entry.input.hasAttribute('aria-invalid'));
      if (editingAny) editingAny.cell.classList.add('editing');
      queueDraw();
    }
    const begin = entry => {
      entry.input.value = entry.edit; entry.input.hidden = false; entry.show.hidden = true; entry.cell.classList.add('editing');
      entry.input.focus(); entry.input.select();
    };
    const end = entry => {
      entry.input.hidden = true; entry.show.hidden = false; entry.cell.classList.remove('editing'); entry.input.removeAttribute('aria-invalid');
      if (same(errorTarget, entry.target)) { error = null; errorTarget = null; refused = null; }
      render(); entry.show.focus();
    };
    const commit = entry => {
      if (entry.input.hidden) return;
      const text = entry.input.value;
      if (text === entry.edit) return end(entry);
      const action = entry.target === 'hex' ? {op:'text', text} : entry.target.kind === 'intensity' ? {op:'intensity', text} : {op:'value', row:entry.target.row, index:entry.target.index, text};
      if (act(action, entry.target)) end(entry);
      else { entry.input.focus(); }
    };
    for (const entry of fields) {
      entry.show.addEventListener('click', () => { if (entry.suppress) { entry.suppress = false; return; } begin(entry); });
      entry.input.addEventListener('keydown', e => {
        if (e.isComposing) return;
        if (e.key === 'Enter') { e.preventDefault(); commit(entry); }
        else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); end(entry); }
      });
      entry.input.addEventListener('blur', () => { if (!picking && root.open) commit(entry); });
      if (entry.target === 'hex') continue;
      entry.show.addEventListener('keydown', e => {
        if (!['ArrowUp','ArrowDown'].includes(e.key)) return;
        e.preventDefault(); act({op:'scrub', target:entry.target, pixels:e.key === 'ArrowUp' ? 2 : -2, speed:speed(e)}, entry.target); act({op:'end_scrub', cancel:false}, entry.target);
      });
      entry.show.addEventListener('pointerdown', e => { if (e.button !== 0) return; scrub = {entry, id:e.pointerId, y:e.clientY, active:false}; entry.show.setPointerCapture(e.pointerId); });
      entry.show.addEventListener('pointermove', e => {
        if (scrub?.id !== e.pointerId || scrub.entry !== entry) return;
        const pixels = scrub.y - e.clientY;
        if (!scrub.active && Math.abs(pixels) < 4) return;
        scrub.active = true; e.preventDefault(); act({op:'scrub', target:entry.target, pixels, speed:speed(e)}, entry.target);
      });
      for (const name of ['pointerup','pointercancel']) entry.show.addEventListener(name, e => {
        if (scrub?.id !== e.pointerId || scrub.entry !== entry) return;
        if (scrub.active) { entry.suppress = name === 'pointerup'; act({op:'end_scrub', cancel:name === 'pointercancel'}, entry.target); }
        scrub = null;
      });
    }
    function toggleMenu(row) {
      const menu = rows[row].menu, open = menu.hidden;
      rows.forEach(r => { r.menu.hidden = true; r.format.setAttribute('aria-expanded','false'); });
      menu.hidden = !open; rows[row].format.setAttribute('aria-expanded', String(open));
      if (open) ([...menu.children].find(item => item.getAttribute('aria-checked') === 'true') ?? menu.firstChild)?.focus();
    }
    root.addEventListener('keydown', e => {
      if (e.key === 'Escape' && rows.some(r => !r.menu.hidden)) { e.preventDefault(); e.stopPropagation(); rows.forEach((r, i) => { if (!r.menu.hidden) { r.menu.hidden = true; r.format.focus(); } }); }
      else if (e.key === 'Escape' && !sheet.hidden) { e.preventDefault(); e.stopPropagation(); if (document.activeElement === search && search.value) { search.value = ''; searchChanged(); } else showSheet(false); }
    });
    root.addEventListener('pointerdown', e => { if (!e.target.closest('.color-row-name')) rows.forEach(r => { r.menu.hidden = true; }); });
    const editable = target => target?.matches?.('input');
    root.addEventListener('paste', e => {
      if (editable(e.target)) return;
      const text = e.clipboardData?.getData('text/plain'); if (!text) return;
      e.preventDefault(); act({op:'text', text});
    });
    root.addEventListener('copy', e => {
      if (editable(e.target) || String(document.getSelection() ?? '')) return;
      e.preventDefault(); e.clipboardData.setData('text/plain', view.hex);
    });
    async function copyText(node, text) {
      try { await navigator.clipboard.writeText(text); } catch (failure) { console.error('Copy color', failure); return; }
      node.replaceChildren(icon('check')); node.title = copy.copied; node.classList.add('copied');
      setTimeout(() => { node.replaceChildren(icon('copy')); node.title = copy.copy; node.classList.remove('copied'); }, 1200);
    }
    const tile = (entry, name) => {
      const node = button('', () => act({op:'color', color:entry.color}), 'palette-tile'), paint = element('span', 'palette-paint'); node.dataset.colorTile = name;
      paint.style.background = rgba(entry.rgba); node.append(paint); node.title = entry.detail; node.setAttribute('aria-label', entry.detail);
      node.classList.toggle('selected', !!entry.current); return node;
    };
    const fillRecent = () => {
      const sections = app.swatch_sheet('', view.value).sections;
      recent.replaceChildren(...(sections.find(section => section.palette == null)?.tiles ?? []).map(entry => tile(entry, 'recent')));
    };
    function fillSheet() {
      const shown = app.swatch_sheet(editor.picker.editor.search, view.value);
      sheetBody.replaceChildren(...shown.sections.map(section => {
        const block = element('section', 'color-sheet-section'), heading = element('h3'), grid = element('div', 'color-sheet-tiles');
        heading.append(element('span', '', section.title), element('span', 'color-sheet-count', section.count));
        grid.append(...section.tiles.map(entry => tile(entry, 'sheet')));
        if (section.can_add) {
          const add = button('', () => { dispatch({type:'color', action:{op:'library', action:{op:'store', palette:section.palette, name:'', color:view.value}}}); fillSheet(); fillRecent(); }, 'palette-add');
          add.dataset.palette = section.palette; add.append(icon('plus')); add.title = catalog().palettes.add_current; add.setAttribute('aria-label', add.title); grid.append(add);
        }
        block.append(heading, grid); return block;
      }), ...(shown.empty ? [element('p', 'color-sheet-empty', shown.empty)] : []));
    }
    const searchChanged = () => { request({op:'search', text:search.value}); fillSheet(); };
    search.addEventListener('input', searchChanged);
    function showSheet(open) {
      sheetOpen = open; body.inert = open; recent.hidden = open; mini.hidden = !open; more.classList.toggle('open', open);
      more.title = more.ariaLabel = open ? copy.close_swatches : copy.all_swatches;
      if (open) { search.value = editor.picker.editor.search; fillSheet(); sheet.hidden = false; sheetBody.scrollTop = 0; sheet.getBoundingClientRect(); search.focus(); }
      sheet.classList.toggle('open', open);
      if (!open) { setTimeout(() => { if (!sheetOpen) sheet.hidden = true; }, 260); more.focus(); }
    }
    showSheet(false); more.blur();
    function startPick() {
      picking = true; root.close();
      dispatch({type:'color_picker', action:{kind:'editor', original:view.value, touch_offset:44 * (devicePixelRatio || 1)}});
      refresh();
    }
    let stripCorner = 'top_right', stripHover = null;
    const canvasRect = () => document.getElementById('canvas')?.getBoundingClientRect();
    const placeStrip = () => {
      const surface = canvasRect(), camera = app.camera(), sample = app.color_preview().picker.sample_point;
      if (!surface?.width || !camera?.viewport?.[0]) return;
      const scale = Number(camera.viewport[0]) / surface.width, avoid = [sample, stripHover && [(stripHover[0] - surface.x) * scale, (stripHover[1] - surface.y) * scale]].filter(Boolean);
      const placed = app.color_ui({type:'strip_placement', area:camera.work_area.map(Number), size:[strip.offsetWidth * scale, strip.offsetHeight * scale], scale, avoid, corner:stripCorner});
      stripCorner = placed.corner; strip.style.left = `${surface.x + placed.origin[0] / scale}px`; strip.style.top = `${surface.y + placed.origin[1] / scale}px`;
    };
    strip.addEventListener('pointermove', e => { if (e.pointerType === 'mouse' || e.pointerType === 'pen') { stripHover = [e.clientX, e.clientY]; placeStrip(); } });
    strip.addEventListener('pointerleave', () => { stripHover = null; });
    function refresh() {
      if (!picking) return;
      const picker = app.color_preview().picker;
      if (picker.editor) {
        const sample = picker.preview ?? view.value, shown = app.color_ui({type:'editor_strip', editor, sample});
        const [original, sampled] = app.color_ui({type:'preview', colors:[view.value, sample], document_space:view.panel.rgb_space, display_space:'Srgb', rendition});
        stripOriginal.style.background = rgba(original.rgba); stripSample.style.background = rgba(sampled.rgba);
        stripHex.textContent = shown.hex; stripEv.textContent = shown.intensity ?? ''; stripSpace.textContent = shown.label; stripValues.textContent = shown.values.join('  ');
        strip.hidden = false; placeStrip();
        return;
      }
      picking = false; strip.hidden = true;
      if (picker.picked) act({op:'color', color:picker.picked});
      root.showModal(); pick.focus();
    }
    editors.add(refresh);
    Object.defineProperty(root, 'language', {set() { if (closed) return; request(null); if (refused) error = request(refused); render(); if (!sheet.hidden) fillSheet(); }});
    bindCopy(root, () => app.language_tag(), 'language');
    const resize = new ResizeObserver(() => { layoutKey = ''; paintKey = ''; queueDraw(); }); resize.observe(wheelBox);
    root.addEventListener('close', () => {
      if (picking) return;
      closed = true; editors.delete(refresh); resize.disconnect(); painter.dispose(); cancelAnimationFrame(frame); strip.remove(); root.remove();
      if (json(editor.picker.editor) !== memory) dispatch({type:'color', action:{op:'editor_memory', memory:editor.picker.editor}});
      resolve(result);
    });
    fillRecent(); render(); root.showModal(); draw(); current.blur(); title.tabIndex = -1; title.focus();
  });
}
