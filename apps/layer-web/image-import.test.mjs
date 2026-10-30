import assert from 'node:assert/strict';
import test from 'node:test';
import {createImageImport} from './image-import.js';

test('a manual Import cannot claim files from a canceled deferred drop',async t=>{
  const previous={document:globalThis.document,window:globalThis.window},events=new Map();
  globalThis.document={addEventListener:(name,handler)=>events.set(name,handler),querySelectorAll:()=>[]};
  globalThis.window={addEventListener(){}};
  t.after(()=>Object.assign(globalThis,previous));
  const canvas={width:100,height:100,getBoundingClientRect:()=>({x:0,y:0,width:100,height:100})};
  let pending=true,pickers=0,reads=0;
  const app={bootstrap_view:()=>({}),document_delivery_copy:()=>({}),
    state:()=>({commands:[{id:'import_image',enabled:true}]}),file_drop:()=> 'import_image',
    request_image_drop:()=>({id:41,change:{}}),image_drop_pending:()=>pending,
    capture_image_import:()=>({free(){}}),finish_document:()=>({}),prepare_images:()=>{reads++;assert.fail('retired files reached the decoder');}};
  const importer=createImageImport({app,canvas,applyChange(){},message:error=>assert.fail(error)});
  events.get('drop')({target:canvas,clientX:10,clientY:10,preventDefault(){},dataTransfer:{types:['Files'],files:[{name:'Old drop.png'}]}});
  pending=false;
  await importer.run(42,async()=>{pickers++;return [];});
  assert.equal(pickers,1);
  assert.equal(reads,0);
});
