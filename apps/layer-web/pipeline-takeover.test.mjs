import assert from 'node:assert/strict';

/** A synchronous edit that needs a pipeline while its asynchronous compile is
 * still in flight compiles it instead of finding the recipe gone. */
export async function checkPipelineTakeover({evaluate,settle}) {
  const wait=(expression,timeout=60000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};(function check(){try{if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}catch(e){reject(e)}})();})`);
  const click=label=>evaluate(`(()=>{const b=[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent===${JSON.stringify(label)});if(!b)throw Error('Missing '+${JSON.stringify(label)});b.click()})()`);
  await wait('layerApp.startupTimes.complete!==null',120000);
  await evaluate(`(()=>{const hold=Object.assign(document.createElement('div'),{id:'hold-optional-compiles',popover:'manual'});document.body.append(hold);hold.showPopover();})()`);
  await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'})`);
  await wait('!!document.querySelector("dialog[open]")');
  if(await evaluate('!![...document.querySelectorAll("dialog[open] button")].find(b=>b.textContent==="Discard Changes")'))await click('Discard Changes');
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Color space"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open]');d.querySelector('select[aria-label="Color space"]').value='DisplayP3';d.querySelector('select[aria-label="Bit depth"]').value='U8';})()`);
  await click('Create');
  await wait('!layerApp.documents.busy()&&!layerApp.state().document_file.busy&&layerApp.app.brush_ready()&&layerApp.app.document_color().space==="DisplayP3"',120000);
  await settle();
  await evaluate(`layerApp.dispatch({type:'invoke',command:'select_all'})`);
  await wait('layerApp.app.brush_ready()&&!layerApp.app.shader_work_pending(false)');
  await settle();
  const deselect=await evaluate(`new Promise((resolve,reject)=>{
    const app=layerApp.app,step=app.compile_startup_step,undo=()=>layerApp.state().commands.find(c=>c.id==='undo').enabled;
    app.compile_startup_step=function(optional){
      delete app.compile_startup_step;
      const compiling=step.call(this,optional);
      window.pipelineTakeoverStep=compiling;
      (async()=>{
        for(let i=0;i<32&&app.shader_work_pending(false);i++)await null;
        if(app.shader_work_pending(false))return reject(Error('The fill mask pipelines did not start compiling'));
        const started=performance.now();
        layerApp.dispatch({type:'invoke',command:'deselect'});
        resolve({ms:performance.now()-started,filled:undo()&&!layerApp.state().commands.find(c=>c.id==='deselect').enabled});
      })();
      return compiling;
    };
    layerApp.dispatch({type:'invoke',command:'fill_selection'});
    setTimeout(()=>reject(Error('No compile step followed Fill')),10000);
  })`);
  assert.equal(deselect.filled,true,'Deselect submitted the fill while its mask pipelines were compiling');
  await evaluate(`window.pipelineTakeoverStep.then(()=>{delete window.pipelineTakeoverStep;document.querySelector('#hold-optional-compiles').remove();})`);
  await wait('layerApp.app.brush_ready()');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'undo'})`);
  await settle();
  assert.equal(await evaluate(`layerApp.state().commands.find(c=>c.id==='deselect').enabled`),true,'Undo restores the selection');
  const compiled=await evaluate(`(()=>{layerApp.dispatch({type:'invoke',command:'fill_selection'});const started=performance.now();layerApp.dispatch({type:'invoke',command:'deselect'});return performance.now()-started})()`);
  console.log(`Deselect submitting a fill: ${deselect.ms.toFixed(1)} ms while its pipelines compiled asynchronously, ${compiled.toFixed(1)} ms once compiled`);
}
