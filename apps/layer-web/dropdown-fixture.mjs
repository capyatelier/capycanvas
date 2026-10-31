import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';

export function chooseDropdown(selector,value,by='value') {
  const node=typeof selector==='string'?document.querySelector(selector):selector;
  if(!node||node.disabled)throw Error(`Missing enabled dropdown ${selector}`);
  node.click();
  const menu=document.querySelector('.panel-context-menu:popover-open');
  if(!menu)throw Error(`Dropdown did not open ${selector}`);
  const rows=[...menu.querySelectorAll('button')];
  const row=by==='index'?rows[Number(value)]:rows.find(row=>String(by==='label'?row.menuItem.label:row.menuItem.command)===String(value));
  if(!row||row.disabled)throw Error(`Missing enabled choice ${value} in ${selector}`);
  row.click();
}

export const dropdownChoice=(selector,value,by='value')=>`(${chooseDropdown.toString()})(${JSON.stringify(selector)},${JSON.stringify(value)},${JSON.stringify(by)})`;
export async function captureDropdowns({call,evaluate,settle},selector,directory,name) {
  const count=await evaluate(`document.querySelector(${JSON.stringify(selector)}).querySelectorAll('.dropdown').length`);
  const key=async()=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key:'Escape',code:'Escape',windowsVirtualKeyCode:27});await settle();};
  const click=async()=>{const p=await evaluate(`(()=>{const r=dropdownTestTrigger.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...p,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();};
  for(let index=0;index<count;index++) {
    if(!await evaluate(`(()=>{window.dropdownTestTrigger=document.querySelector(${JSON.stringify(selector)}).querySelectorAll('.dropdown')[${index}];return dropdownTestTrigger.checkVisibility()&&!dropdownTestTrigger.disabled})()`))continue;
    await evaluate(`dropdownTestTrigger.scrollIntoView({block:'nearest'})`);await settle();
    for(const state of ['closed','open']) {
      if(state==='open'){await click();assert.equal(await evaluate(`!!document.querySelector('.panel-context-menu:popover-open')`),true,`${name} dropdown opens`);}
      const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}-dropdown-${index}-${state}.png`,Buffer.from(data,'base64'));
    }
    await click();assert.equal(await evaluate(`!!document.querySelector('.panel-context-menu:popover-open')`),false,`${name} clicking the trigger again closes its menu`);
    await click();await key();assert.equal(await evaluate(`document.activeElement===dropdownTestTrigger&&!document.querySelector('.panel-context-menu:popover-open')`),true,`${name} Escape closes the menu and returns focus`);
  }
}
