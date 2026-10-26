export function createPanelVectorQueue(project,failed) {
  let pending,ready,running=false,publishing=false,epoch=0,sequence=0,published=0,contentPending=false;
  function publish(request,result) {
    if(request.epoch!==epoch||request.sequence<=published)return;
    if(publishing){ready={request,result};return;}
    published=request.sequence;
    const only=request.layoutOnly&&!contentPending;
    publishing=true;
    try {
      request.commit(request.layout,result,only);
      if(request.epoch===epoch)contentPending=false;
    } catch(error) {if(request.epoch===epoch)failed(error);}
    finally {
      publishing=false;
      const latest=ready;ready=null;
      if(latest)publish(latest.request,latest.result);
    }
  }
  async function drain() {
    if(running||!pending)return;
    const request=pending;pending=null;
    running=true;
    try {
      const result=await project(request.layout);
      publish(request,result);
    } catch(error) {if(request.epoch===epoch&&request.sequence>published)failed(error);}
    finally {running=false;drain();}
  }
  return (layout,layoutOnly,commit)=>{
    if(!layoutOnly){epoch++;contentPending=true;}
    const request={layout,commit,epoch,sequence:++sequence,layoutOnly};
    if(!layout.collapsed.some(c=>c.open)) {
      epoch++;pending=null;
      request.epoch=epoch;publish(request,{columns:[],triangles:new Float32Array()});
      return;
    }
    pending=request;
    drain();
  };
}
