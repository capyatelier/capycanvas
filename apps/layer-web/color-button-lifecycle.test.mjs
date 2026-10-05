import assert from 'node:assert/strict';
import {test} from 'node:test';
import {colorButton} from './color-controls.js';
import {colorDialogDom,fakeColorApp} from './color-dialog-fixture.mjs';

test('deferred color dialog publishes only a live field with the original owner',async t=>{
  for(const mode of ['live','removed','new owner']) {
    const dom=colorDialogDom(t),selected={space:'Srgb',rgba:[0.3,0.4,0.5,1]},{app}=fakeColorApp({selected});
    app.color_ui=(previous=>request=>request.type==='preview'?[{rgba:selected.rgba,in_gamut:true}]:previous(request))(app.color_ui);
    let owner='document:layer';const changes=[];
    const field=colorButton({app,label:'Color',element:dom.element,button:dom.button,change:value=>changes.push(value),current:()=>owner});
    field.update(selected);
    const pending=new Promise(resolve=>{field.node.dispatchEvent({type:'click',preventDefault(){}});setTimeout(resolve,0);});
    await pending;
    const dialog=dom.doc.body.children.find(node=>node.tagName==='DIALOG');assert.equal(dialog.open,true);
    if(mode==='removed')field.dispose();if(mode==='new owner')owner='another document:layer';
    dialog.descendants().find(node=>node.className==='suggested-action').click();
    await new Promise(resolve=>setTimeout(resolve,0));
    assert.deepEqual(changes,mode==='live'?[selected]:[],mode);
    assert.equal(dom.doc.body.children.length,0);
  }
});
