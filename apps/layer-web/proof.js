import {importProfile,chooseProfileLibrary} from './export-controls.js';

// One CPU worker per editor. Termination cancels synchronous Wasm immediately
// and releases its high-water heap. A replacement never queues behind old work.
export function createProof({app,dialog,element,button,applyChange,wake}) {
  let work=null,setup=false;
  const label=element("output","proof-status");label.id="proof-status";label.hidden=true;
  document.getElementById("canvas-status").prepend(label);
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
    const status=app.proof_status();label.textContent=status.text;label.title=status.error||status.text;label.hidden=!status.text;
    if(setup)return;
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
  async function run(id){
    setup=true;cancel();
    let candidate=null,closed=false,committing=false;
    try{
      const model=app.proof_form(),original=model.document_profile;
      await dialog("Proof Setup",(form,finish)=>{
        let recipe=structuredClone(model.recipe),profiles=[],busy=false;
        form.append(element("p","","Preview how colors will look in print."));
        const field=(name,node)=>{node.setAttribute("aria-label",name);const root=element("label","document-size",name);root.append(node);form.append(root);return node;};
        const profile=field("Proof profile",element("select"));
        const option=(group,p)=>{const i=profiles.push(p)-1,node=element("option","",p.name);node.value=i;group.append(node);return String(i);};
        if(original){const group=element("optgroup");group.label="Document Profile";profile.append(group);option(group,original);}
        const saved=element("optgroup");saved.label="Saved Profiles";profile.append(saved);
        const standard=element("optgroup");standard.label="Standard Color Spaces";profile.append(standard);
        model.profiles.forEach(p=>option(standard,p));
        const add=element("option","","Add Profile…");add.value="add";profile.append(add);
        const manage=element("option","","Manage Profiles…");manage.value="manage";profile.append(manage);
        let selected=original?"0":String(profiles.findIndex(p=>JSON.stringify(p.profile)===JSON.stringify(recipe.profile)));
        profile.value=selected;
        const select=(name,items,value)=>{const node=element("select");items.forEach(([id,name])=>{const n=element("option","",name);n.value=id;node.append(n);});node.value=value;return field(name,node);};
        const intent=select("Rendering intent",[["RelativeColorimetric","Relative colorimetric"],["Perceptual","Perceptual"],["Saturation","Saturation"],["AbsoluteColorimetric","Absolute colorimetric"]],recipe.conversion.intent);
        const bpc=field("Black point compensation",element("input"));bpc.type="checkbox";bpc.checked=recipe.conversion.black_point_compensation;
        const simulation=select("Print simulation",[["0","Colors only"],["1","Black ink"],["2","Paper and ink"]],recipe.simulate_paper?"2":recipe.simulate_black_ink?"1":"0");
        const constrain=()=>{bpc.disabled=busy||intent.value==="AbsoluteColorimetric";if(intent.value==="AbsoluteColorimetric")bpc.checked=false;};intent.onchange=constrain;constrain();
        const issue=element("p","error-message"),progress=element("p");progress.setAttribute("role","status");
        const refresh=async()=>{const entries=await app.profile_library("list");if(closed)return;const current=profiles[Number(selected)];saved.replaceChildren();for(const entry of entries){if(entry.issue||entry.visible===false)continue;const value=option(saved,{name:entry.name,id:entry.id});if(current?.id===entry.id)selected=value;}if(current?.id&&!entries.some(e=>e.id===current.id&&!e.issue&&e.visible!==false))selected=option(saved,current);profile.value=selected;};
        profile.onchange=async()=>{
          const value=profile.value;if(!["add","manage"].includes(value)){selected=value;return;}profile.value=selected;
          try{const imported=value==="add"?await importProfile(app,element):await chooseProfileLibrary({app,element,button,manage:true});
            if(closed)return;if(imported){selected=option(saved,imported);profile.value=selected;}else await refresh();issue.textContent="";
          }catch(e){issue.textContent=String(e);}
        };
        const footer=element("footer");
        const dismiss=button("Cancel",()=>{if(committing)return;closed=true;cancel();finish(null);});
        const apply=button("Apply",async()=>{
          if(busy)return;busy=true;issue.textContent="";progress.textContent="Preparing preview…";
          const inputs=[...form.querySelectorAll("select,input,button")].filter(n=>n!==dismiss);inputs.forEach(n=>n.disabled=true);
          try{
            let p=profiles[Number(selected)];if(p.id)p=await app.profile_library("get",p.id);if(closed)return;
            recipe={name:p.name,profile:p.profile,conversion:{intent:intent.value,black_point_compensation:bpc.checked},simulate_paper:simulation.value==="2",simulate_black_ink:simulation.value!=="0"};
            candidate=app.proof_begin(id,recipe);
            const result=await prepare(candidate,null);if(closed)return;
            app.proof_check(candidate);candidate.load(result.edge,result.dark,result.bytes);
            committing=true;dismiss.disabled=true;progress.textContent="Applying proof…";
            const old=candidate.preservation();if(old)await app.profile_library("import",undefined,old);
            app.proof_check(candidate);applyChange(app.proof_apply(candidate,true));wake();closed=true;finish(true);
          }catch(e){if(!closed&&e.name!=="AbortError")issue.textContent=String(e);}
          finally{candidate?.free();candidate=null;busy=false;committing=false;if(!closed){inputs.forEach(n=>n.disabled=false);dismiss.disabled=false;progress.textContent="";constrain();}}
        },"suggested-action");
        footer.append(dismiss,apply);form.append(issue,progress,footer);form.onsubmit=e=>e.preventDefault();
        form.closest("dialog").addEventListener("cancel",e=>{if(committing)e.preventDefault();else{closed=true;cancel();}});
        refresh().catch(e=>{issue.textContent=String(e);});
      });
    }finally{closed=true;cancel();setup=false;sync();}
  }
  return {run,sync,cancel};
}
