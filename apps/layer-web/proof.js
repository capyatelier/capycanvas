import {importProfile,chooseProfileLibrary} from './export-controls.js';

// One CPU worker per editor. Termination cancels synchronous Wasm immediately
// and releases its high-water heap. A replacement never queues behind old work.
export function createProof({app,element,button,icon,applyChange,wake}) {
  let work=null,setup=null,finishPending=async()=>{},hasPending=()=>false,refreshLibrary=()=>{};
  // The same immutable 512² Rust illustration as GTK, built off the UI thread.
  const texture=element('canvas');texture.width=texture.height=512;
  let pattern=null,patternPixels=null;
  function restoreTexture(){if(patternPixels)texture.getContext('2d',{willReadFrequently:true}).putImageData(patternPixels,0,0);}
  function ensurePattern(){
    if(pattern)return;
    pattern=new Promise((resolve,reject)=>{
      const worker=new Worker(new URL('./proof-worker.js',import.meta.url),{type:'module'});
      worker.onmessage=({data})=>{worker.terminate();if(data.error)reject(Error(data.error));else{patternPixels=new ImageData(new Uint8ClampedArray(data.result.bytes),512,512);restoreTexture();resolve();}};
      worker.onerror=e=>{e.preventDefault();worker.terminate();reject(Error(e.message));};worker.postMessage({type:'texture'});
    });
    pattern.then(()=>refreshPanel()).catch(e=>console.error('Proof illustration:',e));
  }
  texture.addEventListener('contextrestored',()=>{restoreTexture();refreshPanel();});
  const mounts=new Set();let primary=null,mountPending=false;
  const scheduleMount=()=>{if(!mountPending){mountPending=true;requestAnimationFrame(()=>{mountPending=false;placePanel();refreshPanel();});}};
  const mountObserver=new ResizeObserver(scheduleMount);
  function mount(root){if(!primary)primary=root;mounts.add(root);mountObserver.observe(root);scheduleMount();return()=>{mountObserver.unobserve(root);mounts.delete(root);scheduleMount();};}
  function placePanel(){
    const visible=[...mounts].reverse().find(n=>n.isConnected&&!n.closest("[inert]")&&n.getBoundingClientRect().width>0&&n.getBoundingClientRect().height>0);
    if(!panel&&visible)run(null);
    const target=visible||primary;
    if(panel&&target&&panel.parentElement!==target){cancelContacts();target.append(panel);}
  }
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
  function disposePanel(){cancelContacts();clearTimeout(pendingTimer);pendingTimer=null;cancel();panel?.remove();panel=null;setup=null;refreshPanel=()=>{};finishPending=async()=>{};hasPending=()=>false;refreshLibrary=()=>{};}
  function run(id,sdr=false){
    if(id!==null)applyChange(app.proof_control({type:"reveal"}));
    if(panel){refreshLibrary();if(sdr)applyChange(app.proof_control({type:'mode',mode:'sdr'}));refreshPanel();return;}
    setup={id};cancel();panelEpoch=app.state().document_file.epoch;
    panel=element('div','proof-controls');panel.setAttribute('aria-label','Proof');const owner=panel;
    const mode=element('div','proof-modes');mode.setAttribute('aria-label','Proof mode');mode.setAttribute('role','group');
    for(const[value,label]of[['off','Off'],['sdr','SDR'],['print','Print']]){const o=button(label,()=>{mode.value=value;mode.onchange();});o.value=value;mode.append(o);}panel.append(mode);
    const sdrPage=element('div','proof-sdr'),printPage=element('div','proof-print'),issue=element('p','error-message'),status=element('p');status.setAttribute('role','status');
    panel.append(sdrPage,printPage,issue,status);primary?.append(panel);
    const model=app.proof_form();let recipe=structuredClone(model.recipe),appliedRecipe=JSON.stringify(model.document_profile),profiles=[],selected='';
    const printSettings=model.print_settings;
    const send=action=>{try{applyChange(app.proof_control(action));wake();refreshPanel();}catch(e){issue.textContent=String(e);}};
    const field=(root,label,node)=>{node.setAttribute('aria-label',label);const row=element('label','document-size',label);row.append(node);root.append(row);return node;};
    const select=(root,label,options,value)=>{const node=element('select');for(const[id,name]of options){const option=element('option','',name);option.value=id;node.append(option);}node.value=value;return field(root,label,node);};
    const canvas=element('canvas','proof-tone-pad');canvas.width=canvas.height=256;canvas.tabIndex=0;canvas.setAttribute('role','slider');canvas.setAttribute('aria-label','SDR balance and contrast');sdrPage.append(canvas);
    canvas.addEventListener('contextrestored',()=>refreshPanel());
    let padContact=null,keyContact=false,activePart=0,dialSize=256,lastTouch=null,pointerStart=null,pointerMoved=false;
    const dial=(point=null,part=null)=>app.color_ui({type:'proof_dial',size:dialSize,recipe:app.proof_form().rendition,point,part});
    const coordinates=e=>{const r=canvas.getBoundingClientRect();return[(e.clientX-r.x)*dialSize/r.width,(e.clientY-r.y)*dialSize/r.height];};
    const change=(phase,recipe=app.proof_form().rendition)=>send({type:'rendition',phase,recipe});
    const update=(e,phase)=>change(phase,dial(coordinates(e),activePart).recipe);
    const control=(part,edit)=>app.color_ui({type:'proof_control',recipe:app.proof_form().rendition,part,edit});
    const cancelDial=()=>{if(padContact!==null||keyContact){padContact=null;keyContact=false;change('cancel');}};
    const atomic=recipe=>{cancelDial();change('down');change('up',recipe);};
    const resetPart=part=>atomic(control(part,{type:'reset'}));
    canvas.onpointerdown=e=>{
      if(e.button!==0||padContact!==null)return;
      const target=dial(coordinates(e)).hit;if(target==null)return;const hit=Number(target);if(hit===3)return;
      if(hit===0)canvas.focus();else arcControls[Number(hit)-1].input.focus();activePart=Number(hit);canvas.setPointerCapture(e.pointerId);e.preventDefault();
      if(e.pointerType==='touch'&&lastTouch&&e.timeStamp-lastTouch.time<350&&Math.hypot(e.clientX-lastTouch.x,e.clientY-lastTouch.y)<20&&activePart===lastTouch.part){lastTouch=null;resetPart(activePart);return;}
      cancelDial();padContact=e.pointerId;pointerStart=[e.clientX,e.clientY];pointerMoved=false;change('down');update(e,'move');
    };
    canvas.onmousedown=e=>{if(e.detail===2){const hit=dial(coordinates(e)).hit;if(hit!=null){cancelDial();resetPart(Number(hit));}}};
    canvas.onpointermove=e=>{if(e.pointerId===padContact){pointerMoved ||= Math.hypot(e.clientX-pointerStart[0],e.clientY-pointerStart[1])>8;update(e,'move');}};
    canvas.onpointerup=e=>{if(e.pointerId===padContact){padContact=null;update(e,'up');if(e.pointerType==='touch')lastTouch=pointerMoved?null:{time:e.timeStamp,x:e.clientX,y:e.clientY,part:activePart};}};
    for(const name of ['pointercancel','lostpointercapture'])canvas.addEventListener(name,e=>{if(e.pointerId===padContact)cancelDial();});
    function keyDown(e){
      if(e.key==='Escape'){e.preventDefault();cancelDial();return;}
      if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;
      e.preventDefault();if(padContact!==null)return;
      if(!keyContact){keyContact=true;change('down');}
      const axis=['ArrowLeft','ArrowRight'].includes(e.key)?0:1,steps=(['ArrowLeft','ArrowDown'].includes(e.key)?-1:1)*(e.shiftKey?10:1);
      change('move',control(activePart,{type:'step',axis,steps}));
    }
    function keyUp(e){if(keyContact&&e.key.startsWith('Arrow')){e.preventDefault();keyContact=false;change('up');}}
    canvas.onkeydown=keyDown;canvas.onkeyup=keyUp;canvas.onblur=()=>{cancelDial();refreshPanel();};canvas.onfocus=()=>{activePart=0;refreshPanel();};
    cancelContacts=cancelDial;
    const accessible=element('div','proof-dial-accessibility');sdrPage.append(accessible);
    const arcControls=model.numbers.map((spec,i)=>{
      const input=element('input');Object.assign(input,{type:'range',min:spec.numeric.min,max:spec.numeric.max,step:spec.numeric.step});input.setAttribute('aria-label',spec.label);
      input.onfocus=()=>{activePart=i+1;refreshPanel();};input.onblur=()=>{cancelDial();refreshPanel();};input.onkeydown=keyDown;input.onkeyup=keyUp;
      input.oninput=()=>{const r=app.proof_form().rendition;r[spec.key]=Number(input.value);atomic(r);};accessible.append(input);return{input,spec};
    });
    const reset=button('Reset SDR appearance',()=>resetPart(3));reset.className='proof-dial-reset';reset.setAttribute('aria-label','Reset SDR appearance');reset.replaceChildren(icon('reset'));sdrPage.append(reset);
    const icons=['appearance','grain','brightness_contrast','hue_saturation'].map(name=>{const node=icon(name);node.classList.add('proof-dial-icon');sdrPage.append(node);return node;});
    const profile=field(printPage,'Profile',element('select'));profile.setAttribute('aria-label','Proof profile');const placeholder=element('option','','Choose Profile…');placeholder.value='';placeholder.disabled=true;profile.append(placeholder);
    const option=(group,p)=>{let index=p.id?profiles.findIndex(item=>item.id===p.id):-1;if(index<0)index=profiles.push(p)-1;else profiles[index]=p;const o=element('option','',p.name);o.value=index;group.append(o);return String(index);};
    const documentGroup=element('optgroup');documentGroup.label='Document Profile';profile.append(documentGroup);let documentIndex=null;
    const documentProfile=p=>{documentGroup.replaceChildren();if(!p)return null;if(documentIndex===null)documentIndex=profiles.length;profiles[documentIndex]=p;const o=element('option','',p.name);o.value=documentIndex;documentGroup.append(o);return String(documentIndex);};
    if(model.print_settings.profile)selected=documentProfile(model.print_settings.profile);
    const saved=element('optgroup');saved.label='Saved Profiles';profile.append(saved);
    const standard=element('optgroup');standard.label='Standard Color Spaces';profile.append(standard);
    for(const p of model.profiles)option(standard,p);
    for(const[id,label]of[['add','Add Profile…'],['manage','Manage Profiles…']]){const o=element('option','',label);o.value=id;profile.append(o);}profile.value=selected;
    const simulation=select(printPage,'Simulate',model.simulations.map(c=>[c.value,c.label]),printSettings.simulation);
    const intent=select(printPage,'Intent',model.intents.map(c=>[c.value,c.label]),printSettings.intent);
    const bpc=field(printPage,'Black point compensation',element('input'));bpc.type='checkbox';bpc.className='panel-check';bpc.checked=printSettings.bpc;bpc.disabled=intent.value==='AbsoluteColorimetric';
    const gamut=field(printPage,'Gamut warning',element('input'));gamut.type='checkbox';gamut.className='panel-check';gamut.onchange=()=>applyChange(app.dispatch({type:'invoke',command:'gamut_warning'}));
    let serial=0,preparing=null,lastMode=model.mode;
    const sameRecipe=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
    const commit=value=>{committing=value;for(const node of owner.querySelectorAll('button,select,input'))node.disabled=value;if(!value){bpc.disabled=intent.value==='AbsoluteColorimetric';refreshPanel();}};
    const preparePrint=async()=>{
      clearTimeout(pendingTimer);pendingTimer=null;const ticket=++serial;cancel();issue.textContent='';status.textContent='Preparing print preview…';let candidate;
      try{
        if(selected===''){status.textContent='';return;}
        let p=profiles[Number(selected)];if(p.id)p=await app.profile_library('get',p.id);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        // Color UI transports JSON integer bytes as BigInt; proof recipes use
        // the JSON/ICC byte representation used by proof_form and profile storage.
        recipe=JSON.parse(JSON.stringify(app.color_ui({type:'print_proof',settings:{profile:p,intent:intent.value,bpc:bpc.checked,simulation:simulation.value}}),(_,v)=>typeof v==='bigint'?Number(v):v));
        if(sameRecipe(recipe,app.proof_form().document_profile)){status.textContent='';return;}
        candidate=app.proof_begin(0xffffffff,recipe);const result=await prepare(candidate,null);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        app.proof_check(candidate);candidate.load(result.edge,result.dark,result.bytes);
        commit(true);const old=candidate.preservation();if(old)await app.profile_library('import',undefined,old);
        if(panel!==owner||ticket!==serial||mode.value!=='print')return;
        app.proof_check(candidate);applyChange(app.proof_apply(candidate,true));wake();status.textContent='';refreshPanel();
      }catch(e){if(panel===owner&&ticket===serial&&e.name!=='AbortError'){issue.textContent=String(e);restorePrint(app.proof_form());status.textContent='';}}
      finally{candidate?.free();commit(false);}
    };
    const startPrepare=()=>{const job=preparePrint();preparing=job;job.finally(()=>{if(preparing===job)preparing=null;});return job;};
    hasPending=()=>!!(pendingTimer||preparing);
    finishPending=async()=>{if(pendingTimer){clearTimeout(pendingTimer);pendingTimer=null;startPrepare();}if(preparing)await preparing;if(app.proof_form().mode==='print'&&issue.textContent)throw Error(issue.textContent);};
    const schedule=()=>{if(committing)return;clearTimeout(pendingTimer);cancel();bpc.disabled=intent.value==='AbsoluteColorimetric';if(bpc.disabled)bpc.checked=false;pendingTimer=setTimeout(startPrepare,180);};
    profile.onchange=async()=>{const value=profile.value;if(!['add','manage'].includes(value)){selected=value;schedule();return;}profile.value=selected;try{const p=value==='add'?await importProfile(app,element):await chooseProfileLibrary({app,element,button,manage:true});if(panel!==owner)return;if(p){selected=option(saved,p);profile.value=selected;schedule();}}catch(e){issue.textContent=String(e);}};
    intent.onchange=bpc.onchange=simulation.onchange=schedule;
    mode.onchange=()=>{if(committing)return;const next=mode.value;cancelContacts();serial++;clearTimeout(pendingTimer);pendingTimer=null;cancel();mode.value=next;send({type:'mode',mode:mode.value});if(mode.value==='print'&&selected!=='')schedule();};
    function restorePrint(form){
      recipe=structuredClone(form.recipe);
      const current=documentProfile(form.print_settings.profile);selected=current??'';profile.value=selected;
      const saved=form.print_settings;intent.value=saved.intent;bpc.checked=saved.bpc;bpc.disabled=intent.value==='AbsoluteColorimetric';simulation.value=saved.simulation;
    }
    refreshPanel=()=>{
      if(!panel)return;const mapped=panel.isConnected&&panel.getBoundingClientRect().width>0;
      // Hidden idle panels need no ICC/form serialization. A pending draft must
      // still observe Off or document replacement so its worker is cancelled.
      if(!mapped&&!hasPending()){cancelDial();return;}
      if(app.state().document_file.epoch!==panelEpoch){disposePanel();run(null);placePanel();return;}
      const form=app.proof_form();
      if(lastMode!==form.mode){lastMode=form.mode;if(form.mode!=='print'){serial++;clearTimeout(pendingTimer);pendingTimer=null;cancel();status.textContent='';}}
      const savedRecipe=JSON.stringify(form.document_profile);
      if(savedRecipe!==appliedRecipe){appliedRecipe=savedRecipe;restorePrint(form);}
      mode.value=form.mode;for(const b of mode.children){b.setAttribute('aria-pressed',String(b.value===form.mode));b.disabled=committing||(b.value==='sdr'&&!form.hdr);b.hidden=b.value==='sdr'&&!form.hdr;}
      sdrPage.hidden=form.mode!=='sdr';printPage.hidden=form.mode!=='print';gamut.checked=app.state().gamut_warning;gamut.disabled=committing||!form.document_profile;
      for(const{input,spec}of arcControls)input.value=form.rendition[spec.key];
      if(sdrPage.hidden||!mapped){cancelDial();return;}
      ensurePattern();
      {const host=panel.parentElement,outer=host.getBoundingClientRect(),top=canvas.getBoundingClientRect().top-outer.top+host.scrollTop;dialSize=Math.max(128,Math.floor(Math.min(sdrPage.clientWidth,host.clientHeight-top-4)));canvas.style.width=canvas.style.height=`${dialSize}px`;const pixels=Math.ceil(dialSize*devicePixelRatio);if(canvas.width!==pixels)canvas.width=canvas.height=pixels;}
      const d=dial(),ctx=canvas.getContext('2d',{willReadFrequently:true}),[cx,cy]=d.center;
      ctx.setTransform(canvas.width/dialSize,0,0,canvas.height/dialSize,0,0);ctx.clearRect(0,0,dialSize,dialSize);ctx.save();ctx.beginPath();ctx.arc(cx,cy,d.radius,0,2*Math.PI);ctx.clip();if(patternPixels)ctx.drawImage(texture,cx-d.radius,cy-d.radius,d.radius*2,d.radius*2);else{ctx.fillStyle='#808080';ctx.fill();}ctx.restore();
      const marker=(p,r,focus)=>{ctx.beginPath();ctx.arc(...p,r,0,Math.PI*2);ctx.strokeStyle='rgba(0,0,0,.65)';ctx.lineWidth=4;ctx.stroke();ctx.strokeStyle='white';ctx.lineWidth=2;ctx.stroke();if(focus){ctx.beginPath();ctx.arc(...p,r+3,0,Math.PI*2);ctx.strokeStyle='rgba(255,255,255,.65)';ctx.lineWidth=1;ctx.stroke();}};
      for(const[a,index]of d.arcs.map((a,i)=>[a,i])){const g=a.geometry,gradient=ctx.createLinearGradient(a.path[0][0],0,a.path.at(-1)[0],0);if(index===0){gradient.addColorStop(0,'#0a0a0a');gradient.addColorStop(.5,'#8c8c8c');gradient.addColorStop(1,'#fff');}else{gradient.addColorStop(0,'#f2f2f2');gradient.addColorStop(1,'#268cd9');}ctx.beginPath();a.path.forEach((p,i)=>i?ctx.lineTo(...p):ctx.moveTo(...p));ctx.strokeStyle=gradient;ctx.lineWidth=g.width;ctx.lineCap='round';ctx.stroke();marker(a.point,g.marker_radius,arcControls[index].input.matches(':focus-visible'));}
      marker(d.marker,d.marker_radius,canvas.matches(':focus-visible'));
      ctx.fillStyle=getComputedStyle(panel).color;ctx.font=`${d.text_size}px system-ui`;ctx.textAlign='center';
      const width=canvas.getBoundingClientRect().width,left=(sdrPage.clientWidth-width)/2,scale=width/dialSize,top=canvas.offsetTop;
      d.readouts.forEach((r,i)=>{const value=Math.round(d.percentages[i]),text=`${i===1||i===2?value>=0?'+':'':''}${value}%`;if(r.curve){const[radius,angle,reverse]=r.curve,sign=reverse?-1:1,total=ctx.measureText(text).width;let advance=-total/2;for(const ch of text){const w=ctx.measureText(ch).width,a=angle*Math.PI/180+sign*(advance+w/2)/radius;ctx.save();ctx.translate(cx+radius*Math.cos(a),cy+radius*Math.sin(a));ctx.rotate(a+(reverse?-1:1)*Math.PI/2);ctx.fillText(ch,0,0);ctx.restore();advance+=w;}}else ctx.fillText(text,...r.text);const[x,y,w,h]=r.icon;Object.assign(icons[i].style,{left:`${left+x*scale}px`,top:`${top+y*scale}px`,width:`${w*scale}px`,height:`${h*scale}px`});});
      const[x,y,w,h]=d.reset;Object.assign(reset.style,{left:`${left+x*scale}px`,top:`${top+y*scale}px`,width:`${w*scale}px`,height:`${h*scale}px`});
      canvas.setAttribute('aria-valuetext',`Balance ${Math.round(form.rendition.balance*100)}%, contrast ${Math.round(form.rendition.contrast*100)}%. Arrow keys adjust; Shift takes larger steps, Escape cancels.`);

    };
    if(sdr)send({type:'mode',mode:'sdr'});refreshPanel();
    refreshLibrary=()=>app.profile_library('list').then(entries=>{
      if(panel!==owner)return;
      saved.replaceChildren();for(const p of entries)if(!p.issue&&p.visible!==false)option(saved,{id:p.id,name:p.name});profile.value=selected;
    }).catch(e=>{if(panel===owner)issue.textContent=String(e);});
    profile.addEventListener('focus',()=>refreshLibrary());refreshLibrary();
  }
  return {run,mount,hasPending:()=>hasPending(),finishPending:()=>finishPending(),sync(){sync();placePanel();refreshPanel();},cancel};
}
