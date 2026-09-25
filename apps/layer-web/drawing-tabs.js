// DOM input/measurement only. Drawing identity, order, drop validation, width
// policy and undo belong to Rust; this is separate from docked panel tabs.
export function createDrawingTabs({app,element,button,icon,applyChange,select,close,openFiles,message,busy}) {
  const root=element('div','drawing-title'),plain=document.querySelector('#document-title');
  root.id='drawing-title';plain.replaceWith(root);root.append(plain);
  const strip=element('div','drawing-tabs');strip.setAttribute('role','tablist');strip.setAttribute('aria-label','Drawings');
  const compact=button('',()=>showSelector(),'drawing-selector');compact.setAttribute('aria-label','Select drawing');
  root.append(strip,compact);
  const rows=new Map();let model,signature='',popup=null,contact=null,rowMenu=null,suppress=false;
  const same=(a,b)=>String(a)===String(b);
  const id=value=>BigInt(value);
  const fail=error=>message(String(error));
  const invoke=fn=>{try{Promise.resolve(fn()).catch(fail);}catch(error){fail(error);}};
  const editing=()=>app.state().customization.header_editing;
  function dismissRowMenu(){rowMenu?.remove();rowMenu=null;}
  function showRowMenu(tabId){
    dismissRowMenu();if(!popup)return;
    rowMenu=element('div','drawing-row-menu');rowMenu.setAttribute('role','group');rowMenu.setAttribute('aria-label','Drawing actions');
    const action=(label,fn)=>rowMenu.append(button(label,()=>{dismissRowMenu();invoke(fn);}));
    action('Select',()=>{dismiss();return select(id(tabId));});
    action('Move earlier',()=>{applyChange(app.step_document(id(tabId),false));refresh(true);});
    action('Move later',()=>{applyChange(app.step_document(id(tabId),true));refresh(true);});
    action('Close',()=>{dismiss();return close(id(tabId));});
    popup.append(rowMenu);
  }
  function cancelDrag(){
    const held=contact;contact=null;
    if(held){clearTimeout(held.timer);cancelAnimationFrame(held.scrollFrame);if(held.node.hasPointerCapture(held.pointer))held.node.releasePointerCapture(held.pointer);
      if(held.slide){held.slide.overlay.remove();for(const n of held.slide.nodes)n.classList.remove('dragged-tab-source');}}
    for(const n of document.querySelectorAll('[data-drawing-drop]'))n.removeAttribute('data-drawing-drop');
    document.body.classList.remove('drawing-dragging');
  }
  function hits(container){const area=container.getBoundingClientRect();return [...container.children].filter(n=>n.dataset.drawingId).flatMap(n=>{const r=n.getBoundingClientRect(),x=Math.max(area.left,r.left),y=Math.max(area.top,r.top),width=Math.min(area.right,r.right)-x,height=Math.min(area.bottom,r.bottom)-y;return width>0&&height>0?[{id:id(n.dataset.drawingId),bounds:{x,y,width,height}}]:[];});}
  function startSlide(held){
    const rect=n=>{const b=n.getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height};};
    const nodes=[...strip.children].filter(n=>n.dataset.drawingId),clip=rect(strip),base=root.getBoundingClientRect(),overlay=element('div','drawing-slide');
    overlay.setAttribute('aria-hidden','true');overlay.inert=true;
    Object.assign(overlay.style,{left:`${clip.x-base.x}px`,top:`${clip.y-base.y}px`,width:`${clip.width}px`,height:`${clip.height}px`});
    const tabs=nodes.map(n=>({id:id(n.dataset.drawingId),bounds:rect(n)}));
    const copies=nodes.map((n,i)=>{const copy=n.cloneNode(true),b=tabs[i].bounds;copy.removeAttribute('data-drawing-id');
      copy.classList.add(same(n.dataset.drawingId,held.id)?'dragged-tab-preview':'neighbor-tab-preview');
      Object.assign(copy.style,{left:`${b.x-clip.x}px`,top:`${b.y-clip.y}px`,width:`${b.width}px`,height:`${b.height}px`,transform:'translateX(0px)'});
      n.classList.add('dragged-tab-source');overlay.append(copy);return copy;});
    root.append(overlay);overlay.getBoundingClientRect();
    held.slide={hits:tabs,clip,nodes,copies,overlay,source:nodes.findIndex(n=>same(n.dataset.drawingId,held.id))};
  }
  function slideAt(e){const s=contact.slide;return s&&app.document_slide({id:id(contact.id),hits:s.hits,clip:s.clip,press:[contact.x,contact.y],point:[e.clientX,e.clientY]});}
  function target(e){
    if(!contact.vertical){const slide=slideAt(e);return slide?.attached?{before:slide.before}:null;}
    return app.document_drop(hits(contact.container),e.clientX,e.clientY,true);
  }
  function preview(e){
    if(!contact.vertical){
      const s=contact.slide,slide=slideAt(e);if(!slide){cancelDrag();return;}
      s.copies.forEach((copy,i)=>{const x=i===s.source?slide.bounds.x-s.hits[i].bounds.x:slide.offsets[i];copy.style.transform=`translateX(${Math.round(x*devicePixelRatio)/devicePixelRatio}px)`;});
      return;
    }
    const drop=target(e);
    for(const node of contact.container.children)node.removeAttribute('data-drawing-drop');
    if(drop){const row=[...contact.container.children].find(n=>same(n.dataset.drawingId,drop.before))??contact.container.lastElementChild;row?.setAttribute('data-drawing-drop',drop.before==null?'after':'before');}
  }
  function scrollDrag(held){
    if(contact!==held||!held.dragging)return;
    const r=held.container.getBoundingClientRect(),p=held.position;
    if(p&&p.clientX>=r.left&&p.clientX<=r.right&&p.clientY>=r.top&&p.clientY<=r.bottom){
      held.container.scrollTop+=p.clientY<r.top+28?-8:p.clientY>r.bottom-28?8:0;preview(p);
    }
    held.scrollFrame=requestAnimationFrame(()=>scrollDrag(held));
  }
  function drag(e){
    const held=contact;if(!held||held.pointer!==e.pointerId)return;
    if(!held.node.isConnected||editing()||busy()||held.width!==root.getBoundingClientRect().width){cancelDrag();return;}
    const moved=Math.hypot(e.clientX-held.x,e.clientY-held.y)>6;
    if(!held.armed&&moved){suppress=true;cancelDrag();return;} // preserve touch/pen list scrolling
    if(!held.dragging&&moved){held.dragging=true;suppress=true;clearTimeout(held.timer);dismissRowMenu();held.node.setPointerCapture(e.pointerId);document.body.classList.add('drawing-dragging');if(held.vertical)held.scrollFrame=requestAnimationFrame(()=>scrollDrag(held));else startSlide(held);}
    if(!held.dragging)return;
    e.preventDefault();e.stopPropagation();
    held.position={clientX:e.clientX,clientY:e.clientY};preview(e);
  }
  function release(e,cancel=false){
    if(!contact||contact.pointer!==e.pointerId)return;
    const held=contact,drop=held.dragging&&!cancel?target(e):null;
    if(held.held){suppress=true;e.preventDefault();}
    if(cancel)dismissRowMenu();
    cancelDrag();
    if(held.dragging){e.preventDefault();e.stopPropagation();if(drop)invoke(()=>{applyChange(app.reorder_document(id(held.id),drop.before==null?undefined:id(drop.before)));refresh(true);});}
  }
  function bind(node,tabId,container,vertical=false,handle=false){
    node.addEventListener('pointerdown',e=>{
      if(e.button!==0||!e.isPrimary||contact||editing()||busy())return;
      suppress=false;dismissRowMenu();
      const immediate=!vertical||handle||e.pointerType==='mouse';
      contact={id:tabId,node,container,vertical,pointer:e.pointerId,pointerType:e.pointerType,x:e.clientX,y:e.clientY,width:root.getBoundingClientRect().width,armed:immediate,dragging:false};
      if(immediate){node.setPointerCapture(e.pointerId);e.preventDefault();node.focus({preventScroll:true});}
      else {const held=contact;held.timer=setTimeout(()=>{if(contact===held){held.armed=true;held.held=true;suppress=true;held.node.setPointerCapture(held.pointer);showRowMenu(tabId);}},450);}
      e.stopPropagation();
    });
    node.addEventListener('pointermove',drag);
    // Keep browser scrolling until the hold wins, then retain that contact.
    node.addEventListener('touchmove',e=>{if(contact?.node===node&&contact.held&&e.touches.length===1)e.preventDefault();},{passive:false});
    node.addEventListener('pointerup',e=>release(e));node.addEventListener('pointercancel',e=>release(e,true));
    node.addEventListener('lostpointercapture',()=>cancelDrag());
    node.addEventListener('click',e=>{e.stopPropagation();if(suppress||editing()||handle){suppress=false;e.preventDefault();return;}dismiss();invoke(()=>select(id(tabId)));});
    node.addEventListener('keydown',e=>{
      if(editing())return;
      if(e.shiftKey&&e.key==='F10'){e.preventDefault();vertical?showRowMenu(tabId):showSelector(tabId);return;}
      if(e.ctrlKey&&e.shiftKey&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){
        e.preventDefault();invoke(()=>{applyChange(app.step_document(id(tabId),['ArrowRight','ArrowDown'].includes(e.key)));refresh(true);});
      }else if(!vertical&&['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){
        e.preventDefault();const index=model.tabs.findIndex(t=>same(t.id,tabId));
        const next=e.key==='Home'?model.tabs[0]:e.key==='End'?model.tabs.at(-1):model.tabs[(index+(e.key==='ArrowRight'?1:model.tabs.length-1))%model.tabs.length];
        invoke(async()=>{await select(id(next.id));rows.get(String(next.id))?.select.focus();});
      }else if(e.key==='Delete'){e.preventDefault();dismiss();invoke(()=>close(id(tabId)));}
    });
    node.addEventListener('contextmenu',e=>{if(editing())return;e.preventDefault();
      // Native touch/pen context events must not replace the row owning the
      // held contact. Mouse context menus still use secondary click only.
      if(contact?.node===node&&contact.pointerType!=='mouse')return;
      cancelDrag();vertical?showRowMenu(tabId):showSelector(tabId);
    });
  }
  function dismiss(){cancelDrag();dismissRowMenu();if(popup){popup.close();popup=null;}}
  function showSelector(focusId){
    if(editing()||busy())return;dismiss();refresh();
    const dialog=element('dialog','drawing-list document-dialog');dialog.setAttribute('aria-label','Drawings');popup=dialog;
    const heading=element('header');heading.append(element('h2','','Drawings'),button('Done',dismiss));dialog.append(heading);
    const list=element('div','drawing-list-rows');list.setAttribute('role','list');dialog.append(list);
    for(const tab of model.tabs){
      const row=element('div','drawing-list-row');row.dataset.drawingId=tab.id;row.setAttribute('role','listitem');
      const grip=button('',()=>{});grip.className='drawing-grip';grip.setAttribute('aria-label',`Reorder ${tab.title}`);grip.append(icon('grip'));
      const pick=button('',()=>{});pick.className='drawing-list-pick';pick.append(element('strong','',`${tab.modified?'● ':''}${tab.title}`),element('small','',tab.location));pick.setAttribute('aria-current',String(same(tab.id,model.selected)));
      const remove=button('',()=>{dismiss();invoke(()=>close(id(tab.id)));});remove.append(icon('close-document'));remove.setAttribute('aria-label',`Close ${tab.title}`);
      row.append(grip,pick,remove);list.append(row);bind(grip,tab.id,list,true,true);bind(pick,tab.id,list,true);
    }
    const footer=element('footer');
    for(const [redo,label,enabled] of [[false,'Undo Reorder',model.can_undo],[true,'Redo Reorder',model.can_redo]]){const b=button(label,()=>{applyChange(app.document_order_history(redo));dismiss();showSelector(focusId);});b.disabled=!enabled;footer.append(b);}
    dialog.append(footer);dialog.addEventListener('close',()=>{if(popup===dialog)popup=null;dialog.remove();dismissRowMenu();cancelDrag();});
    document.body.append(dialog);dialog.showModal();
    [...list.children].find(n=>same(n.dataset.drawingId,focusId??model.selected))?.querySelector('.drawing-list-pick').focus();
  }
  function refresh(force=false){
    const width=root.getBoundingClientRect().width;model=app.document_tabs(width);
    const key=JSON.stringify(model,(_,v)=>typeof v==='bigint'?String(v):v);if(!force&&key===signature)return;signature=key;
    if(contact){const present=model.tabs.some(t=>same(t.id,contact.id));if(!present||model.compact!==strip.hidden&&model.tabs.length>1)cancelDrag();}
    plain.hidden=model.tabs.length>1;strip.hidden=model.tabs.length<2||model.compact;compact.hidden=model.tabs.length<2||!model.compact;
    const selected=model.tabs.find(t=>same(t.id,model.selected));compact.textContent=`${selected?.modified?'● ':''}${selected?.title??'Drawing'} ▾`;
    root.dataset.count=model.tabs.length;
    for(const [key,row]of rows)if(!model.tabs.some(t=>same(t.id,key))){row.root.remove();rows.delete(key);}
    for(const [i,tab]of model.tabs.entries()){
      const key=String(tab.id);let row=rows.get(key);
      if(!row){const node=element('div','drawing-tab');node.dataset.drawingId=key;const pick=button('',()=>{}),remove=button('',()=>invoke(()=>close(id(tab.id))));
        pick.setAttribute('role','tab');pick.className='drawing-tab-pick';remove.className='drawing-tab-close';remove.append(icon('close-document'));
        remove.addEventListener('pointerdown',e=>e.stopPropagation());remove.addEventListener('click',e=>e.stopPropagation());
        node.append(pick,remove);row={root:node,select:pick,close:remove};rows.set(key,row);bind(pick,tab.id,strip);}
      row.select.textContent=`${tab.modified?'● ':''}${tab.title}`;row.select.title=`${tab.title}\n${tab.location}`;
      row.select.setAttribute('aria-selected',String(same(tab.id,model.selected)));row.select.tabIndex=same(tab.id,model.selected)?0:-1;
      row.close.setAttribute('aria-label',`Close ${tab.title}`);row.close.disabled=busy();
      if(strip.children[i]!==row.root)strip.insertBefore(row.root,strip.children[i]??null);
    }
    if(popup){
      const list=popup.querySelector('.drawing-list-rows');
      for(const [i,tab]of model.tabs.entries()){
        const row=[...list.children].find(n=>same(n.dataset.drawingId,tab.id));
        if(row&&list.children[i]!==row)list.insertBefore(row,list.children[i]??null);
      }
      const actions=popup.querySelectorAll('footer button');actions[0].disabled=!model.can_undo;actions[1].disabled=!model.can_redo;
    }
  }
  const observer=new ResizeObserver(()=>{cancelDrag();refresh();});observer.observe(root);
  window.addEventListener('blur',()=>{cancelDrag();dismissRowMenu();});document.addEventListener('visibilitychange',()=>{if(document.hidden){cancelDrag();dismissRowMenu();}});
  window.addEventListener('keydown',e=>{if(e.key==='Escape'&&(contact||rowMenu)){e.preventDefault();cancelDrag();dismissRowMenu();}});
  root.addEventListener('dragover',e=>{if([...e.dataTransfer.types].includes('Files')&&!editing()){e.preventDefault();e.stopPropagation();e.dataTransfer.dropEffect='copy';}});
  root.addEventListener('drop',e=>{if(editing())return;e.preventDefault();e.stopPropagation();const files=[...e.dataTransfer.files];if(files.length)invoke(()=>openFiles(files));});
  return {root,refresh,showSelector,cancel:cancelDrag,key(e){
    if(editing()||e.target.closest('input,textarea,select,[contenteditable=true]')||document.querySelector('dialog[open]'))return false;
    if(e.altKey&&!e.ctrlKey&&['PageUp','PageDown'].includes(e.key)){e.preventDefault();invoke(()=>select(app.adjacent_document(e.key==='PageDown')));return true;}
    if(e.ctrlKey&&e.altKey&&e.code==='KeyD'){e.preventDefault();showSelector();return true;}
    if(e.ctrlKey&&e.altKey&&e.code==='KeyW'){e.preventDefault();invoke(()=>close(id(model.selected)));return true;}
    if(root.contains(e.target)&&e.ctrlKey&&e.code==='KeyZ'){e.preventDefault();invoke(()=>{applyChange(app.document_order_history(e.shiftKey));refresh(true);});return true;}
    return false;
  }};
}
