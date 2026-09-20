import assert from 'node:assert/strict';

// Caller opens an HDR fixture and waits for its first published guide.
export async function checkGpuTone({call,evaluate,settle}) {
  const status=()=>evaluate("JSON.parse(JSON.stringify(layerApp.app.tone_status(),(_,v)=>typeof v==='bigint'?Number(v):v))");
  const before=await status();assert.ok(before.ready&&before.retained);
  await evaluate(`(async()=>{window.toneControl=layerApp.app.capture_control();window.toneCandidate=await layerApp.app.tone_prepare(toneControl)})()`);
  const p=await evaluate(`(()=>{const r=layerApp.canvas.getBoundingClientRect(),c=layerApp.app.camera(),a=c.work_area;return{x:r.x+(a[0]+a[2]*.5)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]*.5)*r.height/c.viewport[1]}})()`);
  const pen=(type,x,y)=>call('Input.dispatchMouseEvent',{type,x,y,button:'left',buttons:type==='mouseReleased'?0:1,pointerType:'pen',force:type==='mouseReleased'?0:.6});
  try {
    await pen('mousePressed',p.x,p.y);
    await pen('mouseMoved',p.x+18,p.y+10);
    await evaluate('new Promise(r=>setTimeout(r,450))');
    const held=await status();assert.equal(held.idle,false);assert.equal(held.retained,true);assert.equal(held.publications,before.publications);
    assert.equal(await evaluate('toneControl.cancelled()'),true,'Pen contact immediately cancels queued guide work');
    assert.equal(await evaluate('layerApp.app.tone_apply(toneCandidate)'),false,'Late guide cannot publish during contact');
    await pen('mouseReleased',p.x+18,p.y+10);await settle();
    const start=performance.now();
    await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){const s=layerApp.app.tone_status();if(s.error)reject(Error(s.error));else if(s.ready&&s.publications>${before.publications})resolve();else if(performance.now()-start>60000)reject(Error('Guide did not refresh after pen up'));else setTimeout(poll,20)}poll()})`);
    const refreshed=await status();assert.equal(refreshed.retained,true);
    console.log('GPU tone retained during pen contact; late publication rejected; refreshed after pen up in',Math.round(performance.now()-start),'ms');
    await evaluate("layerApp.dispatch({type:'invoke',command:'undo'})");
    await evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){const s=layerApp.app.tone_status();if(s.ready&&s.publications>${refreshed.publications})resolve();else if(s.error||performance.now()-start>60000)reject(Error(s.error||'Undo guide timeout'));else setTimeout(poll,20)}poll()})`);
  } finally { await pen('mouseReleased',p.x+18,p.y+10);await evaluate('toneControl.free();delete window.toneControl;delete window.toneCandidate'); }
}
