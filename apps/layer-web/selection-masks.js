// Native DOM projection. Rust owns mask destinations, commands and menu policy.
import { noticePlacement } from "./notice.js";
import { captureSliderContacts } from "./numeric.js";

export const REFINE_WIDTH = 360;

export function createSelectionUi({app,state,element,button,icon,numberField,dispatch,workspace,layout,bar}) {
  const menuButton=(label,kind)=>{
    const node=button(label,()=>{
      const r=node.getBoundingClientRect();
      node.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:r.left,clientY:r.bottom}));
    },'selection-menu-button');
    node.dataset.context='{}';node.layerMenu=()=>app.selection_menu(kind);
    node.setAttribute('aria-haspopup','menu');node.title=label;
    node.append(icon('chevron-down'));return node;
  };
  const send=action=>dispatch({type:'selection',action});
  // The Refine panel previews each value on the canvas, which stays undimmed
  // and interactive. It is neither modal nor a popover, so canvas contacts keep
  // their meaning, and its buttons never take focus.
  let panel=null,title,number=null,kind=null,below=null,bounds=null,transform='';
  function open() {
    panel=element('section','refine-panel');panel.id='selection-refine-panel';
    panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','false');panel.tabIndex=-1;
    title=element('h2','refine-title');title.id='selection-refine-title';panel.setAttribute('aria-labelledby',title.id);
    const actions=element('div','refine-actions');
    actions.append(button('Cancel',()=>send({op:'cancel_resize'})),button('Apply',()=>send({op:'apply_resize'}),'suggested-action'));
    panel.append(title,actions);
    panel.addEventListener('mousedown',e=>{if(!e.target.closest('input'))e.preventDefault();});
    panel.addEventListener('contextmenu',e=>e.preventDefault());
    panel.addEventListener('keydown',e=>{
      if(e.isComposing||e.defaultPrevented||!['Enter','Escape'].includes(e.key))return;
      e.preventDefault();e.stopPropagation();
      send({op:e.key==='Enter'?'apply_resize':'cancel_resize'});
    });
    below=bar();
    workspace.append(panel);
    panel.focus({preventScroll:true});
  }
  function close() {
    panel.remove();panel=number=kind=bounds=below=null;transform='';
  }
  // Bottom centre of the work area, above a canvas action bar along that edge.
  // A bar hidden during a canvas contact keeps its place, so the panel holds still.
  function place() {
    const resolved=layout();
    if(!panel||!resolved)return;
    below=bar()??below;
    const anchor=noticePlacement(resolved,below),width=Math.min(REFINE_WIDTH,anchor.width);
    panel.style.width=`${width}px`;
    const height=panel.offsetHeight,ratio=globalThis.devicePixelRatio||1,align=v=>Math.round(v*ratio)/ratio;
    bounds={x:anchor.x-width/2,y:anchor.y-height,width,height};
    const next=`translate(${align(bounds.x)}px, ${align(bounds.y)}px)`;
    if(next!==transform)panel.style.transform=transform=next;
  }
  function refresh() {
    const view=state().layer_tools.selection_resize;
    if(!view){if(panel)close();return;}
    if(!panel)open();
    title.textContent=view.title;
    const rebuilt=kind!==view.kind;
    if(rebuilt) {
      const field=numberField(view.numeric,view.label,radius=>send({op:'resize_radius',radius}));
      field.id='selection-refine-value';field.dataset.refineKind=kind=view.kind;
      if(view.numeric.kind==='slider'){
        captureSliderContacts(field);
        field.slider.addEventListener('touchstart',e=>e.preventDefault(),{passive:false});
      }
      if(number)number.replaceWith(field);else title.after(field);
      number=field;
    }
    number.update(view.radius);
    if(rebuilt)place();
  }
  return {menuButton,refresh,place,bounds:()=>panel?bounds:null};
}
