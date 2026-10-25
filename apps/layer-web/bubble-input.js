export function createBubbleInput({app, workspace, records, send, wake, menu, dismiss, cancelWorkspace, viewport,leaveCanvas}) {
  let contact, source, lastRing, suppressClick=false;
  const point=e=>[e.clientX,e.clientY];
  const distance=(a,b)=>Math.hypot(a[0]-b[0],a[1]-b[1]);
  const local=(record,p)=>p.map((v,i)=>v-record.center[i]);
  const consume=e=>{if(e.type!=='pointerdown'||e.pointerType!=='mouse')e.preventDefault();e.stopImmediatePropagation();return true;};
  const grab=(id,rim=false)=>({kind:rim?'rim':'bubble',id});
  const drag=(g,phase,p)=>{send({type:'drag',grab:g,phase,position:p,viewport:viewport()});wake();};
  const hit=(r,p)=>app.bubble_hit(r.view.id,...local(r,p));
  const native=e=>e.target.closest?.('.bubble-card :is(.layer-row,.layer-swipe-actions,.scroll-thumb,input,textarea,select,[contenteditable=true],.numeric-field,.range-control,.curve-editor)');
  function target(p) {
    for(const r of [...records.values()].reverse())if(r.center&&!r.faded&&hit(r,p)!=null)return r;
  }
  function stopHold(c=contact){if(c?.timer){clearTimeout(c.timer);c.timer=null;}}
  function restore(c) {
    if(c.mode==='native') {
      const event=new PointerEvent('pointercancel',{bubbles:true,pointerId:c.id,pointerType:'touch'});
      event.bubbleNativeCancel=true;c.target.dispatchEvent(event);
    }
    finishColor(c,true);
    if(['ring','beads','choose'].includes(c.mode))send({type:c.mode,id:c.r.view.id,phase:'cancel',position:[0,0]});
    if(c.mode==='moving')drag(c.grab,'cancel',c.last);
    c.mode='pending';
  }
  function pick(c,p) {
    send({type:c.mode==='intensity'?'intensity':'wheel',id:c.r.view.id,phase:c.colorStarted?'move':'down',part:c.wheel,position:local(c.r,p)});
    c.colorStarted=true;
  }
  function finishColor(c,cancel) {
    if(!c.colorStarted)return;
    c.colorStarted=false;
    send({type:c.mode==='intensity'?'intensity':'wheel',id:c.r.view.id,phase:cancel?'cancel':'up',part:c.wheel,position:local(c.r,c.last)});
  }
  function begin(c,p) {
    const h=c.hit,id=c.r.view.id;
    if(h==='wheel') {
      const size=c.r.view.diameter-14;
      c.wheel=app.color_ui({type:'wheel_hit',size,point:local(c.r,p).map(v=>v+size/2),shape:app.color_panel().shape});
      c.mode='wheel';if(c.wheel)pick(c,p);
    } else if(h==='intensity') {
      const repeated=c.device!=='mouse'&&lastRing?.kind===h&&lastRing.id===id&&c.time-lastRing.time<=400&&distance(p,lastRing.point)<=8;
      lastRing=repeated?null:{kind:h,id,time:c.time,point:p};c.mode=repeated?'reset':'intensity';
      if(repeated)send({type:'set_intensity',id,stops:0});else pick(c,p);
      c.r.color?.focus();
    } else if(h==='ring') {
      const repeated=c.device!=='mouse'&&lastRing?.kind===h&&lastRing.id===id&&c.time-lastRing.time<=400&&distance(p,lastRing.point)<=8;
      lastRing=repeated?null:{kind:h,id,time:c.time,point:p};
      c.mode=repeated?'reset':'ring';
      send({type:repeated?'reset_ring':'ring',id,...(repeated?{}:{phase:'down'}),position:local(c.r,p)});
    } else if(h==='bead'||typeof h==='object'&&h.item!=null) {
      c.mode=h==='bead'?'beads':'choose';
      send({type:c.mode,id,phase:'down',position:local(c.r,p)});
    } else c.mode='pending';
  }
  function hold(c) {
    if(contact!==c||c.pair)return;
    c.timer=null;lastRing=null;restore(c);c.held=true;menu(c.r,c.start);
  }
  function pairPoints(c) {
    const [a,b]=[...c.fingers.values()];
    return {mid:a.map((v,i)=>(v+b[i])/2),distance:distance(a,b)};
  }
  function down(e) {
    suppressClick=false;
    if(e.button!==0||e.target.closest?.('[popover]:popover-open,dialog[open],.popover'))return;
    const p=point(e);
    if(contact?.mode==='native'&&native(e))return;
    if(contact&&e.pointerType==='touch') {
      const c=contact;
      if(distance(local(c.r,p),[0,0])>c.r.packet.reach+90)return;
      c.fingers.set(e.pointerId,p);workspace.setPointerCapture(e.pointerId);
      if(c.fingers.size===2&&!c.pair) {
        stopHold();lastRing=null;restore(c);dismiss();
        const pair=pairPoints(c);c.pair={...pair,start:Math.max(1,pair.distance),scale:1,ended:false};
        c.grab=grab(c.r.view.id);drag(c.grab,'down',pair.mid);c.mode='pair';
      }
      return consume(e);
    }
    if(contact)return;
    const r=target(p);
    if(r) {
      if(e.target.closest?.('.bubble-pill'))return;
      const h=hit(r,p),c={r,hit:h,start:p,last:p,time:e.timeStamp,id:e.pointerId,device:e.pointerType,target:e.target,list:!!e.target.closest('.bubble-card .layer-list'),fingers:new Map([[e.pointerId,p]])};
      if(native(e)) {if(e.pointerType==='touch'){contact=c;c.mode='native';}return;}
      contact=c;begin(c,p);workspace.setPointerCapture(e.pointerId);
      if(h!=='card'||!e.target.closest('button'))c.timer=setTimeout(()=>hold(c),c.wheel||h==='intensity'?1000:500);r.mark?.(h,true);if(!c.list)return consume(e);return;
    }
    const node=e.target.closest?.('[data-header-item],[data-workspace-drag],.tool-choice-button');
    if(!node||node.disabled||document.querySelector('#header-editor')?.hidden===false)return;
    let g;
    if(node.bubbleChoice)g=app.bubble_grab({type:'choice',action:node.bubbleChoice});
    else {
      const item=node.dataset.headerItem?{kind:'header',id:Number(node.dataset.headerItem)}:JSON.parse(node.dataset.workspaceDrag||'{}').item;
      if(!['header','tile'].includes(item?.kind))return;
      g=app.bubble_grab({type:'grab',grab:item,held:false});
    }
    if(g)source={id:e.pointerId,grab:g,node,container:node.closest('#header,.content-drawer,.dock-group')||node.parentElement,start:p,last:p,held:false,moving:false};
  }
  function move(e) {
    const p=point(e);
    if(source?.id===e.pointerId) {
      const s=source;s.last=p;
      if(!s.moving) {
        if(s.held)return;
        if(!s.node.isConnected){source=null;return;}
        const b=s.container.getBoundingClientRect();
        if(p[0]>=b.left&&p[0]<=b.right&&p[1]>=b.top&&p[1]<=b.bottom)return;
        cancelWorkspace();
        const cancel=new PointerEvent('pointercancel',{bubbles:true,pointerId:e.pointerId,pointerType:e.pointerType});
        cancel.bubbleTransfer=true;s.node.dispatchEvent(cancel);
        dismiss();workspace.setPointerCapture(e.pointerId);s.moving=true;drag(s.grab,'down',p);
      }
      drag(s.grab,'move',p);return consume(e);
    }
    const c=contact;
    if(!c) {
      if(e.buttons)return;
      const r=target(p);
      for(const record of records.values())record.mark?.(record===r?hit(record,p):null,false);
      workspace.style.setProperty('--bubble-pointer',r&&e.pointerType==='pen'?'none':'');
      if(r&&!native(e)){leaveCanvas();workspace.classList.add('bubble-pointer');if(e.pointerType==='pen')workspace.style.cursor='none';else workspace.style.cursor='default';return consume(e);}
      workspace.classList.remove('bubble-pointer');
      workspace.style.cursor='';return;
    }
    if(!c.fingers.has(e.pointerId))return;
    c.fingers.set(e.pointerId,p);c.last=p;
    if(c.mode==='native')return;
    if(c.pair) {
      if(!c.pair.ended) {
        const pair=pairPoints(c);c.pair.mid=pair.mid;
        if(c.r.view.kind==='color') {
          c.pair.scale=app.bubble_pinched_size(c.r.view.diameter,pair.distance/c.pair.start)/c.r.view.diameter;
          c.r.pinch=c.pair.scale;
        }
        drag(c.grab,'move',pair.mid);
      }
      return consume(e);
    }
    const far=distance(c.start,p)>8;
    if(far){stopHold();lastRing=null;}
    if(c.held) {
      if(!far)return consume(e);
      c.held=false;dismiss();
      if(c.hit!=='wheel')begin(c,p);else c.mode='wheel';
    }
    if(c.list)return;
    if(c.mode==='intensity')pick(c,p);
    else if(c.mode==='wheel') {
      if(app.bubble_color_drag_out(c.r.view.diameter,...local(c.r,p))) {
        restore(c);c.grab=grab(c.r.view.id,true);drag(c.grab,'down',p);c.mode='moving';
      } else if(c.wheel)pick(c,p);
    } else if(c.mode==='pending'&&far) {
      c.grab=grab(c.r.view.id);drag(c.grab,'down',c.start);c.mode='moving';
    }
    if(c.mode==='moving')drag(c.grab,'move',p);
    else if(['ring','beads','choose'].includes(c.mode))send({type:c.mode,id:c.r.view.id,phase:'move',position:local(c.r,p)});
    c.r.mark?.(hit(c.r,p),true);return consume(e);
  }
  function end(e,cancel=false) {
    if(e?.bubbleScroll||e?.bubbleNativeCancel||e?.bubbleTransfer)return;
    const p=e?point(e):contact?.last||source?.last;
    if(source&&(!e||source.id===e.pointerId)) {
      const s=source;source=null;
      if(s.moving){drag(s.grab,cancel?'cancel':'up',p);suppressClick=true;if(e)consume(e);}
    }
    const c=contact;if(!c||e&&!c.fingers.has(e.pointerId))return;
    if(c.mode==='native'){contact=null;return;}
    stopHold();if(cancel)lastRing=null;
    if(c.pair) {
      if(!c.pair.ended) {
        c.pair.ended=true;c.r.pinch=1;drag(c.grab,cancel?'cancel':'up',c.pair.mid);
        if(!cancel&&c.r.view.kind==='color'&&Math.abs(c.pair.scale-1)>.01)send({type:'resize',id:c.r.view.id,scale:c.pair.scale});
      }
    } else if(cancel){restore(c);if(c.held)dismiss();}
    else if(!c.held) {
      if(c.mode==='moving')drag(c.grab,'up',p);
      else if(['ring','beads','choose'].includes(c.mode))send({type:c.mode,id:c.r.view.id,phase:'up',position:local(c.r,p)});
      else if(c.mode==='pending') {
        if(c.hit==='card')c.target.closest('button')?.click();
        else send({type:'tap',id:c.r.view.id,position:local(c.r,p)});
      }
    }
    finishColor(c,false);
    if(e)c.fingers.delete(e.pointerId);else c.fingers.clear();
    if(!c.fingers.size){contact=null;c.r.mark?.(null,false);}
    suppressClick=true;wake();if(e)return consume(e);
  }
  workspace.addEventListener('workspace-drag-held',()=>{if(source)source.held=true;});
  workspace.addEventListener('workspace-context-claimed',()=>{if(source)source.held=true;});
  window.addEventListener('blur',()=>end(null,true));
  window.addEventListener('keydown',e=>{if(e.key==='Escape'&&(contact||source)){end(null,true);consume(e);}},true);
  return {
    handle(e) {
      if(e.type==='pointerdown')return down(e);
      if(e.type==='pointermove')return move(e);
      if(e.type==='pointerup')return end(e);
      if(e.type==='pointercancel')return end(e,true);
      if(e.type==='lostpointercapture'&&e.target===workspace)return end(e,true);
      if(e.type==='click'&&suppressClick){suppressClick=false;return consume(e);}
      if(e.type==='contextmenu') {const r=target(point(e));if(r&&!native(e)){menu(r,point(e));return consume(e);}}
      if(e.type==='dblclick') {const r=target(point(e));if(r){const h=hit(r,point(e));if(h==='intensity'){send({type:'set_intensity',id:r.view.id,stops:0});return consume(e);}if(h==='ring'){send({type:'reset_ring',id:r.view.id,position:local(r,point(e))});return consume(e);}}}
    },
    validate(){if(contact&&!records.has(contact.r.view.id)){
      const c=contact;stopHold();contact=null;dismiss();
      for(const id of c.fingers.keys())if(workspace.hasPointerCapture(id))workspace.releasePointerCapture(id);
    }},
    turning:id=>contact?.r.view.id===id&&contact.mode==='beads'&&distance(contact.start,contact.last)>8,
    busy:()=>!!contact||!!source?.moving,
  };
}
