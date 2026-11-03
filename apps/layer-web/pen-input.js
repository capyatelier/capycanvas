export function paintToolKind(event) {
  if (event.pointerType === 'touch') return 3;
  if (event.pointerType === 'pen') return event.buttons & 32 ? 2 : 0;
  return 1;
}

export function browserDeviceClass(coarse, screen) {
  return coarse ? Math.min(screen.width, screen.height) < 600 ? 'phone' : 'tablet' : 'desktop';
}

export function observePenInput({workspace,canvas,dispatch,position,owner=()=>undefined}) {
  let registered,registeredOwner;
  for(const [name,phase] of [['pointerdown',1],['pointermove',2],['pointerup',3],['pointercancel',4]]) {
    workspace.addEventListener(name,event=>{
      if(event.pointerType!=='pen')return;
      const utility=event.target===canvas?null:event.target.closest?.('#pen-pressure-dialog,#pen-tilt-dialog');
      if(!utility && event.target!==canvas)return;
      const id=event.pointerId>>>0,context=owner();
      if(!utility && registered===id && registeredOwner===context)return;
      const sample=utility?[...position(event),event.pressure,((event.tiltX||0)*Math.PI)/180,((event.tiltY||0)*Math.PI)/180,
        ((event.twist||0)*Math.PI)/180,0,Math.max(0,event.timeStamp)*1e6,phase===2 && !(event.buttons&33)?0:phase]:null;
      dispatch({type:'pen_input',device:{id:BigInt(id),key:'web:pen',label:'',native_to_surface:{reverse_horizontal:false,reverse_vertical:false,swap:false}},sample});
      registered=id;registeredOwner=context;
    },{capture:true});
  }
}
