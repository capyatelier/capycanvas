import assert from 'node:assert/strict';
import test from 'node:test';
import {createMenu} from './menu.js';
import {contextPopoverLayout,createContextPopover} from './context-popover.js';
import {colorDialogDom} from './color-dialog-fixture.mjs';

test('rebuilding menu pages cancels the old captured contact before further motion',t=>{
  const dom=colorDialogDom(t),previous=globalThis.getComputedStyle;
  const mutation=globalThis.MutationObserver;
  globalThis.MutationObserver=class {observe(){}disconnect(){}};t.after(()=>globalThis.MutationObserver=mutation);
  globalThis.getComputedStyle=()=>({direction:'ltr'});t.after(()=>globalThis.getComputedStyle=previous);
  const element=(...args)=>{
    const node=dom.element(...args);
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


test('context popover prefers centered below placement with a tracking arrow',()=>{
  const value=contextPopoverLayout({anchor:{x:180,y:80,width:40,height:30},size:{width:240,height:100},viewport:{x:0,y:0,width:400,height:300}});
  assert.deepEqual(value,{left:80,top:122,maxHeight:178,side:'bottom',arrowTip:120,arrowBase:120});
});

test('context popover flips above when the bottom cannot fit its body',()=>{
  const value=contextPopoverLayout({anchor:{x:180,y:260,width:40,height:24},size:{width:240,height:100},viewport:{x:0,y:0,width:400,height:300}});
  assert.deepEqual(value,{left:80,top:148,maxHeight:248,side:'top',arrowTip:120,arrowBase:120});
});

test('edge-clamped context popover keeps its arrow tip on the contact',()=>{
  for(const [x,left,tip,base]of [[0,0,12,27],[256,40,228,213]]){
    const value=contextPopoverLayout({anchor:{x,y:40,width:24,height:24},size:{width:240,height:100},viewport:{x:0,y:0,width:280,height:220}});
    assert.equal(value.left,left);assert.equal(value.arrowTip,tip);assert.equal(value.arrowBase,base);assert.equal(value.top,76);
    assert.equal(value.left+value.arrowTip,x+12,'horizontal body clamping does not move the arrow off its anchor');
  }
});

test('short context viewport constrains the body independently of its arrow',()=>{
  const viewport={x:10,y:20,width:260,height:100},value=contextPopoverLayout({anchor:{x:100,y:45,width:40,height:20},size:{width:200,height:300},viewport});
  assert.equal(value.side,'bottom');assert.equal(value.top,77);assert.equal(value.maxHeight,43);
  assert.ok(value.left>=viewport.x);assert.ok(value.left+200<=viewport.x+viewport.width);
  assert.equal(value.top+value.maxHeight,viewport.y+viewport.height);
  assert.equal(value.top-12,65,'the arrow occupies its own twelve pixels above the constrained body');
});


test('context popover highlight follows its initial row and clears on native interaction',t=>{
  const {element,button,doc}=colorDialogDom(t),previous=globalThis.visualViewport;
  globalThis.visualViewport={offsetLeft:30,offsetTop:50,width:300,height:200};t.after(()=>{globalThis.visualViewport=previous;});
  const menu=createContextPopover({element,classes:'fixture-menu'}),disabled=button('Disabled',()=>{}),first=button('First',()=>{}),second=button('Second',()=>{});disabled.disabled=true;
  menu.body.append(disabled,first,second);doc.body.append(menu.root);
  menu.body.querySelector=selector=>menu.body.descendants().find(node=>selector==='button:not(:disabled)'?node.tagName==='BUTTON'&&!node.disabled:node.hasAttribute('data-menu-initial'))??null;
  let size={width:220,height:80};menu.root.getBoundingClientRect=()=>{assert.equal(menu.body.style.maxHeight,'');return size;};
  const anchor={x:100,y:80,width:40,height:20},layout=menu.place(anchor,true);
  assert.equal(menu.root.getAttribute('role'),'menu');assert.equal(menu.root.popover,'manual');assert.equal(menu.root.style.left,'30px');assert.equal(menu.root.style.top,'112px');assert.equal(menu.body.style.maxHeight,'138px');
  assert.equal(menu.root.style.getPropertyValue('--menu-arrow-tip'),`${layout.arrowTip}px`);assert.equal(menu.root.style.getPropertyValue('--menu-arrow-base'),`${layout.arrowBase}px`);
  assert.equal(disabled.hasAttribute('data-menu-initial'),false);assert.equal(first.hasAttribute('data-menu-initial'),true);assert.equal(second.hasAttribute('data-menu-initial'),false);
  const focused=doc.activeElement;
  for(const type of ['pointermove','focusin','scroll']){menu.place(anchor,true);menu.body.dispatchEvent({type});assert.equal(first.hasAttribute('data-menu-initial'),false,`${type} retires the initial highlight`);menu.place(anchor);assert.equal(first.hasAttribute('data-menu-initial'),false,'reflow cannot restore a retired highlight');}
  first.disabled=true;menu.place(anchor,true);assert.equal(second.hasAttribute('data-menu-initial'),true);assert.equal(first.hasAttribute('data-menu-initial'),false);
  menu.body.dispatchEvent({type:'scroll'});size={width:220,height:120};globalThis.visualViewport.height=110;menu.place(anchor);
  assert.equal(menu.body.style.maxHeight,'48px');assert.equal(menu.root.dataset.side,'bottom');assert.equal(second.hasAttribute('data-menu-initial'),false);assert.equal(doc.activeElement,focused,'placement never takes keyboard focus');
  menu.root.dispatchEvent({type:'toggle',newState:'open'});assert.equal(menu.root.firstChild,menu.body);const thumb=menu.root.children.at(-1);assert.equal(thumb.className,'scroll-thumb');assert.equal(menu.root.children.length,2);
  menu.root.dispatchEvent({type:'toggle',newState:'open'});assert.equal(menu.root.children.length,2,'open publication cannot duplicate its overlay indicator');
  menu.root.dispatchEvent({type:'toggle',newState:'closed'});assert.equal(menu.root.children.length,1,'closing disposes the overlay indicator');
  menu.root.dispatchEvent({type:'toggle',newState:'open'});assert.notEqual(menu.root.children.at(-1),thumb);menu.root.dispatchEvent({type:'toggle',newState:'closed'});assert.equal(menu.root.children.length,1);
});
