import {createDialogHost} from './dialog-host.js';
import {createCurveEditor} from './curve-editor.js';
import {addScrollbar} from './scrollbars.js';

export function createPenResponse({app,state,workspace,element,button,icon,numberField,dispatch}) {
  let panel,host,editor,scroll,heading,device,close,firmer,lighter,sensitivityTitle,reset,cancel,apply,direction,circle,arrows,axes,drag,mode,measured,measureFrame,initialized=false;
  const viewport=()=>[workspace.clientWidth,workspace.clientHeight];
  const position=e=>{const r=workspace.getBoundingClientRect();return [e.clientX-r.left,e.clientY-r.top];};
  const model=()=>state()[`${mode}_calibration`];
  const send=action=>dispatch(mode==='tilt'?{type:'tilt_calibration',action:{kind:'panel',action}}:{type:'pressure_calibration',action});
  function dispose(){
    if(measureFrame)cancelAnimationFrame(measureFrame);
    measureFrame=null;
    scroll?.disposeScrollbar();scroll=null;
    const retired=editor;editor=null;retired?.dispose();host?.hide();host?.dispose();host=null;panel?.remove();panel=null;drag=null;measured=null;initialized=false;
  }
  function measure(){
    measureFrame=null;if(!panel)return;
    const body=panel.querySelector('.utility-body');
    const reserve=panel.querySelector('.curve-frame > span');
    panel.style.setProperty('--curve-gutter',`${reserve.getBoundingClientRect().width}px`);
    const r=panel.getBoundingClientRect(),v=viewport(),view=model();
    if(!view)return;
    const area=app.layout(...v).phone?.work_area??{width:v[0],height:v[1]},header=panel.querySelector('.utility-header'),footer=panel.querySelector('.utility-actions');
    const extent=app.calibration_extent([area.width,area.height],[r.width,body.scrollHeight+header.getBoundingClientRect().height+footer.getBoundingClientRect().height]);
    const key=[...extent,...v].join(',');
    if(key!==measured || Math.abs(view.bounds.width-extent[0])>.5 || Math.abs(view.bounds.height-extent[1])>.5){
      measured=key;send({kind:'measure',extent,viewport:v});
    } else if(!initialized){
      initialized=true;panel.style.visibility='';editor.graph.focus({preventScroll:true});
    }
  }
  function refresh(){
    const next=state().tilt_calibration?'tilt':state().pressure_calibration?'pressure':null;
    if(mode!==next){dispose();mode=next;}
    if(!next)return;
    const view=model(),prefix=`pen-${mode}`;
    if(!panel){
      const target={kind:mode},panelKind=mode;
      const act=action=>dispatch(panelKind==='tilt'?{type:'tilt_calibration',action:{kind:'panel',action}}:{type:'pressure_calibration',action});
      panel=element('dialog','dock-group floating-panel utility-panel');panel.id=`${prefix}-dialog`;
      panel.style.visibility='hidden';
      const header=element('div','dock-tabs utility-header'),title=element('div','utility-heading');
      heading=element('strong','utility-title');heading.id=`${prefix}-title`;panel.setAttribute('aria-labelledby',heading.id);
      device=element('span','utility-device');title.append(heading,device);
      close=button('',()=>act({kind:'cancel'}),'utility-close');close.id=`${prefix}-close`;close.append(icon('window-close'));header.append(title,close);
      const body=element('div','utility-body');
      editor=createCurveEditor({element,button,icon,numberField,dispatch,target,initial:view.editor,label:view.title});
      const sensitivity=element('div',mode==='tilt'?'selection-modes utility-sensitivity utility-compact-sensitivity':'utility-sensitivity'),actions=element('div','preference-footer separated utility-actions');
      firmer=button('',()=>act({kind:'sensitivity',lighter:false}));firmer.id=`${prefix}-firmer`;
      lighter=button('',()=>act({kind:'sensitivity',lighter:true}));lighter.id=`${prefix}-lighter`;sensitivity.append(firmer,lighter);
      if(mode==='tilt'){firmer.append(icon('minus'));lighter.append(icon('plus'));}
      reset=button('',()=>dispatch({type:'curve_editor',target,action:{kind:'reset'}}),'dialog-button');reset.id=`${prefix}-reset`;
      cancel=button('',()=>act({kind:'cancel'}),'dialog-button');cancel.id=`${prefix}-cancel`;
      apply=button('',()=>act({kind:'apply'}),'suggested-action');apply.id=`${prefix}-apply`;actions.append(reset,element('span'),cancel,apply);
      direction=element('section','utility-direction-section');direction.append(element('strong','utility-direction-title'));
      const controls=element('div','utility-controls-row');
      sensitivityTitle=element('span','utility-sensitivity-title');
      if(mode==='tilt'){const group=element('section','utility-sensitivity-section');group.append(sensitivityTitle,sensitivity);controls.append(group);}
      circle=document.createElementNS('http://www.w3.org/2000/svg','svg');circle.classList.add('utility-direction');circle.id='pen-tilt-direction';circle.setAttribute('viewBox','0 0 72 72');circle.setAttribute('role','img');
      circle.onpointerdown=e=>{e.preventDefault();e.stopPropagation();};
      const ring=document.createElementNS(circle.namespaceURI,'circle');ring.setAttribute('cx','36');ring.setAttribute('cy','36');ring.setAttribute('r','31');circle.append(ring);
      arrows=[false,true].map(adjusted=>{const path=document.createElementNS(circle.namespaceURI,'path');path.classList.add(adjusted?'adjusted':'input');circle.append(path);return path;});
      const bar=element('div','selection-modes utility-direction-controls');bar.setAttribute('role','group');
      axes=[['reverse_horizontal','reverse-horizontal','flip-horizontal'],['reverse_vertical','reverse-vertical','flip-vertical'],['swap','swap-axes','swap-orientation']].map(([key,id,glyph])=>{
        const control=button('',()=>dispatch({type:'tilt_calibration',action:{kind:'axes',axes:{...model().axes,[key]:!model().axes[key]}}}));
        control.id=`pen-tilt-${id}`;control.append(icon(glyph));bar.append(control);return control;
      });
      direction.append(bar);
      if(mode==='tilt'){editor.node.querySelector('.curve-plot').append(circle);controls.append(direction);body.append(editor.node,controls);}
      else body.append(editor.node,sensitivity);panel.append(header,body,actions);scroll=addScrollbar(body,panel);workspace.append(panel);
      host=createDialogHost({app,dialog:panel,header,cancel,primary:apply,icon,copy:app.bootstrap_view().common,onCancel:()=>act({kind:'cancel'}),onDismiss:()=>dispatch({type:'curve_editor',target,action:{kind:'key',epoch:model().editor.controls.epoch,key_event:'Escape',pressed:true,repeat:false,modifiers:{command:false,shift:false,alt:false}}}),placement:()=>model().bounds});
      const contact=(phase,e)=>act({kind:'drag',phase,position:e?position(e):[0,0],viewport:viewport()});
      title.onpointerdown=e=>{if(e.button)return;e.preventDefault();e.stopPropagation();drag=e.pointerId;title.setPointerCapture(e.pointerId);contact('down',e);};
      title.onpointermove=e=>{if(drag===e.pointerId)contact('move',e);};
      title.onpointerup=e=>{if(drag===e.pointerId){contact('up',e);drag=null;title.releasePointerCapture(e.pointerId);}};
      title.onpointercancel=title.onlostpointercapture=()=>{if(drag!=null){drag=null;contact('cancel');}};
      panel.addEventListener('pointerdown',e=>e.stopPropagation());
    }
    heading.textContent=view.title;close.title=view.close;close.setAttribute('aria-label',view.close);
    for(const [node,text] of [[firmer,view.firmer],[lighter,view.lighter],[reset,view.reset],[cancel,view.cancel],[apply,view.apply]]){if(mode!=='tilt'||(node!==firmer&&node!==lighter))node.textContent=text;if(node===firmer||node===lighter){node.title=text;node.setAttribute('aria-label',text);}}
    firmer.disabled=!view.firmer_enabled;lighter.disabled=!view.lighter_enabled;apply.disabled=mode==='tilt'&&!view.ready;
    device.hidden=direction.hidden=mode!=='tilt';
    if(mode==='tilt'){
      sensitivityTitle.textContent=view.sensitivity;device.textContent=view.device;direction.firstChild.textContent=view.direction;circle.setAttribute('aria-label',view.direction);
      for(const [index,[label,active]] of [[view.reverse_horizontal,view.axes.reverse_horizontal],[view.reverse_vertical,view.axes.reverse_vertical],[view.swap_axes,view.axes.swap]].entries()){
        axes[index].title=label;axes[index].setAttribute('aria-label',label);axes[index].setAttribute('aria-pressed',active);
      }
      [view.input_direction,view.adjusted_direction].forEach((vector,index)=>{
        if(!vector){arrows[index].setAttribute('d','');return;}
        const [x,y]=vector.map(v=>v*31),length=Math.hypot(x,y),[ux,uy]=length?[x/length,y/length]:[0,0];
        arrows[index].setAttribute('d',`M36 36L${36+x} ${36+y}M${36+x-ux*6-uy*3} ${36+y-uy*6+ux*3}L${36+x} ${36+y}L${36+x-ux*6+uy*3} ${36+y-uy*6-ux*3}`);
      });
    }
    editor.update(view.editor,view.title);
    const reserve=panel.querySelector('.curve-frame > span');reserve.textContent=view.axis_gutter;reserve.setAttribute('aria-hidden','true');
    const area=app.layout(...viewport()).phone?.work_area??{width:viewport()[0],height:viewport()[1]};
    const [width]=app.calibration_extent([area.width,area.height],[panel.scrollWidth,0]);panel.style.width=`${width}px`;
    panel.style.maxHeight=`${area.height}px`;host.present(view.dialog);
    if(!measureFrame)measureFrame=requestAnimationFrame(measure);
  }
  const observer=new ResizeObserver(()=>{measured=null;if(panel&&!measureFrame)measureFrame=requestAnimationFrame(measure);});observer.observe(workspace);
  return {refresh};
}
