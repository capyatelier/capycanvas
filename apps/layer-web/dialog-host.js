import {composingKey} from './text-input.js';
import {bindCopy} from './localization.js';

export function createDialogHost({app,dialog,header,cancel,primary,icon,copy,onCancel,onBack=onCancel,subpage=()=>false,search,clearSearch,cancelLabel=()=>copy.cancel}) {
  let intent,previousFocus,expectedCloses=0,observedHeader,done,back,close;
  const slots=new Map([cancel,primary].filter(Boolean).map(node=>[node,{parent:node.parentNode,next:node.nextSibling}]));
  const footer=primary?.closest('footer');
  dialog.dataset.dialogPart='page';
  function escape() {
    if(!intent)return;
    const action=app.dialog_escape(subpage(),!!search&&document.activeElement===search,!!search?.value);
    if(action==='clear_search')clearSearch();else if(action==='back')onBack();else onCancel();
  }
  const cancelled=event=>{event.preventDefault();escape();};
  const key=event=>{if(event.key==='Escape'&&!composingKey(event)){event.preventDefault();event.stopPropagation();escape();}};
  const closed=()=>{if(expectedCloses){expectedCloses--;return;}if(intent&&!dialog.open)onCancel();};
  const outside=event=>{
    if(!intent||event.target!==dialog)return;
    const b=dialog.getBoundingClientRect();
    if(event.clientX<b.left||event.clientX>b.right||event.clientY<b.top||event.clientY>b.bottom)onCancel();
  };
  dialog.addEventListener('cancel',cancelled);dialog.addEventListener('keydown',key);dialog.addEventListener('close',closed);dialog.addEventListener('click',outside);
  function layout() {
    if(!intent||!dialog.open)return;
    const row=typeof header==='function'?header():header;
    if(observedHeader!==row){
      if(observedHeader){observedHeader.classList.remove('page-header');observedHeader.style.minHeight='';observedHeader.style.gridTemplateColumns='';delete observedHeader.dataset.dialogPart;resize.unobserve(observedHeader);}
      observedHeader=row;row.dataset.dialogPart='header';resize.observe(row);
    }
    const v=window.visualViewport,visible={x:v?.offsetLeft??0,y:v?.offsetTop??0,width:v?.width??innerWidth,height:v?.height??innerHeight};
    const presentation=app.dialog_presentation(intent.role,innerWidth,innerHeight,visible),page=presentation.kind==='page';
    const actions=app.dialog_header(intent,subpage());
    if(page&&actions.start==='cancel'&&!close){close=document.createElement('button');close.type='button';close.append(icon('window-close'));bindCopy(close,cancelLabel,'ariaLabel');close.addEventListener('click',onCancel);resize.observe(close);}
    if(close&&close.parentNode!==row)row.append(close);
    if(actions.start==='back'&&!back){back=document.createElement('button');back.type='button';back.className='dialog-back';back.append(icon('go-previous'));bindCopy(back,()=>copy.back,'ariaLabel');back.addEventListener('click',onBack);resize.observe(back);}
    if(back&&back.parentNode!==row)row.append(back);
    if(page&&actions.end==='close'&&!done){done=document.createElement('button');done.type='button';done.className='suggested-action';bindCopy(done,()=>copy.done);done.addEventListener('click',onCancel);resize.observe(done);}
    if(done&&done.parentNode!==row)row.append(done);
    const start=actions.start==='back'?back:actions.start?(page?close:cancel):null,end=actions.end==='close'?(page?done:primary??cancel):actions.end?primary:null;
    for(const control of new Set([cancel,primary,done,back,close].filter(Boolean))){
      control.hidden=control!==start&&control!==end;delete control.dataset.dialogPart;
      control.classList.toggle('page-action-start',page&&control===start);control.classList.toggle('page-action-end',page&&control===end);control.style.height='';
    }
    const focus=document.activeElement;
    for(const [control,slot]of slots){
      if(page&&(control===start||control===end)){if(control.parentNode!==row)row.append(control);}
      else if(!page&&control.parentNode!==slot.parent)slot.parent.insertBefore(control,slot.next?.parentNode===slot.parent?slot.next:null);
    }
    if((focus===start||focus===end)&&document.activeElement!==focus)focus.focus({preventScroll:true});
    if(start)start.dataset.dialogPart='start';if(end)end.dataset.dialogPart='end';
    dialog.classList.toggle('dialog-page',page);row.classList.toggle('page-header',page);footer?.classList.toggle('projected-actions',page);
    for(const [name,key]of Object.entries({left:'x',top:'y',width:'width',height:'height'}))dialog.style[name]=page?`${presentation.bounds[key]}px`:'';
    const height=page?Math.max(start?.getBoundingClientRect().height??0,end?.getBoundingClientRect().height??0):0;
    if(page){if(start)start.style.height=`${height}px`;if(end)end.style.height=`${height}px`;}
    row.style.gridTemplateColumns=page?`minmax(${start?.getBoundingClientRect().width??0}px,1fr) minmax(0,auto) minmax(${end?.getBoundingClientRect().width??0}px,1fr)`:'';
    row.style.minHeight=page?`${Math.max(46,height+12)+1}px`:'';
  }
  window.addEventListener('resize',layout);window.visualViewport?.addEventListener('resize',layout);window.visualViewport?.addEventListener('scroll',layout);
  const resize=new ResizeObserver(layout);resize.observe(cancel);if(primary)resize.observe(primary);
  return {
    present(next) {
      intent=next;dialog.setAttribute('role',intent.role==='alert'?'alertdialog':'dialog');dialog.setAttribute('aria-modal',String(intent.modal));
      const opening=!dialog.open;
      if(opening){previousFocus=document.activeElement;if(intent.modal)dialog.showModal();else dialog.show();}
      layout();return opening;
    },
    hide() {
      intent=null;if(!dialog.open)return;expectedCloses++;dialog.close();
      if(!document.querySelector('dialog[open]')&&previousFocus?.isConnected)previousFocus.focus({preventScroll:true});
    },
    layout,
    dismiss(){if(intent){if(app.dialog_header(intent,subpage()).start==='back')onBack();else onCancel();}},
    dispose(){resize.disconnect();window.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('scroll',layout);dialog.removeEventListener('cancel',cancelled);dialog.removeEventListener('keydown',key);dialog.removeEventListener('close',closed);dialog.removeEventListener('click',outside);done?.remove();back?.remove();close?.remove();},
  };
}
