import test from 'node:test';
import assert from 'node:assert/strict';
import {createGpuDiagnostics,gpuErrorText,suspendGpuForReport,gpuReportDetails} from './gpu-diagnostics.js';

test('failure report captures context before retirement and retains the original error when recovery fails',()=>{
  const diagnostics=createGpuDiagnostics();
  diagnostics.record({role:'canvas',kind:'out_of_memory',message:'Texture allocation failed'});
  let alive=true;
  const report=diagnostics.capture(new Error('Device lost'),()=>({transform:alive,resident_bytes:327000000n}));
  suspendGpuForReport(report,()=>{alive=false;throw new Error('Readback failed');});
  diagnostics.record({role:'canvas',kind:'device_lost',message:'Destroyed'});
  assert.equal(report.error,'Device lost');
  assert.equal(report.recovery_error,'Readback failed');
  assert.equal(report.context.transform,true);
  assert.equal(report.events.length,1);
  assert.equal(JSON.parse(diagnostics.text()).context.resident_bytes,'327000000');
});

test('diagnostics stay bounded and reporting survives an unavailable Wasm instance',()=>{
  const diagnostics=createGpuDiagnostics();
  for(let i=0;i<30;i++)diagnostics.record({role:'worker',kind:'validation',message:`${i}:`+'x'.repeat(6000)});
  const report=diagnostics.capture({stage:'renderer',message:'Original failure'},()=>{throw Error('Wasm unavailable');});
  assert.equal(report.error,'Original failure');
  assert.equal(report.context.diagnostic_error,'Wasm unavailable');
  assert.equal(report.events.length,16);
  assert.ok(report.events.every(event=>event.message.length===4096));
  assert.equal(gpuErrorText('plain error'),'plain error');
});

test('failure details copy exactly the visible report and remain available without clipboard permission',async()=>{
  const element=(tag,className='',text='')=>({tag,className,text,children:[],append(...children){this.children.push(...children);}});
  const button=(label,click)=>({label,click});
  const diagnostics=createGpuDiagnostics(),report=diagnostics.capture('GPU stopped',()=>({canvas:[2048,1536]}));
  const copy={failure_details:'Failure details',copy_failure_details:'Copy failure details'};
  let copied;
  const details=gpuReportDetails({report,diagnostics,copy,element,button,clipboard:{writeText:async text=>{copied=text;}}});
  await details.children[1].click();
  assert.equal(copied,details.children[2].text);
  const unavailable=gpuReportDetails({report,diagnostics,copy,element,button,clipboard:undefined});
  await unavailable.children[1].click();
  assert.equal(unavailable.open,true);
});
