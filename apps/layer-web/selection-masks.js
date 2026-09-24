// Native DOM projection. Rust owns mask destinations, commands and menu policy.
export function createSelectionUi({app,state,element,button,icon,numberField,dispatch}) {
  const menuButton=(label,kind)=>{
    const node=button(label,()=>{
      const r=node.getBoundingClientRect();
      node.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:r.left,clientY:r.bottom}));
    },'selection-menu-button');
    node.dataset.context='{}';node.layerMenu=()=>app.selection_menu(kind);
    node.setAttribute('aria-haspopup','menu');node.title=label;
    node.append(icon('chevron-down'));return node;
  };
  let dialog,number;
  const send=action=>dispatch({type:'selection',action});
  return {menuButton,refresh(){
    const view=state().layer_tools.selection_resize;
    if(!view){if(dialog){dialog.close();dialog.remove();dialog=null;}return;}
    if(!dialog){
      dialog=element('dialog','document-dialog');dialog.id='selection-resize-dialog';
      dialog.append(element('h2','',view.title));
      number=numberField(view.numeric,'Distance',radius=>send({op:'resize_radius',radius}));
      const actions=element('div','dialog-actions');
      actions.append(button('Cancel',()=>send({op:'cancel_resize'})),button('Apply',()=>send({op:'apply_resize'})));
      dialog.append(number,actions);dialog.oncancel=e=>{e.preventDefault();send({op:'cancel_resize'});};
      document.body.append(dialog);dialog.showModal();
    }
    number.update(view.radius);
  }};
}
