// Optional CDP pen replay for an EMPTY, dedicated refresh-test document.
// These measure event delivery and CPU submission, not physical display latency.
import { writeFile } from 'node:fs/promises';
import { performance as hostClock } from 'node:perf_hooks';

export async function replayRefreshStrokes({ call, evaluate, before, output, signal }) {
  const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
  async function wait(condition) {
    const deadline = Date.now() + 120000;
    while (Date.now() < deadline) {
      signal?.throwIfAborted();
      try { if (await evaluate(condition)) return; }
      catch (error) { if (!/context|navigat|layerApp/.test(String(error))) throw error; }
      await delay(50);
    }
    throw Error(`Stroke fixture timed out: ${condition}`);
  }
  try {
    await wait(`performance.timeOrigin !== ${before} && window.refreshTrace && window.layerApp?.app.brush_ready() && !document.querySelector('dialog[open]')`);
    const origin = await evaluate(`(()=>{
      if(layerApp.state().document_file.modified)throw Error('Stroke fixture requires an unmodified drawing');
      const camera=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=camera.work_area,k=r.width/camera.viewport[0];
      return {x:r.x+(a[0]+a[2]/2)*k,y:r.y+(a[1]+a[3]/2)*k,radius:Math.min(a[2],a[3])*.22*k,brush:layerApp.state().brush.preset};
    })()`);
    await evaluate(`(()=>{
      const a=window.strokeAudit={segments:[],frames:[],events:[],contact:false,last:null};
      a.originalFrame=layerApp.app.frame;a.originalPen=layerApp.app.pen;
      layerApp.app.pen=function(...args){for(let i=0;i<args[0].length;i+=11)if(!(args[0][i+9]&1))a.last=args[0][i+8];return a.originalPen.apply(this,args);};
      layerApp.app.frame=function(...args){const start=performance.now();try{return a.originalFrame.apply(this,args);}finally{if(a.contact)a.frames.push({start,end:performance.now(),input:a.last});}};
      a.signal=new AbortController();for(const type of ['pointerdown','pointermove','pointerrawupdate','pointerup'])layerApp.canvas.addEventListener(type,e=>{if(a.contact)a.events.push({type,time:e.timeStamp,arrival:performance.now(),coalesced:e.getCoalescedEvents?.().length??0});},{capture:true,signal:a.signal.signal});
    })()`);
    async function stroke(label, duration) {
      await evaluate(`strokeAudit.contact=true;strokeAudit.segments.push({label:${JSON.stringify(label)},start:performance.now(),complete:layerApp.startupTimes.complete});undefined`);
      const started = hostClock.now(); let sent = 0;
      const send = (type, seconds) => call('Input.dispatchMouseEvent', {
        type, x: origin.x + origin.radius * Math.sin(seconds * 3.2),
        y: origin.y + origin.radius * .65 * Math.sin(seconds * 4.7),
        button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, pointerType: 'pen',
        force: type === 'mouseReleased' ? 0 : .65, tiltX: 10, tiltY: 15,
      });
      await send('mousePressed', 0);
      try {
        while (hostClock.now() - started < duration) {
          signal?.throwIfAborted();
          await send('mouseMoved', (hostClock.now() - started) / 1000); sent++;
          await delay(Math.max(0, started + sent * 1000 / 120 - hostClock.now()));
        }
      } finally {
        await send('mouseReleased', (hostClock.now() - started) / 1000);
        await delay(150);
        await evaluate(`strokeAudit.contact=false;Object.assign(strokeAudit.segments.at(-1),{end:performance.now(),sent:${sent},undoEnabled:layerApp.state().commands.find(c=>c.id==='undo').enabled});layerApp.dispatch({type:'invoke',command:'undo'});undefined`);
      }
    }
    await stroke('during-startup', 7000);
    await wait('layerApp.startupTimes.complete != null && layerApp.app.document_park_ready()');
    await delay(500);
    await stroke('after-startup', 5000);
    await delay(500);
    const data = await evaluate('({segments:strokeAudit.segments,frames:strokeAudit.frames,events:strokeAudit.events,times:layerApp.startupTimes,modified:layerApp.state().document_file.modified})');
    data.origin = origin;
    await writeFile(output, JSON.stringify(data, null, 2));
    if (data.modified || data.segments.some(s => !s.undoEnabled)) throw Error('Stroke fixture did not paint and undo cleanly');
    return data.segments;
  } finally {
    await evaluate('if(window.strokeAudit){strokeAudit.signal.abort();layerApp.app.frame=strokeAudit.originalFrame;layerApp.app.pen=strokeAudit.originalPen;}undefined').catch(() => {});
  }
}
