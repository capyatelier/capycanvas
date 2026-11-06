import {composingKey} from './text-input.js';
import {bindCopy} from './localization.js';

const presentedDialogs=new Map();
export const hasOpenDialog=()=>presentedDialogs.size>0;
export function hasModalDialog(){for(const intent of presentedDialogs.values())if(intent.modal)return true;return false;}

function measureAlert(dialog,header,responses,buttons) {
  const title=header.querySelector('h2'),rootStyle=dialog.style.cssText,titleStyle=title?.style.cssText,stacked=responses.classList.contains('alert-responses-stacked'),buttonStyles=buttons.map(node=>node.style.cssText);
  try{
    dialog.style.cssText+=';width:max-content;min-width:0;max-width:none;height:auto;max-height:none;';
    responses.classList.remove('alert-responses-stacked');
    const widths=buttons.map((node,index)=>{node.style.cssText+=';flex:none;width:max-content;max-width:none;';const width=Math.ceil(node.getBoundingClientRect().width);node.style.cssText=buttonStyles[index];return width;});
    const natural=Math.ceil(dialog.getBoundingClientRect().width),style=getComputedStyle(dialog);
    if(title)title.style.cssText+=';width:max-content;max-width:none;';
    const heading=title?.textContent?Math.ceil(title.getBoundingClientRect().width)+(parseFloat(style.paddingLeft)||0)+(parseFloat(style.paddingRight)||0):0;
    if(title)title.style.cssText=titleStyle;
    responses.classList.add('alert-responses-stacked');
    buttons.forEach(node=>{node.style.cssText+=';flex:none;width:0;max-width:none;';});dialog.style.width='min-content';
    return{minimum:Math.ceil(dialog.getBoundingClientRect().width),natural,heading,responses:widths};
  }finally{
    buttons.forEach((node,index)=>{node.style.cssText=buttonStyles[index];});
    responses.classList.toggle('alert-responses-stacked',stacked);if(title)title.style.cssText=titleStyle;dialog.style.cssText=rootStyle;
  }
}

export function createDialogHost({app,dialog,header,headerStyle='raised',cancel,primary:sourcePrimary,retainPrimary=false,endSlot,icon,copy,onCancel,onBack=onCancel,subpage=()=>false,search,clearSearch,cancelLabel=()=>copy.cancel,placement,onKey,onDismiss}) {
  let primary=typeof sourcePrimary==='function'?sourcePrimary():sourcePrimary;
  let intent,previousFocus,expectedCloses=0,outsideDown=false,resizeFrame=0,observedHeader,done,back,close,contentChanges;
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
      if(observedHeader){observedHeader.classList.remove('page-header');observedHeader.style.minHeight='';observedHeader.style.gridTemplateColumns='';delete observedHeader.dataset.dialogPart;delete observedHeader.dataset.headerStyle;resize.unobserve(observedHeader);}
      observedHeader=row;resize.observe(row);
    }
    const v=window.visualViewport,visible={x:v?.offsetLeft??0,y:v?.offsetTop??0,width:v?.width??innerWidth,height:v?.height??innerHeight};
    const positioned=placement?.(visible),presentation=positioned?{kind:'floating'}:app.dialog_presentation(intent.role,innerWidth,innerHeight,visible),page=presentation.kind==='page'||presentation.kind==='canvas_panel';
    let bounds=positioned??presentation.bounds;
    const alert=!page&&intent.frame.kind==='alert';
    dialog.dataset.dialogFrame=alert?'alert':'editor';
    dialog.classList.toggle('short',alert&&app.alert_dialog_short(visible.height));
    if(!alert)(primary??cancel).parentElement?.classList.remove('alert-responses-stacked');
    const actions=app.dialog_header(intent,subpage()),parts=!alert;
    if(parts)row.dataset.dialogPart='header';else delete row.dataset.dialogPart;
    if(parts)row.dataset.headerStyle=headerStyle;else delete row.dataset.headerStyle;
    if(page&&actions.start==='cancel'&&!close){close=document.createElement('button');close.type='button';close.append(icon('window-close'));bindCopy(close,cancelLabel,'ariaLabel');close.addEventListener('click',onCancel);resize.observe(close);}
    if(close&&close.parentNode!==row)row.append(close);
    if(actions.start==='back'&&!back){back=document.createElement('button');back.type='button';back.className='dialog-back';back.append(icon('go-previous'));bindCopy(back,()=>copy.back,'ariaLabel');back.addEventListener('click',onBack);resize.observe(back);}
    if(back&&back.parentNode!==row)row.append(back);
    if(page&&actions.end==='close'&&!done){done=document.createElement('button');done.type='button';done.className='suggested-action';bindCopy(done,()=>copy.done);done.addEventListener('click',onCancel);resize.observe(done);}
    if(done&&done.parentNode!==row)row.append(done);
    const start=actions.start==='back'?back:actions.start?(page?close:cancel):null,end=actions.end==='close'?(page?done:primary??cancel):actions.end?primary:null;
    for(const control of new Set([...slots.keys(),done,back,close].filter(Boolean))){
      control.hidden=control!==start&&control!==end&&!(retainPrimary&&control===primary);delete control.dataset.dialogPart;
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
    }else if(!page&&!alert&&!positioned&&intent.role==='editor'){
      dialog.style.width='';
      const style=getComputedStyle(dialog),inset=style.boxSizing==='border-box'?0:
        ['paddingLeft','paddingRight','borderLeftWidth','borderRightWidth'].reduce((sum,key)=>sum+(parseFloat(style[key])||0),0);
      bounds={width:Math.max(0,app.floating_dialog_width(visible.width,dialog.getBoundingClientRect().width)-inset)};
    }
    if(alert){
      const responses=(primary??cancel).parentElement;
      if(!contentChanges){contentChanges=new MutationObserver(queueLayout);contentChanges.observe(dialog,{childList:true,subtree:true,characterData:true});resize.observe(responses);}
      const buttons=[...responses.children].filter(node=>node.tagName==='BUTTON'&&!node.hidden);
      const layout=app.alert_dialog_layout([visible.width,visible.height],measureAlert(dialog,row,responses,buttons),parseFloat(getComputedStyle(dialog).fontSize),intent.frame.prefer_wide);
      bounds={width:layout.width};dialog.classList.toggle('short',layout.short);responses.classList.toggle('alert-responses-stacked',layout.stacked);
    }
    for(const [name,key]of Object.entries({left:'x',top:'y',width:'width',height:'height'}))dialog.style[name]=bounds?.[key]!=null?`${bounds[key]}px`:'';
    if(positioned)dialog.style.maxHeight=positioned.maxHeight!=null?`${positioned.maxHeight}px`:'';
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
    dispose(){presentedDialogs.delete(dialog);if(resizeFrame)cancelAnimationFrame(resizeFrame);resize.disconnect();contentChanges?.disconnect();window.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('resize',layout);window.visualViewport?.removeEventListener('scroll',layout);dialog.removeEventListener('cancel',cancelled);dialog.removeEventListener('keydown',key);dialog.removeEventListener('close',closed);dialog.removeEventListener('click',outside);dialog.removeEventListener('pointerdown',down);dialog.removeEventListener('pointercancel',cancelContact);done?.remove();back?.remove();close?.remove();},
  };
}

export function createProgressDialog({app,element,button,icon,intent,title,body,progressBar=false,onCancel}) {
  const dialog=element('dialog','document-dialog file-progress'),header=element('header','dialog-header'),heading=element('h2','',title),label=element('p','',body),footer=element('footer');
  let track;
  const cancel=button(()=>app.bootstrap_view().common.cancel,onCancel);header.append(heading);footer.append(cancel);dialog.append(header);
  if(progressBar){
    const extra=element('div','alert-extra');track=element('div','alert-progress-track');label.className='alert-progress-caption';label.id=crypto.randomUUID();track.setAttribute('role','progressbar');track.setAttribute('aria-labelledby',label.id);track.append(element('span'));extra.append(label,track);dialog.append(extra);
  }else if(body)dialog.append(label);
  dialog.append(footer);document.body.append(dialog);
  const host=createDialogHost({app,dialog,header,cancel,icon,copy:app.bootstrap_view().common,onCancel:()=>cancel.click()});host.present(intent);
  return {title:heading,label,cancel,complete(){track?.setAttribute('aria-valuenow','100');},remove(){host.hide();host.dispose();dialog.remove();}};
}
