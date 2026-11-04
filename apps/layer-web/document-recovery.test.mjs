import assert from 'node:assert/strict';
import test from 'node:test';
import {createDocumentRecovery} from './document-recovery.js';

function fixture(t,{blocked=false,destinationError}={}) {
  const previous={document:globalThis.document,window:globalThis.window};
  globalThis.document={addEventListener(){},hidden:false};globalThis.window={addEventListener(){}};
  t.after(()=>{globalThis.document=previous.document;globalThis.window=previous.window;});
  t.mock.method(globalThis,'setInterval',()=>0);
  const events=[],operations=[],messages=[],tabs=[];
  let stored={generation:1,drawings:[{id:1,key:'saved-drawing'}],active:1,blocked:blocked?[1]:[],restoring:[],clean_exit:false};
  let adopted,destinationDurable=false,freed=0;
  const own={key:'own',async publish(manifest){
    operations.push('destination_publish');
    if(adopted!=null) {
      assert.ok(destinationDurable||stored.drawings.some(drawing=>drawing.key==='saved-drawing'));
      if(destinationError)throw destinationError;
      assert.ok(manifest.drawings.some(drawing=>drawing.key==='saved-drawing'));destinationDurable=true;
    }
  }};
  const origin={async read(){operations.push('source_read');return structuredClone(stored);},async publish(next){
    operations.push('source_publish');
    if(!next.drawings.some(drawing=>drawing.key==='saved-drawing'))assert.equal(destinationDurable,true,'Source membership can disappear only after durable destination publication');
    stored=structuredClone(next);
  },free(){freed++;}};
  const app={session_maintenance(){},document_tabs:()=>({tabs,selected:adopted??0}),session_stamp_for:()=>({}),session_checkpoint_interval:()=>2000,
    session_binding:()=> 'saved-drawing',session_existing_binding:()=>adopted??null,session_current:()=>true,
    open_session_window:async key=>key==null?own:origin,session_windows:async()=>['abandoned'],
    import_legacy_sessions:async()=>{operations.push('import_legacy');return [];},
    prepare_session_retirement:async()=>operations.push('prepare_retirement'),retire_session_copy:async()=>operations.push('retire'),
    document_recovery_unavailable:detail=>detail,
    session_manifest_update(raw,event){
      events.push(structuredClone(event));const value=raw?JSON.parse(raw):{generation:0,drawings:[],blocked:[],restoring:[],active:0,clean_exit:false};
      const next={...value,generation:value.generation+1};
      switch(event.type){
        case 'interrupted':next.blocked=[...new Set([...value.blocked,...value.restoring.map(ticket=>ticket.id)])];next.restoring=[];break;
        case 'stage':case 'reconcile':next.drawings=event.drawings;next.active=event.active;next.clean_exit=event.clean_exit??false;break;
        case 'begin_restore':next.restoring=[...value.restoring,{id:event.id,generation:next.generation}];break;
        case 'finish_restore':next.restoring=value.restoring.filter(ticket=>ticket.id!==event.attempt.id);if(!event.success)next.blocked=[...value.blocked,event.attempt.id];break;
        case 'remove':next.drawings=value.drawings.filter(drawing=>drawing.id!==event.id);next.blocked=value.blocked.filter(id=>id!==event.id);next.restoring=value.restoring.filter(ticket=>ticket.id!==event.id);break;
        default:assert.fail(`Unexpected manifest event ${event.type}`);
      }
      return next;
    }};
  const recovery=createDocumentRecovery({app,failed:async()=>null,message:reader=>messages.push(reader()),restore:async()=>{
    assert.equal(blocked,false,'Dismissal cannot restore');operations.push('adopt');adopted=9;tabs.push({id:9,uri:null,modified:true});return adopted;
  }});
  return {recovery,events,operations,messages,stored:()=>stored,destinationDurable:()=>destinationDurable,freed:()=>freed};
}

test('dismissing a failed drawing leaves its blocked checkpoint available without retrying or discarding',async t=>{
  const f=fixture(t,{blocked:true});await f.recovery.start();
  assert.equal(f.events.some(event=>['retry_restore','remove','begin_restore'].includes(event.type)),false);
  assert.equal(f.operations.filter(operation=>operation==='import_legacy').length,1);assert.deepEqual(f.messages,[]);
  assert.equal(f.operations.includes('adopt'),false);assert.equal(f.operations.includes('retire'),false);
  assert.deepEqual(f.stored().drawings,[{id:1,key:'saved-drawing'}]);assert.deepEqual(f.stored().blocked,[1]);assert.equal(f.freed(),1);
});

test('adoption followed by destination publication failure preserves source membership without marking the drawing failed',async t=>{
  const error=Error('Destination publication failed'),f=fixture(t,{destinationError:error});
  await assert.rejects(f.recovery.start(),result=>result===error);
  assert.equal(f.operations.filter(operation=>operation==='adopt').length,1);
  assert.equal(f.destinationDurable(),false);assert.deepEqual(f.stored().drawings,[{id:1,key:'saved-drawing'}]);
  assert.deepEqual(f.stored().blocked,[]);assert.equal(f.stored().restoring.length,1);
  assert.equal(f.events.some(event=>event.type==='remove'||event.type==='finish_restore'&&!event.success),false);
  assert.equal(f.operations.some(operation=>['prepare_retirement','retire'].includes(operation)),false);
  assert.ok(f.messages.some(message=>message.includes('Destination publication failed')));assert.equal(f.freed(),1);
});

test('successful adoption removes source membership only after destination publication is durable',async t=>{
  const f=fixture(t);await f.recovery.start();
  assert.equal(f.destinationDurable(),true);assert.deepEqual(f.stored().drawings,[]);assert.deepEqual(f.stored().blocked,[]);
  assert.equal(f.events.some(event=>event.type==='finish_restore'&&event.success),true);
  assert.equal(f.operations.some(operation=>['prepare_retirement','retire'].includes(operation)),false);assert.equal(f.freed(),1);
  assert.deepEqual(f.messages,[]);
});

test('already durable restored drawings observe kept storage without another checkpoint',async t=>{
  const previous=Object.getOwnPropertyDescriptor(globalThis,'navigator');let observations=0;
  Object.defineProperty(globalThis,'navigator',{configurable:true,value:{storage:{async persisted(){observations++;return true;},async persist(){assert.fail('Kept storage does not need another request');}}}});
  t.after(()=>{if(previous)Object.defineProperty(globalThis,'navigator',previous);else delete globalThis.navigator;});
  const f=fixture(t);await f.recovery.start();
  assert.equal(observations,1);assert.equal(f.recovery.protected(),true);
  await f.recovery.autosave();assert.equal(observations,1);assert.equal(f.recovery.protected(),true);
});

function lifecycleFixture(t,{held=false}={}) {
  const previous={document:globalThis.document,window:globalThis.window},documentEvents=new Map(),windowEvents=new Map();
  const document={hidden:false,addEventListener:(name,callback)=>documentEvents.set(name,callback)};
  globalThis.document=document;globalThis.window={addEventListener:(name,callback)=>windowEvents.set(name,callback)};
  t.after(()=>{globalThis.document=previous.document;globalThis.window=previous.window;});
  let interval,current=false,release,started;
  const pending=new Promise(resolve=>{release=resolve;}),begun=new Promise(resolve=>{started=resolve;});
  t.mock.method(globalThis,'setInterval',callback=>{interval=callback;return 0;});
  const checkpoints=[],published=[],messages=[],background=[];
  const app={session_maintenance(){},document_tabs:()=>({selected:1,tabs:[{id:1,uri:'file:///private.capy',modified:true}]}),
    session_background:hidden=>background.push(hidden),session_binding:()=> 'private-drawing',session_current:()=>current,session_checkpoint_interval:()=>2000,
    open_session_window:async()=>({key:'private-window',async publish(value){published.push(structuredClone(value));}}),
    session_manifest_update(raw,event){const value=raw?JSON.parse(raw):{generation:0};return {...value,generation:value.generation+1,drawings:event.drawings,active:event.active,clean_exit:event.clean_exit??false};},
    async checkpoint_session(id,force){assert.equal(id,1n);checkpoints.push(force);started();if(held&&checkpoints.length===1)await pending;if(force)current=true;return force;},
    document_recovery_unavailable:detail=>detail};
  const recovery=createDocumentRecovery({app,message:reader=>messages.push(reader())});
  return {recovery,checkpoints,published,messages,background,begun,release,
    tick:()=>interval(),hide(){document.hidden=true;documentEvents.get('visibilitychange')();},pagehide:()=>windowEvents.get('pagehide')()};
}

test('hiding the page forces its drawing checkpoint without marking a clean exit',async t=>{
  const f=lifecycleFixture(t);f.hide();await f.recovery.autosave();
  assert.deepEqual(f.checkpoints,[true]);assert.equal(f.published.at(-1).clean_exit,false);assert.deepEqual(f.messages,[]);assert.deepEqual(f.background,[true]);
});

test('the ordinary recovery interval keeps checkpoint cadence',async t=>{
  const f=lifecycleFixture(t);f.tick();await f.recovery.autosave();
  assert.deepEqual(f.checkpoints,[false]);assert.equal(f.published.at(-1).clean_exit,false);assert.deepEqual(f.messages,[]);
});

test('pagehide forces its drawing checkpoint and marks a clean exit',async t=>{
  const f=lifecycleFixture(t);f.pagehide();await f.recovery.autosave();
  assert.deepEqual(f.checkpoints,[true]);assert.equal(f.published.at(-1).clean_exit,true);assert.deepEqual(f.messages,[]);
});

test('a forced hide is retained while an ordinary checkpoint is running',async t=>{
  const f=lifecycleFixture(t,{held:true});f.tick();await f.begun;f.hide();f.release();await f.recovery.autosave();
  assert.deepEqual(f.checkpoints,[false,true]);assert.equal(f.published.at(-1).clean_exit,false);assert.deepEqual(f.messages,[]);
});
