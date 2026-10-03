
export function openSizeDialog({element,button,host,name,body,copy,apply,cancel}) {
  const dialog=element('dialog','document-dialog size-dialog');dialog.id=`${name}-dialog`;
  const form=element('form');form.method='dialog';
  const title=element('h2');title.id=`${name}-title`;title.tabIndex=-1;title.autofocus=true;dialog.setAttribute('aria-labelledby',title.id);
  const message=element('p','size-dialog-message');message.setAttribute('role','status');
  const footer=element('footer'),applyButton=button(copy.apply_label,apply,'suggested-action');
  const cancelButton=button(copy.cancel_label,cancel);footer.append(cancelButton,applyButton);
  form.append(title,...body,message,footer);
  form.addEventListener('submit',e=>{e.preventDefault();apply();});
  dialog.addEventListener('cancel',e=>{e.preventDefault();cancel();});
  dialog.append(form);host().append(dialog);dialog.showModal();
  return {dialog,title,message,apply:applyButton,relabel(copy){applyButton.textContent=copy.apply_label;cancelButton.textContent=copy.cancel_label;},close(){dialog.close();dialog.remove();}};
}

export function sizeSelect({element,label,choices,key,act}) {
  const select=element('select','size-dialog-select');select.setAttribute('aria-label',label);
  for(const choice of choices){const option=element('option','',choice.label);option.value=choice[key];select.append(option);}
  select.relabel=choices=>{for(const choice of choices){const option=[...select.options].find(o=>o.value===choice[key]);if(option)option.textContent=choice.label;}};
  select.addEventListener('change',()=>{const next=select.value;act({op:key,[key]:next});});
  return select;
}

export function sizeCheck({element,key,act}) {
  const input=element('input');input.type='checkbox';
  input.addEventListener('change',()=>{const next=input.checked;act({op:key,[key]:next});});
  const text=element('span'),row=element('label','size-dialog-check');row.append(input,text);
  return {row,input,text};
}

// Number fields that send each value typed so far once it reads as a number,
// so the message and Apply follow the typing. A field is rebuilt only when its
// numeric spec changes, which keeps the text being typed.
export function sizeFields({element,numberField,resolve,key,names,send}) {
  const nodes=names.map(()=>element('div')),specs=names.map(()=>null);
  function show(index,control,label,value,current) {
    const spec=JSON.stringify(control);
    if(specs[index]!==spec) {
      const op=names[index],number=numberField(control,label,value=>send({op,value}));
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
