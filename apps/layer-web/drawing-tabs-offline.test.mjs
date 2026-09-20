import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

// Actual tablet presentation check: bypass the browser HTTP cache and reload
// the packaged app with networking disabled, then exchange two GPU editors.
export async function checkDrawingTabsOffline({call,evaluate,settle}) {
  const wait=condition=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+240000;function poll(){if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,50);}poll();})`);
  const ready=()=>wait('window.layerApp?.startupTimes.complete!=null&&layerApp.app.brush_ready()&&!layerApp.documents.busy()&&layerApp.app.document_park_ready()');
  const invoke=command=>evaluate(`layerApp.dispatch({type:'invoke',command:${JSON.stringify(command)}})`);
  await evaluate('navigator.serviceWorker.ready.then(()=>true)');
  await wait('!!navigator.serviceWorker.controller');
  console.log('Offline check: service worker controls the packaged page');
  assert.deepEqual((await call('Page.getInstallabilityErrors')).installabilityErrors,[]);
  await call('Network.enable');
  await call('Network.setCacheDisabled',{cacheDisabled:true});
  try {
    await call('Network.emulateNetworkConditions',{offline:true,latency:0,downloadThroughput:-1,uploadThroughput:-1});
    const previous=await evaluate('performance.timeOrigin');
    // A CDP hard reload also bypasses service-worker control in Chrome. Disable
    // the HTTP cache separately, while preserving normal PWA navigation.
    await call('Page.reload',{ignoreCache:false});
    console.log('Offline check: navigation requested');
    const end=Date.now()+30000;
    for(;;){
      try{if(await evaluate(`performance.timeOrigin!==${previous}&&document.readyState==='complete'`))break;}
      catch(error){if(!/context|navigated/i.test(String(error)))throw error;}
      if(Date.now()>end)throw Error('Offline navigation timed out');
      await new Promise(resolve=>setTimeout(resolve,50));
    }
    await ready();
    console.log('Offline check: cold canvas is ready');
    // Android Chrome can reset navigator.onLine on navigation while CDP still
    // blocks requests. Prove the network restriction with an uncached URL that
    // the service worker deliberately does not handle (including a 404 would
    // count as online). Only this expected request error is exempted by the host.
    assert.equal(await evaluate(`fetch('./__capy-tabs-offline-probe?'+Date.now(),{cache:'no-store'}).then(()=>false,()=>true)`),true,'Uncached requests must fail after offline navigation');
    await evaluate("layerApp.dispatch({type:'select_brush',id:1});layerApp.dispatch({type:'set_theme',theme:'light'})");
    await invoke('fit_canvas');await ready();await settle();
    const first=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
    const p=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    const pixels=async()=>{
      const shot=await call('Page.captureScreenshot',{format:'png',clip:{x:p.x-70,y:p.y-25,width:180,height:70,scale:1}});
      return evaluate(`(async()=>{const i=new Image();i.src='data:image/png;base64,${shot.data}';await i.decode();const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);const data=x.getImageData(0,0,c.width,c.height).data;let white=0;for(let k=0;k<data.length;k+=4)if(data[k]>245&&data[k+1]>245&&data[k+2]>245)white++;return white;})()`);
    };
    const blank=await pixels();
    for(const [type,dx,buttons] of [['mousePressed',-50,1],['mouseMoved',0,1],['mouseMoved',80,1],['mouseReleased',80,0]]){
      await call('Input.dispatchMouseEvent',{type,x:p.x+dx,y:p.y,button:'left',buttons,clickCount:1,pointerType:'pen',force:buttons?.7:0});await settle();
    }
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:1,y:1,buttons:0,pointerType:'pen'});
    await ready();await settle();
    assert.equal(await evaluate('layerApp.state().document_file.modified'),true);
    const ink=await pixels();assert.ok(ink<blank-50,`Offline GPU ink must present (${blank} -> ${ink} white pixels)`);
    await invoke('new_document');
    await wait(`!![...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Create')`);
    await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(n=>n.textContent==='Create').click()`);await ready();
    assert.equal(await evaluate('layerApp.app.document_tabs(0).tabs.length'),2);
    await evaluate(`layerApp.documents.select(BigInt(${first}))`);await ready();await settle();
    assert.equal(await pixels(),ink,'Returning to the parked drawing restores the same presented ink');
    await mkdir('artifacts/document-tabs/web',{recursive:true});
    const shot=await call('Page.captureScreenshot',{format:'png'});
    await writeFile('artifacts/document-tabs/web/huion-packaged-offline-tabs.png',Buffer.from(shot.data,'base64'));
    console.log('PASS packaged offline tabs: service-worker cold load, real pen ink, GPU retirement/restoration, exact presented ink', {blank,ink});
  } finally {
    await call('Network.emulateNetworkConditions',{offline:false,latency:0,downloadThroughput:-1,uploadThroughput:-1});
    await call('Network.setCacheDisabled',{cacheDisabled:false});
  }
}
