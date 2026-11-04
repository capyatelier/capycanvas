import {liveCopy} from './localization.js';
import {createDialogHost} from './dialog-host.js';
import {captureSliderContacts} from './numeric.js';

export function createPreviewPanel({app,name,view,kind,value,apply,cancel,element,button,icon,numberField,workspace}) {
  const common=liveCopy(app,'bootstrap_view').common;
  let panel=null,title,body,host,number=null,shown=null;
  function open() {
    panel=element('dialog','document-dialog refine-panel');panel.id=`${name}-panel`;panel.tabIndex=-1;
    const header=element('header','dialog-header'),actions=element('footer','refine-actions');
    title=element('h2','refine-title');title.id=`${name}-title`;panel.setAttribute('aria-labelledby',title.id);header.append(title);
    body=element('div','dialog-content');
    const cancelButton=button(()=>common.cancel,cancel),primary=button(()=>common.apply,apply,'suggested-action');actions.append(cancelButton,primary);
    body.append(actions);panel.append(header,body);workspace.append(panel);
    host=createDialogHost({app,dialog:panel,header,cancel:cancelButton,primary,icon,copy:common,onCancel:cancel});
    panel.addEventListener('contextmenu',e=>e.preventDefault());
    panel.addEventListener('keydown',e=>{if(!e.defaultPrevented&&e.key==='Enter'){e.preventDefault();e.stopPropagation();apply();}});
  }
  function close() {host.hide();host.dispose();panel.remove();panel=number=shown=null;}
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
        field.slider.addEventListener('touchstart',e=>{if(e.cancelable)e.preventDefault();},{passive:false});
      }
      if(number)number.replaceWith(field);else body.prepend(field);
      number=field;
    }
    number.relabel?.(v.label);number.update(v.radius);host.present(v.dialog);
  }
  return {refresh,place:()=>host?.layout(),bounds:()=>{if(!panel)return null;const {x,y,width,height}=panel.getBoundingClientRect();return{x,y,width,height};}};
}
