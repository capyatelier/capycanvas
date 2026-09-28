// Native projection of the shared Canvas Size dialog. Sizes, units, limits,
// validation and history live in Rust.
export function createCanvasSizeUi({state,element,button,icon,numberField,resolve,dispatch,host=()=>document.body}) {
  const send=action=>dispatch({type:'canvas_size',action});
  const view=()=>state().layer_tools.canvas_size;
  let dialog=null,title,fields,specs,unit,relative,relativeText,anchorLabel,anchors,message,apply;
  // Typed text reaches the draft before any other choice, so no edit is lost.
  const commit=()=>fields.every(field=>field.commit());
  const act=action=>{if(commit()&&view())send(action);};
  function applyChanges() {
    if(commit()&&view()?.can_apply)send({op:'apply'});
  }
  function open(v) {
    dialog=element('dialog','document-dialog canvas-size-dialog');dialog.id='canvas-size-dialog';
    const form=element('form');form.method='dialog';
    title=element('h2');title.id='canvas-size-title';title.tabIndex=-1;title.autofocus=true;dialog.setAttribute('aria-labelledby',title.id);
    fields=[element('div','canvas-size-field'),element('div','canvas-size-field')];specs=[null,null];
    const values=element('div','canvas-size-fields');values.append(...fields);
    unit=element('select','canvas-size-unit');unit.setAttribute('aria-label','Unit');
    for(const choice of v.units){const option=element('option','',choice.label);option.value=choice.unit;unit.append(option);}
    unit.addEventListener('change',()=>{const next=unit.value;act({op:'unit',unit:next});});
    relative=element('input');relative.type='checkbox';
    relative.addEventListener('change',()=>{const next=relative.checked;act({op:'relative',relative:next});});
    relativeText=element('span');
    const relativeRow=element('label','canvas-size-relative');relativeRow.append(relative,relativeText);
    const options=element('div','canvas-size-options');options.append(unit,relativeRow);
    anchorLabel=element('span','canvas-size-anchor-label');
    const grid=element('div','canvas-anchor');grid.setAttribute('role','group');
    grid.addEventListener('mousedown',e=>e.preventDefault());
    anchors=v.anchors.map(choice=>{
      const cell=button('',()=>act({op:'anchor',anchor:choice.anchor}),'canvas-anchor-cell');
      cell.tabIndex=-1;cell.dataset.anchor=choice.anchor;cell.title=choice.label;cell.setAttribute('aria-label',choice.label);
      cell.append(icon('rectangle-fill'));grid.append(cell);return cell;
    });
    const anchorRow=element('div','canvas-size-anchor');anchorRow.append(anchorLabel,grid);
    message=element('p','canvas-size-message');message.setAttribute('role','status');
    const footer=element('footer');
    apply=button('Apply',applyChanges,'suggested-action');
    footer.append(button('Cancel',()=>send({op:'cancel'})),apply);
    form.append(title,values,options,anchorRow,message,footer);
    form.addEventListener('submit',e=>{e.preventDefault();applyChanges();});
    dialog.addEventListener('cancel',e=>{e.preventDefault();send({op:'cancel'});});
    dialog.append(form);host().append(dialog);dialog.showModal();
  }
  function field(axis,v) {
    const spec=JSON.stringify(v.numeric[axis]);
    if(specs[axis]===spec)return;
    const op=axis?'height':'width',control=v.numeric[axis];
    const number=numberField(control,v.labels[axis],value=>send({op,value}));
    number.dataset.canvasSize=op;
    number.entry.addEventListener('input',()=>{
      let value;
      try{value=resolve({control,value:view().values[axis],operation:{type:'expression',text:number.entry.value}}).value;}catch{return;}
      if(value!==view().values[axis])send({op,value});
    });
    fields[axis].replaceWith(number);fields[axis]=number;specs[axis]=spec;
  }
  function refresh() {
    const v=view();
    if(!v){if(dialog){const closing=dialog;dialog=null;closing.close();closing.remove();}return;}
    if(!dialog)open(v);
    title.textContent=v.title;
    for(const axis of [0,1]){field(axis,v);fields[axis].update(v.values[axis]);}
    unit.value=v.unit;relative.checked=v.relative;relativeText.textContent=v.relative_label;anchorLabel.textContent=v.anchor_label;
    for(const cell of anchors) {
      const chosen=cell.dataset.anchor===v.anchor;
      cell.setAttribute('aria-pressed',String(chosen));cell.classList.toggle('selected',chosen);
    }
    message.textContent=v.message;apply.disabled=!v.can_apply;
  }
  return {refresh,dialog:()=>dialog};
}
