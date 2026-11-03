import assert from 'node:assert/strict';
import test from 'node:test';
import {createMenu} from './menu.js';
import {colorDialogDom} from './color-dialog-fixture.mjs';

test('rebuilding menu pages cancels the old captured contact before further motion',t=>{
  const dom=colorDialogDom(t),previous=globalThis.getComputedStyle;
  const mutation=globalThis.MutationObserver;
  globalThis.MutationObserver=class {observe(){}disconnect(){}};t.after(()=>globalThis.MutationObserver=mutation);
  globalThis.getComputedStyle=()=>({direction:'ltr'});t.after(()=>globalThis.getComputedStyle=previous);
  const element=(...args)=>{
    const node=dom.element(...args);node.style.setProperty=(key,value)=>node.style[key]=value;
    node.hasPointerCapture=()=>false;node.releasePointerCapture=()=>{};
    node.querySelectorAll=()=>node.descendants().filter(child=>child.dataset.pressed!==undefined);
    return node;
  };
  const button=(text,run,cls)=>{const node=element('button',cls,text);node.addEventListener('click',run);return node;};
  const root={page:null,path:[]},child={page:{kind:'drawings'},path:[]};
  const state={phone:{menu_route:child}},actions=[],inputs=[],tracker={free(){}};
  const row={label:'Study',enabled:true,target:null,selected:null,close:null};
  let pages=[{id:root,parent:null,title:'Menu',checks:false,sections:[[{...row,target:child}]]},
    {id:child,parent:root,title:'Drawings',checks:true,sections:[[row]]}];
  const app={header_view:()=>({sizes:[{id:'medium',tile:48}]}),phone_menu:()=>({pages,strip:Array.from({length:5},()=>({label:'Action',enabled:true,icon:'settings'})),close_width:34}),
    phone_menu_contact:()=>tracker,phone_menu_input(actual,input){assert.equal(actual,tracker);inputs.push(input);return input.kind==='swipe'?{swipe:{kind:'back',progress:.2}}:{};}};
  const customization={menuRow(item){const node=button('',()=>{});node.updateMenuRow=next=>node.menuItem=next;node.disposeMenuRow=()=>{};return node;}};
  const menu=createMenu({app,state:()=>state,element,button,icon:dom.icon,dispatch:action=>actions.push(action),customization,place(){}});
  dom.doc.body.append(menu.root);
  const scroll=menu.root.children[0].children[0];scroll.clientWidth=300;
  const current=scroll.descendants().find(node=>node.dataset.route===JSON.stringify(child));
  const target=current.descendants().find(node=>node.dataset.position);target.closest=()=>target;
  const event={button:0,isPrimary:true,pointerId:1,target,clientX:20,clientY:20,timeStamp:1,preventDefault(){}};
  menu.root.dispatchEvent({...event,type:'pointerdown'});
  menu.root.dispatchEvent({...event,type:'pointermove',clientX:80,timeStamp:20});
  assert.equal(inputs.at(-1).kind,'swipe');
  state.phone.menu_route=root;menu.refresh();
  const retired=inputs.length;
  assert.doesNotThrow(()=>target.onclick({detail:1}));
  assert.equal(inputs.length,retired);
  pages=pages.slice(0,1);pages[0].sections=[[row]];
  menu.refresh();assert.equal(inputs.at(-1).kind,'cancel_press');
  const count=inputs.length;
  assert.doesNotThrow(()=>menu.root.dispatchEvent({...event,type:'pointermove',clientX:120,timeStamp:30}));
  assert.doesNotThrow(()=>menu.root.dispatchEvent({...event,type:'pointerup',clientX:120,timeStamp:31}));
  assert.equal(inputs.length,count);assert.deepEqual(actions,[]);
  assert.doesNotThrow(()=>target.onclick({detail:1}));
  assert.deepEqual(actions,[]);
  const strip=menu.root.children.at(-1);menu.dispose();
  assert.doesNotThrow(()=>strip.onclick({detail:0}));
  const disposed=inputs.length;
  assert.doesNotThrow(()=>menu.root.dispatchEvent({...event,type:'pointerdown'}));
  assert.doesNotThrow(()=>menu.root.dispatchEvent({...event,type:'keydown',key:'Escape',stopPropagation(){}}));
  assert.equal(inputs.length,disposed);
  assert.deepEqual(actions,[]);
});
