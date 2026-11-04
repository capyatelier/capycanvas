import {createToolbarComponent} from './toolbar-components.js';
import {updatePaintPairIcon} from './color-controls.js';
import {browserDragThreshold} from './pen-scroll.js';

export function createPhone({app,state,workspace,capy,header,element,button,icon,dropdown,place,dispatch,rasterWorker,customization,changed}) {
  const root=element('div','window-bar');root.id='phone-frame';root.hidden=true;workspace.append(root);
  const parts={};
  for(const id of ['bar-tools','bar-capy','tools-chip','sliders-chip']) {
    const node=element('div',id.endsWith('chip')?'phone-chip':'phone-bar');node.id=`phone-${id}`;root.append(node);parts[id]=node;
  }
  const capyTarget=element('div','phone-target');root.append(capyTarget);
  const settingsTarget=element('div','phone-target'),settings=button('',()=>send({kind:'tool_settings'}),'tile-button phone-tool');
  settings.id='phone-tool-settings';settingsTarget.append(settings);root.append(settingsTarget);
  const tools=new Map(),sliders=new Map();
  let geometry,active=false,insets=[...state().phone.insets],contact,preview,lifted,suppressClick=false;
  const send=action=>dispatch({type:'phone',action});
  for(const name of ['bar-tools','bar-capy'])customization.target(parts[name],{kind:'phone_toolbar'});
  customization.target(capyTarget,{kind:'zen_mode'});
  function position(node,bounds) {
    node.hidden=!bounds||bounds.width<=0||bounds.height<=0;
    if(!node.hidden)place(node,bounds);
  }
  function target(node,child,hit,bounds,contact=hit) {
    node.onclick=event=>{if(event.target===node&&contact.width>0&&contact.height>0)child.click();};
    position(node,hit);position(child,{...bounds,x:bounds.x-hit.x,y:bounds.y-hit.y});
  }
  function refreshButton(node,view) {
    node.disabled=!(view.enabled||view.has_variants);node.title=view.disabled_reason??view.tooltip;
    node.setAttribute('aria-label',view.label);node.setAttribute('aria-pressed',String(view.selected));
    if(node.firstChild?.dataset.asset!==view.icon){const glyph=icon(view.icon,true);if(node.firstChild)node.firstChild.replaceWith(glyph);else node.append(glyph);}
    if(node.firstChild?.hasAttribute('data-paint-pair'))updatePaintPairIcon(node.firstChild,app.paint_pair());
    const variants=node.querySelector('.tool-variations');if(variants)variants.hidden=!view.has_variants;
  }
  function refresh() {
    report();
    if(!active)return;
    if(contact&&(contact.hold_ms!==app.phone_hold_ms()||contact.bar&&!app.phone_bar_valid(contact.bar)))cancelContact();
    const authored=state().workspace.layout.phone??[],views=app.phone_tools();
    for(const [id,record]of tools)if(!authored.some(tile=>tile.id===id)){record.root.remove();tools.delete(id);}
    for(const [index,tile]of authored.entries()) {
      let record=tools.get(tile.id);
      if(!record) {
        const wrapper=element('div','phone-target'),node=tile.control.kind==='divider'?element('div','tile-divider'):button('',()=>send({kind:'tool',tile:tile.id}),'tile-button phone-tool');
        node.id=`phone-tool-${tile.id}`;wrapper.append(node);root.append(wrapper);record={root:wrapper,node};tools.set(tile.id,record);
        wrapper.onclick=event=>{if(event.target===wrapper)node.click();};
        customization.target(wrapper,{kind:'phone_tile',tile:tile.id});
        if(tile.control.kind==='divider')node.setAttribute('role','separator');
        else {node.append(icon(views[index].icon,true),customization.variationMarker());node.addEventListener('dblclick',()=>{if(record.view.double_click)dispatch({type:'double_click_tool',control:record.view.resolved_control});});}
      }
      record.view=views[index];
      if(tile.control.kind!=='divider')refreshButton(record.node,record.view);
      placeTool(record,index,tile);
    }
    refreshButton(settings,app.phone_settings());
    target(settingsTarget,settings,geometry.settings_hit,geometry.settings);
    for(const [index,name]of ['size','strength'].entries()) {
      const control={kind:index===0?'brush_size_slider':'brush_strength_slider'},component=app.toolbar_component(control);
      const presentation={component,label:component.value_label,slider_style:{show_value:false,expand:false}};
      let slider=sliders.get(name);
      if(!slider) {
        slider=createToolbarComponent({app,control,presentation,view:{tile_style:'medium'},element,button,icon,dropdown,dispatch,place,rasterWorker});
        slider.id=`component-slider-${name}`;root.append(slider);sliders.set(name,slider);
      } else slider.updateComponent(presentation);
      const bounds=geometry[name];position(slider,bounds);
      if(!slider.hidden)slider.layoutComponent(bounds,geometry.edge==='top'?'horizontal':'vertical','medium');
      else slider.closeComponent();
    }
    const headerView=app.header_view(),command=headerView.capy;
    root.style.setProperty('--header-tile',`${headerView.sizes.find(size=>size.id==='medium').tile}px`);
    if(capy.firstChild?.dataset.asset!==command.icon)capy.replaceChildren(icon(command.icon));
    capy.title=app.capy_caption(workspace.clientWidth,workspace.clientHeight);capy.setAttribute('aria-label',capy.title);capy.disabled=false;
    target(capyTarget,capy,geometry.capy_target,geometry.capy,geometry.capy_hit);
  }
  function layout(resolved) {
    geometry=resolved.phone;
    const next=!!geometry;
    if(next!==active) {
      cancelContact();
      active=next;root.hidden=!active;header.phone(active);
      const parent=active?capyTarget:workspace;
      if(parent.moveBefore)parent.moveBefore(capy,null);
      else {const focused=document.activeElement===capy;parent.append(capy);if(focused)capy.focus({preventScroll:true});}
      if(!active)for(const slider of sliders.values())slider.closeComponent();
    }
    if(!active)return;
    for(const [name,bounds]of [['bar-tools',geometry.bars[0]],['bar-capy',geometry.bars[1]],['tools-chip',geometry.chips[0]],['sliders-chip',geometry.chips[1]]])position(parts[name],bounds);
    root.style.setProperty('--tile-icon-size',`${geometry.icon}px`);
    root.style.setProperty('--header-capy-icon',`${geometry.capy_icon}px`);
    refresh();changed();
  }
  const probe=element('div');probe.id='phone-safe-insets';workspace.append(probe);
  function placeTool(record,index,tile) {
    const visual=(preview??geometry.tools)[index],hit=index===lifted?visual:tile.control.kind==='divider'?geometry.tools[index]:geometry.tool_hits[index];
    position(record.root,hit);position(record.node,{...visual,x:visual.x-hit.x,y:visual.y-hit.y});
    record.root.style.zIndex=index===lifted?'1':'';record.root.classList.toggle('header-drag-preview',index===lifted);
  }
  function placePreview(reply) {
    preview=reply?.geometry;lifted=reply?.lifted==null?undefined:Number(reply.lifted);
    for(const [index,tile]of (state().workspace.layout.phone??[]).entries()) {
      const record=tools.get(tile.id);if(!record)continue;
      placeTool(record,index,tile);
    }
  }
  function cancelContact() {
    const previous=contact;contact=null;
    if(!previous)return;
    suppressClick=true;
    clearTimeout(previous.timer);
    if(previous.capy)send({kind:'capy_drop',position:[-1,-1]});
    previous.bar?.free();
    if(root.hasPointerCapture(previous.id))root.releasePointerCapture(previous.id);
    delete root.dataset.contact;preview=undefined;lifted=undefined;if(geometry)placePreview();
  }
  const point=event=>{const bounds=workspace.getBoundingClientRect();return[event.clientX-bounds.x,event.clientY-bounds.y];};
  function capture() {if(contact&&!root.hasPointerCapture(contact.id))root.setPointerCapture(contact.id);}
  window.addEventListener('pointerdown',event=>{
    cancelContact();
    if(!active||!root.contains(event.target)||event.button||!event.isPrimary)return;
    suppressClick=false;
    const start=point(event),capyPress=capyTarget.contains(event.target),bar=capyPress?null:app.phone_bar_contact(...start);
    if(!capyPress&&!bar)return;
    contact={id:event.pointerId,start,bar,capy:capyPress,position:start,held:false,hold_ms:app.phone_hold_ms()};
    const current=contact;
    current.timer=setTimeout(()=>{
      if(contact!==current)return;
      if(current.capy){current.held=true;send({kind:'capy_hold'});capture();suppressClick=true;return;}
      const reply=app.phone_bar_input(current.bar,{kind:'arm'});
      if(reply.outcome.kind==='held'){current.held=true;root.dataset.contact='held';capture();placePreview(reply);suppressClick=true;}
      else if(reply.outcome.kind==='cancel')cancelContact();
    },current.hold_ms);
  },true);
  window.addEventListener('pointermove',event=>{
    if(contact?.id!==event.pointerId)return;
    contact.position=point(event);
    const past_slop=Math.hypot(contact.position[0]-contact.start[0],contact.position[1]-contact.start[1])>browserDragThreshold;
    if(contact.capy){if(past_slop&&!contact.held){clearTimeout(contact.timer);contact.cancelled=true;capture();suppressClick=true;}return;}
    if(contact.cancelled)return;
    const reply=app.phone_bar_input(contact.bar,{kind:'motion',position:contact.position,past_slop});
    if(reply.outcome.kind==='window_move'){
      clearTimeout(contact.timer);contact.cancelled=true;capture();suppressClick=true;placePreview();
    } else if(reply.outcome.kind==='dragging'){
      event.preventDefault();root.dataset.contact='dragging';placePreview(reply);suppressClick=true;
    } else if(reply.outcome.kind==='cancel'){suppressClick=true;cancelContact();}
  },true);
  window.addEventListener('pointerup',event=>{
    if(contact?.id!==event.pointerId)return;
    const current=contact,position=point(event);contact=null;clearTimeout(current.timer);
    if(root.hasPointerCapture(current.id))root.releasePointerCapture(current.id);
    delete root.dataset.contact;placePreview();
    if(current.capy){if(current.held)send({kind:'capy_drop',position});return;}
    const {outcome}=app.phone_bar_input(current.bar,{kind:'finish',position});current.bar.free();
    if(current.cancelled)return;
    if(outcome.kind==='move'){send({kind:'move_tool',...outcome.value});suppressClick=true;}
    else if(outcome.kind==='menu'){
      const source=outcome.value==null?parts['bar-tools']:tools.get(outcome.value)?.root;
      if(source)customization.showContext(source,[event.clientX,event.clientY]);suppressClick=true;
    } else if(outcome.kind==='cancel')suppressClick=true;
  },true);
  window.addEventListener('pointercancel',event=>{if(contact?.id===event.pointerId){suppressClick=true;cancelContact();}},true);
  root.addEventListener('lostpointercapture',event=>{if(contact?.id===event.pointerId){suppressClick=true;cancelContact();}});
  root.addEventListener('click',event=>{if(suppressClick&&event.detail){suppressClick=false;event.preventDefault();event.stopImmediatePropagation();}},true);
  window.addEventListener('blur',()=>{suppressClick=true;cancelContact();});
  window.addEventListener('keydown',event=>{
    if(event.key==='Escape'&&contact){suppressClick=true;cancelContact();event.preventDefault();event.stopPropagation();}
  },true);
  function report(){
    if(!state().workspace_ready||insets.every((value,index)=>value===state().phone.insets[index]))return;
    send({kind:'metrics',native:[0,0],native_counts:[0,0],movable:false,insets});
  }
  const indices=new Map();
  const observer=new ResizeObserver(entries=>{
    for(const entry of entries)insets[indices.get(entry.target)]=entry.contentRect.width;
    report();
  });
  for(const [index,side]of ['left','top','right','bottom'].entries()){
    const measure=element('div');measure.style.width=`env(safe-area-inset-${side})`;
    indices.set(measure,index);probe.append(measure);observer.observe(measure);
  }
  return {layout,refresh};
}
