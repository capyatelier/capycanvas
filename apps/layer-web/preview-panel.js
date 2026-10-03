import {liveCopy} from './localization.js';
import { noticePlacement } from "./notice.js";
import { captureSliderContacts } from "./numeric.js";

export const PREVIEW_PANEL_WIDTH = 360;

export function createPreviewPanel({app,name,view,kind,value,apply,cancel,element,button,numberField,workspace,layout,bar}) {
  const common=liveCopy(app,'bootstrap_view').common;
  let panel=null,title,number=null,shown=null,below=null,bounds=null,transform='';
  function open() {
    panel=element('section','refine-panel');panel.id=`${name}-panel`;
    panel.setAttribute('role','dialog');panel.setAttribute('aria-modal','false');panel.tabIndex=-1;
    title=element('h2','refine-title');title.id=`${name}-title`;panel.setAttribute('aria-labelledby',title.id);
    const actions=element('div','refine-actions');
    actions.append(button(()=>common.cancel,cancel),button(()=>common.apply,apply,'suggested-action'));
    panel.append(title,actions);
    panel.addEventListener('mousedown',e=>{if(!e.target.closest('input'))e.preventDefault();});
    panel.addEventListener('contextmenu',e=>e.preventDefault());
    panel.addEventListener('keydown',e=>{
      if(e.isComposing||e.defaultPrevented||!['Enter','Escape'].includes(e.key))return;
      e.preventDefault();e.stopPropagation();
      (e.key==='Enter'?apply:cancel)();
    });
    below=bar();
    workspace.append(panel);
    panel.focus({preventScroll:true});
  }
  function close() {
    panel.remove();panel=number=shown=bounds=below=null;transform='';
  }
  // Bottom centre of the work area, above a canvas action bar along that edge.
  // A bar hidden during a canvas contact keeps its place, so the panel holds still.
  function place() {
    const resolved=layout();
    if(!panel||!resolved)return;
    below=bar()??below;
    const anchor=noticePlacement(resolved,below),width=Math.min(PREVIEW_PANEL_WIDTH,anchor.width);
    panel.style.width=`${width}px`;
    const height=panel.offsetHeight,ratio=globalThis.devicePixelRatio||1,align=v=>Math.round(v*ratio)/ratio;
    bounds={x:anchor.x-width/2,y:anchor.y-height,width,height};
    const next=`translate(${align(bounds.x)}px, ${align(bounds.y)}px)`;
    if(next!==transform)panel.style.transform=transform=next;
  }
  function refresh() {
    const v=view();
    if(!v){if(panel)close();return;}
    if(!panel)open();
    title.textContent=v.title;
    const rebuilt=shown!==JSON.stringify([v.numeric,v.kind]);
    if(rebuilt) {
      const field=numberField(v.numeric,v.label,value);
      field.id=`${name}-value`;field.dataset.kind=kind(v);shown=JSON.stringify([v.numeric,v.kind]);
      if(v.numeric.kind==='slider'){
        captureSliderContacts(field);
        field.slider.addEventListener('touchstart',e=>e.preventDefault(),{passive:false});
      }
      if(number)number.replaceWith(field);else title.after(field);
      number=field;
    }
    number.relabel?.(v.label);
    number.update(v.radius);
    if(rebuilt)place();
  }
  return {refresh,place,bounds:()=>panel?bounds:null};
}
