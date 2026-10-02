import {openSizeDialog,sizeCheck,sizeFields,sizeSelect} from './size-dialog.js';

export function createImageSizeUi({state,element,button,numberField,resolve,dispatch,host=()=>document.body}) {
  const send=action=>dispatch({type:'image_size',action});
  const view=()=>state().layer_tools.image_size;
  let shell=null,fields,unit,constrain,resampleLabel,resample;
  const act=action=>{if(fields.commit()&&view())send(action);};
  function open(v) {
    fields=sizeFields({element,numberField,resolve,key:'imageSize',names:['width','height','resolution'],send});
    const size=element('div','size-dialog-fields');size.append(fields.nodes[0],fields.nodes[1]);
    unit=sizeSelect({element,label:v.units.find(choice=>choice.unit===v.unit).label,choices:v.units,key:'unit',act});
    constrain=sizeCheck({element,key:'constrain',act});
    const options=element('div','size-dialog-row');options.append(unit,constrain.row);
    const resolution=element('div','size-dialog-fields');resolution.append(fields.nodes[2]);
    resampleLabel=element('span');
    resample=sizeSelect({element,label:v.resample_label,choices:v.resamples,key:'resample',act});
    const resampleRow=element('div','size-dialog-row');resampleRow.append(resampleLabel,resample);
    shell=openSizeDialog({element,button,host,copy:v,name:'image-size',body:[size,options,resolution,resampleRow],
      apply:()=>{if(fields.commit()&&view()?.can_apply)send({op:'apply'});},cancel:()=>send({op:'cancel'})});
  }
  function refresh() {
    const v=view();
    if(!v){shell?.close();shell=null;return;}
    if(!shell)open(v);
    shell.title.textContent=v.title;
    for(const axis of [0,1])fields.show(axis,v.numeric[axis],v.labels[axis],v.values[axis],()=>view().values[axis]);
    fields.show(2,v.resolution_numeric,v.resolution_label,v.resolution,()=>view().resolution);
    unit.value=v.unit;unit.setAttribute('aria-label',v.units.find(choice=>choice.unit===v.unit).label);constrain.input.checked=v.constrain;constrain.text.textContent=v.constrain_label;
    resampleLabel.textContent=v.resample_label;resample.value=v.resample;
    shell.message.textContent=v.message;shell.apply.disabled=!v.can_apply;
  }
  return {refresh,dialog:()=>shell?.dialog??null};
}
