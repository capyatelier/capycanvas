// Retained DOM projection of Rust's title bar. DOM measurements are inputs;
// allocation, overflow, drag slots, validation and publication stay in Rust.
import { workspaceSwitcherMenu } from './workspace-switcher.js';

export function createHeader({app, state, workspace, element, button, icon, place, dispatch, customization, systemStatus, updateZen}) {
  const root = document.querySelector('#header');
  const retained = element('div'); retained.hidden = true; workspace.append(retained);
  const title = document.querySelector('#document-title');
  const switcher = document.querySelector('.workspace-switcher');
  const recovery = document.querySelector('.workspace-recovery');
  retained.append(title, switcher, systemStatus.root);
  if (recovery) workspace.append(recovery);
  root.replaceChildren(); root.tabIndex = -1;
  customization.target(root, {kind:'header', id:null});
  const bank = element('div', 'header-editor chrome'); bank.id = 'header-editor'; bank.hidden = true;
  bank.setAttribute('aria-label', 'Customize Title Bar'); workspace.append(bank);
  const records = new Map(), chips = new Map();
  const zones = ['left','center','right'].map(zone => {
    const node = element('div', 'header-zone'); node.dataset.zone = zone; root.append(node); return node;
  });
  let view, modelKey, geometry, metrics, insets = [0,0], size, editing = false, selected = null;
  let contact, ghost, frame = 0, measured = '', suppressed = null;
  let buttonContact;
  function clearButtonPress(e) {
    if(!buttonContact||(e&&e.pointerId!==buttonContact.id))return;
    buttonContact.node.removeAttribute('data-header-pressed');buttonContact=null;
  }
  function moveButtonPress(e) {
    if(buttonContact?.id!==e.pointerId)return;
    const {node,parent}=buttonContact;
    if(!node.isConnected||node.parentNode!==parent||node.disabled){clearButtonPress();return;}
    const r=node.getBoundingClientRect();
    node.toggleAttribute('data-header-pressed',e.clientX>=r.left&&e.clientX<r.right&&e.clientY>=r.top&&e.clientY<r.bottom);
  }
  const send = action => dispatch({type:'customize', action:{type:'header', action}});
  const entries = () => view.model.zones.flat();
  const bounds = node => {const r=node.getBoundingClientRect(), p=root.getBoundingClientRect(); return {x:r.x-p.x,y:r.y-p.y,width:r.width,height:r.height};};
  const point = e => {const r=root.getBoundingClientRect(); return [e.clientX-r.x,e.clientY-r.y];};
  function select(id, focus = false) {
    selected = id;
    for (const [key, r] of records) r.root.classList.toggle('editing-selection', editing && key === id);
    if (focus) (records.get(id)?.root.hidden === false ? records.get(id).root : root).focus({preventScroll:true});
  }
  function menu(model, label, glyph, className = '') {
    const node = element('details', `header-menu ${className}`); node.name = 'workspace-menu';
    const summary = element('summary', '', glyph ? null : label); summary.setAttribute('aria-label', label);
    if (glyph) summary.append(icon(glyph));
    const contents = element('div', 'popover'); contents.setAttribute('role', 'menu'); node.append(summary, contents);
    node.refreshMenu = () => {
      customization.renderMenu(contents, model(), () => {node.open=false; updateZen();});
      const r = node.getBoundingClientRect();
      contents.style.left = `${Math.min(0, innerWidth-r.x-contents.offsetWidth-6)}px`;
    };
    node.addEventListener('toggle', () => {if(node.open)node.refreshMenu(); updateZen();});
    return node;
  }
  const application = id => app.editor_models(0,0).application_menus.find(m=>m.id===id).model;
  const primary = () => app.header_view().primary_menu;
  const workspaceChoices = () => workspaceSwitcherMenu(JSON.parse(app.workspace_view()));
  const recoveryMenu = menu(primary, 'Title bar recovery: menus and customization', 'menu');
  recoveryMenu.id = 'header-recovery'; root.append(recoveryMenu);
  const overflow = zones.map((_, index) => {
    const node = menu(() => ({title:'Title Bar',sections:[]}), `More ${['left','center','right'][index]} title-bar items`, 'menu', 'header-overflow');
    node.id = `header-overflow-${index}`;
    const rows=new Map();
    node.refreshMenu = () => {
      const contents=node.querySelector('.popover');
      for(const [id,row] of rows)if(!geometry.hidden[index].includes(id)){row.remove();rows.delete(id);}
      for(const [order,id] of geometry.hidden[index].entries()) {
        const spec=view.items.find(i=>i.id===id);
        let row=rows.get(id);
        if(!row) {
          row=button('',()=>{
            const entry=entries().find(e=>e.id===id);if(!entry)return;
            if(editing){node.open=false;select(id,true);}
            else if(['menu','menu_labels','workspaces'].includes(entry.item.kind)) {
              customization.renderMenu(contents, entry.item.kind==='workspaces'?workspaceChoices():primary(),()=>{node.open=false;});
            } else {node.open=false;activate(entry);}
          });
          const grip=element('span','header-item-grip');grip.setAttribute('aria-hidden','true');grip.append(icon('grip'));
          row.append(grip,element('span','header-overflow-label'));row.dataset.headerOverflowItem=id;
          customization.target(row,{kind:'header',id});rows.set(id,row);
        }
        row.querySelector('.header-overflow-label').textContent=spec.label;
        row.querySelector('.header-item-grip').hidden=!editing;row.disabled=!editing&&!spec.enabled;
        if(contents.children[order]!==row)contents.insertBefore(row,contents.children[order]||null);
      }
      // Nested application menus replace this container outside editing.
      for(const child of [...contents.children])if(!rows.has(Number(child.dataset.headerOverflowItem)))child.remove();
      contents.style.left=`${Math.min(0,innerWidth-node.getBoundingClientRect().x-contents.offsetWidth-6)}px`;
    };
    root.append(node); return node;
  });
  function activate(entry) {
    const command={capy:'zen_mode',settings:'settings',fullscreen:'fullscreen'}[entry.item.kind];
    dispatch(command?{type:'invoke',command}:{type:'activate_header_item',id:entry.id});
  }
  function build(entry) {
    const node=element('div','header-item'); node.dataset.headerItem=entry.id; node.dataset.kind=entry.item.kind;
    node.id=`header-item-${entry.id}`; customization.target(node,{kind:'header',id:entry.id});
    const grip=element('span','header-item-grip'); grip.setAttribute('aria-hidden','true'); grip.append(icon('grip'));
    const content=element('div','header-item-content'); node.append(grip,content);
    const r={root:node,content,grip,entry};
    const kind=entry.item.kind;
    if(['capy','settings','fullscreen','tool'].includes(kind)) {
      const b=button('',()=>activate(entry),'header-tool'); r.button=b;
      if(kind==='capy')b.id='zen-button';
      if(kind==='fullscreen')b.id='fullscreen';
      const command={capy:'zen_mode',settings:'settings',fullscreen:'fullscreen'}[kind];
      if(command)b.dataset.command=command;
      content.append(b);
    } else if(kind==='menu') content.append(menu(primary,'Main Menu','menu','header-menu-overflow'));
    else if(kind==='menu_labels') {
      const labels=element('div','header-menu-labels');
      for(const spec of app.editor_models(0,0).application_menus) {
        const m=menu(()=>application(spec.id),spec.label); m.dataset.menu=spec.id;
        if(spec.id==='window')m.querySelector('.popover').id='workspace-menu'; labels.append(m);
      }
      r.full=labels;
      r.compact=menu(primary,'Application menus','menu','header-menu-labels-compact');
      content.append(labels,r.compact);
    } else if(kind==='workspaces') {
      r.full=switcher;
      content.append(switcher);
      r.compact=menu(workspaceChoices,'Workspaces'); r.compact.id='header-workspace-selector';
      content.append(r.compact);
    } else if(kind==='document_title') content.append(title);
    else if(kind==='clock'||kind==='battery') {
      r.status=kind==='clock'?systemStatus.clock:systemStatus.battery;
      r.placeholder=element('span','header-status-placeholder',kind==='clock'?'Clock':'Battery');
      content.append(r.status,r.placeholder);
    }
    root.append(node); return r;
  }
  function buildBank() {
    const glyphs={capy:'zen-looking-up',menu:'menu',menu_labels:'menu',settings:'settings',fullscreen:'fullscreen-enter',workspaces:'menu',document_title:'new-document'};
    for(const spec of [{item:{kind:'tools'},label:'Add Tools…'},...view.components]) {
      const kind=spec.item.kind, chip=element('div','header-component'); chip.dataset.headerComponent=kind;
      chip.id=`header-component-${kind}`; chip.title=`Drag ${spec.label} into the title bar`;
      chip.setAttribute('aria-label',spec.label);
      const grip=element('span','header-component-grip'); grip.setAttribute('aria-hidden','true');grip.append(icon('grip'));chip.append(grip);
      if(glyphs[kind])chip.append(icon(glyphs[kind]));
      chip.append(element('span','',spec.label)); bank.append(chip); chips.set(kind,{node:chip,spec});
    }
    const options=element('div','header-editor-options'), choices=element('div','header-size-choices');
    choices.setAttribute('role','group'); choices.setAttribute('aria-label','Title bar size');
    for(const s of view.sizes) {
      const b=button(s.label,()=>send({type:'set_size',size:s.id})); b.dataset.headerSize=s.id; choices.append(b);
    }
    const label=element('label'), footer=element('input'); footer.type='checkbox'; footer.id='header-canvas-info';
    footer.addEventListener('change',()=>send({type:'canvas_info',visible:footer.checked})); label.append(footer,'Show footer');
    const cancel=button('Cancel',()=>send({type:'cancel'})); cancel.id='header-edit-cancel';
    const done=button('Done',()=>send({type:'edit',editing:false}),'suggested-action'); done.id='header-edit-done';
    options.append(choices,label,cancel,done); bank.append(options);
  }
  function refresh() {
    view=app.header_view(); size=view.sizes.find(s=>s.id===view.model.size);
    const key=JSON.stringify([view.model,view.editing]);
    if(key!==modelKey) {
      clearButtonPress();
      end(null,true); const wasEditing=editing; editing=view.editing; modelKey=key;
      const focused=root.contains(document.activeElement);
      for(const m of root.querySelectorAll('details[open]'))m.open=false;
      for(const [id,r] of records) if(!entries().some(e=>e.id===id && JSON.stringify(e.item)===JSON.stringify(r.entry.item))) {
        for(const child of [title,switcher,systemStatus.clock,systemStatus.battery])if(r.root.contains(child))retained.append(child);
        r.root.remove();records.delete(id);
      }
      for(const entry of entries())if(!records.has(entry.id))records.set(entry.id,build(entry));
      if(!chips.size)buildBank();
      root.classList.toggle('header-editing',editing); bank.hidden=!editing;
      if(!editing||!entries().some(e=>e.id===selected))selected=null;
      select(selected,focused&&editing);
      if(editing&&!wasEditing)bank.querySelector(`[data-header-size="${size.id}"]`).focus();
      if(!editing&&wasEditing)document.querySelector('#canvas').focus({preventScroll:true});
    }
    root.dataset.size=size.id;
    const cssColor=rgba=>`rgb(${rgba.slice(0,3).map(v=>Math.round(v*255)).join(' ')} / ${rgba[3]})`;
    root.style.setProperty('--header-foreground',cssColor(state().colors.foreground));
    root.style.setProperty('--header-background',cssColor(state().colors.background));
    for(const [name,value] of Object.entries({tile:size.tile,icon:size.icon})) {
      root.style.setProperty(`--header-${name}`,`${value}px`);
      bank.style.setProperty(`--header-${name}`,`${value}px`);
    }
    root.style.height=`${size.height}px`; bank.style.top=`${size.height+6}px`;
    bank.style.maxHeight=`${Math.max(1,workspace.clientHeight-size.height-12)}px`;
    for(const [id,r] of records) {
      const spec=view.items.find(i=>i.id===id); r.root.setAttribute('aria-label',spec.label);
      r.root.tabIndex=editing?0:-1; r.content.inert=editing; r.grip.hidden=!editing;
      if(r.button) {
        const kind=r.entry.item.kind, command=state().commands.find(c=>c.id===r.button.dataset.command);
        const glyph=r.entry.item.control?.kind==='color'?'colors':command?.icon||spec.icon;
        if(glyph && r.button.firstChild?.dataset.asset!==glyph)r.button.replaceChildren(icon(glyph));
        r.button.title=command?.tooltip||spec.label; r.button.setAttribute('aria-label',command?.label||spec.label);
        r.button.disabled=!editing&&(!spec.enabled||(kind==='fullscreen'&&!document.fullscreenEnabled));
        r.button.setAttribute('aria-pressed',String(spec.selected));
        r.button.classList.toggle('brush-color',r.entry.item.control?.kind==='color');
      }
      if(r.status)r.placeholder.hidden=!editing||!r.status.hidden;
    }
    for(const {node,spec} of chips.values()) {
      node.hidden=!!spec.singleton&&entries().some(e=>e.item.kind===spec.item.kind);
      node.inert=entries().length>=128;
    }
    for(const b of bank.querySelectorAll('[data-header-size]'))b.setAttribute('aria-pressed',String(b.dataset.headerSize===size.id));
    bank.querySelector('#header-canvas-info').checked=state().workspace.layout.canvas_info.visible;
    document.querySelector('#canvas-status').hidden=!state().workspace.layout.canvas_info.visible;
    for(const m of root.querySelectorAll('details[open]'))m.refreshMenu?.();
    queue();
  }
  function measure() {
    const extra=editing?20:0;
    return entries().map(entry=>{
      const r=records.get(entry.id),kind=entry.item.kind;
      if(r.status&&!editing&&r.status.hidden)return{id:entry.id,width:0,compact:0};
      // Natural text widths are measured independently of allocated/animated
      // neighbors. The compact selector keeps the same workspace choices.
      const wasHidden=r.root.hidden; r.root.hidden=false;r.root.classList.add('header-measuring');
      const fullHidden=r.full?.hidden,compactHidden=r.compact?.hidden;
      if(r.compact){r.full.hidden=false;r.compact.hidden=true;}
      let width=size.tile,compact;
      if(kind==='workspaces'){width=Math.max(144,switcher.scrollWidth);compact=144;}
      else if(kind==='document_title'){width=180;compact=80;}
      // Fold the labels inside their original item before shared whole-item
      // overflow can hide that item and the rest of its region.
      else if(kind==='menu_labels'){width=Math.max(width,r.full.scrollWidth);compact=size.tile;}
      else if(['clock','battery'].includes(kind))width=Math.max(width,r.content.scrollWidth);
      if(r.compact){r.full.hidden=fullHidden;r.compact.hidden=compactHidden;}
      r.root.classList.remove('header-measuring');r.root.hidden=wasHidden;
      return{id:entry.id,width:width+extra,compact:(compact??width)+extra};
    });
  }
  function present(g, dragging=false) {
    let focus;
    for(const [id,r] of records) {
      const b=g.items.find(i=>i.id===id)?.bounds;
      if(!b&&!dragging) {
        for(const menu of r.root.querySelectorAll('details[open]'))menu.open=false;
        if(r.root.contains(document.activeElement))focus=editing?root:overflow[view.model.zones.findIndex(z=>z.some(e=>e.id===id))].firstChild;
      }
      r.root.hidden=!b;
      if(b) {
        place(r.root,b);
        if(r.compact) {
          const compact=b.width+.01<metrics.find(m=>m.id===id).width;
          const disappearing=compact?r.full:r.compact,appearing=compact?r.compact:r.full;
          if(!disappearing.hidden) {
            for(const menu of [disappearing,...disappearing.querySelectorAll('details')])if(menu.tagName==='DETAILS')menu.open=false;
            if(disappearing.contains(document.activeElement))focus=appearing.querySelector('summary,button')||r.root;
          }
          r.full.hidden=compact;r.compact.hidden=!compact;
        }
      }
      r.root.style.visibility=dragging&&contact.source.kind==='item'&&contact.source.value===id?'hidden':'';
    }
    g.zones.forEach((b,i)=>{zones[i].hidden=!editing;place(zones[i],b);});
    g.overflow.forEach((b,i)=>{
      if(!b){overflow[i].open=false;if(overflow[i].contains(document.activeElement))focus=view.model.zones[i].map(e=>records.get(e.id)).find(r=>!r.root.hidden)?.content.querySelector('summary,button')||root;}
      overflow[i].hidden=!b;if(b)place(overflow[i],b);
      const summary=overflow[i].firstChild,ids=g.hidden[i];
      if(editing&&ids.length===1)summary.dataset.headerOverflowSource=ids[0];
      else delete summary.dataset.headerOverflowSource;
    });
    focus?.focus({preventScroll:true});
  }
  function allocate() {
    frame=0; if(!view)return;
    if(contact?.active)return;
    const width=root.clientWidth;
    recoveryMenu.hidden=editing||entries().some(e=>['capy','menu','menu_labels','workspaces'].includes(e.item.kind));
    insets=[0,recoveryMenu.hidden?0:size.tile];
    if(!recoveryMenu.hidden)place(recoveryMenu,{x:width-size.tile-6,y:6,width:size.tile,height:size.tile});
    metrics=measure();geometry=app.header_geometry(width,insets,metrics);present(geometry);
    const items=[...geometry.items];
    geometry.hidden.forEach((ids,i)=>{if(geometry.overflow[i])for(const id of ids)items.push({id,bounds:geometry.overflow[i]});});
    const height=size.height+(editing?bank.offsetHeight+12:0), key=JSON.stringify([height,items]);
    if(key!==measured){measured=key;dispatch({type:'measure_header',height,items});}
  }
  function queue(){if(!frame)frame=requestAnimationFrame(allocate);}
  function start(e) {
    if(!editing||e.button!==0||contact)return;
    const overflowItem=e.target.closest('[data-header-overflow-item],[data-header-overflow-source]');
    if(e.target.closest('.popover')&&!overflowItem)return;
    const item=overflowItem||e.target.closest('[data-header-item]'),chip=e.target.closest('[data-header-component]');
    const chooser=e.target.closest('.header-overflow > summary');
    if(!item&&!chip&&!chooser){if(e.target===root)select(null,true);return;}
    const node=item||chip||chooser,source=item?{kind:'item',value:Number(item.dataset.headerItem??item.dataset.headerOverflowItem??item.dataset.headerOverflowSource)}:
      chip?(chip.dataset.headerComponent==='tools'?{kind:'tools'}:{kind:'component',value:chips.get(chip.dataset.headerComponent).spec.item}):null;
    if(node.inert)return;
    if(item)select(source.value,true);
    contact={id:e.pointerId,node,parent:node.parentNode,source,press:point(e),grab:bounds(node),active:false,width:root.clientWidth};
    // Stable workspace capture survives hiding the source and live slide.
    workspace.setPointerCapture(e.pointerId); e.preventDefault();
  }
  function move(e) {
    if(contact?.id!==e.pointerId)return;
    if(!contact.node.isConnected||contact.node.parentNode!==contact.parent){end(null,true);return;}
    const p=point(e);
    if(!contact.active) {
      if(Math.hypot(p[0]-contact.press[0],p[1]-contact.press[1])<=8)return;
      // A chooser for several hidden items opens on release; only an
      // individually identified item can start a shared drag.
      if(!contact.source){end(null,true);return;}
      if(!app.begin_header_drag({source:contact.source,width:contact.width,insets,metrics,press:contact.press,grab:contact.grab})){end(null,true);return;}
      contact.active=true;customization.dismissContext();suppressed=contact.id;
      ghost=contact.node.cloneNode(true);ghost.removeAttribute('id');
      for(const n of ghost.querySelectorAll('[id]'))n.removeAttribute('id');
      ghost.classList.add('header-drag-preview');ghost.inert=true;ghost.hidden=false;
      if(contact.node.matches('[data-header-overflow-item]'))ghost.classList.add('header-overflow-preview');
      ghost.style.setProperty('--header-tile',`${size.tile}px`);ghost.style.setProperty('--header-icon',`${size.icon}px`);
      for(const name of ['--header-foreground','--header-background'])ghost.style.setProperty(name,root.style.getPropertyValue(name));
      workspace.append(ghost);root.classList.add('header-dragging');workspace.dataset.headerDragging='true';
      for(const menu of root.querySelectorAll('details[open]'))menu.open=false;
    }
    const preview=app.header_drag_preview(...p);
    if(!preview){end(null,true);return;}
    present(preview.geometry,true);place(ghost,preview.held);
    ghost.classList.toggle('header-drag-remove',preview.detached&&contact.source.kind==='item');
    zones.forEach((z,i)=>z.classList.toggle('header-drop-zone',preview.target?.[0]===['left','center','right'][i]));
    e.preventDefault();
  }
  function end(e,cancel=false) {
    if(!contact||(e&&e.pointerId!==contact.id))return;
    const c=contact;contact=null;
    cancel ||= root.clientWidth!==c.width || !c.node.isConnected || c.node.parentNode!==c.parent;
    if(cancel)customization.dismissContext();
    const action=c.active?app.finish_header_drag(...(e?point(e):c.press),cancel):null;
    if(c.node.matches('[data-header-overflow-item],.header-overflow > summary')) {
      if(!c.active&&!cancel) {
        const menu=c.node.closest('details');
        menu.open=c.node.tagName==='SUMMARY'&&!menu.open;
      }
      suppressed=c.id;
    }
    ghost?.remove();ghost=null;root.classList.remove('header-dragging');delete workspace.dataset.headerDragging;
    for(const z of zones)z.classList.remove('header-drop-zone');
    if(workspace.hasPointerCapture(c.id))workspace.releasePointerCapture(c.id);
    if(geometry)present(geometry);
    if(action)dispatch(action);
    if(c.source?.kind==='item')select(entries().some(i=>i.id===c.source.value)?c.source.value:null,true);
    queue();
  }
  root.addEventListener('pointerdown',start);bank.addEventListener('pointerdown',start);
  // Chromium synthesizes touch mousedown/up together on release, so :active
  // alone supplies no held feedback. Track only the visual contact here;
  // native clicks, context holds and the editor's drag capture keep ownership.
  root.addEventListener('pointerdown',e=>{
    const node=e.target.closest('.header-tool');
    if(editing||e.button!==0||!e.isPrimary||!node||node.disabled)return;
    clearButtonPress();buttonContact={id:e.pointerId,node,parent:node.parentNode};
    node.setAttribute('data-header-pressed','');
  });
  window.addEventListener('pointermove',moveButtonPress,{capture:true});
  for(const name of ['pointerup','pointercancel','lostpointercapture'])window.addEventListener(name,clearButtonPress,{capture:true});
  window.addEventListener('blur',()=>clearButtonPress());
  window.addEventListener('resize',()=>clearButtonPress());
  window.addEventListener('pointerdown',()=>{suppressed=null;},{capture:true});
  window.addEventListener('pointermove',move,{capture:true});
  window.addEventListener('pointerup',e=>end(e),{capture:true});
  window.addEventListener('pointercancel',e=>end(e,true),{capture:true});
  workspace.addEventListener('lostpointercapture',e=>end(e,true));
  window.addEventListener('blur',()=>end(null,true));
  window.addEventListener('resize',()=>{end(null,true);queue();});
  window.addEventListener('click',e=>{
    if(e.pointerId===suppressed||editing&&e.target.closest('.header-item-content,[data-header-component]')) {
      suppressed=null;e.preventDefault();e.stopImmediatePropagation();
    }
  },{capture:true});
  window.addEventListener('keydown',e=>{
    if(e.key==='Escape')clearButtonPress();
    if(e.isComposing||e.target.closest('input,select,textarea,[contenteditable=true],dialog[open],.popover,.panel-context-menu'))return;
    if(e.key==='Escape'&&contact){end(null,true);e.preventDefault();e.stopImmediatePropagation();return;}
    if(!editing||!(root.contains(e.target)||e.target===root)||selected==null)return;
    let action;
    if(e.key==='Delete'||e.key==='Backspace')action={type:'customize',action:{type:'header',action:{type:'remove',id:selected}}};
    else if(e.key==='ArrowLeft'||e.key==='ArrowRight')action=app.header_step(selected,e.key==='ArrowRight');
    else if(e.key==='ContextMenu'||e.key==='F10'&&e.shiftKey) {
      const node=records.get(selected)?.root||root,b=node.getBoundingClientRect();
      node.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:b.x,clientY:b.bottom}));
    } else return;
    e.preventDefault();e.stopImmediatePropagation();if(action)dispatch(action);
  },{capture:true});
  root.addEventListener('contextmenu',e=>{const item=e.target.closest('[data-header-item]');if(editing&&item)select(Number(item.dataset.headerItem));});
  new ResizeObserver(()=>{if(contact&&contact.width!==root.clientWidth)end(null,true);queue();}).observe(root);
  new ResizeObserver(queue).observe(bank);
  new MutationObserver(()=>{
    if(contact&&(!contact.node.isConnected||contact.node.parentNode!==contact.parent))end(null,true);
    if(buttonContact&&(!buttonContact.node.isConnected||buttonContact.node.parentNode!==buttonContact.parent))clearButtonPress();
  })
    .observe(workspace,{childList:true,subtree:true});
  new MutationObserver(queue).observe(switcher,{childList:true,subtree:true,characterData:true});
  new MutationObserver(()=>{for(const r of records.values())if(r.status)r.placeholder.hidden=!editing||!r.status.hidden;queue();})
    .observe(systemStatus.root,{subtree:true,attributes:true,attributeFilter:['hidden'],characterData:true,childList:true});
  for(const node of [systemStatus.clock,systemStatus.battery])new MutationObserver(()=>{const r=[...records.values()].find(r=>r.status===node);if(r)r.placeholder.hidden=!editing||!node.hidden;queue();})
    .observe(node,{attributes:true,attributeFilter:['hidden'],childList:true,characterData:true,subtree:true});
  refresh();
  return {refresh,queue};
}
