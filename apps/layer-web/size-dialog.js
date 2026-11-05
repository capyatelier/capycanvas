import {createDialogHost} from './dialog-host.js';
import {createPageKit} from './page-kit.js';

export function openSizeDialog({app,element,button,icon,host,name,body,copy,apply,cancel}) {
  const kit=createPageKit({element,icon});
  const dialog=element('dialog','document-dialog size-dialog');dialog.id=`${name}-dialog`;
  const form=element('form');form.method='dialog';
  const title=element('h2');title.id=`${name}-title`;title.tabIndex=-1;title.autofocus=true;dialog.setAttribute('aria-labelledby',title.id);
  const message=element('p','size-dialog-message');message.setAttribute('role','status');
  const applyButton=button(copy.apply_label,apply,'suggested-action'),cancelButton=button(copy.cancel_label,cancel);
  const footer=kit.footer({actions:[cancelButton,applyButton]});
  const header=element('header','dialog-header');header.append(title);
  form.append(...body,message);
  form.addEventListener('submit',e=>{e.preventDefault();apply();});
  const content=kit.body(form);
  dialog.append(header,content.root,footer);host().append(dialog);
  const shell=createDialogHost({app,dialog,header,cancel:cancelButton,primary:applyButton,icon,copy:app.bootstrap_view().common,onCancel:cancel});shell.present(copy.dialog);
  return {dialog,title,message,apply:applyButton,relabel(copy){applyButton.textContent=copy.apply_label;cancelButton.textContent=copy.cancel_label;},close(){shell.hide();shell.dispose();content.dispose();dialog.remove();}};
}

export function sizeCheck({element,key,act}) {
  const input=element('input','panel-check');input.type='checkbox';
  input.addEventListener('change',()=>{const next=input.checked;act({op:key,[key]:next});});
  const text=element('span'),row=element('label','size-dialog-check');row.append(input,text);
  return {row,input,text};
}

// Number fields that send each value typed so far once it reads as a number,
// so the message and Apply follow the typing. A field is rebuilt only when its
// numeric spec changes, which keeps the text being typed.
export function sizeFields({element,numberField,resolve,key,names,send,inline=false,present=node=>node}) {
  const nodes=names.map(()=>element('div')),specs=names.map(()=>null);
  function show(index,control,label,value,current) {
    const spec=JSON.stringify(control);
    if(specs[index]!==spec) {
      const op=names[index],number=numberField(control,label,value=>send({op,value}),inline);
      present(number);
      number.dataset[key]=op;
      number.entry.addEventListener('input',()=>{
        let typed;
        try{typed=resolve({control,value:current(),operation:{type:'expression',text:number.entry.value}}).value;}catch{return;}
        if(typed!==current())send({op,value:typed});
      });
      nodes[index].replaceWith(number);nodes[index]=number;specs[index]=spec;
    }
    nodes[index].relabel?.(label);
    nodes[index].update(value);
  }
  return {nodes,show,commit:()=>nodes.every(node=>node.commit())};
}
