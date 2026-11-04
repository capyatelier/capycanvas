import assert from 'node:assert/strict';

export async function checkDocumentErrors({evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+20000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20);}poll();})`);
  const epoch=await evaluate('String(layerApp.app.state().document_file.epoch)');
  const initial=await evaluate('layerApp.app.document_tabs(1000).tabs.map(tab=>String(tab.id))');
  await evaluate(`window.documentFailure=null;void layerApp.documents.openFiles([new File(['invalid'],'own-broken.capy')]).then(()=>{documentFailure={resolved:true}},error=>{documentFailure={type:error?.constructor?.name,message:error?.message??String(error)}});`);
  await wait(`!!document.querySelector('#file-launch-error[open]')`);
  assert.deepEqual(await evaluate(`(()=>{const dialog=document.querySelector('#file-launch-error');return{role:dialog.getAttribute('role'),modal:dialog.getAttribute('aria-modal'),pending:documentFailure===null,text:dialog.textContent.includes('own-broken.capy')}})()`),{role:'alertdialog',modal:'true',pending:true,text:true},'Corrupt opening presents the shared alert before rejecting');
  await evaluate(`[...document.querySelectorAll('#file-launch-error button')].find(button=>button.textContent===layerApp.app.bootstrap_view().ok).click()`);
  await wait('documentFailure!==null');
  const failure=await evaluate('(()=>{const result=documentFailure;delete window.documentFailure;return result})()');
  assert.ok(failure?.message,'corrupt opening rejects with its raw diagnostic');
  await settle();
  assert.deepEqual(await evaluate('layerApp.app.document_tabs(1000).tabs.map(tab=>String(tab.id))'),initial,'caught corrupt opening releases the Wasm borrow and preserves drawings');
  const literal='Own İı ไทย Tiếng Việt 雪🎨 {diagnostic}';
  assert.equal(await evaluate(`layerApp.app.color_feature_error_copy(new Error(${JSON.stringify(literal)}))`),literal);
  assert.equal(await evaluate(`layerApp.app.color_feature_error_copy(new DOMException(${JSON.stringify(literal)},'QuotaExceededError'))`),literal);
  assert.equal(await evaluate(`layerApp.app.color_feature_error_copy(${JSON.stringify(literal)})`),literal);
  assert.equal(await evaluate(`layerApp.app.color_feature_error_copy({z:2,a:'own'})`),JSON.stringify({a:'own',z:2}));
  assert.equal(await evaluate('String(layerApp.app.state().document_file.epoch)'),epoch,'copy formatting leaves the next actual Wasm method callable and epoch unchanged');
  await evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:JSON.stringify({...layerApp.state().settings,language:{Explicit:'fr'}})})`);
  await wait(`!layerApp.app.language_pending()&&document.documentElement.lang==='fr'`);
  assert.equal(await evaluate(`layerApp.app.color_feature_error_copy(new Error(${JSON.stringify(literal)}))`),literal,'unexpected diagnostics stay literal after publication');
  assert.deepEqual(await evaluate('layerApp.app.document_tabs(1000).tabs.map(tab=>String(tab.id))'),initial);
  console.log('PASS corrupt opening and raw Error/string/object copy release Wasm borrows, preserve drawings and literal diagnostic text');
}
