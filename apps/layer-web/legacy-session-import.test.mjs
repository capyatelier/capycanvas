import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {installSessionStoreFixture} from './session-store-fixture.mjs';

export async function checkLegacySessionImport({evaluate,settle}) {
  await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+60000;function check(){if(window.layerApp&&layerApp.startupTimes.complete!==null&&layerApp.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready())resolve();else if(performance.now()>end)reject(Error('Legacy import fixture startup did not finish'));else setTimeout(check,30);}check();})`);
  await evaluate('layerApp.documents.startRecovery()');
  const fixture=new URL('../../crates/layer-core/src/package/codec/fixtures/v1.0.14-paint-session/',import.meta.url);
  const head=JSON.parse(await readFile(new URL('head.json',fixture),'utf8'));
  const current=await readFile(new URL(`generations/${head.current.id}.json`,fixture),'utf8');
  const previous=await readFile(new URL(`generations/${head.previous.id}.json`,fixture),'utf8');
  const digest=value=>createHash('sha256').update(value).digest('hex');
  assert.equal(digest(current),head.current.sha256);assert.equal(digest(previous),head.previous.sha256);
  const records=[JSON.parse(previous),JSON.parse(current)],resourceIds=[...new Set(records.flatMap(record=>record.resources.map(resource=>resource.id)))];
  const resources=await Promise.all(resourceIds.map(async id=>({id,bytes:Array.from(await readFile(new URL(`resources/${id}.bin`,fixture)))})));
  const writer=await readFile(new URL('./fixtures/1.0.15/restart-store.js',import.meta.url),'utf8');
  await installSessionStoreFixture(evaluate);
  await evaluate(`window.releasedSessionFixture=(()=>{${writer.replace(/^export /gm,'')};return createRestartStore();})()`);
  await evaluate(`(async()=>{
    window.legacyFixtureKey='released-import-'+crypto.randomUUID();window.legacyFixtureWindow='released-window-'+crypto.randomUUID();
    const id=Math.max(...layerApp.app.document_tabs(0).tabs.map(tab=>Number(tab.id)))+1024;
    window.legacyFixtureId=id;
    const payloads=${JSON.stringify(resources)}.flatMap(({id,bytes})=>{const data=new Uint8Array(bytes),blocks=[];for(let offset=0;offset<data.length;offset+=4*1024*1024)blocks.push({id:id+'-'+offset/(4*1024*1024),bytes:data.slice(offset,offset+4*1024*1024)});return blocks;});
    const ids=payloads.map(payload=>payload.id);
    await releasedSessionFixture.publish(legacyFixtureKey,{generation:1,resources:ids,metadata:${JSON.stringify(previous)}},payloads);
    await releasedSessionFixture.publish(legacyFixtureKey,{generation:2,resources:ids,metadata:${JSON.stringify(current)}},[],[],1);
    await releasedSessionFixture.publishManifest(legacyFixtureWindow,{generation:1,drawings:[{id,key:legacyFixtureKey}],active:id,clean_exit:false,restoring:[],blocked:[]});
    window.legacyFixtureRead=async()=>({record:await releasedSessionFixture.read(legacyFixtureKey),manifest:await releasedSessionFixture.manifest(legacyFixtureWindow),bytes:(await releasedSessionFixture.resources(legacyFixtureKey,ids)).map(bytes=>Array.from(bytes))});
    window.legacyFixtureBefore=await legacyFixtureRead();
  })()`);
  const sourceKey=await evaluate('legacyFixtureKey');
  assert.deepEqual(await evaluate('layerApp.app.import_legacy_sessions()'),[],'Actual importer accepts the released paint session');
  const imported=await evaluate(`(async()=>{
    const ledger=(await sessionStoreFixture.index('imports')).current.metadata,record=ledger.records[legacyFixtureKey];
    return {record,drawing:await sessionStoreFixture.snapshot(record.drawing),unchanged:JSON.stringify(await legacyFixtureRead())===JSON.stringify(legacyFixtureBefore)};
  })()`);
  assert.equal(imported.unchanged,true);assert.equal(imported.record.complete,true);
  assert.deepEqual(imported.record.source,{key:sourceKey,generation:2,sha256:digest(current)});
  assert.equal(imported.record.baseline,imported.drawing.current.id);
  assert.deepEqual(imported.drawing.current.metadata.metadata,records[1].metadata,'Imported current state retains the released UI metadata');
  assert.deepEqual(imported.drawing.previous.metadata.metadata,records[0].metadata,'Imported previous state retains the released fallback metadata');
  for(const [actual,expected] of [[imported.drawing.current.metadata,records[1]],[imported.drawing.previous.metadata,records[0]]]) {
    assert.equal(actual.current.checkpoint,expected.current.checkpoint);assert.equal(actual.undo.length,expected.undo.length);assert.equal(actual.redo.length,expected.redo.length);
  }
  const beforeRepeat=await evaluate(`(async()=>({ledger:await sessionStoreFixture.index('imports'),files:await sessionStoreFixture.files(${JSON.stringify(imported.record.drawing)})}))()`);
  assert.deepEqual(await evaluate('layerApp.app.import_legacy_sessions()'),[]);
  assert.deepEqual(await evaluate(`(async()=>({ledger:await sessionStoreFixture.index('imports'),files:await sessionStoreFixture.files(${JSON.stringify(imported.record.drawing)})}))()`),beforeRepeat,'Repeated upgrade reads neither republish nor recopy the imported drawing');
  await evaluate(`(async()=>{
    const app=layerApp.app,initial=app.session_stamp_for(app.document_tabs(0).selected);
    const candidate=await app.prepare_session_restart(${JSON.stringify(imported.record.drawing)},true,async()=>null);
    if(!candidate)throw Error('Imported fixture is unexpectedly busy');
    app.adopt_session_restart(candidate,initial,BigInt(legacyFixtureId),false);
    await layerApp.documents.select(BigInt(legacyFixtureId));
    layerApp.dispatch({type:'set_zoom',zoom:.4375});
  })()`);
  await settle();
  assert.equal(await evaluate('layerApp.app.checkpoint_session(layerApp.app.document_tabs(0).selected,true)'),true,'The edited OPFS branch completes an explicit durable checkpoint');
  await evaluate('layerApp.documents.autosave()');
  assert.ok(await evaluate("layerApp.state().layers.some(layer=>layer.label==='Upgrade source')"),'The actual app adopts the released authored paint layer');
  const edited=await evaluate(`(async()=>({record:await sessionStoreFixture.snapshot(${JSON.stringify(imported.record.drawing)}),files:await sessionStoreFixture.files(${JSON.stringify(imported.record.drawing)})}))()`);
  assert.notEqual(edited.record.current.id,imported.record.baseline,'A real app checkpoint advances the OPFS drawing beyond the import baseline');
  assert.equal(edited.record.current.metadata.metadata.camera.zoom,.4375);
  const downgraded=JSON.parse(current);downgraded.metadata.camera.zoom=.625;downgraded.metadata.unsaved_name='Released downgrade branch';
  const downgradedMetadata=JSON.stringify(downgraded);
  await evaluate(`(async()=>{
    const {createRestartStore}=(()=>{${writer.replace(/^export /gm,'')};return {createRestartStore};})();
    window.releasedSessionFixture=createRestartStore();
    const record=await releasedSessionFixture.read(legacyFixtureKey);
    await releasedSessionFixture.publish(legacyFixtureKey,{generation:3,resources:record.current.resources,metadata:${JSON.stringify(downgradedMetadata)}},[],[],2);
    window.legacyFixtureDowngraded=await legacyFixtureRead();
  })()`);
  assert.deepEqual(await evaluate('layerApp.app.import_legacy_sessions()'),[]);
  const upgraded=await evaluate(`(async()=>{
    const record=(await sessionStoreFixture.index('imports')).current.metadata.records[legacyFixtureKey];
    return {record,drawing:await sessionStoreFixture.snapshot(record.drawing),unchanged:JSON.stringify(await legacyFixtureRead())===JSON.stringify(legacyFixtureDowngraded),windows:await sessionStoreFixture.windows()};
  })()`);
  assert.equal(upgraded.unchanged,true);assert.equal(upgraded.record.complete,true);
  assert.deepEqual(upgraded.record.source,{key:sourceKey,generation:3,sha256:digest(downgradedMetadata)});
  assert.notEqual(upgraded.record.drawing,imported.record.drawing,'A downgraded edit forks from the locally modified OPFS drawing');
  assert.equal(upgraded.record.baseline,upgraded.drawing.current.id);
  assert.equal(upgraded.drawing.current.metadata.metadata.camera.zoom,.625);
  assert.equal(upgraded.drawing.current.metadata.metadata.unsaved_name,'Released downgrade branch');
  assert.deepEqual(await evaluate(`(async()=>({record:await sessionStoreFixture.snapshot(${JSON.stringify(imported.record.drawing)}),files:await sessionStoreFixture.files(${JSON.stringify(imported.record.drawing)})}))()`),edited,'The original edited OPFS branch remains byte-for-byte intact');
  const keys=new Set(upgraded.windows.flatMap(({manifest})=>manifest?.drawings.map(drawing=>drawing.key)??[]));
  assert.ok(keys.has(imported.record.drawing)&&keys.has(upgraded.record.drawing),'Both edited branches retain recoverable window membership');
  assert.equal(await readFile(new URL(`generations/${head.current.id}.json`,fixture),'utf8'),current);
  assert.equal(await readFile(new URL(`generations/${head.previous.id}.json`,fixture),'utf8'),previous);
  console.log('PASS actual legacy importer upgrade/downgrade/upgrade: immutable source, explicit baseline/digest, idempotence and separate edited branches');
}
