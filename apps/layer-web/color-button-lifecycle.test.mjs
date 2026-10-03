import assert from 'node:assert/strict';
import {test} from 'node:test';
import {colorButton} from './color-controls.js';

class Node {
  constructor(tag) {this.tag=tag;this.children=[];this.style={};this.dataset={};this.listeners=new Map();}
  append(...children) {this.children.push(...children);for(const child of children)child.parent=this;}
  insertBefore(child,before) {child.remove();const index=this.children.indexOf(before);this.children.splice(index<0?this.children.length:index,0,child);child.parent=this;}
  remove() {if(this.parent)this.parent.children=this.parent.children.filter(child=>child!==this);this.parent=null;}
  setAttribute(name,value) {this[name]=value;}
  addEventListener(name,callback) {this.listeners.set(name,callback);}
  close() {this.listeners.get('close')?.();}
  showModal() {this.open=true;}
  focus() {document.activeElement=this;}
  get options() {return this.children;}
}
function descendants(node) {return [node,...node.children.flatMap(descendants)];}

test('deferred color dialog publishes only a live field with the original owner',async()=>{
  const previous=globalThis.document;
  try {
    for(const mode of ['live','removed','new owner']) {
      globalThis.document={body:new Node('body'),activeElement:null};
      let owner='document:layer',changes=[];
      const selected={space:'Srgb',rgba:[0.3,0.4,0.5,1]};
      const app={state:()=>({document_file:{epoch:1},layer_tools:{},colors:{hdr_depth:false}}),
        catalog:()=>({native_copy:{color:{edit:'Edit',model:'Model',intensity_ev:'EV',base:'Base',adjusted:'Adjusted',use_color:'Use'}}}),
        bootstrap_view:()=>({common:{cancel:'Cancel'}}),language_tag:()=> 'en',
        color_panel:()=>({rgb_space:'Srgb',hdr:false}),
        color_ui:()=>({value:selected,draft:{model:'document_rgb',fields:['0.3','0.4','0.5','1'],intensity:null},models:[['document_rgb','RGB']],labels:['R','G','B','A']})};
      const element=(tag,css='',text='')=>{const node=new Node(tag);node.textContent=typeof text==='function'?text():text;return node;};
      const button=(label,callback)=>{const node=element('button','',label);node.click=()=>callback();return node;};
      const field=colorButton({app,label:'Color',element,button,change:value=>changes.push(value),current:()=>owner});
      const pending=field.node.click();
      const dialog=document.body.children[0];assert.equal(dialog.tag,'dialog');assert.equal(dialog.open,true);
      if(mode==='removed')field.dispose();if(mode==='new owner')owner='another document:layer';
      const apply=descendants(dialog).find(node=>node.tag==='button'&&node.textContent==='Use');
      assert.ok(apply);apply.click();await pending;
      assert.deepEqual(changes,mode==='live'?[selected]:[],mode);
      assert.equal(document.body.children.length,0);
    }
  } finally {globalThis.document=previous;}
});
