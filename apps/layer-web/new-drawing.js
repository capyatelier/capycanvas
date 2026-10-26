import {createWorkflowStack} from './workflow-stack.js';
import {colorForm} from './color-editor.js';
import {paperThumbnail} from './paper-picker.js';
import {sizeFields} from './size-dialog.js';
import {addScrollbar} from './scrollbars.js';
import {composingKey} from './text-input.js';
import {rgba,json} from './color-wheel.js';

const PREVIEW = [[232, 250], [196, 214]], NARROW_PREVIEW = [[348, 140], [312, 104]];
const PRESET_ART = [[116, 86], [74, 74]], SUMMARY_ART = [[36, 36], [34, 34]];
const NARROW = '(max-width: 744px)';
const grains = new Map(), painted = new Map();

function grain(app, id) {
  if (id == null) return null;
  if (grains.has(id)) return grains.get(id);
  const gray = app.paper_preview(id);
  if (!gray) return null;
  const side = Math.round(Math.sqrt(gray.length)), pixels = new Uint8ClampedArray(gray.length * 4);
  for (let i = 0; i < gray.length; i++) {
    const shade = gray[i] / 255 - .5;
    pixels.fill(shade < 0 ? 0 : 255, i * 4, i * 4 + 3);
    pixels[i * 4 + 3] = Math.round((shade < 0 ? -shade * .7 : shade * .25) * 255);
  }
  const canvas = document.createElement('canvas'); canvas.width = canvas.height = side;
  canvas.getContext('2d').putImageData(new ImageData(pixels, side, side), 0, 0);
  const url = `url(${canvas.toDataURL()})`;
  grains.set(id, url);
  return url;
}

function paint(app, color) {
  const key = json(color);
  if (!painted.has(key)) painted.set(key, rgba(app.color_ui({type:'preview', colors:[color]})[0].rgba));
  return painted.get(key);
}

// A sheet drawn at its proportions in its paper color, grain and transparency.
function sheetArt(app, element, bounds, mini) {
  const node = element('span', 'new-drawing-art'), sheet = element('span', mini ? 'new-drawing-sheet mini' : 'new-drawing-sheet'), texture = element('span', 'new-drawing-grain');
  sheet.append(texture); node.append(sheet);
  let size = bounds, shown = null;
  const show = view => {
    shown = view;
    const [[width, height], [fitWidth, fitHeight]] = size, [w, h] = view.extent;
    const scale = Math.min(Math.min(fitWidth, fitHeight) / Math.max(w, h), fitWidth / w, fitHeight / h);
    Object.assign(node.style, {width:`${width}px`, height:`${height}px`});
    Object.assign(sheet.style, {width:`${Math.max(4, Math.round(w * scale))}px`, height:`${Math.max(4, Math.round(h * scale))}px`, background:view.shown ? paint(app, view.color) : ''});
    sheet.classList.toggle('new-drawing-clear', !view.shown);
    Object.assign(texture.style, {backgroundImage:grain(app, view.texture) ?? 'none', backgroundSize:`${mini ? 14 : 40}px`, opacity:view.shown ? 1 : .35});
  };
  return {node, show, resize(next) { size = next; if (shown) show(shown); }};
}

const text = (node, value) => { if (node.textContent !== value) node.textContent = value; };
const named = (node, value) => { node.title = value; node.setAttribute('aria-label', value); };

export function screenInfo() {
  const extent = [screen.width, screen.height].map(side => Math.round(side * (devicePixelRatio || 1)));
  return extent.every(side => side > 0) ? {extent, ppi:null} : null;
}

// Presents the session's New drawing dialog: every input is a NewDrawingAction,
// and every label, choice and state comes from `state().new_drawing`.
export function createNewDrawing({app, state, applyChange, element, button, icon, numberField, importPaper}) {
  let ui = null, finish = null, creating = false;
  const view = () => state().new_drawing ?? null;
  function send(action) {
    try { applyChange(app.dispatch({type:'new_drawing', action})); ui?.report(''); return true; }
    catch (error) { ui?.report(String(error)); return false; }
  }
  function refresh() {
    const shown = view();
    if (!shown) {
      ui?.close(); ui = null;
      const done = finish; finish = null;
      done?.(creating); creating = false;
      return;
    }
    if (!finish) return;
    ui ??= build();
    ui.render(shown);
  }
  function create() {
    if (!ui.commit()) return;
    creating = true;
    if (!send({op:'create'})) creating = false;
  }

  function build() {
    const narrow = matchMedia(NARROW);
    const root = element('dialog', 'picker-dialog new-drawing-dialog'); root.id = 'new-drawing-dialog';
    const title = element('h2', 'dialog-title'); title.id = 'new-drawing-title'; title.tabIndex = -1; root.setAttribute('aria-labelledby', title.id);
    const menu = element('div', 'panel-context-menu new-drawing-menu'); menu.popover = 'auto'; menu.setAttribute('role', 'menu');
    function popup(items, x, y) {
      menu.replaceChildren(...items.map(item => {
        const node = button('', () => { menu.hidePopover(); send(item.action); }, item.selected ? 'selected' : '');
        node.setAttribute('role', 'menuitem'); node.disabled = item.disabled ?? false;
        node.append(element('span', '', item.label));
        if (item.detail) node.append(element('span', 'new-drawing-dim', item.detail));
        return node;
      }));
      menu.showPopover();
      const box = menu.getBoundingClientRect();
      menu.style.left = `${Math.max(6, Math.min(x, innerWidth - box.width - 6))}px`;
      menu.style.top = `${Math.max(6, Math.min(y, innerHeight - box.height - 6))}px`;
      (menu.querySelector('button.selected:not(:disabled)') ?? menu.querySelector('button:not(:disabled)'))?.focus();
    }
    const contextMenu = (node, items) => node.addEventListener('contextmenu', e => {
      e.preventDefault(); e.stopPropagation();
      const r = node.getBoundingClientRect();
      popup(items(), e.clientX || r.left, e.clientY || r.bottom);
    });
    const choice = (id, action) => {
      const node = element('select', 'new-drawing-select'); node.id = id;
      node.addEventListener('change', () => { const next = action(Number(node.value)); if (next) send(next); });
      return node;
    };
    const fill = (node, choices, index) => {
      const key = json(choices.map(entry => entry.label));
      if (node.shownChoices !== key) {
        node.shownChoices = key;
        node.replaceChildren(...choices.map((entry, i) => { const option = element('option', '', entry.label); option.value = String(i); return option; }));
      }
      node.value = String(Math.max(0, index));
    };
    const keyGrid = (grid, tiles, pick, use) => grid.addEventListener('keydown', e => {
      const index = tiles().indexOf(document.activeElement);
      if (index < 0) return;
      if (e.key === 'Enter') { e.preventDefault(); use(); return; }
      const columns = tiles().filter(node => node.offsetTop === tiles()[0].offsetTop).length;
      const next = {ArrowLeft:index - 1, ArrowRight:index + 1, ArrowUp:index - columns, ArrowDown:index + columns, Home:0, End:tiles().length - 1}[e.key];
      if (next == null) return;
      e.preventDefault();
      if (!tiles()[next]) return;
      pick(next); tiles()[next].focus();
    });
    const roving = (tiles, selected) => {
      const index = Math.max(0, tiles.findIndex((_, i) => selected(i)));
      tiles.forEach((node, i) => { node.setAttribute('aria-checked', String(selected(i))); node.tabIndex = i === index ? 0 : -1; });
    };
    let shown = null;

    const art = sheetArt(app, element, narrow.matches ? NARROW_PREVIEW : PREVIEW, false);
    const preview = button('', () => send({op:'presets'}), 'new-drawing-preview'); preview.id = 'new-drawing-preview'; preview.append(art.node);
    const preset = element('span', 'new-drawing-preset'), orientationName = element('span', 'new-drawing-dim');
    const caption = element('span', 'new-drawing-caption-line'); caption.id = 'new-drawing-caption'; caption.append(preset, orientationName);
    const capacity = element('span', 'new-drawing-small'); capacity.id = 'new-drawing-capacity';
    const captions = element('div', 'new-drawing-captions'); captions.append(caption, capacity);
    const left = element('div', 'new-drawing-left'); left.append(preview, captions);

    const nameLabel = element('span'), name = element('input', 'new-drawing-entry');
    name.id = 'new-drawing-name'; name.type = 'text'; name.autocomplete = 'off'; name.spellcheck = false;
    name.addEventListener('input', () => send({op:'name', text:name.value}));
    name.addEventListener('keydown', e => { if (e.key === 'Enter' && !composingKey(e)) { e.preventDefault(); create(); } });
    const nameRow = element('label', 'new-drawing-row new-drawing-name-row'); nameRow.append(nameLabel, name);

    const numbers = sizeFields({element, numberField, resolve:request => app.number_input(request), key:'newDrawing', names:['width', 'height', 'resolution'], send, inline:true});
    const field = (id, ...content) => { const node = element('div', 'new-drawing-field'); node.id = id; node.append(...content); return node; };
    const widthPrefix = element('span', 'new-drawing-affix'), heightPrefix = element('span', 'new-drawing-affix');
    const link = button('', () => send({op:'linked', linked:!shown.linked}), 'new-drawing-link'); link.id = 'new-drawing-link';
    const linked = element('div', 'new-drawing-linked');
    linked.append(field('new-drawing-width', widthPrefix, numbers.nodes[0]), link, field('new-drawing-height', heightPrefix, numbers.nodes[1]));
    const unit = choice('new-drawing-unit', index => shown.units[index] && {op:'unit', unit:shown.units[index].value});
    const turn = orientation => { const node = button('', () => send({op:'orientation', orientation})); node.id = `new-drawing-${orientation}`; node.append(icon(orientation)); return node; };
    const portrait = turn('portrait'), landscape = turn('landscape');
    const orientation = element('div', 'new-drawing-well new-drawing-icons'); orientation.id = 'new-drawing-orientation'; orientation.setAttribute('role', 'group'); orientation.append(portrait, landscape);
    const dimensions = element('div', 'new-drawing-row'); dimensions.append(linked, unit, element('span', 'dialog-gap'), orientation);
    const resolutionLabel = element('span', 'new-drawing-label'), resolutionUnit = element('span', 'new-drawing-affix');
    const resolutionField = field('new-drawing-resolution-field', numbers.nodes[2], resolutionUnit); resolutionField.classList.add('new-drawing-combo');
    const resolutions = button('', () => {
      const r = resolutionField.getBoundingClientRect();
      popup(shown.resolutions.map(entry => ({label:entry.label, detail:entry.purpose, disabled:!entry.enabled, selected:entry.selected, action:{op:'resolution', value:entry.value}})), r.left, r.bottom + 4);
    }, 'new-drawing-chevron');
    resolutions.id = 'new-drawing-resolution-choices'; resolutions.setAttribute('aria-haspopup', 'menu'); resolutions.append(icon('chevron-down')); resolutionField.append(resolutions);
    const size = element('span', 'new-drawing-small new-drawing-dim new-drawing-size'); size.id = 'new-drawing-size';
    const resolutionRow = element('div', 'new-drawing-row'); resolutionRow.append(resolutionLabel, resolutionField, size);
    const sizeRows = element('div', 'new-drawing-rows'); sizeRows.append(dimensions, resolutionRow);
    const heading = () => element('h3', 'new-drawing-heading');
    const section = (head, content) => { const node = element('section', 'new-drawing-section'); node.append(head, content); return node; };
    const sizeHeading = heading(), colorHeading = heading(), paperHeading = heading();

    const captioned = (id, action) => {
      const label = element('span', 'new-drawing-caption'), select = choice(id, action), cell = element('label', 'new-drawing-cell');
      cell.append(label, select); return {label, select, cell};
    };
    const space = captioned('new-drawing-space', index => shown.spaces[index] && {op:'space', space:shown.spaces[index].value});
    const depth = captioned('new-drawing-depth', index => shown.depths[index] && {op:'depth', depth:shown.depths[index].value});
    const blending = captioned('new-drawing-blending', index => shown.blendings[index] && {op:'blending', blending:shown.blendings[index].value});
    const colors = element('div', 'new-drawing-colors'); colors.append(space.cell, depth.cell, blending.cell);

    const thumbnail = paperThumbnail({app, element, icon, size:36});
    const textureName = element('span', 'new-drawing-tile-label'), paperColorName = element('span', 'new-drawing-tile-label'), patch = element('span', 'new-drawing-patch');
    const textureRelief = element('span', 'new-drawing-caption new-drawing-dim'); textureRelief.id = 'new-drawing-texture-relief';
    const textureText = element('span', 'new-drawing-tile-text'); textureText.append(textureName, textureRelief);
    const texture = button('', () => send({op:'paper'}), 'new-drawing-paper-tile'); texture.id = 'new-drawing-texture'; texture.append(thumbnail.node, textureText);
    const paperColor = button('', () => send({op:'paper_color'}), 'new-drawing-paper-tile'); paperColor.id = 'new-drawing-paper-color'; paperColor.append(patch, paperColorName);
    const papers = element('div', 'new-drawing-papers'); papers.append(texture, paperColor);

    const form = element('div', 'new-drawing-form');
    form.append(nameRow, section(sizeHeading, sizeRows), section(colorHeading, colors), section(paperHeading, papers));
    const body = element('div', 'new-drawing-body'); body.append(left, form);
    const page = element('div', 'dialog-page new-drawing-page'); page.append(title, body);
    const presetsLabel = element('span'), presetsIcon = icon('chevron-down');
    presetsIcon.classList.add('new-drawing-up');
    const presets = button('', () => send({op:'presets'}), 'dialog-button new-drawing-open-presets'); presets.id = 'new-drawing-presets-button'; presets.append(presetsLabel, presetsIcon);
    const cancel = button('', () => send({op:'cancel'}), 'dialog-button'); cancel.id = 'new-drawing-cancel';
    const createButton = button('', create, 'suggested-action'); createButton.id = 'new-drawing-create';
    const footer = element('footer', 'dialog-footer'); footer.append(presets, element('span', 'dialog-gap'), cancel, createButton);
    const scrolled = scroller => { const frame = element('div', 'new-drawing-scroll'); frame.append(scroller); addScrollbar(scroller, frame); return frame; };
    const main = element('div', 'new-drawing-main'); main.append(scrolled(page), footer);

    const drawer = (id, head, content, bottom) => {
      const node = element('div', 'new-drawing-drawer'); node.id = id; node.hidden = true;
      const scroll = element('div', 'new-drawing-drawer-body'); scroll.append(...content);
      node.append(...(head ? [head] : []), scrolled(scroll), bottom); return node;
    };
    const search = element('input'); search.id = 'new-drawing-search'; search.type = 'search'; search.autocomplete = 'off'; search.spellcheck = false;
    search.addEventListener('input', () => send({op:'search', query:search.value}));
    search.addEventListener('keydown', e => {
      if (e.key !== 'Escape' || composingKey(e)) return;
      e.preventDefault(); e.stopPropagation(); send({op:'back'});
    });
    const searchBox = element('label', 'new-drawing-search'); searchBox.append(icon('search'), search);
    const searchHead = element('div', 'new-drawing-drawer-head'); searchHead.append(searchBox);
    const tabs = element('div', 'new-drawing-tabs'); tabs.id = 'new-drawing-tabs'; tabs.setAttribute('role', 'tablist');
    const presetGrid = element('div', 'new-drawing-preset-grid'); presetGrid.id = 'new-drawing-presets'; presetGrid.setAttribute('role', 'radiogroup');
    const empty = element('p', 'new-drawing-small new-drawing-dim new-drawing-empty'); empty.id = 'new-drawing-empty';
    const summaryArt = sheetArt(app, element, SUMMARY_ART, true);
    const summaryName = element('span', 'new-drawing-summary-name'), summarySize = element('span', 'new-drawing-small new-drawing-dim'), summaryDetails = element('span', 'new-drawing-small new-drawing-dim');
    summaryName.id = 'new-drawing-summary-name'; summarySize.id = 'new-drawing-summary-size'; summaryDetails.id = 'new-drawing-summary-details';
    const summaryTop = element('span', 'new-drawing-summary-top'); summaryTop.append(summaryName, summarySize);
    const summaryText = element('span', 'new-drawing-summary-text'); summaryText.append(summaryTop, summaryDetails);
    const summary = element('div', 'new-drawing-summary'); summary.append(summaryArt.node, summaryText);
    const browserCancel = button('', () => send({op:'close_drawer'}), 'dialog-button'); browserCancel.id = 'new-drawing-browser-cancel';
    const saveAs = button('', () => send({op:'save_as'}), 'dialog-button'); saveAs.id = 'new-drawing-save-as';
    const select = button('', () => send({op:'use_preset'}), 'suggested-action'); select.id = 'new-drawing-select';
    const actions = element('div', 'new-drawing-actions'); actions.append(browserCancel, saveAs, select);
    const saveName = element('input', 'new-drawing-entry'); saveName.id = 'new-drawing-save-name'; saveName.type = 'text'; saveName.autocomplete = 'off'; saveName.spellcheck = false;
    saveName.addEventListener('input', () => send({op:'preset_name', text:saveName.value}));
    saveName.addEventListener('keydown', e => { if (e.key === 'Enter' && !composingKey(e)) { e.preventDefault(); if (!saveConfirm.disabled) send({op:'save_preset'}); } });
    const saveGroup = choice('new-drawing-save-group', index => shown.drawer?.save?.groups[index] && {op:'preset_group', group:shown.drawer.save.groups[index].value});
    const saveCancel = button('', () => send({op:'cancel_save'}), 'dialog-button'); saveCancel.id = 'new-drawing-save-cancel';
    const saveConfirm = button('', () => send({op:'save_preset'}), 'suggested-action'); saveConfirm.id = 'new-drawing-save-confirm';
    const saving = element('div', 'new-drawing-actions new-drawing-saving'); saving.append(saveName, saveGroup, saveCancel, saveConfirm);
    const actionStack=element('div');actionStack.append(actions,saving);const actionPages=createWorkflowStack(actionStack);
    const browserFooter = element('footer', 'dialog-footer'); browserFooter.append(summary, actionStack);
    const browser = drawer('new-drawing-browser', searchHead, [tabs, presetGrid, empty], browserFooter);
    let presetTiles = [], presetKey = '', tabKey = '', naming = false;
    keyGrid(presetGrid, () => presetTiles, index => send({op:'pick_preset', index}), () => send({op:'use_preset'}));

    const paperTitle = element('h2', 'dialog-title');
    const paperHead = element('div', 'new-drawing-drawer-head'); paperHead.append(paperTitle);
    const paperGrid = element('div', 'texture-grid'); paperGrid.id = 'new-drawing-papers'; paperGrid.setAttribute('role', 'radiogroup');
    let importing = false;
    async function importTexture() {
      if (importing) return;
      importing = true;
      try { await importPaper(); }
      catch (error) { ui?.report(String(error)); }
      finally { importing = false; }
    }
    const importPreview = element('span', 'paper-preview'), importCaption = element('span', 'texture-caption'), importFrame = element('span', 'texture-frame');
    importPreview.setAttribute('aria-hidden', 'true'); importPreview.append(icon('plus')); importFrame.append(importPreview);
    const importTile = button('', importTexture, 'texture-tile'); importTile.id = 'new-drawing-paper-import'; importTile.tabIndex = -1; importTile.append(importFrame, importCaption);
    const relief = sizeFields({element, numberField, resolve:request => app.number_input(request), key:'newDrawing', names:['paper_relief'], send, inline:true});
    const reliefLabel = element('span', 'new-drawing-relief-label'), reliefUnit = element('span', 'new-drawing-affix');
    const reliefField = field('new-drawing-paper-relief-field', relief.nodes[0], reliefUnit); reliefField.classList.add('new-drawing-combo');
    const reliefRow = element('div', 'new-drawing-relief'); reliefRow.id = 'new-drawing-paper-relief'; reliefRow.append(reliefLabel, reliefField);
    const usePaperTexture = () => { if (relief.commit()) send({op:'use_paper'}); };
    const paperCancel = button('', () => send({op:'close_drawer'}), 'dialog-button'); paperCancel.id = 'new-drawing-paper-cancel';
    const usePaper = button('', usePaperTexture, 'suggested-action'); usePaper.id = 'new-drawing-paper-use';
    const paperFooter = element('footer', 'dialog-footer'); paperFooter.append(reliefRow, element('span', 'dialog-gap'), paperCancel, usePaper);
    const paperDrawer = drawer('new-drawing-paper-drawer', paperHead, [paperGrid], paperFooter);
    let paperTiles = [], paperKey = '';
    keyGrid(paperGrid, () => [...paperTiles, importTile], index => { if (paperTiles[index]) send({op:'pick_paper', id:shown.drawer.tiles[index].id}); },
      () => { if (document.activeElement === importTile) importTexture(); else usePaperTexture(); });

    const colorPage = element('div', 'new-drawing-color-page');
    const mode = (transparent, glyph) => {
      const node = button('', () => send({op:'transparent', transparent})), label = element('span');
      node.id = `new-drawing-color-mode-${transparent ? 'transparent' : 'color'}`; node.append(icon(glyph), label);
      return {node, label};
    };
    const solid = mode(false, 'solid'), clear = mode(true, 'transparent');
    const modes = element('div', 'new-drawing-well new-drawing-mode'); modes.id = 'new-drawing-color-mode'; modes.setAttribute('role', 'group'); modes.append(solid.node, clear.node);
    const paletteTitle = element('span'), paletteCount = element('span', 'new-drawing-dim');
    const paletteHead = element('div', 'new-drawing-palette-head'); paletteHead.append(paletteTitle, paletteCount);
    const paletteTiles = element('div', 'new-drawing-palette-tiles'); paletteTiles.id = 'new-drawing-paper-color-tiles';
    const palette = element('div', 'new-drawing-palette'); palette.id = 'new-drawing-paper-colors'; palette.append(paletteHead, paletteTiles);
    const add = button('', () => send({op:'add_paper_color'}), 'palette-add'); add.id = 'new-drawing-add-paper-color'; add.append(icon('plus'));
    const colorCancel = button('', () => send({op:'close_drawer'}), 'dialog-button'); colorCancel.id = 'new-drawing-color-cancel';
    const useColor = button('', () => { if (!useColor.disabled) send({op:'use_paper_color'}); }, 'suggested-action'); useColor.id = 'new-drawing-color-use';
    const colorFooter = element('footer', 'dialog-footer'); colorFooter.append(element('span', 'dialog-gap'), colorCancel, useColor);
    const colorDrawer = drawer('new-drawing-color-drawer', null, [colorPage], colorFooter);
    let colorUi = null, paletteKey = '';

    const report = element('p', 'new-drawing-error'); report.setAttribute('role', 'status'); report.hidden = true;
    const content = element('div', 'dialog-content'); content.append(main, browser, paperDrawer, colorDrawer, report);
    root.append(content, menu); document.body.append(root);
    const drawers = {presets:browser, paper:paperDrawer, paper_color:colorDrawer};
    const arrange = () => { art.resize(narrow.matches ? NARROW_PREVIEW : PREVIEW); };
    narrow.addEventListener('change', arrange);
    root.addEventListener('cancel', e => { e.preventDefault(); if (!colorUi?.form.closeMenus()) send({op:'back'}); });
    root.addEventListener('close', () => { if (ui?.root === root) send({op:'cancel'}); });
    root.addEventListener('keydown', e => { if (e.key === 'Escape' && colorUi?.form.closeMenus()) { e.preventDefault(); e.stopPropagation(); } });

    function slide(node, open) {
      if (open === node.classList.contains('open')) return;
      if (open) { node.hidden = false; node.getBoundingClientRect(); }
      node.classList.toggle('open', open);
      if (!open) setTimeout(() => { if (!node.classList.contains('open')) node.hidden = true; }, 260);
    }
    function renderPage(v) {
      const c = v.copy;
      text(title, c.title); text(nameLabel, c.name); named(name, c.name); name.placeholder = v.untitled;
      if (name.value !== v.name) name.value = v.name;
      text(sizeHeading, c.size); text(widthPrefix, c.width); text(heightPrefix, c.height);
      numbers.show(0, v.numeric, c.width_name, v.values[0], () => view().values[0]);
      numbers.show(1, v.numeric, c.height_name, v.values[1], () => view().values[1]);
      numbers.show(2, v.resolution_numeric, c.resolution, v.setup.resolution, () => view().setup.resolution);
      named(link, c.link); link.setAttribute('aria-pressed', String(v.linked));
      if (link.dataset.linked !== String(v.linked)) { link.dataset.linked = String(v.linked); link.replaceChildren(icon(v.linked ? 'link' : 'unlink')); }
      fill(unit, v.units, v.units.findIndex(entry => entry.value === v.setup.unit)); unit.setAttribute('aria-label', c.unit);
      for (const [node, label, value] of [[portrait, c.portrait, 'portrait'], [landscape, c.landscape, 'landscape']]) {
        named(node, label); const on = v.page.orientation === value; node.setAttribute('aria-pressed', String(on)); node.classList.toggle('selected', on);
      }
      text(resolutionLabel, c.resolution); text(resolutionUnit, c.resolution_unit); named(resolutions, c.common_resolutions);
      text(size, v.page.size); size.classList.toggle('error', v.page.problem);
      text(colorHeading, c.color);
      for (const [entry, label, choices, current] of [[space, c.space, v.spaces, v.setup.color.space], [depth, c.depth, v.depths, v.setup.color.depth], [blending, c.blending, v.blendings, v.setup.blend_space]]) {
        text(entry.label, label); fill(entry.select, choices, choices.findIndex(item => item.value === current));
      }
      blending.select.disabled = v.blending_note != null; blending.cell.title = v.blending_note ?? '';
      text(paperHeading, c.paper); texture.title = c.paper; paperColor.title = c.paper_color;
      if (v.sheet.texture == null) thumbnail.none(); else thumbnail.show(v.sheet.texture);
      text(textureName, v.page.texture); text(textureRelief, v.page.relief ?? ''); textureRelief.hidden = v.page.relief == null;
      text(paperColorName, v.page.paper_color);
      patch.style.background = v.sheet.shown ? paint(app, v.sheet.color) : ''; patch.classList.toggle('new-drawing-clear', !v.sheet.shown);
      paperColor.disabled = !v.paper_color_enabled;
      art.show(v.sheet); text(preset, v.page.preset); text(orientationName, ` · ${v.page.orientation_label}`);
      text(capacity, v.page.capacity); capacity.title = v.page.memory;
      named(preview, c.presets); text(presetsLabel, c.presets); text(cancel, c.cancel); text(createButton, c.create);
      createButton.disabled = !v.can_create;
    }
    function renderBrowser(v, opening) {
      const b = v.drawer, c = v.copy;
      if (search.value !== b.query) search.value = b.query;
      search.placeholder = c.search; search.setAttribute('aria-label', c.search);
      const tabsShown = json(b.tabs.map(tab => tab.label));
      if (tabsShown !== tabKey) {
        tabKey = tabsShown;
        tabs.replaceChildren(...b.tabs.map((tab, index) => { const node = button(tab.label, () => send({op:'tab', index}), 'new-drawing-pill'); node.setAttribute('role', 'tab'); node.dataset.newDrawingTab = index; return node; }));
      }
      [...tabs.children].forEach((node, index) => { const on = b.tabs[index].selected; node.setAttribute('aria-selected', String(on)); node.classList.toggle('selected', on); node.disabled = !b.tabs_enabled; });
      const tilesShown = json(b.tiles.map(tile => ({...tile, selected:false})));
      if (opening || tilesShown !== presetKey) {
        presetKey = tilesShown;
        presetTiles = b.tiles.map((tile, index) => {
          const node = button('', () => send({op:'pick_preset', index}), 'new-drawing-preset'), frame = element('span', 'new-drawing-preset-frame'), sheet = sheetArt(app, element, PRESET_ART, true);
          node.dataset.newDrawingPreset = index; node.setAttribute('role', 'radio'); node.title = `${tile.name}\n${tile.dimensions} · ${tile.details}`;
          sheet.show(tile.sheet); frame.append(sheet.node);
          node.append(frame, element('span', 'new-drawing-tile-name', tile.name), element('span', 'new-drawing-caption new-drawing-dim', tile.dimensions), element('span', 'new-drawing-caption new-drawing-dim', tile.details));
          node.classList.toggle('disabled', !tile.enabled);
          node.addEventListener('dblclick', () => { if (send({op:'pick_preset', index})) send({op:'use_preset'}); });
          if (tile.own) contextMenu(node, () => [{label:view().copy.rename, action:{op:'rename_preset', index}}, {label:view().copy.delete, action:{op:'delete_preset', index}}]);
          return node;
        });
        presetGrid.replaceChildren(...presetTiles);
      }
      roving(presetTiles, index => b.tiles[index].selected);
      presetTiles.forEach((node, index) => node.classList.toggle('selected', b.tiles[index].selected));
      text(empty, b.empty ?? ''); empty.hidden = b.empty == null;
      summaryArt.show(b.summary_sheet); text(summaryName, b.summary.name); text(summarySize, b.summary.size); text(summaryDetails, b.summary.details);
      text(browserCancel, c.cancel); text(saveAs, b.save_label); text(select, c.select); select.disabled = !b.can_select;
      actionPages.show(b.save == null?actions:saving);
      const starting = b.save != null && !naming;
      naming = b.save != null;
      if (!b.save) return;
      const save = b.save;
      if (saveName.value !== save.name) saveName.value = save.name;
      saveName.placeholder = c.preset_name; saveName.setAttribute('aria-label', c.preset_name);
      saveName.classList.toggle('error', !save.can_save); saveName.toggleAttribute('aria-invalid', !save.can_save);
      fill(saveGroup, save.groups, save.groups.findIndex(group => group.value === save.group)); named(saveGroup, c.preset_category);
      text(saveCancel, c.cancel); text(saveConfirm, save.confirm); saveConfirm.disabled = !save.can_save;
      if (starting) { saveName.focus(); saveName.select(); }
    }
    function renderPapers(v, opening) {
      const c = v.copy, {tiles, relief: shownRelief} = v.drawer;
      text(paperTitle, c.paper); text(importCaption, c.import); importTile.title = c.import; text(paperCancel, c.cancel); text(usePaper, c.use_texture);
      text(reliefLabel, c.relief); reliefLabel.title = c.relief; text(reliefUnit, c.relief_unit);
      relief.show(0, shownRelief.numeric, c.relief, shownRelief.value, () => view()?.drawer?.relief?.value);
      if (opening) relief.nodes[0].cancelEditing();
      relief.nodes[0].setDisabled(!shownRelief.enabled); reliefRow.classList.toggle('disabled', !shownRelief.enabled);
      const key = json(tiles.map(tile => [tile.id, tile.label]));
      if (opening || key !== paperKey) {
        paperKey = key;
        paperTiles = tiles.map((tile, index) => {
          const node = button('', () => send({op:'pick_paper', id:tile.id}), 'texture-tile'), frame = element('span', 'texture-frame'), preview = paperThumbnail({app, element, icon, size:96});
          node.dataset.newDrawingPaper = index; node.setAttribute('role', 'radio'); node.title = tile.label;
          if (tile.id == null) preview.none(); else preview.show(tile.id);
          frame.append(preview.node); node.append(frame, element('span', 'texture-caption', tile.label));
          node.addEventListener('dblclick', usePaperTexture);
          return node;
        });
        paperGrid.replaceChildren(...paperTiles, importTile);
      }
      roving(paperTiles, index => tiles[index].selected);
    }
    function openColors(drawer) {
      const holder = element('div', 'new-drawing-color-form');
      try {
        const form = colorForm({app, root:holder, element, button, rendition:app.color_panel().rendition ?? null, copyable:false,
          open:{colors:state().layer_tools.mask_editing?.colors ?? state().colors, color:drawer.color, opaque:true},
          live:() => colorUi?.form === form,
          rendered:({view:edited, blocked}) => {
            useColor.disabled = blocked;
            const value = json(edited.value);
            if (colorUi && value !== colorUi.sent) { colorUi.sent = value; send({op:'paper_color_value', color:edited.value}); }
            dress();
          }});
        colorUi = {form, sent:json(form.view().value)};
        form.title.tabIndex = -1;
        form.title.textContent = view()?.copy.paper_color ?? '';
        form.current.hidden = form.captions.hidden = true;
        form.head.classList.add('new-drawing-color-head'); form.head.append(modes);
        form.values.append(palette);
        holder.append(form.body); colorPage.replaceChildren(holder);
        paletteKey = '';
        form.render();
      } catch (error) { ui?.report(String(error)); }
    }
    function closeColors() {
      if (!colorUi) return;
      colorUi.form.dispose(); colorUi = null;
      setTimeout(() => { if (!colorUi) colorPage.replaceChildren(); }, 260);
    }
    function dress() {
      const d = shown?.drawer;
      if (!colorUi || d?.kind !== 'paper_color') return;
      const {form} = colorUi, edited = form.view();
      for (const node of [form.left, form.values]) node.style.opacity = d.transparent ? .4 : '';
      form.fresh.style.background = d.transparent ? '' : rgba(edited.new.rgba); form.fresh.classList.toggle('new-drawing-clear', d.transparent);
      text(form.hex, d.transparent ? shown.copy.transparent : edited.hex); form.hex.classList.toggle('new-drawing-transparent', d.transparent);
      for (const [entry, on] of [[solid, !d.transparent], [clear, d.transparent]]) { entry.node.setAttribute('aria-pressed', String(on)); entry.node.classList.toggle('selected', on); }
    }
    function renderColors(v) {
      const d = v.drawer, c = v.copy;
      if (!colorUi) return;
      text(colorUi.form.title, c.paper_color);
      text(solid.label, c.color); text(clear.label, c.transparent); modes.setAttribute('aria-label', c.sheet_mode);
      text(paletteTitle, c.paper_colors); text(paletteCount, d.count); named(add, c.add_paper_color); add.disabled = !d.can_add;
      text(colorCancel, c.cancel); text(useColor, c.use_color);
      const key = json(d.palette.map(entry => [entry.color, entry.name]));
      if (key !== paletteKey) {
        paletteKey = key;
        const previews = d.palette.length ? app.color_ui({type:'preview', colors:d.palette.map(entry => entry.color)}) : [];
        paletteTiles.replaceChildren(...d.palette.map((entry, index) => {
          const node = colorUi.form.tile({color:entry.color, rgba:previews[index].rgba, detail:entry.name, current:entry.selected}, 'paper');
          node.dataset.newDrawingPaperColor = index;
          contextMenu(node, () => [{label:view().copy.delete, action:{op:'remove_paper_color', index}}]);
          return node;
        }), add);
      }
      [...paletteTiles.children].forEach((node, index) => { if (d.palette[index]) node.classList.toggle('selected', d.palette[index].selected); });
      dress();
    }
    function render(v) {
      const previous = shown; shown = v;
      renderPage(v);
      const kind = v.drawer?.kind ?? null, was = previous?.drawer?.kind ?? null, opening = kind !== was;
      if (opening && was === 'paper_color') closeColors();
      if (kind === 'presets') renderBrowser(v, opening);
      else if (kind === 'paper') renderPapers(v, opening);
      else if (kind === 'paper_color') { if (opening) openColors(v.drawer); renderColors(v); }
      if (kind !== 'presets') naming = false;
      for (const [name, node] of Object.entries(drawers)) slide(node, name === kind);
      main.inert = kind != null;
      if (!opening) return;
      if (menu.matches(':popover-open')) menu.hidePopover();
      if (kind === 'presets') search.focus();
      else if (kind === 'paper') (paperTiles.find(node => node.tabIndex === 0) ?? paperGrid).focus();
      else if (kind === 'paper_color') colorUi?.form.title.focus();
      else ({presets, paper:texture, paper_color:paperColor})[was]?.focus();
    }
    root.showModal(); title.focus();
    return {root, render,
      commit:() => numbers.commit(),
      localize() { colorUi?.form.relocalize(); },
      report(message) { text(report, message); report.hidden = !message; },
      close() {
        actionPages.finish();closeColors(); narrow.removeEventListener('change', arrange);
        ui = null; if (root.open) root.close(); root.remove();
      }};
  }
  return {
    async run() {
      applyChange(app.dispatch({type:'new_drawing', action:{op:'open', screen:screenInfo(), untitled:app.document_tabs(0).upcoming_id}}));
      return new Promise(resolve => { finish = resolve; refresh(); });
    },
    refresh,
    localize() { ui?.localize(); },
  };
}
