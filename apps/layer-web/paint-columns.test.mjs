import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

export async function checkPaintColumns({call,evaluate,settle}) {
  const output=process.env.LAYER_TEST_ARTIFACTS;
  if(output)await mkdir(output,{recursive:true});
  const wait=async(expression,timeout=30000)=>{
    const deadline=Date.now()+timeout;
    while(!await evaluate(expression)){if(Date.now()>deadline)throw Error(`Timed out: ${expression}`);await new Promise(r=>setTimeout(r,50));}
  };
  const input=value=>evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify(value))});null`);
  const idle=`(v=>v.ready&&!v.busy&&!v.form)(JSON.parse(layerApp.app.workspace_view()))`;
  await input({type:'switch',id:'builtin:workspace:illustrator'});
  await wait(`JSON.parse(layerApp.app.workspace_view()).id==='builtin:workspace:illustrator'&&${idle}`);
  await input({type:'form',kind:'reset',id:null});
  await wait(`!!JSON.parse(layerApp.app.workspace_view()).form`);
  await input({type:'submit',name:'',source:null});
  await wait(idle);
  assert.deepEqual(await evaluate('layerApp.state().workspace.layout.fit_height_groups'),[10,14]);
  const geometry=()=>evaluate(`(()=>{
    const group=id=>document.querySelector('.dock-group[data-group="'+id+'"]');
    const panel=id=>group(id).querySelector('.panel');
    const measured=Object.fromEntries(layerApp.app.panel_measurements().map(m=>[m.panel,m.content_height]));
    const color=panel(10),navigator=panel(14),overview=navigator.querySelector('.navigator-overview').getBoundingClientRect();
    return {viewport:[innerWidth,innerHeight],toolSet:group(6).getBoundingClientRect().height,tool:group(7).getBoundingClientRect().height,
      color:color.getBoundingClientRect().height,colorScroll:color.scrollHeight-color.clientHeight,navigator:navigator.getBoundingClientRect().height,
      measured:{color:measured.color,navigator:measured.navigator},overview:[overview.width,overview.height],aspect:layerApp.app.navigator_aspect(),hdr:layerApp.app.color_panel().hdr};
  })()`);
  const stable=async()=>{
    let previous;
    for(let i=0;i<60;i++){
      await settle();await new Promise(r=>setTimeout(r,100));
      const next=await geometry();
      if(previous&&JSON.stringify(next)===JSON.stringify(previous))return next;
      previous=next;
    }
    throw Error(`Paint columns did not settle: ${JSON.stringify(previous)}`);
  };
  const results=[];
  const check=async label=>{
    const g=await stable();
    const minimum=72;
    assert.ok(Math.abs(g.toolSet-g.tool)<=1,`${label}: Tool Set and Tool share the rest ${JSON.stringify(g)}`);
    if(Math.abs(g.color-g.measured.color)>1){
      assert.ok(g.color<g.measured.color&&Math.abs(g.tool-minimum)<=1,`${label}: only a short column shrinks Color ${JSON.stringify(g)}`);
    } else assert.ok(g.colorScroll<=1,`${label}: Color fits without scrolling ${JSON.stringify(g)}`);
    assert.ok(Math.abs(g.navigator-g.measured.navigator)<=1,`${label}: Navigator fits its content ${JSON.stringify(g)}`);
    assert.ok(Math.abs(g.overview[1]-g.overview[0]*g.aspect)<=2,`${label}: overview follows the document ${JSON.stringify(g)}`);
    if(output){const shot=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${output}/paint-columns-${label}.png`,Buffer.from(shot.data,"base64"));}
    results.push({label,...g});
    return g;
  };
  const tall=async run=>{
    const [width]=await evaluate('[innerWidth]');
    await call("Emulation.setDeviceMetricsOverride",{width,height:1500,deviceScaleFactor:0,mobile:false});
    try{return await run();}finally{await call("Emulation.clearDeviceMetricsOverride");await settle();}
  };
  const sdr=await check("sdr-native");
  const sdrTall=await tall(()=>check("sdr-tall"));
  assert.ok(!sdr.hdr&&sdrTall.color>=sdr.color);
  await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'})`);
  await wait(`!!document.querySelector('dialog[open] select[aria-label="Bit depth"]')`);
  await evaluate(`(()=>{const d=document.querySelector('dialog[open] select[aria-label="Bit depth"]').closest('dialog');const [w,h]=d.querySelectorAll('input[type=number]');w.value=900;h.value=1200;d.querySelector('select[aria-label="Bit depth"]').value='F16';[...d.querySelectorAll('button')].find(b=>b.textContent==='Create').click();})()`);
  await wait(`layerApp.app.document_color().depth==='F16'&&layerApp.app.color_panel().hdr&&!layerApp.state().document_file.busy`,60000);
  const hdr=await check("hdr-native");
  const hdrTall=await tall(()=>check("hdr-tall"));
  assert.ok(hdrTall.measured.color>sdrTall.measured.color+10,`HDR Color is taller ${hdrTall.measured.color} > ${sdrTall.measured.color}`);
  assert.equal(hdr.aspect,1);
  await evaluate(`layerApp.documents.close(layerApp.app.document_tabs(0).selected)`);
  await wait(`layerApp.app.document_tabs(0).tabs.length===1&&!layerApp.documents.busy()`);
  console.log("Paint fitted columns",JSON.stringify(results));
}
