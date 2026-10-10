import {composingKey} from './text-input.js';
import {gestureNumberField} from './numeric.js';

export function createCurveEditor({element,button,icon,numberField,dispatch,target,initial,label,enabled=()=>true,histogram,footer,reset=false}) {
  const svg=(tag,attrs={})=>{const e=document.createElementNS('http://www.w3.org/2000/svg',tag);for(const [k,v] of Object.entries(attrs))e.setAttribute(k,v);return e;};
  let control=initial,drag,held,pressCount,clickCount,width=200,height=200;
  const send=action=>dispatch({type:'curve_editor',target,action});
  const node=element('div','curve-field'),frame=element('div','curve-frame'),plot=element('div','curve-plot');
  const graph=svg('svg',{viewBox:'0 0 200 200',preserveAspectRatio:'none',class:'curve-editor',role:'group',tabindex:0});
  const grid=svg('path',{stroke:'currentColor',opacity:.2}),white=svg('path',{fill:'none',stroke:'currentColor','stroke-dasharray':'3 3',opacity:.7});
  const polygon=svg('path',{fill:'none',stroke:'currentColor','stroke-dasharray':'3 3',opacity:.35});
  const path=svg('path',{fill:'none',stroke:'currentColor','stroke-width':1.5}),points=svg('g'),marker=svg('circle',{r:4,fill:'var(--accent)',stroke:'currentColor'});
  graph.append(grid,white,polygon,path,points,marker);
  const resetButton=reset?button('',()=>send({kind:'reset'})):null;
  if(resetButton){resetButton.append(icon('reset'));resetButton.dataset.action='curve-reset';}
  if(histogram)plot.append(histogram.node);
  plot.append(graph);if(resetButton)plot.append(resetButton);
  const vertical=element('div','curve-axis curve-axis-y'),horizontal=element('div','curve-axis curve-axis-x');
  for(const axis of [vertical,horizontal])axis.append(...Array.from({length:3},()=>element('span')));
  vertical.children[1].className='curve-axis-label';horizontal.children[1].className='curve-axis-label';
  frame.append(vertical,plot,element('span'),horizontal);node.append(frame);
  const coordinates=[];
  if(initial.controls.coordinate_readouts){
    const row=element('div','curve-coordinates');node.append(row);
    for(const [index,axis] of ['input','output'].entries()){
      const number=gestureNumberField({numberField,numeric:initial.controls.numeric,label:initial.controls.axes[index].label,
        request:()=>({kind:'number',epoch:control.controls.epoch,axis}),send,gesture:(phase,action)=>({kind:'gesture',phase,action}),valueOnly:true});
      number.dataset.curveAxis=axis;
      const ev=element('div','curve-ev'),cell=element('div','curve-coordinate'),caption=element('label','property-row');
      caption.append(element('span','',initial.controls.axes[index].label),number);cell.append(caption,ev);row.append(cell);coordinates.push({axis,number,ev,caption});
    }
  }
  if(footer)node.append(footer.node);
  const current=()=>({epoch:control.controls.epoch});
  const geometry=()=>{const rect=graph.getBoundingClientRect(),inset=control.controls.inset;return {rect,inset,extent:[Math.max(1,rect.width-2*inset),Math.max(1,rect.height-2*inset)]};};
  const position=(e,g)=>[e.clientX-g.rect.left-g.inset,e.clientY-g.rect.top-g.inset];
  const contact=(phase,e)=>{if(drag)send({kind:'contact',...drag.owner,phase,point:e?position(e,drag.geometry):[0,0],extent:drag.geometry.extent});};
  const cancel=()=>{const owner=drag?.owner??held;drag=null;held=null;if(owner)send({kind:'contact',epoch:owner.epoch,phase:'cancel',point:[0,0],extent:[1,1]});};
  graph.onpointerdown=e=>{if(e.button)return;e.preventDefault();e.stopPropagation();graph.focus({preventScroll:true});cancel();pressCount=control.points.length;
    drag={id:e.pointerId,geometry:geometry(),owner:current()};graph.setPointerCapture(e.pointerId);contact('down',e);};
  graph.onpointermove=e=>{if(drag?.id===e.pointerId){e.preventDefault();contact('move',e);}};
  graph.onpointerup=e=>{if(drag?.id===e.pointerId){contact('up',e);drag=null;graph.releasePointerCapture(e.pointerId);}};
  graph.onpointercancel=graph.onlostpointercapture=cancel;
  const remove=(e,point_count=null)=>{const g=geometry();send({kind:'remove_at',...current(),point:position(e,g),extent:g.extent,point_count});};
  graph.onclick=e=>{if(e.detail===1)clickCount=pressCount;};
  graph.ondblclick=e=>{e.preventDefault();e.stopPropagation();remove(e,clickCount);};
  graph.oncontextmenu=e=>{e.preventDefault();e.stopPropagation();cancel();remove(e);};
  const keyEvent=(e,pressed)=>{
    if(composingKey(e)||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Delete','Backspace','Escape'].includes(e.key))return;
    if(pressed&&(e.ctrlKey||e.metaKey||e.altKey))return;
    e.preventDefault();e.stopPropagation();const owner=held?.key_event===e.key?held:current();
    if(pressed)held={...owner,key_event:e.key};
    send({kind:'key',epoch:owner.epoch,key_event:e.key,pressed,repeat:e.repeat,modifiers:{command:e.ctrlKey||e.metaKey,shift:e.shiftKey,alt:e.altKey}});
    if((!pressed&&held?.key_event===e.key)||e.key==='Escape'){held=null;if(e.key==='Escape')drag=null;}
  };
  graph.onkeydown=e=>keyEvent(e,true);graph.onkeyup=e=>keyEvent(e,false);graph.onblur=cancel;window.addEventListener('blur',cancel);
  function draw(){
    const curve=control.controls,i=curve.inset,w=Math.max(1,width-2*i),h=Math.max(1,height-2*i);
    const line=values=>values.map(([x,y],n)=>`${n?'L':'M'}${i+x*w} ${i+(1-y)*h}`).join(' ');
    graph.setAttribute('viewBox',`0 0 ${width} ${height}`);
    grid.setAttribute('d',[.25,.5,.75].map(p=>`M${i+p*w} ${i}v${h}M${i} ${i+p*h}h${w}`).join(' '));
    const [x,y]=curve.axes.map(a=>a.white);white.setAttribute('d',x==null?'':`M${i+w*x} ${i}v${h}M${i} ${i+h-h*y}h${w}`);
    path.setAttribute('d',line(control.plot));polygon.setAttribute('d',curve.control_polygon?line(control.points):'');
    points.replaceChildren(...control.points.map(([x,y],index)=>svg('circle',{cx:i+x*w,cy:i+(1-y)*h,r:BigInt(index)===curve.selected?5:3.5,fill:BigInt(index)===curve.selected?'none':'currentColor',stroke:'currentColor','stroke-width':1.5})));
    marker.hidden=!control.marker;marker.style.display=control.marker?'':'none';if(control.marker){marker.setAttribute('cx',i+control.marker[0]*w);marker.setAttribute('cy',i+(1-control.marker[1])*h);}
  }
  const observer=new ResizeObserver(()=>{const r=graph.getBoundingClientRect();if(r.width&&r.height){width=r.width;height=r.height;draw();}});observer.observe(graph);
  function update(c,caption=label){
    control=c;label=caption;histogram?.refresh();footer?.refresh();const curve=c.controls;
    node.title=curve.help;graph.setAttribute('aria-label',caption);node.classList.toggle('curve-without-readouts',!curve.coordinate_readouts);
    if(resetButton){resetButton.hidden=!c.modified;resetButton.title=curve.reset_label;}
    curve.axes.forEach((axis,index)=>{const row=index?vertical:horizontal;[...row.children].forEach((n,j)=>n.textContent=(index?[axis.maximum,axis.label,axis.minimum]:[axis.minimum,axis.label,axis.maximum])[j]);});
    for(const [index,{axis,number,ev,caption}] of coordinates.entries()){
      const value=curve[axis];number.relabel(curve.axes[index].label);caption.firstChild.textContent=curve.axes[index].label;
      number.update(value?.value??0,value?.text??'');number.setDisabled(!enabled()||!value||value.read_only);ev.hidden=curve.domain.kind!=='log_hdr';ev.textContent=value?.ev??'';
    }
    draw();
  }
  update(initial);
  return {node,graph,update,refreshHistogram(){histogram?.refresh();footer?.refresh();},dispose(){observer.disconnect();histogram?.dispose();window.removeEventListener('blur',cancel);cancel();coordinates.forEach(({number})=>number.dispose());}};
}
