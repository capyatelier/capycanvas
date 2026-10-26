let nextSurface=0;

export function colorTouchInput({app,applyChange,wake}) {
  const surface=++nextSurface,contacts=new Map();
  let claimed=false;
  const send=(id,phase,position,time)=>{
    const reply=app.input({type:'color_touch',surface,id,phase,position,time_ns:Math.round(time*1e6)});
    applyChange(reply.change);wake();return reply.handled;
  };
  const cancel=()=>{
    for(const [id,position] of contacts)send(id,'cancel',position,performance.now());
    contacts.clear();claimed=false;
  };
  window.addEventListener('blur',cancel);
  return {
    route(event,inside) {
      if(event.pointerType!=='touch')return false;
      const phase={pointerdown:'down',pointermove:'move',pointerup:'up',pointercancel:'cancel'}[event.type];
      if(!phase||phase==='down'&&!inside||phase!=='down'&&!contacts.has(event.pointerId))return false;
      const position=[event.clientX*devicePixelRatio,event.clientY*devicePixelRatio];
      contacts.set(event.pointerId,position);
      claimed=send(event.pointerId,phase,position,event.timeStamp)||claimed;
      const handled=claimed;
      if(phase==='up'||phase==='cancel'){contacts.delete(event.pointerId);if(!contacts.size)claimed=false;}
      return handled;
    },
    dispose(){cancel();window.removeEventListener('blur',cancel);},
  };
}
