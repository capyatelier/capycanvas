import {openSizeDialog,sizeCheck,sizeFields,sizeSelect} from './size-dialog.js';

export function createCanvasSizeUi({state,element,button,icon,numberField,resolve,dispatch,host=()=>document.body}) {
  const send=action=>dispatch({type:'canvas_size',action});
  const view=()=>state().layer_tools.canvas_size;
  let shell=null,fields,unit,relative,anchorLabel,anchors;
  // Typed text reaches the draft before any other choice, so no edit is lost.
  const act=action=>{if(fields.commit()&&view())send(action);};
  function open(v) {
    fields=sizeFields({element,numberField,resolve,key:'canvasSize',names:['width','height'],send});
    const values=element('div','size-dialog-fields');values.append(...fields.nodes);
    unit=sizeSelect({element,label:'Unit',choices:v.units,key:'unit',act});
    relative=sizeCheck({element,key:'relative',act});
    const options=element('div','size-dialog-row');options.append(unit,relative.row);
    anchorLabel=element('span');
    const grid=element('div','canvas-anchor');grid.setAttribute('role','group');
    grid.addEventListener('mousedown',e=>e.preventDefault());
    anchors=v.anchors.map(choice=>{
      const cell=button('',()=>act({op:'anchor',anchor:choice.anchor}),'canvas-anchor-cell');
      cell.tabIndex=-1;cell.dataset.anchor=choice.anchor;cell.title=choice.label;cell.setAttribute('aria-label',choice.label);
      cell.append(icon('rectangle-fill'));grid.append(cell);return cell;
    });
    const anchorRow=element('div','size-dialog-row');anchorRow.append(anchorLabel,grid);
    shell=openSizeDialog({element,button,host,name:'canvas-size',body:[values,options,anchorRow],
      apply:()=>{if(fields.commit()&&view()?.can_apply)send({op:'apply'});},cancel:()=>send({op:'cancel'})});
  }
  function refresh() {
    const v=view();
    if(!v){shell?.close();shell=null;return;}
    if(!shell)open(v);
    shell.title.textContent=v.title;
    for(const axis of [0,1])fields.show(axis,v.numeric[axis],v.labels[axis],v.values[axis],()=>view().values[axis]);
    unit.value=v.unit;relative.input.checked=v.relative;relative.text.textContent=v.relative_label;anchorLabel.textContent=v.anchor_label;
    for(const cell of anchors) {
      const chosen=cell.dataset.anchor===v.anchor;
      cell.setAttribute('aria-pressed',String(chosen));cell.classList.toggle('selected',chosen);
    }
    shell.message.textContent=v.message;shell.apply.disabled=!v.can_apply;
  }
  return {refresh,dialog:()=>shell?.dialog??null};
}
