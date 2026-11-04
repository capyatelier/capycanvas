import {createDialogHost} from './dialog-host.js';
import {liveCopy,bindCopy} from './localization.js';
import {addScrollbar} from './scrollbars.js';

const previews = new Map(), dialogs = new Set();
export function refreshTextureDialogs() { for (const refresh of [...dialogs]) refresh(); }

function previewSource(app, id) {
  if (id == null) return null;
  if (previews.has(id)) return previews.get(id);
  const gray = app.paper_preview(id);
  if (!gray) return null;
  const side = Math.round(Math.sqrt(gray.length)), rgba = new Uint8ClampedArray(gray.length * 4);
  for (let i = 0, j = 0; i < gray.length; i++, j += 4) { rgba[j] = rgba[j + 1] = rgba[j + 2] = gray[i]; rgba[j + 3] = 255; }
  const source = document.createElement('canvas'); source.width = source.height = side;
  source.getContext('2d', {willReadFrequently: true}).putImageData(new ImageData(rgba, side, side), 0, 0);
  previews.set(id, source);
  return source;
}

export function paperThumbnail({app, element, icon, size}) {
  const node = element('span', 'paper-preview');
  node.setAttribute('aria-hidden', 'true');
  let shown;
  const show = id => {
    if (shown && shown.id === (id ?? null) && (shown.found || id == null)) return;
    const source = previewSource(app, id);
    if (source) {
      const canvas = element('canvas'), side = Math.min(source.width, Math.round(size * (devicePixelRatio || 1)));
      canvas.width = canvas.height = side;
      const context = canvas.getContext('2d', {willReadFrequently: true});
      context.imageSmoothingQuality = 'high'; context.drawImage(source, 0, 0, side, side);
      node.replaceChildren(canvas);
    } else if (!shown || shown.found) node.replaceChildren(icon('paper'));
    shown = {id: id ?? null, found: !!source};
  };
  return {node, show, none() { node.replaceChildren(icon('none')); shown = null; }};
}

const paperId = tile => tile?.id ?? null;

function texturePicker(state, layer) {
  const view = state().layer_properties;
  if (String(view.layer) !== String(layer)) return null;
  const control = view.controls.find(c => c.paper && c.kind.kind === 'choice');
  return control && {key: control.key, paper: control.paper, selected: Number(control.value.value)};
}

export function chooseTexture({app, layer, state, dispatch, element, button, icon, message}) {
  const opened = texturePicker(state, layer);
  if (!opened) return;
  const copy = liveCopy(app, 'catalog').native_copy.paper, bootstrap = liveCopy(app, 'bootstrap_view');
  const epoch = String(state().document_file.epoch);
  const root = element('dialog', 'picker-dialog texture-dialog'); root.id = 'texture-dialog';
  bindCopy(root, () => copy.choose, 'ariaLabel');
  const title = element('h2', 'dialog-title', () => copy.choose), grid = element('div', 'texture-grid');
  grid.setAttribute('role', 'radiogroup'); bindCopy(grid, () => copy.choose, 'ariaLabel');
  const header=element('header','dialog-header');header.append(title);
  const page = element('div', 'dialog-page texture-page'); page.append(grid);
  const importButton = button('', () => { const action = shown.import?.action; shell.hide(); if (action) dispatch({type: 'effect', action}); }, 'dialog-button');
  const cancel = button(() => bootstrap.common.cancel, () => root.close(), 'dialog-button');
  const apply = button(() => copy.use_texture, () => { finish(); shell.hide(); }, 'suggested-action');
  importButton.id = 'texture-import'; cancel.id = 'texture-cancel'; apply.id = 'texture-apply';
  const footer = element('footer', 'dialog-footer'); footer.append(importButton, element('span', 'dialog-gap'), cancel, apply);
  const content = element('div', 'dialog-content'); content.append(page, footer);
  const menu = element('div', 'panel-context-menu'); menu.popover = 'auto'; menu.setAttribute('role', 'menu');
  menu.addEventListener('toggle', e => { if (e.newState === 'closed' && !grid.contains(document.activeElement)) reveal(); });
  root.append(header,content, menu); document.body.append(root); addScrollbar(page, content);
  const shell=createDialogHost({app,dialog:root,header,cancel,primary:apply,icon,copy:bootstrap.common,onCancel:()=>shell.hide()});
  let shown = null, shownKey = '', pending = null, tiles = [];
  function rename(action) {
    const form = element('dialog', 'workspace-form'); form.id = 'paper-rename-dialog';
    bindCopy(form, () => copy.rename_title, 'ariaLabel');
    const label = element('label', '', () => bootstrap.common.name), input = element('input'), error = element('p', 'workspace-error'), footer = element('footer');
    input.id = 'paper-rename-name'; input.value = action.name; input.maxLength = 64; label.append(input);
    const save = button(() => copy.rename, () => { renameShell.hide(); dispatch({type: 'effect', action: {...action, name: input.value}}); }, 'suggested-action');
    const validate = () => { error.textContent = app.native_caption({type: 'paper_name_error', name: input.value}); save.disabled = !!error.textContent; };
    input.oninput = validate;
    input.onkeydown = e => { if (e.key === 'Enter') { e.preventDefault(); if (!save.disabled) save.click(); } };
    const cancel=button(() => bootstrap.common.cancel, () => renameShell.hide()),header=element('header','dialog-header');header.append(element('h2', '', () => copy.rename_title));
    footer.append(cancel,save);form.append(header,label,error,footer);
    const renameShell=createDialogHost({app,dialog:form,header,cancel,primary:save,icon,copy:bootstrap.common,onCancel:()=>renameShell.hide()});
    form.addEventListener('close', () => { renameShell.dispose();form.remove(); reveal(); });
    validate(); document.body.append(form); renameShell.present(copy.rename_dialog); input.select();
  }
  function openMenu(index, e) {
    const items = shown.tiles[index].actions;
    if (!items.length) return;
    e.preventDefault(); e.stopPropagation();
    menu.replaceChildren(...items.map(entry => {
      const item = button(entry.label, () => { menu.hidePopover(); if (entry.action.op === 'rename_paper') rename(entry.action); else dispatch({type: 'effect', action: entry.action}); });
      item.setAttribute('role', 'menuitem'); return item;
    }));
    menu.showPopover();
    const tile = tiles[index].getBoundingClientRect(), box = menu.getBoundingClientRect();
    const [x, y] = e.clientX || e.clientY ? [e.clientX, e.clientY] : [tile.left, tile.bottom];
    menu.style.left = `${Math.max(6, Math.min(x, innerWidth - box.width - 6))}px`;
    menu.style.top = `${Math.max(6, Math.min(y, innerHeight - box.height - 6))}px`;
    menu.querySelector('button')?.focus();
  }
  function reveal(node = grid.querySelector('[tabindex="0"]')) {
    if (!node) return;
    node.focus({preventScroll: true});
    if (node.offsetTop === tiles[0].offsetTop) page.scrollTop = 0; else node.scrollIntoView({block: 'nearest'});
  }
  function choose(index) {
    pending = paperId(shown.tiles[index]);
    mark();
  }
  function mark() {
    const index = Math.max(0, shown.tiles.findIndex(tile => paperId(tile) === pending));
    tiles.forEach((node, i) => { const checked = paperId(shown.tiles[i]) === pending; node.setAttribute('aria-checked', String(checked)); node.tabIndex = i === index ? 0 : -1; });
  }
  function tile(entry, index) {
    const node = button('', () => choose(index), 'texture-tile'), frame = element('span', 'texture-frame'), preview = paperThumbnail({app, element, icon, size: 96});
    node.dataset.textureTile = index; node.setAttribute('role', 'radio'); node.title = entry.label;
    preview.show(entry.id); frame.append(preview.node); node.append(frame, element('span', 'texture-caption', entry.label));
    node.addEventListener('contextmenu', e => openMenu(index, e));
    return node;
  }
  function update({paper, selected}) {
    const key = JSON.stringify(paper, (_, value) => typeof value === 'bigint' ? String(value) : value), focused = grid.contains(document.activeElement);
    if (key !== shownKey) {
      if (!shown || !paper.tiles.some(entry => paperId(entry) === pending)) pending = paperId(paper.tiles[selected]);
      shown = paper; shownKey = key;
      if (menu.matches(':popover-open')) menu.hidePopover();
      tiles = paper.tiles.map(tile);
      grid.replaceChildren(...tiles);
      importButton.hidden = !paper.import; importButton.textContent = paper.import?.label ?? '';
    }
    mark();
    if (focused && !grid.contains(document.activeElement)) reveal();
  }
  function finish() {
    const current = String(state().document_file.epoch) === epoch ? texturePicker(state, layer) : null;
    if (!current) { message(() => bootstrap.action_failed); return; }
    const index = current.paper.tiles.findIndex(entry => paperId(entry) === pending);
    if (index >= 0 && index !== current.selected) dispatch({type: 'effect', action: {op: 'set', layer, key: current.key, value: {kind: 'choice', value: index}}});
  }
  grid.addEventListener('keydown', e => {
    const index = tiles.indexOf(document.activeElement);
    if (index < 0) return;
    if (e.key === 'Enter') { e.preventDefault(); apply.click(); return; }
    const columns = tiles.filter(node => node.offsetTop === tiles[0].offsetTop).length;
    const next = {ArrowLeft: index - 1, ArrowRight: index + 1, ArrowUp: index - columns, ArrowDown: index + columns, Home: 0, End: tiles.length - 1}[e.key];
    if (next == null) return;
    e.preventDefault();
    if (!tiles[next]) return;
    choose(next); reveal(tiles[next]);
  });
  const refresh = () => { const current = String(state().document_file.epoch) === epoch && texturePicker(state, layer); if (current) update(current); };
  dialogs.add(refresh);
  root.addEventListener('close', () => { dialogs.delete(refresh);shell.dispose(); root.remove(); });
  update(opened);
  shell.present(opened.paper.dialog);
  reveal();
}
