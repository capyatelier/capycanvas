import assert from 'node:assert/strict';
import {createDropdown} from './dropdown.js';

const menus=new WeakMap();

export function dropdownFactory({element,button,icon=()=>element('svg')}) {
  const menu={open:false,matches:selector=>selector===':popover-open'&&menu.open,hidePopover(){menu.open=false;}};
  return (label,change)=>createDropdown({element,button,icon,openMenu:node=>{menu.menuOwner=node;menus.set(node,menu);menu.open=true;return menu;}},label,change);
}

export const dropdownChoices=node=>node.menuModel().sections.flat();
export function chooseDropdown(node,value) {
  assert.ok(node&&!node.disabled,'Dropdown is enabled');
  node.click();
  const choice=dropdownChoices(node).find(choice=>choice.command===String(value));
  assert.ok(choice&&choice.enabled,'Dropdown choice is enabled');
  menus.get(node).hidePopover();
  node.menuCommand(choice.command);
}
