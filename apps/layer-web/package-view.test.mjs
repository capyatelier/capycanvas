import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {packageManifest,packageOccurrences} from './package-fixture.test.mjs';

function preservedFixture(bytes, preview, withPreview) {
  return execFileSync('python3', ['-c', `
import base64, io, json, sys, zipfile
request=json.load(sys.stdin)
source=zipfile.ZipFile(io.BytesIO(base64.b64decode(request['source'])))
manifest=json.loads(source.read('manifest.json'))
next(record for record in manifest['objects'] if record['type'] in ['capy.occurrence/2','capy.occurrence/3'])['data']['blend']='future-package-blend'
for record in manifest['objects']:
    if record['type']=='capy.output/1':
        record['data']['name']='Package preview output'
        record['data'].pop('representation',None)
        if request['preview'] and record['id']==manifest['default_output']['ref']:
            record['data']['representation']={'member':'preview.png','size':[2,1],'color':'srgb'}
result=io.BytesIO()
with zipfile.ZipFile(result,'w',compression=zipfile.ZIP_STORED) as output:
    for entry in source.infolist():
        if entry.filename=='preview.png': continue
        payload=json.dumps(manifest,separators=(',',':')).encode() if entry.filename=='manifest.json' else source.read(entry)
        info=zipfile.ZipInfo(entry.filename);info.create_system=0;info.extract_version=20
        output.writestr(info,payload)
    if request['preview']:
        info=zipfile.ZipInfo('preview.png');info.create_system=0;info.extract_version=20
        output.writestr(info,base64.b64decode(request['preview']))
sys.stdout.buffer.write(result.getvalue())
`], {input:JSON.stringify({source:Buffer.from(bytes).toString('base64'),preview:withPreview?Buffer.from(preview).toString('base64'):null}),maxBuffer:32*1024*1024});
}

export async function checkPackageView({evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,25);}poll();})`);
  const ready=()=>wait('layerApp.app.brush_ready()&&!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.document_park_ready()');
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  const snapshot=()=>evaluate(`(()=>{const s=layerApp.state(),f=s.document_file,t=layerApp.app.document_tabs(1000);return JSON.parse(JSON.stringify({file:{epoch:f.epoch,revision:f.revision,modified:f.modified,location:f.location},layers:s.layers,selected:t.selected,tabs:t.tabs.map(t=>t.id)},(_,v)=>typeof v==='bigint'?String(v):v));})()`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open].document-dialog button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b||b.disabled)throw Error('Missing package action '+${JSON.stringify(label)});b.click();})()`);
  const launch=async name=>{
    await evaluate(`(()=>{packageViewTest.model=null;packageViewTest.outcome=null;const file=new File([packageViewTest.inputs.get(${JSON.stringify(name)})],${JSON.stringify(name)});packageViewTest.opening=layerApp.documents.openFiles([{file,handle:packageViewTest.originalHandle}]).then(()=>packageViewTest.outcome={ok:true},error=>packageViewTest.outcome={error:String(error),name:error.name});})()`);
  };
  const open=async name=>{await ready();await launch(name);await wait("packageViewTest.model&&!!document.querySelector('dialog[open].document-dialog')");return evaluate('packageViewTest.model');};
  const close=async model=>{await click(model.close);await wait('packageViewTest.outcome!==null');assert.deepEqual(await evaluate('packageViewTest.outcome'),{ok:true});await ready();};
  const output=async name=>Buffer.from(await evaluate(`Array.from(packageViewTest.files.get(${JSON.stringify(name)}))`));
  const saveAction=async label=>{const attempt=await evaluate('packageViewTest.attempts');await click(label);await wait(`packageViewTest.attempts>${attempt}&&packageViewTest.settled===packageViewTest.attempts`);};
  await wait('window.layerApp?.app.brush_ready()');await ready();
  const theme=await evaluate('document.body.dataset.theme');
  await evaluate(`window.packageViewTest={files:new Map(),inputs:new Map(),attempts:0,settled:0,writes:0,aborts:0,mode:'save',openPicker:window.showOpenFilePicker,savePicker:window.showSaveFilePicker,view:layerApp.app.package_view.bind(layerApp.app),prepare:layerApp.app.prepare_document.bind(layerApp.app)};
    packageViewTest.originalHandle={name:'original.capy',async isSameEntry(other){return other===this},async createWritable(){packageViewTest.writes++;throw Error('Original package must remain untouched')}};
    layerApp.app.package_view=candidate=>{const model=packageViewTest.view(candidate);if(model)packageViewTest.model=model;return model};
    window.showSaveFilePicker=async options=>{
      const attempt=++packageViewTest.attempts;
      if(packageViewTest.mode==='cancel'){packageViewTest.settled=attempt;throw new DOMException('Cancelled','AbortError')}
      if(packageViewTest.mode==='original'){packageViewTest.settled=attempt;return packageViewTest.originalHandle}
      return {name:options.suggestedName,async createWritable(){let bytes;packageViewTest.writes++;return {
        async write(value){if(packageViewTest.mode==='fail')throw Error('Injected package destination failure');bytes=new Uint8Array(value instanceof Blob?await value.arrayBuffer():value)},
        async close(){packageViewTest.files.set(options.suggestedName,bytes);packageViewTest.settled=attempt},
        async abort(){packageViewTest.aborts++;packageViewTest.settled=attempt},
      }}};
    };`);
  try {
    await invoke('save_document_as');await ready();
    const source=await evaluate('Array.from([...packageViewTest.files.values()].at(-1))');
    const sourceManifest=await packageManifest(Uint8Array.from(source));
    assert.ok(packageOccurrences(sourceManifest).length>0);
    const preview=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAAXNSR0IB2cksfwAAAA5JREFUeJxj+M/A8B8EARD4A/1OlcFvAAAAAElFTkSuQmCC','base64');
    const preserved=preservedFixture(source,preview,true),withoutPreview=preservedFixture(source,preview,false);
    const manifest=await packageManifest(preserved),outputs=manifest.outputs.map(ref=>manifest.objects.find(o=>o.id===ref.ref)).map(o=>({id:o.id,name:o.data.name}));
    for(const [name,bytes]of [['unsupported.capy',preserved],['source-only.capy',withoutPreview],['editable.capy',source]])await evaluate(`packageViewTest.inputs.set(${JSON.stringify(name)},Uint8Array.from(atob(${JSON.stringify(Buffer.from(bytes).toString('base64'))}),c=>c.charCodeAt(0)))`);
    await invoke('add_layer');await settle();await ready();
    const incumbent=await snapshot();
    assert.equal(incumbent.file.modified,true);
    for(const currentTheme of ['light','dark']) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(currentTheme)}})`);await settle();
      assert.equal(await evaluate('document.body.dataset.theme'),currentTheme);
      const model=await open('unsupported.capy');
      assert.equal(model.disposition,'preserved');assert.deepEqual(model.extent,[2,1],JSON.stringify(model));assert.deepEqual(model.outputs,outputs);
      assert.deepEqual(model.capabilities,{view:true,copy_original:true,edit:false,save:false,export:true});
      assert.match(model.reason,/blend/i);
      assert.deepEqual(await snapshot(),incumbent,'Viewing a preserved package keeps the incumbent editor and dirty history');
      assert.deepEqual(await evaluate(`Array.from(document.querySelector('dialog[open]').querySelectorAll('p')).slice(1).map(p=>p.textContent)`),outputs.map(o=>o.name));
      await wait("document.querySelector('dialog[open] img')?.complete&&document.querySelector('dialog[open] img').naturalWidth===2");
      assert.deepEqual(await evaluate(`(async()=>{const image=document.querySelector('dialog[open] img');await image.decode();const bitmap=await createImageBitmap(await(await fetch(image.src)).blob());try{const canvas=document.createElement('canvas');canvas.width=2;canvas.height=1;const context=canvas.getContext('2d',{willReadFrequently:true,colorSpace:'srgb'});context.drawImage(bitmap,0,0);return Array.from(context.getImageData(0,0,2,1).data)}finally{bitmap.close()}})()`),[255,0,0,255,0,255,0,255]);
      await saveAction(model.copy_original);assert.deepEqual(await output('unsupported.capy'),preserved,'Copy Original preserves every archive byte');
      await saveAction(model.export_preview);assert.deepEqual(await output('unsupported.png'),Buffer.from(preview),'Export Preview Image copies the verified PNG exactly');
      for(const mode of ['cancel','fail','original']) {
        const before=await evaluate('({files:packageViewTest.files.size,writes:packageViewTest.writes,aborts:packageViewTest.aborts})');
        await evaluate(`packageViewTest.mode=${JSON.stringify(mode)}`);await saveAction(mode==='original'?model.export_preview:model.copy_original);
        assert.equal(await evaluate('packageViewTest.files.size'),before.files);
        if(mode==='fail')assert.equal(await evaluate('packageViewTest.aborts'),before.aborts+1,'A failed destination aborts its partial write');
        else assert.equal(await evaluate('packageViewTest.writes'),before.writes,'Cancel and preview overwrite refusal never open a writer');
        assert.equal(await evaluate("!!document.querySelector('dialog[open] img')"),true);
        assert.deepEqual(await snapshot(),incumbent,'Failed publication retains both the package and incumbent');
      }
      await evaluate("packageViewTest.mode='save'");await saveAction(model.copy_original);assert.deepEqual(await output('unsupported.capy'),preserved,'Retry copies the original after failed publication');
      await close(model);assert.deepEqual(await snapshot(),incumbent);
      const sourceOnly=await open('source-only.capy');
      assert.equal(sourceOnly.disposition,'preserved');assert.equal(sourceOnly.extent??null,null);assert.deepEqual(sourceOnly.outputs,outputs);
      assert.deepEqual(sourceOnly.capabilities,{view:false,copy_original:true,edit:false,save:false,export:false});
      assert.equal(await evaluate("document.querySelectorAll('dialog[open] img').length"),0);
      assert.equal(await evaluate("document.querySelectorAll('dialog[open] footer button').length"),2);
      await saveAction(sourceOnly.copy_original);assert.deepEqual(await output('source-only.capy'),withoutPreview);
      await close(sourceOnly);assert.deepEqual(await snapshot(),incumbent);
    }
    for(const cancelled of [true,false]) {
      await evaluate(`packageViewTest.prepared=false;packageViewTest.gate=new Promise(resolve=>packageViewTest.release=resolve);layerApp.app.prepare_document=async(...args)=>{const candidate=await packageViewTest.prepare(...args);packageViewTest.prepared=true;await packageViewTest.gate;return candidate};`);
      await launch(cancelled?'unsupported.capy':'editable.capy');await wait('packageViewTest.prepared');
      if(cancelled)await evaluate("document.querySelector('.file-progress button').click()");
      else {await evaluate("layerApp.app.dispatch({type:'invoke',command:'add_layer'});layerApp.wake()");await settle();}
      const retained=await snapshot();
      await evaluate('packageViewTest.release()');await wait('packageViewTest.outcome!==null');
      const outcome=await evaluate('packageViewTest.outcome');
      if(cancelled){assert.deepEqual(outcome,{ok:true},'Cancelling a batch open resolves without adopting a drawing');assert.equal(await evaluate('packageViewTest.model'),null,'Cancelled preparation never publishes its package viewer');}
      else assert.match(outcome.error,/drawing changed while opening/i);
      await evaluate('layerApp.app.prepare_document=packageViewTest.prepare');await ready();
      assert.equal(await evaluate("!!document.querySelector('dialog[open].document-dialog')"),false);
      assert.deepEqual(await snapshot(),retained,'Cancelled and stale preparation cannot replace the incumbent');
      if(!cancelled){await invoke('undo');await settle();await ready();const undone=await snapshot();assert.deepEqual(undone.layers,incumbent.layers,'The incumbent retains its own undo after a stale open');assert.equal(undone.file.epoch,incumbent.file.epoch);assert.equal(undone.file.modified,true);assert.deepEqual(undone.tabs,incumbent.tabs);}
    }
    console.log('PASS package view: both themes, verified preview and output inventory, exact original/PNG copy, source-only open, cancellation, failed write/retry, overwrite refusal, stale open and incumbent history');
  } finally {
    await evaluate(`layerApp.app.package_view=packageViewTest.view;layerApp.app.prepare_document=packageViewTest.prepare;window.showOpenFilePicker=packageViewTest.openPicker;window.showSaveFilePicker=packageViewTest.savePicker;packageViewTest.release?.();document.querySelector('dialog[open].document-dialog')?.close();layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});`);
  }
}
