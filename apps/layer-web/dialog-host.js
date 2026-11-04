import {composingKey} from './text-input.js';
import {bindCopy} from './localization.js';

const presentedDialogs=new Map();
export const hasOpenDialog=()=>presentedDialogs.size>0;
export function hasModalDialog(){for(const intent of presentedDialogs.values())if(intent.modal)return true;return false;}

export function createDialogHost({app,dialog,header,cancel,primary:sourcePrimary,endSlot,icon,copy,onCancel,onBack=onCancel,subpage=()=>false,search,clearSearch,cancelLabel=()=>copy.cancel,placement,onKey,onDismiss}) {
  let primary=typeof sourcePrimary==='function'?sourcePrimary():sourcePrimary;
  let intent,previousFocus,expectedCloses=0,outsideDown=false,resizeFrame=0,observedHeader,done,back,close,responseChanges;
  const slots=new Map([cancel,primary].filter(Boolean).map(node=>[node,{parent:node.parentNode,next:node.nextSibling}]));
  const footer=primary?.closest('footer');
  dialog.dataset.dialogPart='page';
  function escape() {
    if(!intent)return;
    if(onDismiss){onDismiss();return;}
    const searchNode=typeof search==='function'?search():search;
    const action=app.dialog_escape(subpage(),!!searchNode&&document.activeElement===searchNode,!!searchNode?.value);
    if(action==='clear_search')clearSearch(searchNode);else if(action==='back')onBack();else onCancel();
  }
  const cancelled=event=>{event.preventDefault();escape();};
  const key=event=>{if(event.defaultPrevented)return;if(event.key==='Escape'&&!composingKey(event)){event.preventDefault();event.stopPropagation();escape();}else{onKey?.(event);if(intent?.modal)event.stopPropagation();}};
  const closed=()=>{if(expectedCloses){expectedCloses--;return;}if(intent&&!dialog.open)onCancel();};
  const outsideBounds=event=>{const b=dialog.getBoundingClientRect();return event.target===dialog&&(event.clientX<b.left||event.clientX>b.right||event.clientY<b.top||event.clientY>b.bottom);};
  const down=event=>{outsideDown=outsideBounds(event);},cancelContact=()=>{outsideDown=false;};
  const outside=event=>{const dismiss=outsideDown&&outsideBounds(event);outsideDown=false;if(intent&&dismiss)onCancel();};
  dialog.addEventListener('cancel',cancelled);dialog.addEventListener('keydown',key);dialog.addEventListener('close',closed);dialog.addEventListener('click',outside);dialog.addEventListener('pointerdown',down);dialog.addEventListener('pointercancel',cancelContact);
  function layout() {
    if(!intent||!dialog.open)return;
    primary=typeof sourcePrimary==='function'?sourcePrimary():sourcePrimary;
    if(primary&&!slots.has(primary)){slots.set(primary,{parent:primary.parentNode,next:primary.nextSibling});resize.observe(primary);}
    const row=typeof header==='function'?header():header;
    if(observedHeader!==row){
      if(observedHeader){observedHeader.classList.remove('page-header');observedHeader.style.minHeight='';observedHeader.style.gridTemplateColumns='';delete observedHeader.dataset.dialogPart;resize.unobserve(observedHeader);}
      observedHeader=row;resize.observe(row);
    }
    const v=window.visualViewport,visible={x:v?.offsetLeft??0,y:v?.offsetTop??0,width:v?.width??innerWidth,height:v?.height??innerHeight};
    const positioned=placement?.(visible),presentation=positioned?{kind:'floating'}:app.dialog_presentation(intent.role,innerWidth,innerHeight,visible),page=presentation.kind==='page'||presentation.kind==='canvas_panel';
    let bounds=positioned??presentation.bounds;
    const actions=app.dialog_header(intent,subpage()),parts=page||intent.role!=='alert';
    if(parts)row.dataset.dialogPart='header';else delete row.dataset.dialogPart;
    if(page&&actions.start==='cancel'&&!close){close=document.createElement('button');close.type='button';close.append(icon('window-close'));bindCopy(close,cancelLabel,'ariaLabel');close.addEventListener('click',onCancel);resize.observe(close);}
    if(close&&close.parentNode!==row)row.append(close);
    if(actions.start==='back'&&!back){back=document.createElement('button');back.type='button';back.className='dialog-back';back.append(icon('go-previous'));bindCopy(back,()=>copy.back,'ariaLabel');back.addEventListener('click',onBack);resize.observe(back);}
    if(back&&back.parentNode!==row)row.append(back);
    if(page&&actions.end==='close'&&!done){done=document.createElement('button');done.type='button';done.className='suggested-action';bindCopy(done,()=>copy.done);done.addEventListener('click',onCancel);resize.observe(done);}
    if(done&&done.parentNode!==row)row.append(done);
    const start=actions.start==='back'?back:actions.start?(page?close:cancel):null,end=actions.end==='close'?(page?done:primary??cancel):actions.end?primary:null;
    for(const control of new Set([...slots.keys(),done,back,close].filter(Boolean))){
      control.hidden=control!==start&&control!==end;delete control.dataset.dialogPart;
      control.classList.toggle('page-action-start',page&&control===start);control.classList.toggle('page-action-end',page&&control===end);control.style.height='';control.style.width='';control.style.right='';
    }
    const focus=document.activeElement;
    for(const [control,slot]of slots){
      if(page&&(control===start||control===end)){if(control.parentNode!==row)row.append(control);}
      else if(!page&&control.parentNode!==slot.parent)slot.parent.insertBefore(control,slot.next?.parentNode===slot.parent?slot.next:null);
    }
    if((focus===start||focus===end)&&document.activeElement!==focus)focus.focus({preventScroll:true});
    if(parts&&start)start.dataset.dialogPart='start';if(parts&&end)end.dataset.dialogPart='end';
    const reserve=endSlot?endSlot.getBoundingClientRect().width+6:0;
    if(page){for(const control of new Set([start,end].filter(Boolean)))control.style.width=`${Math.ceil(control.getBoundingClientRect().width)}px`;if(end)end.style.right=`${6+reserve}px`;}
    dialog.classList.toggle('dialog-page',page);row.classList.toggle('page-header',page);footer?.classList.toggle('projected-actions',page);
    const height=page?Math.max(start?.getBoundingClientRect().height??0,end?.getBoundingClientRect().height??0):0;
    if(page){if(start)start.style.height=`${height}px`;if(end)end.style.height=`${height}px`;}
    row.style.gridTemplateColumns=page?`minmax(${start?.getBoundingClientRect().width??0}px,1fr) minmax(0,auto) minmax(${(end?.getBoundingClientRect().width??0)+reserve}px,1fr)`:'';
    row.style.minHeight=page?`${Math.max(46,height+12)+1}px`:'';
    if(presentation.kind==='canvas_panel'){
      dialog.style.width='';dialog.style.height='';
      const measured=dialog.getBoundingClientRect();
      bounds=app.dialog_bounds(intent.role,[innerWidth,innerHeight],visible,[measured.width,measured.height]);
    }else if(!page&&!positioned&&intent.role==='editor'){
      dialog.style.width='';
      const style=getComputedStyle(dialog),inset=style.boxSizing==='border-box'?0:
        ['paddingLeft','paddingRight','borderLeftWidth','borderRightWidth'].reduce((sum,key)=>sum+(parseFloat(style[key])||0),0);
      bounds={width:Math.max(0,app.floating_dialog_width(visible.width,dialog.getBoundingClientRect().width)-inset)};
    }
    for(const [name,key]of Object.entries({left:'x',top:'y',width:'width',height:'height'}))dialog.style[name]=bounds?.[key]!=null?`${bounds[key]}px`:'';
    if(positioned)dialog.style.maxHeight=positioned.maxHeight!=null?`${positioned.maxHeight}px`:'';
    if(intent.role==='alert'){
      const responses=(primary??cancel).parentElement,style=getComputedStyle(responses);
      if(!responseChanges){responseChanges=new MutationObserver(queueLayout);responseChanges.observe(responses,{childList:true,subtree:true,characterData:true});resize.observe(responses);}
      const buttons=[...responses.children].filter(node=>node.tagName==='BUTTON'&&!node.hidden);
      const widths=buttons.map(node=>{const css=node.style.cssText;node.style.cssText+=';flex:none;width:max-content;max-width:none;';const width=node.getBoundingClientRect().width;node.style.cssText=css;return width;});
      const available=responses.clientWidth-(parseFloat(style.paddingLeft)||0)-(parseFloat(style.paddingRight)||0);
      responses.classList.toggle('alert-responses-stacked',app.alert_responses_stacked(widths,parseFloat(style.columnGap)||0,available));
    }
  }
  function queueLayout(){if(!resizeFrame)resizeFrame=requestAnimationFrame(()=>{resizeFrame=0;layout();});}
  window.addEventListener('resize',layout);window.visualViewport?.addEventListener('resize',layout);window.visualViewport?.addEventListener('scroll',layout);
  const resize=new ResizeObserver(queueLayout);resize.observe(cancel);if(primary)resize.observe(primary);
  return {
    present(next) {
      intent=next;presentedDialogs.set(dialog,intent);dialog.setAttribute('role',intent.role==='alert'?'alertdialog':'dialog');dialog.setAttribute('aria-modal',String(intent.modal));
      const opening=!dialog.open;
      if(opening){previousFocus=document.activeElement;if(intent.modal)dialog.showModal();else dialog.show();}
      layout();return opening;
    },
    hide() {
      intent=null;outsideDown=false;presentedDialogs.delete(dialog);if(!dialog.open)return;expectedCloses++;dialog.close();
      if(!hasOpenDialog()&&previousFocus?.isConnected)previousFocus.focus({preventScroll:true});
    },
    layout,
    dismiss(){if(intent){if(onDismiss){onDismiss();return;}if(app.dialog_header(intent,subpage()).start==='back')onBack();else onCancel();}},
    dispose(){presentedDialogs.delete(dialog);if(resizeFrame)cancelAnimationFrame(resizeFrame);resize.disconnect();responseChanges?.disconnect();window.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('scroll',layout);dialog.removeEventListener('cancel',cancelled);dialog.removeEventListener('keydown',key);dialog.removeEventListener('close',closed);dialog.removeEventListener('click',outside);dialog.removeEventListener('pointerdown',down);dialog.removeEventListener('pointercancel',cancelContact);done?.remove();back?.remove();close?.remove();},
  };
}

export function createProgressDialog({app,element,button,icon,intent,title,body,onCancel}) {
  const dialog=element('dialog','document-dialog file-progress'),header=element('header','dialog-header'),heading=element('h2','',title),label=element('p','',body),footer=element('footer');
  const cancel=button(()=>app.bootstrap_view().common.cancel,onCancel);header.append(heading);footer.append(cancel);dialog.append(header,label,footer);document.body.append(dialog);
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:app.bootstrap_view().common,onCancel:()=>cancel.click()});host.present(intent);
  label.hidden=!body;
  return {title:heading,label,cancel,remove(){host.hide();host.dispose();dialog.remove();}};
}
