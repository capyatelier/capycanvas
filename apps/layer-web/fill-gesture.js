export function createFillGesture({workspace,canvas,element,glassChanged}) {
  const root=element('div','fill-gesture-overlay'),pill=element('div','number-control number-panel fill-gesture-pill');
  root.hidden=true;pill.hidden=true;root.setAttribute('aria-hidden','true');
  const header=element('div','number-header'),label=element('span','number-title number-labels'),value=element('span','number-value');
  const track=element('div','fill-gesture-track'),fill=element('div','fill-gesture-fill');
  header.append(label,value);track.append(fill);pill.append(header,track);
  const svg=tag=>document.createElementNS('http://www.w3.org/2000/svg',tag);
  const drawing=svg('svg'),seals=svg('path'),ring=svg('circle');
  drawing.classList.add('fill-gesture-lines');seals.classList.add('fill-gesture-seals');ring.classList.add('fill-gesture-ring');
  ring.setAttribute('pathLength','1');drawing.append(seals,ring);root.append(drawing,pill);workspace.append(root);
  let placement='',tick='';
  return {refresh(view){
    root.hidden=!view;canvas.classList.toggle('fill-gesture-resize',!!view?.resize_cursor);
    const next=view?.pill,bounds=next?.bounds,key=bounds?[bounds.x,bounds.y,bounds.width,bounds.height].join(','):'';
    pill.hidden=!next;
    if(key!==placement){
      placement=key;
      if(bounds)Object.assign(pill.style,{left:`${bounds.x}px`,top:`${bounds.y}px`,width:`${bounds.width}px`,height:`${bounds.height}px`});
      glassChanged();
    }
    if(!view)return;
    const nextTick=`${view.transaction}:${view.tick}`;
    if(tick!==nextTick){tick=nextTick;if(view.tick)navigator.vibrate?.(8);}
    const path=view.seals.map(([a,b])=>`M${a[0]} ${a[1]}L${b[0]} ${b[1]}`).join('');
    if(seals.getAttribute('d')!==path)seals.setAttribute('d',path);
    ring.style.display=view.ring==null?'none':'';
    ring.setAttribute('cx',view.tip[0]);ring.setAttribute('cy',view.tip[1]);ring.setAttribute('stroke-dasharray',`${view.ring??0} 1`);
    if(next){
      if(label.textContent!==next.label)label.textContent=next.label;
      if(value.textContent!==next.value)value.textContent=next.value;
      Object.assign(track.style,{left:`${next.track[0]-bounds.x}px`,width:`${next.track[1]-next.track[0]}px`});
      fill.style.width=`${next.fill_end-next.track[0]}px`;
    }
  }};
}
