import {bindCopy} from './localization.js';

export function createDropdown({element,button,icon,openMenu},label,change) {
  let view={},menu,reopen=false;
  const read=value=>typeof value==='function'?value():value;
  const node=button('',()=>{
    const skip=reopen;reopen=false;if(skip)return;
    menu=openMenu(node);node.setAttribute('aria-expanded','true');
  },'dropdown');
  const caption=element('span','dropdown-label');node.append(caption,icon('chevron-down'));
  const name=()=>read(view.label??label)??'',text=()=>read(view.text??view.choices?.find(choice=>String(choice.value)===node.value)?.label)??name();
  bindCopy(node,name,'ariaLabel');bindCopy(node,name,'title');bindCopy(caption,text);
  node.setAttribute('aria-haspopup','menu');node.setAttribute('aria-expanded','false');
  node.addEventListener('pointerdown',()=>{reopen=node.getAttribute('aria-expanded')==='true'&&menu?.menuOwner===node;});
  node.menuCommand=value=>{node.update({value});change?.(value);};
  const choicesModel=()=>({title:read(view.label??label),sections:[(view.choices??[]).map(choice=>({...choice,label:read(choice.label),enabled:choice.enabled!==false,selected:String(choice.value)===node.value,command:String(choice.value)}))]});
  node.menuModel=()=>view.menu?.()??choicesModel();
  node.update=next=>{
    view={...view,...next};node.value=String(view.value??'');node.disabled=!!view.disabled;
    node.setAttribute('aria-label',name());node.title=name();
    if(caption.firstChild?.nodeType===3)caption.firstChild.nodeValue=text();else caption.textContent=text();
  };
  node.update({});return node;
}
