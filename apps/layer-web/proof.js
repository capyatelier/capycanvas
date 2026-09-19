import {importProfile,chooseProfileLibrary} from './export-controls.js';

// One CPU worker per editor. Termination cancels synchronous Wasm immediately
// and releases its high-water heap. A replacement never queues behind old work.
export function createProof({app,dialog,element,button,applyChange,wake}) {
  let work=null,setup=null;
  let tone=null,toneGeneration=-1,toneChanged=0;
  const label=element("output","proof-status");label.id="proof-status";label.hidden=true;
  document.getElementById("canvas-status").prepend(label);
  const hdrLabel=element("output","proof-status");hdrLabel.id="hdr-status";hdrLabel.hidden=true;
  document.getElementById("canvas-status").prepend(hdrLabel);
  function syncTone(){
    if(!app.gpu_ready())return;
    const status=app.tone_status();
    hdrLabel.hidden=!status.hdr;
    hdrLabel.textContent=status.error?`SDR preview unavailable: ${status.error}`:status.ready?"HDR artwork · mapped SDR display":"HDR artwork · preparing SDR preview…";
    if(status.generation!==toneGeneration){toneGeneration=status.generation;toneChanged=performance.now();tone?.cancel();wake();}
    if(document.hidden){tone?.cancel();return;}
    if(!status.needed||tone||performance.now()-toneChanged<180)return;
    const generation=status.generation,control=app.capture_control();let worker,rejectWorker;
    const job={cancel(){control.cancel();worker?.terminate();rejectWorker?.(new DOMException("HDR analysis cancelled","AbortError"));}};tone=job;
    app.tone_prepare(control,request=>new Promise((resolve,reject)=>{
      rejectWorker=reject;worker=new Worker(new URL("./proof-worker.js",import.meta.url),{type:"module"});
      worker.onmessage=({data})=>data.error?reject(new Error(data.error)):resolve(data.result);
      worker.onerror=e=>{e.preventDefault();reject(new Error(e.message));};
      worker.postMessage(request,[request.bytes.buffer]);
    })).then(candidate=>{if(control.cancelled()||generation!==toneGeneration){candidate.free();return;}app.tone_apply(candidate);wake();})
      .catch(error=>{if(!control.cancelled())app.tone_failed(Number(generation),String(error));})
      .finally(()=>{worker?.terminate();control.free();if(tone===job)tone=null;});
  }
  setInterval(syncTone,200);
  function cancel(){work?.cancel();}
  function prepare(candidate,generation){
    cancel();
    return new Promise((resolve,reject)=>{
      const worker=new Worker(new URL("./proof-worker.js",import.meta.url),{type:"module"});
      let timer;
      const job={generation,cancel:()=>finish(new DOMException("Proof preparation cancelled","AbortError"))};
      work=job;
      function finish(error,result){clearTimeout(timer);worker.terminate();if(work===job)work=null;error?reject(error):resolve(result);}
      worker.onmessage=({data})=>finish(data.error?new Error(data.error):null,data.result);
      worker.onerror=e=>{e.preventDefault();finish(new Error(e.message||"Proof worker stopped"));};
      worker.onmessageerror=()=>finish(new Error("Invalid proof worker response"));
      timer=setTimeout(()=>finish(new Error("Proof preparation timed out; try another profile")),120000);
      try{worker.postMessage(candidate.request());}catch(e){finish(e);}
    });
  }
  function sync(){
    syncTone();
    const status=app.proof_status();label.textContent=status.text;label.title=status.error||status.text;label.hidden=!status.text;
    if(setup&&(work?.generation===null||pendingTimer))return;
    if(work&&(work.generation!==status.generation||!status.needed||document.hidden))cancel();
    if(!status.needed||work||document.hidden||!app.gpu_ready())return;
    const candidate=app.proof_begin(0,null);
    prepare(candidate,status.generation).then(result=>{
      app.proof_check(candidate);candidate.load(result.edge,result.dark,result.bytes);
      applyChange(app.proof_apply(candidate,false));wake();
    }).catch(e=>{if(e.name!=="AbortError")app.proof_failed(candidate,String(e));})
      .finally(()=>{candidate.free();sync();});
  }
  document.addEventListener("visibilitychange",()=>{if(document.hidden)cancel();else sync();});
  let panel=null,panelEpoch=null,refreshPanel=()=>{},cancelContacts=()=>{},pendingTimer,committing=false;
  function closePanel(){if(committing)return;cancelContacts();clearTimeout(pendingTimer);pendingTimer=null;cancel();panel?.remove();panel=null;setup=null;refreshPanel=()=>{};sync();}
  function run(id,sdr=false){
    if(panel){if(sdr)applyChange(app.proof_control({type:'mode',mode:'sdr'}));refreshPanel();return;}
    setup={id};cancel();panelEpoch=app.state().document_file.epoch;
    panel=element('aside','document-dialog proof-panel');panel.setAttribute('aria-label','Proof');const owner=panel;
    const header=element('header');header.append(element('h2','','Proof'),button('Close',closePanel));panel.append(header);
    const mode=element('select');mode.setAttribute('aria-label','Proof mode');
    for(const[value,label]of[['off','Off'],['sdr','SDR'],['print','Print']]){const o=element('option','',label);o.value=value;mode.append(o);}panel.append(mode);
    const sdrPage=element('div','proof-sdr'),printPage=element('div','proof-print'),issue=element('p','error-message'),status=element('p');status.setAttribute('role','status');
    panel.append(sdrPage,printPage,issue,status);document.body.append(panel);
    const model=app.proof_form();let recipe=structuredClone(model.recipe),profiles=[],selected='';
    const send=action=>{try{applyChange(app.proof_control(action));wake();refreshPanel();}catch(e){issue.textContent=String(e);}};
    const field=(root,label,node)=>{node.setAttribute('aria-label',label);const row=element('label','document-size',label);row.append(node);root.append(row);return node;};
    const select=(root,label,options,value)=>{const node=element('select');for(const[id,name]of options){const option=element('option','',name);option.value=id;node.append(option);}node.value=value;return field(root,label,node);};
    const canvas=element('canvas','proof-tone-pad');canvas.width=canvas.height=256;canvas.tabIndex=0;canvas.setAttribute('role','slider');canvas.setAttribute('aria-label','SDR balance and contrast');sdrPage.append(canvas);
    const texture=element('canvas');texture.width=texture.height=256;const texturePixels=new ImageData(new Uint8ClampedArray(app.proof_texture(256)),256,256);
    const restoreTexture=()=>texture.getContext('2d',{willReadFrequently:true}).putImageData(texturePixels,0,0);restoreTexture();
    texture.addEventListener('contextrestored',()=>{restoreTexture();refreshPanel();});canvas.addEventListener('contextrestored',()=>refreshPanel());
    let padContact=null;
    const pad=(e,phase)=>{const r=canvas.getBoundingClientRect();let x=(e.clientX-r.x)/r.width*2-1,y=1-(e.clientY-r.y)/r.height*2;const len=Math.hypot(x,y);if(len>1){x/=len;y/=len;}send({type:'pad',phase,values:[x,y]});};
    canvas.onpointerdown=e=>{if(e.button!==0||padContact!==null)return;padContact=e.pointerId;canvas.setPointerCapture(e.pointerId);e.preventDefault();pad(e,'down');pad(e,'move');};
    canvas.onpointermove=e=>{if(e.pointerId===padContact)pad(e,'move');};
    canvas.onpointerup=e=>{if(e.pointerId===padContact){padContact=null;pad(e,'up');}};
    for(const name of ['pointercancel','lostpointercapture'])canvas.addEventListener(name,e=>{if(e.pointerId===padContact){padContact=null;send({type:'pad',phase:'cancel',values:[0,0]});}});
    canvas.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home'].includes(e.key))return;e.preventDefault();const values=app.proof_form().pad_values;if(e.key==='Home')values.fill(0);else values[e.key==='ArrowLeft'||e.key==='ArrowRight'?0:1]+=e.key==='ArrowLeft'||e.key==='ArrowDown'?-.02:.02;send({type:'pad',phase:'down',values});send({type:'pad',phase:'up',values});};
    canvas.ondblclick=()=>{send({type:'pad',phase:'down',values:[0,0]});send({type:'pad',phase:'up',values:[0,0]});};
    const controls=model.numbers.map(spec=>{
      const input=element('input');Object.assign(input,{type:'range',min:spec.numeric.min,max:spec.numeric.max,step:spec.numeric.step||.01});field(sdrPage,spec.label,input);const output=element('output');input.parentElement.append(output);
      let active=false;
      const change=phase=>{const r=app.proof_form().rendition;r[spec.key]=Number(input.value);send({type:'rendition',phase,recipe:r});};
      input.onpointerdown=()=>{active=true;change('down');};input.oninput=()=>{if(!active){active=true;change('down');}change('move');};input.onchange=()=>{if(active){active=false;change('up');}};
      input.onpointercancel=()=>{if(active){active=false;change('cancel');}};
      return{input,output,spec,cancel(){if(active){active=false;change('cancel');}}};
    });
    cancelContacts=()=>{if(padContact!==null){padContact=null;send({type:'pad',phase:'cancel',values:[0,0]});}controls.forEach(c=>c.cancel());};
    sdrPage.append(button('Reset SDR appearance',()=>{const r={...app.proof_form().rendition,exposure:0,contrast:1,balance:0,highlight_color:.3};send({type:'rendition',phase:'down',recipe:r});send({type:'rendition',phase:'up',recipe:r});}));
    sdrPage.append(element('p','','The saved SDR rendition is used for SDR viewing, print simulation and SDR delivery. This display presents mapped SDR.'));
    const profile=field(printPage,'Proof profile',element('select'));
    const option=(group,p)=>{const index=profiles.push(p)-1,o=element('option','',p.name);o.value=index;group.append(o);return String(index);};
    if(model.document_profile){const group=element('optgroup');group.label='Document Profile';profile.append(group);selected=option(group,model.document_profile);}
    const saved=element('optgroup');saved.label='Saved Profiles';profile.append(saved);
    const standard=element('optgroup');standard.label='Standard Color Spaces';profile.append(standard);
    for(const p of model.profiles){const v=option(standard,p);if(!selected&&JSON.stringify(p.profile)===JSON.stringify(recipe.profile))selected=v;}
    for(const[id,label]of[['add','Add Profile…'],['manage','Manage Profiles…']]){const o=element('option','',label);o.value=id;profile.append(o);}profile.value=selected;
    const intent=select(printPage,'Rendering intent',[['RelativeColorimetric','Relative'],['Perceptual','Perceptual'],['Saturation','Saturation'],['AbsoluteColorimetric','Absolute']],recipe.conversion.intent);
    const bpc=field(printPage,'Black point compensation',element('input'));bpc.type='checkbox';bpc.checked=recipe.conversion.black_point_compensation;
    const simulation=select(printPage,'Print simulation',[['0','Colors'],['1','Black ink'],['2','Paper and ink']],recipe.simulate_paper?'2':recipe.simulate_black_ink?'1':'0');
    const gamut=field(printPage,'Gamut warning',element('input'));gamut.type='checkbox';gamut.onchange=()=>applyChange(app.dispatch({type:'invoke',command:'gamut_warning'}));
    let serial=0;
    const commit=value=>{committing=value;for(const node of owner.querySelectorAll('button,select,input'))node.disabled=value;if(!value){bpc.disabled=intent.value==='AbsoluteColorimetric';refreshPanel();}};
    const preparePrint=async()=>{
      clearTimeout(pendingTimer);pendingTimer=null;const ticket=++serial;cancel();issue.textContent='';status.textContent='Preparing print preview…';let candidate;
      try{
        let p=profiles[Number(selected)];if(p.id)p=await app.profile_library('get',p.id);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        recipe={name:p.name,profile:p.profile,conversion:{intent:intent.value,black_point_compensation:bpc.checked&&intent.value!=='AbsoluteColorimetric'},simulate_paper:simulation.value==='2',simulate_black_ink:simulation.value!=='0'};
        candidate=app.proof_begin(0xffffffff,recipe);const result=await prepare(candidate,null);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        app.proof_check(candidate);candidate.load(result.edge,result.dark,result.bytes);
        commit(true);const old=candidate.preservation();if(old)await app.profile_library('import',undefined,old);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        app.proof_check(candidate);applyChange(app.proof_apply(candidate,true));wake();status.textContent='';refreshPanel();
      }catch(e){if(panel===owner&&ticket===serial&&e.name!=='AbortError')issue.textContent=String(e);}
      finally{candidate?.free();commit(false);}
    };
    const schedule=()=>{if(committing)return;clearTimeout(pendingTimer);cancel();bpc.disabled=intent.value==='AbsoluteColorimetric';if(bpc.disabled)bpc.checked=false;pendingTimer=setTimeout(preparePrint,180);};
    profile.onchange=async()=>{const value=profile.value;if(!['add','manage'].includes(value)){selected=value;schedule();return;}profile.value=selected;try{const p=value==='add'?await importProfile(app,element):await chooseProfileLibrary({app,element,button,manage:true});if(panel!==owner)return;if(p){selected=option(saved,p);profile.value=selected;schedule();}}catch(e){issue.textContent=String(e);}};
    intent.onchange=bpc.onchange=simulation.onchange=schedule;
    printPage.append(button('Cancel preparation',()=>{if(committing)return;serial++;clearTimeout(pendingTimer);pendingTimer=null;cancel();status.textContent='Preparation cancelled';}),button('Retry',preparePrint));
    mode.onchange=()=>{if(committing)return;const next=mode.value;cancelContacts();serial++;clearTimeout(pendingTimer);pendingTimer=null;cancel();mode.value=next;send({type:'mode',mode:mode.value});if(mode.value==='print')schedule();};
    refreshPanel=()=>{
      if(!panel)return;if(app.state().document_file.epoch!==panelEpoch){closePanel();return;}
      const form=app.proof_form();mode.value=form.mode;mode.querySelector('[value=sdr]').disabled=!form.hdr;
      sdrPage.hidden=form.mode!=='sdr';printPage.hidden=form.mode!=='print';gamut.checked=app.state().gamut_warning;gamut.disabled=committing||!form.document_profile;
      for(const{input,output,spec}of controls){input.value=form.rendition[spec.key];output.textContent=`${Math.round(Number(input.value)*spec.numeric.scale)}${spec.numeric.unit}`;}
      const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.clearRect(0,0,256,256);ctx.save();ctx.beginPath();ctx.arc(128,128,126,0,2*Math.PI);ctx.clip();ctx.drawImage(texture,0,0);ctx.restore();
      const[x,y]=form.pad_values;ctx.beginPath();ctx.arc(128+x*118,128-y*118,7,0,Math.PI*2);ctx.strokeStyle='black';ctx.lineWidth=4;ctx.stroke();ctx.strokeStyle='white';ctx.lineWidth=2;ctx.stroke();
      canvas.setAttribute('aria-valuetext',`Balance ${Math.round(x*100)}%, contrast ${Math.round(y*100)}%`);
    };
    if(sdr)send({type:'mode',mode:'sdr'});refreshPanel();
    if(app.proof_form().mode==='print'&&!model.document_profile)schedule();
    app.profile_library('list').then(entries=>{if(panel===owner)for(const p of entries){if(!p.issue&&p.visible!==false)option(saved,{id:p.id,name:p.name});profile.value=selected;}}).catch(e=>{if(panel===owner)issue.textContent=String(e);});
  }
  return {run,sync(){sync();refreshPanel();},cancel};
}
