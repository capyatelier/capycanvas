import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

// Exercise real browser pen input, presentation and history on the device GPU.
export async function checkContactBrushes({call, evaluate, settle}, photoUrl) {
  const wait = condition => evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+120000;function poll(){if(${condition})resolve();else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,30)}poll()})`);
  const action = async value => { await evaluate(`layerApp.dispatch(${JSON.stringify(value)})`); await settle(); };
  const invoke = command => action({type:'invoke',command});
  const directory = process.env.LAYER_TEST_ARTIFACTS || 'artifacts/contact-brushes/web';
  await mkdir(directory,{recursive:true});
  // Recovery starts asynchronously and can offer several archives. Finish its
  // prompts before injecting pen input, retaining every existing recovery.
  await evaluate(`(async()=>{
    const timer=setInterval(()=>[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click(),30);
    try { await layerApp.documents.startRecovery(); }
    finally { clearInterval(timer); }
  })()`);
  await wait("!document.querySelector('dialog[open]')");
  let photoTab;
  const saved = await evaluate('layerApp.state().settings');
  await action({type:'restore_settings',settings:{...saved,feedback:true,cursor:'none'}});
  await action({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
  await evaluate(`window.contactTestOpen=window.showOpenFilePicker`);
  try {
    {
      await evaluate(`(async()=>{let blob;if(${JSON.stringify(photoUrl || null)}){const response=await fetch(${JSON.stringify(photoUrl || null)});if(!response.ok)throw Error('Photo unavailable');blob=await response.blob();}else{const c=new OffscreenCanvas(2048,1536),x=c.getContext('2d');x.fillStyle='#c5b58c';x.fillRect(0,0,c.width,c.height);blob=await c.convertToBlob({type:'image/png'});}window.showOpenFilePicker=async()=>[{name:'brush-photo.jpg',async getFile(){return new File([blob],'brush-photo.jpg')}}]})()`);
      await invoke('open_document');
      await wait('!layerApp.state().document_file.busy && layerApp.app.brush_ready()');
      photoTab=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
      if(photoUrl) assert.ok(await evaluate('layerApp.state().tabs.some(t=>t.width===9504&&t.height===6336)'), '61 MP photo opened');
    }
    await invoke('fit_canvas');
    await action({type:'set_color',rgba:[0.08,0.015,0.25,1]});
    const region = await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1],width:Math.min(500,a[2]*r.width/c.viewport[0]*.7),height:Math.min(280,a[3]*r.height/c.viewport[1]*.55)}})()`);
    const shot = async () => {
      await settle();
      await evaluate('layerApp.canvas.getContext("webgpu").getConfiguration().device.queue.onSubmittedWorkDone()');
      await settle();
      const {data}=await call('Page.captureScreenshot',{format:'png',clip:{x:region.x-region.width/2,y:region.y-region.height/2,width:region.width,height:region.height,scale:1}});
      const pixels=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();const c=new OffscreenCanvas(image.width,image.height),x=c.getContext('2d');x.drawImage(image,0,0);return [...x.getImageData(0,0,c.width,c.height).data]})()`);
      return {data,pixels};
    };
    const delta=(a,b)=>a.reduce((n,v,i)=>n+(Math.abs(v-b[i])>3),0);
    const expectPixels = async (expected, label) => {
      const deadline=Date.now()+30000;
      let actual;
      do {
        actual=await shot();
        if(delta(expected,actual.pixels)===0)return;
      } while(Date.now()<deadline);
      await writeFile(`${directory}/history-mismatch.png`,Buffer.from(actual.data,'base64'));
      assert.equal(delta(expected,actual.pixels),0,label);
    };
    const presets=(process.env.LAYER_BRUSH_PRESETS || "2,25,26,27,1,28,29,30,31,32,33,34,3,4,5,6,7,9,15,16,17,18,8").split(",").map(Number);
    for (const id of presets) {
      await action({type:'select_brush',id});
      await action({type:'set_brush_size',value:photoUrl?1000:70});
      await wait('layerApp.app.brush_ready()');
      assert.equal(await evaluate('layerApp.state().brush.preset'),id);
      const before=await shot();
      for(let i=0;i<=48;i++) {
        const t=i/48,down=i<48;
        await call('Input.dispatchMouseEvent',{type:i===0?'mousePressed':down?'mouseMoved':'mouseReleased',pointerType:'pen',button:'left',buttons:down?1:0,clickCount:1,x:region.x+(t-.5)*region.width*.72,y:region.y+Math.sin(t*Math.PI*2)*region.height*.24,force:down?.2+.7*Math.sin(t*Math.PI):0,tiltX:id===26?40:0,tiltY:id===26?20:0});
        await settle();
      }
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:1,y:1,buttons:0});
      await wait(`layerApp.state().commands.find(c=>c.id==='undo')?.enabled`);
      const painted=await shot();
      assert.ok(delta(before.pixels,painted.pixels)>100,`Brush ${id} leaves a visible stroke`);
      await writeFile(`${directory}/${String(id).padStart(2,'0')}.png`,Buffer.from(painted.data,'base64'));
      await invoke('undo');
      await expectPixels(before.pixels,`Brush ${id}: undo restores pixels`);
      await invoke('redo');
      await expectPixels(painted.pixels,`Brush ${id}: redo restores stroke`);
      await invoke('undo');
      assert.equal(await evaluate('layerApp.state().host_error??null'),null);
      console.log(`Brush ${id}: visible pressure/curve stroke, undo/redo passed`);
    }
  } finally {
    await evaluate('window.showOpenFilePicker=contactTestOpen;delete window.contactTestOpen');
    await action({type:'restore_settings',settings:saved});
    if(photoTab!=null) {
      await evaluate(`layerApp.documents.close(BigInt(${photoTab}));null`);
      await settle();
      await evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Discard Changes')?.click()`);
      await wait(`!layerApp.state().tabs.some(t=>Number(t.id)===${photoTab})`);
    }
  }
}
