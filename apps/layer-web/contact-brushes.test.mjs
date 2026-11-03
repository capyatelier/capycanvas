import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';

// Exercise real browser pen input, presentation and history on the device GPU.
export async function checkContactBrushes({call, evaluate, settle, penTilt=true}, photoUrl) {
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
  let photoTab;const historyFailures=[];
  const saved = await evaluate('layerApp.state().settings');
  await action({type:'restore_settings',settings:{...saved,feedback:true,cursor:'none'}});
  await action({type:'preferences',action:{type:'edit',id:'missing_profile',value:0}});
  await evaluate(`window.contactTestOpen=window.showOpenFilePicker`);
  try {
    {
      await evaluate(`(async()=>{let blob;if(${JSON.stringify(photoUrl || null)}){const response=await fetch(${JSON.stringify(photoUrl || null)});if(!response.ok)throw Error('Photo unavailable');blob=await response.blob();}else{const c=new OffscreenCanvas(2048,1536),x=c.getContext('2d',{willReadFrequently:true});x.fillStyle='#c5b58c';x.fillRect(0,0,c.width,c.height);blob=await c.convertToBlob({type:'image/png'});}window.showOpenFilePicker=async()=>[{name:'brush-photo.jpg',async getFile(){return new File([blob],'brush-photo.jpg')}}]})()`);
      await invoke('open_document');
      await wait('!layerApp.documents.busy() && !layerApp.state().document_file.busy && layerApp.app.brush_ready()');
      photoTab=await evaluate('Number(layerApp.app.document_tabs(0).selected)');
      if(photoUrl) assert.ok(await evaluate('layerApp.state().tabs.some(t=>t.width===9504&&t.height===6336)'), '61 MP photo opened');
    }
    await invoke('fit_canvas');
    await action({type:'set_color',rgba:[0.08,0.015,0.25,1]});
    const region = await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1],width:Math.min(500,a[2]*r.width/c.viewport[0]*.7),height:Math.min(280,a[3]*r.height/c.viewport[1]*.55)}})()`);
    const theme=await evaluate('layerApp.state().theme');
    await evaluate(`window.contactTiltInput=null;layerApp.canvas.addEventListener('pointermove',event=>{window.contactTiltInput={kind:event.pointerType,x:event.tiltX,y:event.tiltY}},{once:true})`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:region.x,y:region.y,buttons:0,tiltX:50.76848,tiltY:50.76848});
    const deliveredTilt=await evaluate("contactTiltInput?.kind==='pen' && contactTiltInput.x>0 && contactTiltInput.y>0");
    await evaluate('delete window.contactTiltInput');
    if(penTilt)assert.ok(deliveredTilt,'The pen adapter must deliver native DOM pen tilt');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:1,y:1,buttons:0});
    console.log(`Contact fixture: ${theme}, ${penTilt?'pen pressure and tilt':'mouse input'}`);
    const shot = async () => {
      await wait('layerApp.app.drawing_presented() && !layerApp.app.canvas_work_pending()');
      await settle();
      await evaluate('layerApp.canvas.getContext("webgpu").getConfiguration().device.queue.onSubmittedWorkDone()');
      await settle();
      const {data}=await call('Page.captureScreenshot',{format:'png',clip:{x:region.x-region.width/2,y:region.y-region.height/2,width:region.width,height:region.height,scale:1}});
      const image=await evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();const c=new OffscreenCanvas(image.width,image.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);return {pixels:[...x.getImageData(0,0,c.width,c.height).data],width:image.width,height:image.height}})()`);
      return {data,...image};
    };
    const delta=(a,b)=>a.reduce((n,v,i)=>n+(v!==b[i]),0);
    const expectPixels = async (expected, label) => {
      const deadline=Date.now()+30000;
      let actual;
      do {
        actual=await shot();
        if(delta(expected,actual.pixels)===0)return;
      } while(Date.now()<deadline);
      const changed=delta(expected,actual.pixels);
      await writeFile(`${directory}/${label.replace(/[^a-z0-9]+/gi,'-')}.png`,Buffer.from(actual.data,'base64'));
      historyFailures.push({label,changed});console.error(`${label}: ${changed} channels differ exactly`);
    };
    const stroke = async (id, angles=[0,0], stationary=false) => {
      for(let i=0;i<=48;i++) {
        const t=i/48,down=i<48;
        await call('Input.dispatchMouseEvent',{type:i===0?'mousePressed':down?'mouseMoved':'mouseReleased',pointerType:'pen',button:'left',buttons:down?1:0,clickCount:1,x:region.x+(stationary?0:(t-.5)*region.width*.72),y:region.y+(stationary?0:Math.sin(t*Math.PI*2)*region.height*.24),force:down?.2+.7*Math.sin(t*Math.PI):0,tiltX:angles[0],tiltY:angles[1]});
        await settle();
      }
      await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:1,y:1,buttons:0});
    };
    const presets=(process.env.LAYER_BRUSH_PRESETS || "2,25,26,27,43,1,28,29,30,31,32,33,34,3,4,35,5,6,7,9,15,16,17,18,8").split(",").map(Number);
    for (const id of presets) {
      // Erase real paint above the retained photo, then remove that seed too.
      if(id===3){
        await action({type:'select_brush',id:1});
        await action({type:'set_brush_size',value:photoUrl?1400:100});
        await wait('layerApp.app.brush_ready()');await stroke(1);
        await wait('layerApp.app.document_park_ready()');
      }
      await action({type:'select_brush',id});
      await action({type:'set_brush_size',value:photoUrl?1000:70});
      await wait('layerApp.app.brush_ready()');
      assert.equal(await evaluate('layerApp.state().brush.preset'),id);
      const poses=[['0',[0,0]]];
      if(penTilt&&[2,25,26,27,43,6,17].includes(id))poses.push(['40',[0,40]],['60',[0,60]]);
      if(penTilt&&[2,27].includes(id))poses.push(['60-diagonal',[50.76848,50.76848]]);
      for(const [tilt,angles] of poses){
        const label=`${theme} Brush ${id} tilt${tilt}`;
        const before=await shot();
        if(tilt==='60-diagonal'){
          await action({type:'preferences',action:{type:'edit',id:'cursor',value:7}});
          await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:region.x,y:region.y,buttons:0,tiltX:angles[0],tiltY:angles[1]});
          const cursor=await shot();
          let xx=0,xy=0,yy=0,count=0;
          for(let i=0;i<cursor.pixels.length;i+=4)if(cursor.pixels.slice(i,i+3).some((v,c)=>Math.abs(v-before.pixels[i+c])>16)){
            const x=(i/4)%cursor.width-cursor.width/2,y=Math.floor(i/4/cursor.width)-cursor.height/2;
            xx+=x*x;xy+=x*y;yy+=y*y;count++;
          }
          assert.ok(count>20&&xy/(xx+yy)>.2&&Math.abs(xx-yy)/(xx+yy)<.12,`${label}: the GPU cursor follows the physical pen (${count}, ${xx}/${xy}/${yy})`);
          await writeFile(`${directory}/${theme}-${String(id).padStart(2,'0')}-tilt${tilt}-cursor.png`,Buffer.from(cursor.data,'base64'));
          await action({type:'preferences',action:{type:'edit',id:'cursor',value:0}});
          await call('Input.dispatchMouseEvent',{type:'mouseMoved',pointerType:'pen',x:1,y:1,buttons:0});
        }
        await stroke(id,angles,tilt==='60-diagonal');
        await wait(`layerApp.state().commands.find(c=>c.id==='undo')?.enabled`);
        // Wait for the committed native capture, not just an available Undo.
        await wait('layerApp.app.document_park_ready()');
        const painted=await shot();
        assert.ok(delta(before.pixels,painted.pixels)>100,`${label} leaves a visible stroke`);
        if(tilt==='60-diagonal'){
          let xx=0,xy=0,yy=0;
          for(let i=0;i<painted.pixels.length;i+=4){
            const weight=painted.pixels.slice(i,i+3).reduce((sum,v,c)=>sum+Math.abs(v-before.pixels[i+c]),0);
            const x=(i/4)%painted.width-painted.width/2,y=Math.floor(i/4/painted.width)-painted.height/2;
            xx+=weight*x*x;xy+=weight*x*y;yy+=weight*y*y;
          }
          assert.ok(xy/(xx+yy)>.1&&Math.abs(xx-yy)/(xx+yy)<.2,`${label}: diagonal pigment follows the cursor (${xx}/${xy}/${yy})`);
        }
        await writeFile(`${directory}/${theme}-${String(id).padStart(2,'0')}-tilt${tilt}.png`,Buffer.from(painted.data,'base64'));
        await invoke('undo');
        await expectPixels(before.pixels,`${label}: undo restores pixels`);
        await invoke('redo');
        await expectPixels(painted.pixels,`${label}: redo restores stroke`);
        await invoke('undo');
        assert.equal(await evaluate('layerApp.state().host_error??null'),null);
        console.log(`${label}: visible pressure/curve stroke; history ${historyFailures.some(f=>f.label.startsWith(label+":"))?"FAILED":"passed"}`);
      }
      if(id===3)await invoke('undo');
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
  assert.deepEqual(historyFailures,[], 'Committed browser history preserves presented pixels');
}
