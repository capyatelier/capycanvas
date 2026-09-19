import {importProfile,chooseProfileLibrary} from './export-controls.js';

// One CPU worker per editor. Termination cancels synchronous Wasm immediately
// and releases its high-water heap. A replacement never queues behind old work.
export function createProof({app,dialog,element,button,icon,applyChange,wake,dispatch}) {
  let work=null,setup=null;
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
  function disposePanel(){cancelContacts();clearTimeout(pendingTimer);pendingTimer=null;cancel();panel?.remove();panel=null;setup=null;refreshPanel=()=>{};}
  function closePanel(){if(committing)return;disposePanel();dispatch({type:"customize",action:{type:"set_panel_visible",panel:"proof",visible:false}});sync();}
  function run(id,sdr=false){
    if(id!==null)applyChange(app.proof_control({type:"reveal"}));
    if(panel){if(sdr)applyChange(app.proof_control({type:'mode',mode:'sdr'}));refreshPanel();return;}
    setup={id};cancel();panelEpoch=app.state().document_file.epoch;
    panel=element('div','proof-controls');panel.setAttribute('aria-label','Proof');const owner=panel;
    const header=element('header');header.append(element('h2','','Proof'),button('Close',closePanel));panel.append(header);
    const mode=element('div','proof-modes');mode.setAttribute('aria-label','Proof mode');mode.setAttribute('role','group');
    for(const[value,label]of[['off','Off'],['sdr','SDR'],['print','Print']]){const o=button(label,()=>{mode.value=value;mode.onchange();});o.value=value;mode.append(o);}panel.append(mode);
    const sdrPage=element('div','proof-sdr'),printPage=element('div','proof-print'),issue=element('p','error-message'),status=element('p');status.setAttribute('role','status');
    panel.append(sdrPage,printPage,issue,status);primary?.append(panel);
    const model=app.proof_form();let recipe=structuredClone(model.recipe),appliedRecipe=JSON.stringify(model.recipe),profiles=[],selected='';
    const send=action=>{try{applyChange(app.proof_control(action));wake();refreshPanel();}catch(e){issue.textContent=String(e);}};
    const field=(root,label,node)=>{node.setAttribute('aria-label',label);const row=element('label','document-size',label);row.append(node);root.append(row);return node;};
    const select=(root,label,options,value)=>{const node=element('select');for(const[id,name]of options){const option=element('option','',name);option.value=id;node.append(option);}node.value=value;return field(root,label,node);};
    const canvas=element('canvas','proof-tone-pad');canvas.width=canvas.height=256;canvas.tabIndex=0;canvas.setAttribute('role','slider');canvas.setAttribute('aria-label','SDR balance and contrast');sdrPage.append(canvas);
    const texture=element('canvas');texture.width=texture.height=256;const texturePixels=new ImageData(new Uint8ClampedArray(app.proof_texture(256)),256,256);
    const restoreTexture=()=>texture.getContext('2d',{willReadFrequently:true}).putImageData(texturePixels,0,0);restoreTexture();
    texture.addEventListener('contextrestored',()=>{restoreTexture();refreshPanel();});canvas.addEventListener('contextrestored',()=>refreshPanel());
    let padContact=null,activePart=0,dialSize=256;
    const dial=(point=null,part=null)=>app.color_ui({type:'proof_dial',size:dialSize,recipe:app.proof_form().rendition,point,part});
    const coordinates=e=>{const r=canvas.getBoundingClientRect();return[(e.clientX-r.x)*dialSize/r.width,(e.clientY-r.y)*dialSize/r.height];};
    const update=(e,phase)=>send({type:'rendition',phase,recipe:dial(coordinates(e),activePart).recipe});
    const cancelDial=()=>{if(padContact!==null){padContact=null;send({type:'rendition',phase:'cancel',recipe:app.proof_form().rendition});}};
    canvas.onpointerdown=e=>{if(e.button!==0||padContact!==null)return;const hit=dial(coordinates(e)).hit;if(hit==null)return;activePart=Number(hit);padContact=e.pointerId;canvas.setPointerCapture(e.pointerId);e.preventDefault();canvas.focus();send({type:'rendition',phase:'down',recipe:app.proof_form().rendition});update(e,'move');};
    canvas.onpointermove=e=>{if(e.pointerId===padContact&&activePart!==3)update(e,'move');};
    canvas.onpointerup=e=>{if(e.pointerId===padContact){padContact=null;update(e,'up');}};
    for(const name of ['pointercancel','lostpointercapture'])canvas.addEventListener(name,e=>{if(e.pointerId===padContact)cancelDial();});
    const atomic=recipe=>{send({type:'rendition',phase:'down',recipe:app.proof_form().rendition});send({type:'rendition',phase:'up',recipe});};
    const resetPart=part=>{const r=app.proof_form().rendition;if(part===0){r.balance=0;r.contrast=1;}else if(part===1)r.exposure=0;else if(part===2)r.highlight_color=.3;else Object.assign(r,{balance:0,contrast:1,exposure:0,highlight_color:.3});atomic(r);};
    canvas.onkeydown=e=>{if(e.key==='Escape'){e.preventDefault();cancelDial();return;}if(!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home'].includes(e.key))return;e.preventDefault();if(e.key==='Home'){resetPart(activePart);return;}const direction=['ArrowLeft','ArrowDown'].includes(e.key)?-1:1,step=(e.shiftKey?.1:.02)*direction,r=app.proof_form().rendition;if(activePart===1)r.exposure=Math.max(-2,Math.min(2,r.exposure+step*2));else if(activePart===2)r.highlight_color=Math.max(0,Math.min(1,r.highlight_color+step));else{const v=app.proof_form().pad_values;const i=['ArrowLeft','ArrowRight'].includes(e.key)?0:1;v[i]=Math.max(-1,Math.min(1,v[i]+step));send({type:'pad',phase:'down',values:app.proof_form().pad_values});send({type:'pad',phase:'up',values:v});return;}atomic(r);};
    canvas.ondblclick=e=>{const hit=dial(coordinates(e)).hit;if(hit!=null)resetPart(Number(hit));};
    cancelContacts=cancelDial;
    const accessible=element('div','proof-dial-accessibility');sdrPage.append(accessible);
    const arcControls=model.numbers.map((spec,i)=>{const input=element('input');Object.assign(input,{type:'range',min:spec.numeric.min,max:spec.numeric.max,step:spec.numeric.step||.01});input.setAttribute('aria-label',spec.label);input.onfocus=()=>{activePart=i+1;};input.oninput=()=>{const r=app.proof_form().rendition;r[spec.key]=Number(input.value);atomic(r);};input.onkeydown=e=>{if(e.key==='Home'){e.preventDefault();resetPart(i+1);}};accessible.append(input);return{input,spec};});
    const reset=button('Reset SDR appearance',()=>resetPart(3));reset.className='proof-dial-reset';reset.setAttribute('aria-label','Reset SDR appearance');reset.replaceChildren(icon('reset'));sdrPage.append(reset);
    const icons=['layer-appearance-symbolic','layer-grain-symbolic','layer-brightness_contrast-symbolic','layer-hue_saturation-symbolic'].map(name=>{const node=icon(name.replace(/^layer-/,'').replace(/-symbolic$/,''));node.classList.add('proof-dial-icon');sdrPage.append(node);return node;});
    const profile=field(printPage,'Proof profile',element('select'));
    const option=(group,p)=>{const index=profiles.push(p)-1,o=element('option','',p.name);o.value=index;group.append(o);return String(index);};
    const documentGroup=element('optgroup');documentGroup.label='Document Profile';profile.append(documentGroup);let documentIndex=null;
    const documentProfile=p=>{documentGroup.replaceChildren();if(!p)return null;if(documentIndex===null)documentIndex=profiles.length;profiles[documentIndex]=p;const o=element('option','',p.name);o.value=documentIndex;documentGroup.append(o);return String(documentIndex);};
    if(model.document_profile)selected=documentProfile(model.document_profile);
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
      if(!panel||!panel.isConnected||panel.getBoundingClientRect().width===0)return;if(app.state().document_file.epoch!==panelEpoch){disposePanel();run(null);placePanel();return;}
      const form=app.proof_form();
      const savedRecipe=JSON.stringify(form.recipe);
      if(savedRecipe!==appliedRecipe){
        appliedRecipe=savedRecipe;recipe=structuredClone(form.recipe);
        const current=documentProfile(form.document_profile);if(current!==null)selected=current;else selected=String(profiles.findIndex(p=>JSON.stringify(p.profile)===JSON.stringify(recipe.profile)));profile.value=selected;
        intent.value=recipe.conversion.intent;bpc.checked=recipe.conversion.black_point_compensation;simulation.value=recipe.simulate_paper?'2':recipe.simulate_black_ink?'1':'0';
      }
      mode.value=form.mode;for(const b of mode.children){b.setAttribute('aria-pressed',String(b.value===form.mode));b.disabled=committing||(b.value==='sdr'&&!form.hdr);b.hidden=b.value==='sdr'&&!form.hdr;}
      sdrPage.hidden=form.mode!=='sdr';printPage.hidden=form.mode!=='print';gamut.checked=app.state().gamut_warning;gamut.disabled=committing||!form.document_profile;
      for(const{input,spec}of arcControls)input.value=form.rendition[spec.key];
      if(!sdrPage.hidden){const host=panel.parentElement,outer=host.getBoundingClientRect(),top=canvas.getBoundingClientRect().top-outer.top+host.scrollTop;dialSize=Math.max(128,Math.floor(Math.min(256,sdrPage.clientWidth,host.clientHeight-top-20)));canvas.style.width=canvas.style.height=`${dialSize}px`;const pixels=Math.ceil(dialSize*devicePixelRatio);if(canvas.width!==pixels)canvas.width=canvas.height=pixels;}
      const d=dial(),ctx=canvas.getContext('2d',{willReadFrequently:true}),[cx,cy]=d.center;
      restoreTexture();ctx.setTransform(canvas.width/dialSize,0,0,canvas.height/dialSize,0,0);ctx.clearRect(0,0,dialSize,dialSize);ctx.save();ctx.beginPath();ctx.arc(cx,cy,d.radius,0,2*Math.PI);ctx.clip();ctx.drawImage(texture,cx-d.radius,cy-d.radius,d.radius*2,d.radius*2);ctx.restore();
      const marker=(p,r)=>{ctx.beginPath();ctx.arc(...p,r,0,Math.PI*2);ctx.strokeStyle='black';ctx.lineWidth=3;ctx.stroke();ctx.strokeStyle='white';ctx.lineWidth=1.5;ctx.stroke();};
      for(const[a,index]of d.arcs.map((a,i)=>[a,i])){const g=a.geometry,gradient=ctx.createLinearGradient(a.path[0][0],0,a.path.at(-1)[0],0);if(index===0){gradient.addColorStop(0,'#0a0a0a');gradient.addColorStop(.5,'#8c8c8c');gradient.addColorStop(1,'#fff');}else{gradient.addColorStop(0,'#f2f2f2');gradient.addColorStop(1,'#268cd9');}ctx.beginPath();a.path.forEach((p,i)=>i?ctx.lineTo(...p):ctx.moveTo(...p));ctx.strokeStyle=gradient;ctx.lineWidth=g.width;ctx.lineCap='round';ctx.stroke();marker(a.point,g.marker_radius);}
      marker(d.marker,d.marker_radius);
      ctx.fillStyle=getComputedStyle(panel).color;ctx.font=`${d.text_size}px system-ui`;ctx.textAlign='center';
      const width=canvas.getBoundingClientRect().width,left=(sdrPage.clientWidth-width)/2,scale=width/dialSize,top=canvas.offsetTop;
      d.readouts.forEach((r,i)=>{const value=Math.round(d.percentages[i]),text=`${i===1||i===2?value>=0?'+':'':''}${value}%`;if(r.curve){const[radius,angle,reverse]=r.curve,sign=reverse?-1:1,total=ctx.measureText(text).width;let advance=-total/2;for(const ch of text){const w=ctx.measureText(ch).width,a=angle*Math.PI/180+sign*(advance+w/2)/radius;ctx.save();ctx.translate(cx+radius*Math.cos(a),cy+radius*Math.sin(a));ctx.rotate(a+(reverse?-1:1)*Math.PI/2);ctx.fillText(ch,0,0);ctx.restore();advance+=w;}}else ctx.fillText(text,...r.text);const[x,y,w,h]=r.icon;Object.assign(icons[i].style,{left:`${left+x*scale}px`,top:`${top+y*scale}px`,width:`${w*scale}px`,height:`${h*scale}px`});});
      const[x,y,w,h]=d.reset;Object.assign(reset.style,{left:`${left+x*scale}px`,top:`${top+y*scale}px`,width:`${w*scale}px`,height:`${h*scale}px`});
      canvas.setAttribute('aria-valuetext',`Balance ${Math.round(form.rendition.balance*100)}%, contrast ${Math.round(form.rendition.contrast*100)}%. Arrow keys adjust; Shift takes larger steps, Home resets, Escape cancels.`);

    };
    if(sdr)send({type:'mode',mode:'sdr'});refreshPanel();
    if(id!==null&&app.proof_form().mode==='print'&&!model.document_profile)schedule();
    app.profile_library('list').then(entries=>{if(panel===owner)for(const p of entries){if(!p.issue&&p.visible!==false)option(saved,{id:p.id,name:p.name});profile.value=selected;}}).catch(e=>{if(panel===owner)issue.textContent=String(e);});
  }
  return {run,mount,sync(){sync();placePanel();refreshPanel();},cancel};
}
