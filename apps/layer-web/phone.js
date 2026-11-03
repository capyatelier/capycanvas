import {createToolbarComponent} from './toolbar-components.js';
import {updatePaintPairIcon} from './color-controls.js';

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
  let geometry,active=false,insets=[...state().phone.insets];
  const send=action=>dispatch({type:'phone',action});
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
    const authored=state().workspace.layout.phone??[],views=app.phone_tools();
    for(const [id,record]of tools)if(!authored.some(tile=>tile.id===id)){record.root.remove();tools.delete(id);}
    for(const [index,tile]of authored.entries()) {
      let record=tools.get(tile.id);
      if(!record) {
        const wrapper=element('div','phone-target'),node=tile.control.kind==='divider'?element('div','tile-divider'):button('',()=>send({kind:'tool',tile:tile.id}),'tile-button phone-tool');
        node.id=`phone-tool-${tile.id}`;wrapper.append(node);root.append(wrapper);record={root:wrapper,node};tools.set(tile.id,record);
        if(tile.control.kind==='divider')node.setAttribute('role','separator');
        else {node.append(icon(views[index].icon,true),customization.variationMarker());node.addEventListener('dblclick',()=>{if(record.view.double_click)dispatch({type:'double_click_tool',control:record.view.resolved_control});});}
      }
      record.view=views[index];
      if(tile.control.kind!=='divider')refreshButton(record.node,record.view);
      const hit=tile.control.kind==='divider'?geometry.tools[index]:geometry.tool_hits[index];
      target(record.root,record.node,hit,geometry.tools[index]);
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
