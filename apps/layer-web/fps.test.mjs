import assert from 'node:assert/strict';

async function waitMainQuiet(milliseconds) {
  let since=performance.now();const end=since+30000;
  for(;;){
    const now=performance.now();since=Math.max(since,fpsTest.lastMainFrame??since);
    if(now>end)throw Error('Main canvas did not stay quiet within 30000 ms');
    if(now-since>=milliseconds)return layerApp.app.renderer_stats().rows.find(row=>row.label.includes('FPS'));
    if(now>=end)throw Error('Main canvas did not stay quiet within 30000 ms');
    await new Promise(resolve=>setTimeout(resolve,Math.min(30,milliseconds-(now-since),end-now)));
  }
}

function watchMainFrame() {
  const app=layerApp.app,original=app.frame;Object.assign(fpsTest,{app,originalFrame:original});
  app.frame=function(...args){
    const before=fpsTest.acquisitions,result=original.apply(this,args);
    if(fpsTest.acquisitions>before)fpsTest.lastMainFrame=performance.now();
    return result;
  };
}

export async function checkFrameRate({evaluate,settle}) {
  const inPage=(fn,...args)=>evaluate(`(${fn})(...${JSON.stringify(args)})`);
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;(function check(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(check,30)})()})`);
  await wait('layerApp.app.startup_progress().every(Boolean)&&!layerApp.app.canvas_work_pending()');
  await inPage(()=>{
    layerApp.dispatch({type:'customize',action:{type:'set_panel_visible',panel:'stats',visible:true}});
    const group=layerApp.app.layout(innerWidth,innerHeight).groups.find(g=>g.panels.includes('stats'));
    if(group&&group.active!=='stats')layerApp.dispatch({type:'select_panel_tab',group:group.id,panel:'stats'});
    const context=layerApp.canvas.getContext('webgpu'),original=context.getCurrentTexture;
    window.fpsTest={context,original,acquisitions:0,lastMainFrame:null};
    context.getCurrentTexture=function(...args){fpsTest.acquisitions++;return original.apply(this,args)};
  });
  await settle();
  await wait(`!![...document.querySelectorAll('.renderer-stats')].find(node=>node.getClientRects().length)`);
  const row=()=>inPage(()=>layerApp.app.renderer_stats().rows.find(row=>row.label.includes('FPS')));
  try {
    await inPage(watchMainFrame);
    for(const theme of ['light','dark']){
      await inPage(theme=>layerApp.dispatch({type:'set_theme',theme}),theme);
      await settle();
      const idle=await inPage(waitMainQuiet,1300);
      assert.equal(idle.label,'Submitted FPS');
      assert.match(idle.description,/submi/i);
      assert.match(idle.description,/presentation|display/i);
      assert.equal(idle.value,'0',`${theme}: submitted frames expire at idle`);
      await wait(`[...document.querySelectorAll('.renderer-stats .property-row')].some(row=>row.firstChild.textContent==='Submitted FPS'&&row.lastChild.textContent==='0')`);
      await inPage(()=>{
        fpsTest.row=[...document.querySelectorAll('.renderer-stats .property-row')].find(row=>row.firstChild.textContent.includes('FPS'));
        fpsTest.mutations=0;
        fpsTest.observer=new MutationObserver(records=>fpsTest.mutations+=records.length);
        fpsTest.observer.observe(fpsTest.row,{subtree:true,childList:true,attributes:true,characterData:true});
      });
      const callbacks=await inPage(()=>new Promise(resolve=>{
        const start=performance.now(),before=fpsTest.acquisitions;let callbacks=0;
        function tick(){callbacks++;layerApp.wake();if(performance.now()-start<1200)requestAnimationFrame(tick);else resolve({callbacks,acquisitions:fpsTest.acquisitions-before})}
        requestAnimationFrame(tick);
      }));
      assert(callbacks.callbacks>10,'The browser must continue animation callbacks');
      assert.equal(callbacks.acquisitions,0,'Animation callbacks and Stats polling do not publish the canvas');
      assert.equal((await row()).value,'0','Animation callback cadence is not the FPS counter');
      assert.equal(await inPage(()=>fpsTest.mutations),0,'Unchanged FPS values do not mutate their DOM row');
      await inPage(()=>fpsTest.observer.disconnect());
      const moving=await inPage(()=>new Promise(resolve=>{
        const camera=layerApp.app.camera(),area=camera.work_area,cx=area[0]+area[2]/2,cy=area[1]+area[3]/2;
        const start=performance.now(),before=fpsTest.acquisitions;let old={x:cx,y:cy,zoom:1};
        function tick(){
          const phase=(performance.now()-start)/200,x=cx+30*Math.sin(phase),y=cy+20*Math.cos(phase),zoom=Math.exp(.1*Math.sin(phase));
          layerApp.app.gesture(old.x,old.y,x,y,zoom/old.zoom,0);layerApp.wake();old={x,y,zoom};
          if(performance.now()-start<1400)requestAnimationFrame(tick);else requestAnimationFrame(()=>resolve({acquisitions:fpsTest.acquisitions-before,row:layerApp.app.renderer_stats().rows.find(row=>row.label.includes('FPS'))}));
        }
        requestAnimationFrame(tick);
      }));
      assert(moving.acquisitions>10,'Cached pan and zoom publish the main canvas');
      assert.equal(moving.row.label,'Submitted FPS');
      assert(Number(moving.row.value)>0,'Cached navigation has a positive submitted FPS');
      assert.equal((await inPage(waitMainQuiet,1400)).value,'0',`${theme}: the rolling rate retains the idle gap`);
      await inPage(()=>{fpsTest.retiredRow=fpsTest.row;layerApp.app.suspend_gpu()});
      assert.equal(await inPage(()=>layerApp.app.gpu_ready()),false);
      assert.equal((await row()).value,'—','A retired surface does not retain its FPS observation');
      await wait(`fpsTest.row.lastChild.textContent==='—'`);
      const retired=await row();
      assert(await inPage(()=>fpsTest.row===fpsTest.retiredRow&&fpsTest.row.isConnected),'FPS source changes retain the existing row');
      assert.equal(await inPage(()=>fpsTest.row.firstChild.textContent),retired.label,'The DOM label follows the current FPS source');
      assert.equal(await inPage(()=>fpsTest.row.title),retired.description,'The DOM description follows the current FPS source');
      console.log('Submitted FPS:',JSON.stringify({theme,idle,callbacks,moving}));
      if(theme==='light'){
        await inPage(()=>{fpsTest.context.getCurrentTexture=fpsTest.original;fpsTest.app.frame=fpsTest.originalFrame});
        await inPage(()=>layerApp.restartGpu());
        await wait('layerApp.app.startup_progress().every(Boolean)&&!layerApp.app.canvas_work_pending()');
        await inPage(()=>{
          const context=layerApp.canvas.getContext('webgpu'),original=context.getCurrentTexture;
          Object.assign(fpsTest,{context,original,acquisitions:0,lastMainFrame:null});
          context.getCurrentTexture=function(...args){fpsTest.acquisitions++;return original.apply(this,args)};
        });
        await inPage(watchMainFrame);
      }
    }
  } finally {
    await inPage(()=>{fpsTest.observer?.disconnect();fpsTest.context.getCurrentTexture=fpsTest.original;fpsTest.app.frame=fpsTest.originalFrame;delete window.fpsTest});
  }
}
