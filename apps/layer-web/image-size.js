import {openSizeDialog,sizeCheck,sizeFields} from './size-dialog.js';

export function createImageSizeUi({state,element,button,dropdown,numberField,resolve,dispatch,host=()=>document.body}) {
  const send=action=>dispatch({type:'image_size',action});
  const view=()=>state().layer_tools.image_size;
  let shell=null,fields,unit,constrain,resampleLabel,resample;
  const act=action=>{if(fields.commit()&&view())send(action);};
  function open(v) {
    fields=sizeFields({element,numberField,resolve,key:'imageSize',names:['width','height','resolution'],send});
    const size=element('div','size-dialog-fields');size.append(fields.nodes[0],fields.nodes[1]);
    unit=dropdown('',value=>act({op:'unit',unit:value}));
    constrain=sizeCheck({element,key:'constrain',act});
    const options=element('div','size-dialog-row');options.append(unit,constrain.row);
    const resolution=element('div','size-dialog-fields');resolution.append(fields.nodes[2]);
    resampleLabel=element('span');
    resample=dropdown('',value=>act({op:'resample',resample:value}));
    const resampleRow=element('div','size-dialog-row');resampleRow.append(resampleLabel,resample);
    shell=openSizeDialog({element,button,host,copy:v,name:'image-size',body:[size,options,resolution,resampleRow],
      apply:()=>{if(fields.commit()&&view()?.can_apply)send({op:'apply'});},cancel:()=>send({op:'cancel'})});
  }
  function refresh() {
    const v=view();
    if(!v){shell?.close();shell=null;return;}
    if(!shell)open(v);
    shell.title.textContent=v.title;shell.relabel(v);unit.update({choices:v.units.map(c=>({value:c.unit,label:c.label})),value:v.unit,label:v.units.find(c=>c.unit===v.unit).label});
    for(const axis of [0,1])fields.show(axis,v.numeric[axis],v.labels[axis],v.values[axis],()=>view().values[axis]);
    fields.show(2,v.resolution_numeric,v.resolution_label,v.resolution,()=>view().resolution);
    constrain.input.checked=v.constrain;constrain.text.textContent=v.constrain_label;
    resampleLabel.textContent=v.resample_label;resample.update({choices:v.resamples.map(c=>({value:c.resample,label:c.label})),value:v.resample,label:v.resample_label});
    shell.message.textContent=v.message;shell.apply.disabled=!v.can_apply;
  }
  return {refresh,dialog:()=>shell?.dialog??null};
}
