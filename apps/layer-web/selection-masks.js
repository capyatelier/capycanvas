// Native DOM projection. Rust owns mask destinations, commands and menu policy.
export function createSelectionUi({app,catalog,state,workspace,element,button,icon,numberField,dispatch}) {
  const invoke=command=>dispatch({type:'invoke',command});
  const menuButton=(label,kind)=>{
    const node=button(label,()=>{
      const r=node.getBoundingClientRect();
      node.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:r.left,clientY:r.bottom}));
    },'selection-menu-button');
    node.dataset.context='{}';node.layerMenu=()=>app.selection_menu(kind);
    node.setAttribute('aria-haspopup','menu');node.title=label;
    node.append(icon('chevron-down'));return node;
  };
  const root=element('div','selection-mask-actions chrome');root.id='selection-mask-actions';root.hidden=true;
  root.setAttribute('role','group');root.setAttribute('aria-label','Selection mask editing');
  const label=element('strong'),controls=element('div','selection-mask-controls'),reason=element('span','selection-mask-reason');
  const gray=numberField(catalog.opacity,'Foreground mask gray',value=>dispatch({type:'set_tool_setting',id:'mask_gray',value}),true);
  gray.dataset.maskGray='';
  const swap=button('Swap',()=>invoke('swap_mask_colors')),done=button('Done',()=>invoke('return_to_artwork'));
  done.id='selection-mask-done';swap.title='Swap foreground and background mask values';done.title='Return to artwork';
  controls.append(gray,swap,menuButton('Overlay','overlay'),done);root.append(label,controls,reason);workspace.append(root);
  function quickRow() {
    const row=element('div','selection-mask-row');row.dataset.quickMask='';
    const eye=button('',()=>invoke('mask_overlay'),'layer-icon');eye.title='Show or hide mask overlay';eye.setAttribute('aria-label',eye.title);
    const text=element('div','layer-text');text.append(element('strong','','Quick Mask'),element('span','layer-meta','Temporary'));
    const actions=menuButton('Quick Mask actions','quick_mask');
    actions.setAttribute('aria-label','Quick Mask actions');actions.replaceChildren(icon('more'));actions.classList.add('selection-mask-more');
    row.append(eye,text,actions);
    row.refresh=()=>{row.hidden=!state().layer_tools.quick_mask;const shown=state().layer_tools.mask_editing?.overlay;eye.replaceChildren(icon(shown?'eye':'eye-hidden'));eye.setAttribute('aria-pressed',String(!!shown));};
    row.refresh();return row;
  }
  return {menuButton,quickRow,refresh(){
    const view=state().layer_tools.mask_editing;root.hidden=!view;
    if(!view)return;
    label.textContent=view.label;reason.textContent=view.reason??'Black protects · White selects';
    gray.update(view.gray);done.disabled=!state().commands.find(c=>c.id==='return_to_artwork')?.enabled;
  }};
}
