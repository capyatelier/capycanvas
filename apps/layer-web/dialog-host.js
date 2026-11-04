import {composingKey} from './text-input.js';

export function createDialogHost({app,dialog,header,cancel,primary,icon,copy,onCancel,search,clearSearch,cancelLabel=()=>copy.cancel}) {
  let intent,previousFocus,expectedCloses=0;
  const footer=primary?.closest('footer');
  function escape() {
    if(!intent)return;
    const action=app.dialog_escape(false,!!search&&document.activeElement===search,!!search?.value);
    if(action==='clear_search')clearSearch();
    else onCancel();
  }
  const cancelled=event=>{event.preventDefault();escape();};
  const key=event=>{
    if(event.key==='Escape'&&!composingKey(event)){event.preventDefault();event.stopPropagation();escape();}
  };
  const closed=()=>{
    if(expectedCloses){expectedCloses--;return;}
    if(intent&&!dialog.open)onCancel();
  };
  const outside=event=>{
    if(!intent||event.target!==dialog)return;
    const bounds=dialog.getBoundingClientRect();
    if(event.clientX<bounds.left||event.clientX>bounds.right||event.clientY<bounds.top||event.clientY>bounds.bottom)onCancel();
  };
  dialog.addEventListener('cancel',cancelled);dialog.addEventListener('keydown',key);dialog.addEventListener('close',closed);dialog.addEventListener('click',outside);
  function layout() {
    if(!intent||!dialog.open)return;
    const viewport=window.visualViewport,visible={x:viewport?.offsetLeft??0,y:viewport?.offsetTop??0,width:viewport?.width??innerWidth,height:viewport?.height??innerHeight};
    const presentation=app.dialog_presentation(intent.role,innerWidth,innerHeight,visible),page=presentation.kind==='page';
    dialog.classList.toggle('dialog-page',page);header.classList.toggle('page-header',page);footer?.classList.toggle('projected-actions',page);
    cancel.classList.remove('page-action-start','page-action-end');primary?.classList.remove('page-action-end');
    cancel.style.height='';if(primary)primary.style.height='';
    if(!page) {
      for(const name of ['left','top','width','height'])dialog.style[name]='';
      header.style.minHeight='';header.style.gridTemplateColumns='';
      if(cancel.dataset.pageIcon){cancel.textContent=cancelLabel();delete cancel.dataset.pageIcon;}
      cancel.setAttribute('aria-label',cancelLabel());
      cancel.hidden=false;if(primary)primary.hidden=false;return;
    }
    const bounds=presentation.bounds;
    for(const name of ['left','top','width','height'])dialog.style[name]=`${bounds[{left:'x',top:'y',width:'width',height:'height'}[name]]}px`;
    const actions=app.dialog_header(intent,false);
    if(primary)primary.hidden=!actions.end;
    const end=primary??(actions.end==='close'?cancel:null);
    cancel.hidden=!actions.start&&end!==cancel;
    if(actions.start) {
      cancel.classList.add('page-action-start');
      const name='window-close';
      if(cancel.firstElementChild?.dataset.asset!==name)cancel.replaceChildren(icon(name));
      cancel.dataset.pageIcon=name;cancel.setAttribute('aria-label',copy.cancel);
    } else if(end===cancel) {cancel.textContent=copy.done;cancel.setAttribute('aria-label',copy.done);cancel.dataset.pageIcon='done';}
    if(end&&actions.end)end.classList.add('page-action-end');
    const startWidth=actions.start?cancel.getBoundingClientRect().width:0,endWidth=end?.getBoundingClientRect().width??0;
    const height=Math.max(cancel.hidden?0:cancel.getBoundingClientRect().height,end?.getBoundingClientRect().height??0);
    cancel.style.height=`${height}px`;if(end)end.style.height=`${height}px`;
    header.style.gridTemplateColumns=`minmax(${startWidth}px,1fr) minmax(0,auto) minmax(${endWidth}px,1fr)`;header.style.minHeight=`${Math.max(46,height+12)+1}px`;
  }
  window.addEventListener('resize',layout);window.visualViewport?.addEventListener('resize',layout);window.visualViewport?.addEventListener('scroll',layout);
  const resize=new ResizeObserver(layout);resize.observe(header);resize.observe(cancel);if(primary)resize.observe(primary);
  return {
    present(next) {
      intent=next;
      dialog.setAttribute('aria-modal',String(intent.modal));
      const opening=!dialog.open;
      if(opening){previousFocus=document.activeElement;if(intent.modal)dialog.showModal();else dialog.show();}
      layout();return opening;
    },
    hide() {
      intent=null;if(!dialog.open)return;
      expectedCloses++;dialog.close();
      if(!document.querySelector('dialog[open]')&&previousFocus?.isConnected)previousFocus.focus({preventScroll:true});
    },
    layout,
    dispose() {
      resize.disconnect();window.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('scroll',layout);
      dialog.removeEventListener('cancel',cancelled);dialog.removeEventListener('keydown',key);dialog.removeEventListener('close',closed);dialog.removeEventListener('click',outside);
    },
  };
}
