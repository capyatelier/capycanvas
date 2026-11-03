import {browserDragThreshold} from './pen-scroll.js';
import {addScrollbar} from './scrollbars.js';

export function createMenu({app,state,element,button,icon,dispatch,customization,place}) {
  const root=element('div','menu-surface window-bar'),frame=element('div','menu-scroll-frame'),scroll=element('div','menu-scroll'),stack=element('div','menu-pages'),separator=element('hr','menu-separator');
  root.id='phone-menu';scroll.id='phone-menu-scroll';stack.id='phone-menu-pages';scroll.append(stack);frame.append(scroll);root.append(frame,separator);
  const thumb=addScrollbar(scroll,frame,stack);
  const pages=new Map(),strip=[];
  let model,contact,settling;
  const tracker=app.phone_menu_contact(),route=()=>state().phone.menu_route;
  const send=action=>dispatch({type:'phone',action});
  const key=route=>JSON.stringify(route),time=event=>Math.floor(event.timeStamp)>>>0;
  const input=request=>app.phone_menu_input(tracker,request);
  const clearPressed=()=>{for(const row of root.querySelectorAll('[data-pressed]'))delete row.dataset.pressed;};
  function cancelPress(){if(model)input({kind:'cancel_press'});clearPressed();}
  function cancelContact() {
    cancelPress();
    if(contact&&root.hasPointerCapture(contact.id))root.releasePointerCapture(contact.id);
    contact=null;settling?.reveal?.cancel();settling?.cancel();settling=null;
  }
  function show(reset=true) {
    const current=key(route());
    const moveFocus=[...pages].some(([id,page])=>id!==current&&page.root.contains(document.activeElement));
    for(const [id,page]of pages) {
      if(id===current&&page.root.hidden)reset=true;
      page.root.hidden=id!==current;page.root.inert=id!==current;
      page.root.style.transform='';page.root.style.clipPath='';page.root.style.position='';page.root.style.top='';
    }
    stack.inert=false;if(reset)scroll.scrollTop=0;
    if(moveFocus)pages.get(current)?.root.querySelector('button:not(:disabled)')?.focus({preventScroll:true});
  }
  function activate(hit,row,event) {
    if(event?.detail&&!input({kind:'release',hit,inside:true}).accepted)return;
    clearPressed();
    if(hit.kind==='back'){send({kind:'menu_back',route:route()});return;}
    if(hit.kind==='strip'){const item=model.strip[hit.position];if(item.enabled&&item.action)dispatch(item.action);return;}
    if(!row?.enabled)return;
    if(hit.kind==='close'){if(row.close)dispatch(row.close.action);return;}
    if(row.target)send({kind:'menu_select',route:row.target});
    else if(row.action)dispatch(row.action);
  }
  function bind(node,hit,row,page) {
    node.dataset.menuHit=JSON.stringify(hit);node.onclick=event=>{
      if(!model||!root.contains(node)||page&&key(page)!==key(route()))return;
      activate(hit,row?.(),event);
    };
  }
  for(let index=0;index<5;index++) {
    const node=button('',()=>{}, 'tile-button header-tool');
    node.id=`phone-${['preferences','search','finger','zen','swap'][index]}`;
    bind(node,{kind:'strip',position:index});strip.push(node);root.append(node);
  }
  function rebuild(next) {
    cancelContact();
    for(const page of pages.values())for(const record of page.records)record.row.disposeMenuRow();
    stack.replaceChildren();pages.clear();
    for(const page of next.pages) {
      const body=element('div','menu-page workspace-menu-items');body.dataset.route=key(page.id);body.setAttribute('role','menu');body.setAttribute('aria-label',page.title);
      const records=[];
      if(page.parent) {
        const back=button('',()=>{},'submenu-back menu-row'),arrow=element('span','menu-back-arrow'),title=element('span','menu-label');
        arrow.append(icon('go-previous'));title.append(element('span','menu-caption',page.title));back.append(arrow,title);back.dataset.back='true';
        bind(back,{kind:'back'},null,page.id);body.append(back);
      }
      for(const [s,section]of page.sections.entries()) {
        if(s)body.append(element('hr'));
        for(const [i,item]of section.entries()) {
          if(item.is_heading){body.append(element('div','menu-heading',item.label));continue;}
          const row=customization.menuRow(item,()=>{},{checks:page.checks,submenu:!!item.target,detail:item.count||item.hint});row.classList.add('menu-row');
          const hit={kind:'row',position:[s,i]};bind(row,hit,()=>model.pages.find(p=>key(p.id)===key(page.id)).sections[s][i],page.id);
          row.dataset.position=JSON.stringify([s,i]);
          if(item.target?.page?.kind==='drawings')row.id='phone-drawing';
          if(item.action?.type==='phone'&&item.action.action.kind==='drawing')row.id=`phone-drawing-${item.action.action.id}`;
          if(item.close){const line=element('div','menu-drawing-row'),close=button('',()=>{},'tile-button header-tool menu-close');close.append(icon('window-close',true));bind(close,{kind:'close',position:[s,i]},()=>model.pages.find(p=>key(p.id)===key(page.id)).sections[s][i],page.id);line.append(row,close);body.append(line);records.push({row,close,s,i});}
          else {body.append(row);records.push({row,s,i});}
        }
      }
      stack.append(body);pages.set(key(page.id),{root:body,records,page});
    }
  }
  const shape=next=>JSON.stringify(next.pages.map(p=>[p.id,p.parent,p.title,p.checks,p.sections.map(s=>s.map(r=>[r.is_heading?r.label:false,r.target,!!r.close]))]));
  let structure;
  function refresh() {
    const next=app.phone_menu();if(!next)return;
    model=next;root.style.setProperty('--header-tile',`${app.header_view().sizes.find(size=>size.id==='medium').tile}px`);
    const nextStructure=shape(next);
    if(nextStructure!==structure){structure=nextStructure;rebuild(next);}
    for(const [index,node]of strip.entries()) {
      const item=next.strip[index];node.disabled=!item.enabled;node.title=item.label;node.setAttribute('aria-label',item.label);
      if(item.selected==null)node.removeAttribute('aria-pressed');else node.setAttribute('aria-pressed',item.selected);
      if(node.dataset.icon!==item.icon){node.dataset.icon=item.icon;node.replaceChildren(icon(item.icon,true));}
    }
    for(const page of next.pages)for(const record of pages.get(key(page.id)).records) {
      const item=page.sections[record.s][record.i];record.row.updateMenuRow(item,item.count||item.hint);
      if(record.close){record.close.style.width=`${Number(next.close_width)}px`;record.close.title=item.close.label;record.close.setAttribute('aria-label',item.close.label);}
    }
    if(contact&&key(contact.page)!==key(route())||settling&&key(settling.menuRoute)!==key(route()))cancelContact();
    if(!contact&&!settling)show(false);
  }
  function measure() {
    const page=pages.get(key(model.pages.find(p=>!p.parent).id)).root;
    const before=page.style.cssText,hidden=page.hidden;
    page.hidden=false;page.style.cssText=`position:absolute;visibility:${hidden?'hidden':'visible'};width:max-content;inset:auto;`;
    const row=page.querySelector('.menu-row'),rowHeight=row.getBoundingClientRect().height;
    const natural=page.getBoundingClientRect().width;
    page.style.width='min-content';const minimum=page.getBoundingClientRect().width;
    const width=app.phone_menu_width(minimum,natural,rowHeight);
    const contentWidth=width;
    page.style.width=`${contentWidth}px`;const height=page.getBoundingClientRect().height;
    page.hidden=hidden;page.style.cssText=before;
    return measured={width,height,row_height:rowHeight};
  }
  let geometry,measured;
  function layout(placement,edge) {
    root.dataset.edge=edge;
    const rowHeight=measured.row_height;
    geometry=app.phone_menu_geometry(placement,rowHeight);
    place(root,{x:0,y:0,width:placement.bounds.width,height:placement.bounds.height});place(frame,geometry.list);
    place(scroll,{x:0,y:0,width:geometry.list.width,height:geometry.list.height});place(separator,geometry.separator);
    for(const [index,node]of strip.entries()){place(node,geometry.buttons[index]);node.style.setProperty('--tile-icon-size',`${geometry.icon}px`);}
  }
  const rtl=()=>getComputedStyle(root).direction==='rtl';
  function preview(progress) {
    const current=pages.get(key(contact.page)).root,parent=pages.get(key(contact.parent)).root;
    const distance=scroll.clientWidth*progress;
    parent.hidden=false;parent.inert=true;parent.style.position='absolute';parent.style.top=`${scroll.scrollTop}px`;
    parent.style.clipPath=rtl()?`inset(0 0 0 ${scroll.clientWidth-distance}px)`:`inset(0 ${scroll.clientWidth-distance}px 0 0)`;
    current.style.transform=`translateX(${rtl()?-distance:distance}px)`;stack.inert=true;
  }
  root.addEventListener('pointerdown',event=>{
    if(!model||event.button||!event.isPrimary||settling)return;
    const node=event.target.closest('[data-menu-hit]');if(!node)return;
    input({kind:'press',hit:JSON.parse(node.dataset.menuHit)});node.dataset.pressed='true';
    if(!scroll.contains(node))return;
    const page=model.pages.find(p=>key(p.id)===key(route()));
    contact={id:event.pointerId,start:[event.clientX,event.clientY],page:page.id,parent:page.parent,node};
    input({kind:'begin_swipe',time_ms:time(event)});
  },true);
  root.addEventListener('pointermove',event=>{
    if(contact?.id!==event.pointerId)return;
    const delta=[event.clientX-contact.start[0],event.clientY-contact.start[1]];
    const result=input({kind:'swipe',delta,width:scroll.clientWidth,past_slop:Math.hypot(...delta)>browserDragThreshold,rtl:rtl(),time_ms:time(event)}).swipe;
    if(result.kind!=='pending')clearPressed();
    if(result.kind==='back') {
      event.preventDefault();contact.swiping=true;contact.progress=result.progress;
      root.setPointerCapture(event.pointerId);preview(result.progress);
    }
  });
  root.addEventListener('pointerup',event=>{
    if(contact?.id!==event.pointerId)return;
    if(!contact.swiping){contact=null;return;}
    const settle=input({kind:'end_swipe',width:scroll.clientWidth,rtl:rtl(),time_ms:time(event)}).settle;
    const current=pages.get(key(contact.page)).root,parent=pages.get(key(contact.parent)).root;
    const from=current.style.transform,clip=parent.style.clipPath;
    preview(settle.action?1:0);
    const options={duration:matchMedia('(prefers-reduced-motion: reduce)').matches?0:settle.duration_ms,easing:'cubic-bezier(0.3333333333333333,1,0.6666666666666666,1)'};
    settling=current.animate([{transform:from},{transform:current.style.transform}],options);
    settling.reveal=parent.animate([{clipPath:clip},{clipPath:parent.style.clipPath}],options);
    settling.menuRoute=contact.page;contact=null;settling.onfinish=()=>{settling.reveal.cancel();settling=null;if(settle.action)send(settle.action);else show(false);};
  });
  root.addEventListener('pointercancel',()=>{cancelContact();if(model)show(false);});
  scroll.addEventListener('scroll',cancelPress,{passive:true});
  root.addEventListener('keydown',event=>{
    if(!model)return;
    const page=pages.get(key(route())).root,rows=[...page.querySelectorAll('button:not(:disabled)')];
    if(event.key==='Escape'||event.key===(rtl()?'ArrowRight':'ArrowLeft')){
      event.preventDefault();event.stopPropagation();send({kind:'menu_back',route:route()});
    } else if(['ArrowDown','ArrowUp','Home','End'].includes(event.key)){
      event.preventDefault();event.stopPropagation();const current=rows.indexOf(document.activeElement);
      const index=event.key==='Home'?0:event.key==='End'?rows.length-1:(current+(event.key==='ArrowUp'?-1:1)+rows.length)%rows.length;
      rows[index]?.focus({preventScroll:true});rows[index]?.scrollIntoView({block:'nearest'});
    } else if(event.key===(rtl()?'ArrowLeft':'ArrowRight')){event.preventDefault();if(document.activeElement?.menuItem?.target)document.activeElement.click();}
  });
  refresh();
  return{root,anchor:()=>({kind:'phone',anchor:{kind:'capy'}}),refresh,measure,layout,dispose(){cancelContact();thumb.disposeScrollbar();for(const page of pages.values())for(const record of page.records)record.row.disposeMenuRow();tracker.free();model=null;},setEdge(edge){root.dataset.edge=edge;}};
}
