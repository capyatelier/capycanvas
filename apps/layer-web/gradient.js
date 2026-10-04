import {colorButton} from './color-controls.js';
import {createNumberField} from './numeric.js';
import {liveCopy,bindCopy} from './localization.js';
import {composingKey} from './text-input.js';

const key=value=>JSON.stringify(value,(_,v)=>typeof v==='bigint'?String(v):v);

export function gradientPreview({app,element}) {
  const canvas=element('canvas','gradient-preview');
  let gradient,painted,frame=0;
  const schedule=()=>{if(!frame)frame=requestAnimationFrame(()=>{frame=0;paint();});};
  function paint() {
    if(!gradient||!canvas.isConnected||!canvas.clientWidth||!canvas.clientHeight)return;
    const size=[Math.min(2048,Math.max(1,Math.round(canvas.clientWidth*devicePixelRatio))),Math.min(64,Math.max(1,Math.round(canvas.clientHeight*devicePixelRatio)))];
    const document=app.document_color(),panel=app.color_panel(),next=key([gradient,document,panel.rendition,size]);
    if(next===painted)return;
    const image=app.color_ui({type:'gradient',gradient,document_space:document.space,rendition:panel.rendition,image:{size,depth:document.depth}});
    const pixels=new Uint8ClampedArray(image.argb.length*4);
    image.argb.forEach((pixel,i)=>{const v=Number(pixel);pixels.set([v>>>16&255,v>>>8&255,v&255,v>>>24],i*4);});
    canvas.width=size[0];canvas.height=size[1];
    canvas.getContext('2d',{willReadFrequently:true}).putImageData(new ImageData(pixels,...size),0,0);painted=next;
  }
  const resize=new ResizeObserver(schedule);resize.observe(canvas);
  return {node:canvas,update(value){gradient=value;schedule();},dispose(){resize.disconnect();cancelAnimationFrame(frame);}};
}

export function gradientEditor({app,element,button,icon,dispatch}) {
  const copy=liveCopy(app,'catalog').native_copy.color;
  const node=element('div','gradient-editor'),top=element('div','gradient-controls'),strip=element('div','gradient-strip'),bar=element('div','gradient-ramp'),stopsRow=element('div','gradient-stops'),bottom=element('div','gradient-controls');
  const interpolation=element('select');interpolation.dataset.gradientInterpolation='';
  const preview=gradientPreview({app,element});bar.append(preview.node);strip.append(bar,stopsRow);strip.tabIndex=0;strip.style.touchAction='none';
  let control,ownerKey,selected=0,disabled=false,contact,held,numericOwner,disposed=false;
  const target=()=>control.gradient.destination;
  const stops=()=>control.value.value.stops;
  const send=(edit,phase,owner=target())=>{const action={op:'gradient',target:owner,edit};dispatch({type:'effect',action:phase?{op:'gesture',phase,action}:action});};
  const stopEdit=(position,index=selected)=>({kind:'stop',index,position,color:null,remove:false});
  function cancel() {
    const owner=contact?.owner??held?.owner;contact=null;held=null;
    if(owner)send({kind:'reset'},'cancel',owner);
  }
  function select(index) {if(index!==selected){position.cancelEditing();selected=index;}update(control);}
  const actionButton=(glyph,label,run,name)=>{
    const b=button('',run);b.append(icon(glyph));b.dataset.gradientAction=name;bindCopy(b,label,'title');bindCopy(b,label,'ariaLabel');return b;
  };
  const reverse=actionButton('flip-horizontal',()=>control?.gradient.reverse_label??'',()=>{selected=stops().length-1-selected;send({kind:'reverse'});},'reverse');
  const reset=actionButton('reset',()=>copy.reset_gradient,()=>send({kind:'reset'}),'reset');
  const remove=actionButton('minus',()=>copy.remove_stop,()=>{const index=selected;selected=Math.max(0,index-1);send({...stopEdit(0,index),remove:true});},'remove');
  const bucket=actionButton('fill',()=>copy.use_selected,()=>send({kind:'use_current_color',index:selected}),'use-color');
  const color=colorButton({app,label:()=>copy.color,element,button,current:()=>key([target(),selected,control.value.value]),change:value=>send({...stopEdit(stops()[selected].position),color:value})});
  const position=createNumberField({control:app.catalog().opacity,label:copy.position,labels:label=>app.numeric_labels(label),icon,inline:true,valueOnly:true,resolve:request=>app.number_input(request),errorCaption:reason=>app.native_caption({type:'numeric_error',reason}),
    onChange:value=>send({kind:'position',index:numericOwner?.index??selected,operation:{type:'value',value}},numericOwner?'move':null,numericOwner?.target??target())});
  bindCopy(position,()=>copy.position,'title');position.dataset.gradientPosition='';
  position.onEditPhase=phase=>{if(phase==='down')numericOwner={target:target(),index:selected};const owner=numericOwner;if(owner)send({kind:'position',index:owner.index,operation:{type:'step',steps:0}},phase,owner.target);if(phase==='up'||phase==='cancel')numericOwner=null;};
  interpolation.onchange=()=>send({kind:'interpolation',value:interpolation.value});
  top.append(interpolation,reverse,reset);bottom.append(position,remove,color.node,bucket);node.append(top,strip,bottom);
  const buttons=[];
  function update(next) {
    if(!next||disposed)return;
    const destination=next.gradient.destination,nextOwner=key([app.state().document_file.epoch,destination.kind,destination.layer,destination.key]);
    if(ownerKey!==nextOwner){cancel();position.cancelEditing();selected=0;ownerKey=nextOwner;}
    control=next;selected=Math.min(selected,stops().length-1);
    position.relabel(copy.position);
    const modes=control.gradient.interpolations;
    if(key([...interpolation.options].map(o=>[o.value,o.textContent]))!==key(modes))interpolation.replaceChildren(...modes.map(([value,label])=>{const o=element('option','',label);o.value=value;return o;}));
    interpolation.value=control.value.value.interpolation;interpolation.title=control.gradient.interpolation_label;interpolation.setAttribute('aria-label',interpolation.title);
    while(buttons.length>stops().length)buttons.pop().remove();
    while(buttons.length<stops().length){const i=buttons.length,b=button('',()=>select(i));b.dataset.gradientStop=String(i);buttons.push(b);stopsRow.append(b);}
    for(const [i,stop] of stops().entries()){const b=buttons[i];b.style.left=`${stop.position*100}%`;b.classList.toggle('selected',i===selected);b.disabled=disabled;b.setAttribute('aria-label',`${copy.color} ${i+1}`);b.setAttribute('aria-pressed',i===selected);}
    const stop=stops()[selected],interior=selected>0&&selected<stops().length-1;
    position.update(stop.position);position.setDisabled(disabled||!interior);color.update(stop.color);color.disable(disabled);remove.disabled=disabled||!interior;
    for(const b of [interpolation,reverse,reset,bucket])b.disabled=disabled;
    strip.setAttribute('aria-label',copy.add_stop);strip.title=copy.add_stop;preview.update(control.value.value);
  }
  strip.addEventListener('pointerdown',e=>{
    if(e.button||disabled||contact)return;e.preventDefault();e.stopPropagation();cancel();
    const rect=bar.getBoundingClientRect(),p=Math.max(0,Math.min(1,(e.clientX-rect.left)/rect.width));
    const at=e.target.closest('[data-gradient-stop]');let index=at?Number(at.dataset.gradientStop):stops().findIndex(s=>Math.abs(s.position-p)*rect.width<8);
    if(index<0&&!control.gradient.can_add)return;
    const existing=index<0?null:index;index=existing??stops().filter(s=>s.position<p).length;
    select(Math.min(index,stops().length-1));selected=index;strip.focus();strip.setPointerCapture(e.pointerId);
    contact={id:e.pointerId,owner:target(),index,x:e.clientX,width:rect.width,position:existing==null?p:stops()[index].position};
    send(stopEdit(contact.position,existing),'down',contact.owner);
  });
  const move=e=>{if(contact?.id!==e.pointerId)return;const c=contact;send(stopEdit(Math.max(0,Math.min(1,c.position+(e.clientX-c.x)/c.width)),c.index),'move',c.owner);};
  strip.addEventListener('pointermove',move);
  strip.addEventListener('pointerup',e=>{if(contact?.id!==e.pointerId)return;move(e);const c=contact;contact=null;send({kind:'position',index:c.index,operation:{type:'step',steps:0}},'up',c.owner);strip.releasePointerCapture(e.pointerId);});
  for(const type of ['pointercancel','lostpointercapture'])strip.addEventListener(type,cancel);
  strip.addEventListener('keydown',e=>{
    if(composingKey(e)||disabled)return;
    if(e.key==='Escape'&&(contact||held)){e.preventDefault();e.stopPropagation();cancel();}
    else if(['Delete','Backspace'].includes(e.key)){e.preventDefault();cancel();remove.click();}
    else if(['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();e.stopPropagation();const phase=held?'move':'down';held??={owner:target(),index:selected};send({kind:'position',index:held.index,operation:{type:'step',steps:(e.key==='ArrowLeft'?-1:1)*(e.shiftKey?10:1)}},phase,held.owner);}
  });
  strip.addEventListener('keyup',e=>{if(held&&['ArrowLeft','ArrowRight'].includes(e.key)){const h=held;held=null;send({kind:'position',index:h.index,operation:{type:'step',steps:0}},'up',h.owner);}});
  strip.addEventListener('blur',cancel);window.addEventListener('blur',cancel);
  return {node,update,disable(value){disabled=value;if(value)cancel();update(control);},dispose(){cancel();position.dispose();color.dispose();preview.dispose();window.removeEventListener('blur',cancel);disposed=true;}};
}

export function gradientButton(services,control,openPopup) {
  const {element,button}=services,row=element('div','toolbar-option toolbar-gradient');row.dataset.toolbarField='';
  const editor=gradientEditor(services),preview=gradientPreview(services),face=button('',()=>{openPopup(face,editor.node);editor.update(control);});
  face.append(preview.node);row.append(face);
  return {row,gradient:true,update(option){control=option.Gradient??option;face.title=control.label;face.setAttribute('aria-label',control.label);editor.update(control);preview.update(control.value.value);},dispose(){editor.dispose();preview.dispose();}};
}
