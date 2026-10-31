// Native DOM projection. Rust owns mask destinations, commands and menu policy.
import { createPreviewPanel } from "./preview-panel.js";

export function createSelectionUi({app,state,element,button,dropdown,numberField,dispatch,workspace,layout,bar}) {
  const menuButton=(label,kind)=>{
    const node=dropdown(label);node.classList.add('selection-menu-button');
    node.update({menu:()=>app.selection_menu(kind)});return node;
  };
  const send=action=>dispatch({type:'selection',action});
  const refine=createPreviewPanel({app,name:'selection-refine',view:()=>state().layer_tools.selection_resize,kind:v=>v.kind,
    value:radius=>send({op:'resize_radius',radius}),apply:()=>send({op:'apply_resize'}),cancel:()=>send({op:'cancel_resize'}),
    element,button,numberField,workspace,layout,bar});
  return {menuButton,refresh:refine.refresh,place:refine.place,bounds:refine.bounds};
}
