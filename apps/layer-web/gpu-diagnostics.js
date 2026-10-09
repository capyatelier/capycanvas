export function gpuErrorText(error) {
  return String(error?.message ?? error).slice(0,4096);
}

export function createGpuDiagnostics() {
  const events=[];
  let last=null;
  const record=event=>{
    events.push({time:new Date().toISOString(),...event,message:gpuErrorText(event.message??'')});
    if(events.length>16)events.shift();
  };
  return {
    record,
    capture(error,read) {
      let context;
      try{context=read();}catch(failure){context={diagnostic_error:gpuErrorText(failure)};}
      const panic=events.findLast(event=>event.kind==='panic'&&!event.worker);
      last={time:new Date().toISOString(),error:panic?.message??gpuErrorText(error),context,events:events.slice()};
      return last;
    },
    text(report=last) {return JSON.stringify(report,(_,value)=>typeof value==='bigint'?value.toString():value,2);},
    get last(){return last;},
  };
}

export function suspendGpuForReport(report,suspend) {
  try{suspend();}catch(error){report.recovery_error=gpuErrorText(error);}
  return report;
}

export function gpuReportDetails({report,diagnostics,copy,element,button,clipboard}) {
  const details=element('details','gpu-report'),text=diagnostics.text(report);
  details.append(element('summary','',copy.failure_details));
  details.append(button(copy.copy_failure_details,async()=>{
    try{await clipboard.writeText(text);}catch{details.open=true;}
  }),element('pre','',text));
  return details;
}
