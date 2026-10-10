import {createCurveEditor} from './curve-editor.js';
import {composingKey} from './text-input.js';

export function createPressureCalibration({state,workspace,element,button,icon,numberField,panelFrame,dispatch}) {
  let panel,editor,heading,close,firmer,lighter,reset,cancel,apply,drag,measured,measureFrame;
  const send=action=>dispatch({type:'pressure_calibration',action});
  const viewport=()=>[workspace.clientWidth,workspace.clientHeight];
  const position=e=>{const r=workspace.getBoundingClientRect();return [e.clientX-r.left,e.clientY-r.top];};
  function measure(){
    measureFrame=null;if(!panel)return;
    const r=panel.getBoundingClientRect(),v=viewport(),key=[r.width,r.height,...v].join(',');
    if(key!==measured){measured=key;send({kind:'measure',extent:[r.width,r.height],viewport:v});}
  }
  function refresh(){
    const view=state().pressure_calibration;
    if(!view){
      if(panel){
        const closing=panel,retired=editor;panel=null;editor=null;drag=null;measured=null;
        if(measureFrame){cancelAnimationFrame(measureFrame);measureFrame=null;}
        retired.dispose();closing.remove();
      }
      return;
    }
    if(!panel){
      panel=element('div','dock-group floating-panel utility-panel');panel.id='pen-pressure-dialog';panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','false');
      const header=element('div','dock-tabs utility-header');heading=element('strong','utility-title');heading.id='pen-pressure-title';panel.setAttribute('aria-labelledby',heading.id);
      close=button('',()=>send({kind:'cancel'}),'utility-close');close.append(icon('window-close'));header.append(heading,close);
      const body=element('div','panel utility-body');
      editor=createCurveEditor({element,button,icon,numberField,dispatch,target:{kind:'pressure'},initial:view.editor,label:view.title});
      const sensitivity=element('div','utility-sensitivity'),actions=element('div','utility-actions');
      firmer=button('',()=>send({kind:'sensitivity',lighter:false}));lighter=button('',()=>send({kind:'sensitivity',lighter:true}));sensitivity.append(firmer,lighter);
      reset=button('',()=>dispatch({type:'curve_editor',target:{kind:'pressure'},action:{kind:'reset'}}));cancel=button('',()=>send({kind:'cancel'}));apply=button('',()=>send({kind:'apply'}),'accent');actions.append(reset,element('span'),cancel,apply);
      body.append(editor.node,sensitivity,actions);panel.append(header,panelFrame(body,false));workspace.append(panel);
      const contact=(phase,e)=>send({kind:'drag',phase,position:e?position(e):[0,0],viewport:viewport()});
      header.onpointerdown=e=>{if(e.button||e.target.closest('button'))return;e.preventDefault();e.stopPropagation();drag=e.pointerId;header.setPointerCapture(e.pointerId);contact('down',e);};
      header.onpointermove=e=>{if(drag===e.pointerId)contact('move',e);};
      header.onpointerup=e=>{if(drag===e.pointerId){contact('up',e);drag=null;header.releasePointerCapture(e.pointerId);}};
      header.onpointercancel=header.onlostpointercapture=()=>{if(drag!=null){drag=null;contact('cancel');}};
      panel.onkeydown=e=>{if(!composingKey(e)&&e.key==='Escape'){e.preventDefault();e.stopPropagation();dispatch({type:'curve_editor',target:{kind:'pressure'},action:{kind:'key',epoch:state().pressure_calibration.editor.controls.epoch,key_event:e.key,pressed:true,repeat:e.repeat,modifiers:{command:false,shift:false,alt:false}}});}};
      panel.addEventListener('pointerdown',e=>e.stopPropagation());
      requestAnimationFrame(()=>editor?.graph.focus({preventScroll:true}));
    }
    heading.textContent=view.title;close.title=view.close;close.setAttribute('aria-label',view.close);
    for(const [node,text] of [[firmer,view.firmer],[lighter,view.lighter],[reset,view.reset],[cancel,view.cancel],[apply,view.apply]])node.textContent=text;
    firmer.disabled=!view.firmer_enabled;lighter.disabled=!view.lighter_enabled;editor.update(view.editor,view.title);
    panel.style.width=`${view.bounds.width}px`;panel.style.maxHeight=`${viewport()[1]}px`;panel.style.transform=`translate(${view.bounds.x}px,${view.bounds.y}px)`;
    if(!measureFrame)measureFrame=requestAnimationFrame(measure);
  }
  const observer=new ResizeObserver(()=>{measured=null;if(panel&&!measureFrame)measureFrame=requestAnimationFrame(measure);});observer.observe(workspace);
  return {refresh};
}
