let nextSurface=0;

export function layerTouchInput({list,rowAt,app,applyChange,wake,cancelNative}) {
  const surface=++nextSurface,contacts=new Map();
  let claimed=false,suppressUntil=0;
  const send=(id,phase,contact,time)=>{
    const reply=app.input({type:'layer_touch',surface,id,phase,position:contact.position,row:contact.row,time_ns:Math.round(time*1e6)});
    applyChange(reply.change);wake();return reply.handled;
  };
  const cancel=()=>{
    for(const [id,contact]of contacts)send(id,'cancel',contact,performance.now());
    contacts.clear();claimed=false;
  };
  const route=event=>{
    if(event.pointerType!=='touch')return;
    const phase={pointerdown:'down',pointermove:'move',pointerup:'up',pointercancel:'cancel'}[event.type];
    if(phase==='down'){
      const row=rowAt(event.target);
      if(row==null||event.target.closest('input'))return;
      contacts.set(event.pointerId,{row,position:[event.clientX*devicePixelRatio,event.clientY*devicePixelRatio]});
    }
    const contact=contacts.get(event.pointerId);
    if(!contact)return;
    contact.position=[event.clientX*devicePixelRatio,event.clientY*devicePixelRatio];
    const handled=send(event.pointerId,phase,contact,event.timeStamp);
    if(handled&&!claimed){claimed=true;cancelNative();for(const id of contacts.keys())try{list.setPointerCapture(id);}catch{}}
    if(claimed){event.preventDefault();event.stopImmediatePropagation();suppressUntil=performance.now()+400;}
    if(phase==='up'||phase==='cancel'){
      contacts.delete(event.pointerId);
      if(list.hasPointerCapture(event.pointerId))list.releasePointerCapture(event.pointerId);
      if(!contacts.size)claimed=false;
    }
  };
  const scroll=event=>{if(claimed&&event.cancelable)event.preventDefault();};
  const click=event=>{if(event.pointerType==='touch'&&performance.now()<suppressUntil){event.preventDefault();event.stopImmediatePropagation();}};
  const lost=event=>{if(contacts.has(event.pointerId)&&event.target===list)cancel();};
  for(const type of ['pointerdown','pointermove','pointerup','pointercancel'])list.addEventListener(type,route,true);
  list.addEventListener('touchmove',scroll,{capture:true,passive:false});
  list.addEventListener('click',click,true);list.addEventListener('lostpointercapture',lost,true);
  window.addEventListener('blur',cancel);
  return {surface,cancel,dispose(){cancel();for(const type of ['pointerdown','pointermove','pointerup','pointercancel'])list.removeEventListener(type,route,true);
    list.removeEventListener('touchmove',scroll,true);list.removeEventListener('click',click,true);list.removeEventListener('lostpointercapture',lost,true);window.removeEventListener('blur',cancel);}};
}
