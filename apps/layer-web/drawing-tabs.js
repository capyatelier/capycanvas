// DOM input/measurement only. Drawing identity, order, drop validation, width
// policy and undo belong to Rust; this is separate from docked panel tabs.
export function createDrawingTabs({app,element,button,icon,applyChange,select,close,openFiles,message,busy}) {
  const root=element('div','drawing-title'),plain=document.querySelector('#document-title');
  root.id='drawing-title';plain.replaceWith(root);root.append(plain);
  const strip=element('div','drawing-tabs');strip.setAttribute('role','tablist');strip.setAttribute('aria-label','Drawings');
  const compact=button('',()=>showSelector(),'drawing-selector');compact.setAttribute('aria-label','Select drawing');
  root.append(strip,compact);
  const rows=new Map();let model,signature='',popup=null,contact=null,suppress=false;
  const same=(a,b)=>String(a)===String(b);
  const id=value=>BigInt(value);
  const fail=error=>message(String(error));
  const invoke=fn=>{try{Promise.resolve(fn()).catch(fail);}catch(error){fail(error);}};
  const editing=()=>app.state().customization.header_editing;
  function cancelDrag(){
    const held=contact;contact=null;
    if(held){clearTimeout(held.timer);if(held.node.hasPointerCapture(held.pointer))held.node.releasePointerCapture(held.pointer);}
    for(const n of document.querySelectorAll('[data-drawing-drop]'))n.removeAttribute('data-drawing-drop');
    document.body.classList.remove('drawing-dragging');
  }
  function hits(container){return [...container.children].filter(n=>n.dataset.drawingId).map(n=>{const r=n.getBoundingClientRect();return{id:id(n.dataset.drawingId),bounds:{x:r.x,y:r.y,width:r.width,height:r.height}};});}
  function target(e){return app.document_drop(hits(contact.container),e.clientX,e.clientY,contact.vertical);}
  function drag(e){
    const held=contact;if(!held||held.pointer!==e.pointerId)return;
    if(!held.node.isConnected||editing()||busy()||held.width!==root.getBoundingClientRect().width){cancelDrag();return;}
    const moved=Math.hypot(e.clientX-held.x,e.clientY-held.y)>6;
    if(!held.armed&&moved){cancelDrag();return;} // preserve touch/pen list scrolling
    if(!held.dragging&&moved){held.dragging=true;suppress=true;clearTimeout(held.timer);held.node.setPointerCapture(e.pointerId);document.body.classList.add('drawing-dragging');}
    if(!held.dragging)return;
    e.preventDefault();e.stopPropagation();
    const drop=target(e);
    for(const node of held.container.children)node.removeAttribute('data-drawing-drop');
    if(drop){const row=[...held.container.children].find(n=>same(n.dataset.drawingId,drop.before))??held.container.lastElementChild;row?.setAttribute('data-drawing-drop',drop.before==null?'after':'before');}
  }
  function release(e,cancel=false){
    if(!contact||contact.pointer!==e.pointerId)return;
    const held=contact,drop=held.dragging&&!cancel?target(e):null;
    cancelDrag();
    if(held.dragging){e.preventDefault();e.stopPropagation();if(drop)invoke(()=>{applyChange(app.reorder_document(id(held.id),drop.before==null?undefined:id(drop.before)));refresh(true);});}
  }
  function bind(node,tabId,container,vertical=false,handle=false){
    node.addEventListener('pointerdown',e=>{
      if(e.button!==0||!e.isPrimary||contact||editing()||busy())return;
      suppress=false;
      const immediate=!vertical||handle||e.pointerType==='mouse';
      contact={id:tabId,node,container,vertical,pointer:e.pointerId,x:e.clientX,y:e.clientY,width:root.getBoundingClientRect().width,armed:immediate,dragging:false};
      if(immediate){node.setPointerCapture(e.pointerId);e.preventDefault();node.focus({preventScroll:true});}
      else {const held=contact;held.timer=setTimeout(()=>{if(contact===held)held.armed=true;},450);}
      e.stopPropagation();
    });
    node.addEventListener('pointermove',drag);
    node.addEventListener('pointerup',e=>release(e));node.addEventListener('pointercancel',e=>release(e,true));
    node.addEventListener('lostpointercapture',()=>cancelDrag());
    node.addEventListener('click',e=>{e.stopPropagation();if(suppress||editing()||handle){suppress=false;e.preventDefault();return;}dismiss();invoke(()=>select(id(tabId)));});
    node.addEventListener('keydown',e=>{
      if(editing())return;
      if(e.ctrlKey&&e.shiftKey&&['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key)){
        e.preventDefault();invoke(()=>{applyChange(app.step_document(id(tabId),['ArrowRight','ArrowDown'].includes(e.key)));refresh(true);});
      }else if(!vertical&&['ArrowLeft','ArrowRight','Home','End'].includes(e.key)){
        e.preventDefault();const index=model.tabs.findIndex(t=>same(t.id,tabId));
        const next=e.key==='Home'?model.tabs[0]:e.key==='End'?model.tabs.at(-1):model.tabs[(index+(e.key==='ArrowRight'?1:model.tabs.length-1))%model.tabs.length];
        invoke(async()=>{await select(id(next.id));rows.get(String(next.id))?.select.focus();});
      }else if(e.key==='Delete'){e.preventDefault();dismiss();invoke(()=>close(id(tabId)));}
    });
    node.addEventListener('contextmenu',e=>{if(editing())return;e.preventDefault();cancelDrag();showSelector(tabId);});
  }
  function dismiss(){cancelDrag();if(popup){popup.close();popup=null;}}
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
    dialog.append(footer);dialog.addEventListener('close',()=>{if(popup===dialog)popup=null;dialog.remove();cancelDrag();});
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
  window.addEventListener('blur',cancelDrag);document.addEventListener('visibilitychange',()=>{if(document.hidden)cancelDrag();});
  window.addEventListener('keydown',e=>{if(e.key==='Escape')cancelDrag();});
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
