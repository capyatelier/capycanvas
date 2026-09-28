import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

const BLUE=[.1,.3,.8,1];
const bar='.canvas-action-bar';
const visible=`(b=>!!b&&!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`;
const discBar=`layerApp.state().canvas_bar?.context.kind==='clone_source'&&${visible}`;

// Clone Stamp on an empty layer over a reference: Alt and a bound side button
// set the source, strokes copy the reference, the source disc drags at once
// and a tap on it shows its bar, with mouse, touch and pen.
export async function checkClone({call,evaluate,settle,device=false}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??(device?'artifacts/clone/web-tablet':'artifacts/clone/web');
  await mkdir(directory,{recursive:true});
  const wait=(expression,timeout=25000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function check(){if(${expression})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(expression)}));else setTimeout(check,30);}check();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  const invoke=async command=>{
    await wait(`(c=>!c||c.enabled)(layerApp.state().commands.find(c=>c.id===${JSON.stringify(command)}))`);
    await send({type:'invoke',command});
  };
  const command=id=>`layerApp.state().commands.find(c=>c.id===${JSON.stringify(id)})`;
  const selected=id=>evaluate(`!!${command(id)}?.selected`);
  const enabled=id=>evaluate(`!!${command(id)}?.enabled`);
  const tool=()=>evaluate('layerApp.state().layer_tools.tool');
  const size=()=>evaluate('(t=>[t.width,t.height])(layerApp.state().tabs[0])');
  const camera=()=>evaluate('(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return{zoom:c.zoom,t:[...c.translation],v:[...c.viewport],r:{x:r.x,y:r.y,width:r.width,height:r.height}}})()');
  const screen=async([x,y])=>{const c=await camera();return{x:c.r.x+(x*c.zoom+c.t[0])*c.r.width/c.v[0],y:c.r.y+(y*c.zoom+c.t[1])*c.r.height/c.v[1]};};
  const rect=selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()`);
  const middle=async selector=>{const r=await rect(selector);return{x:r.x+r.width/2,y:r.y+r.height/2};};
  const alt=async pressed=>{await call('Input.dispatchKeyEvent',{type:pressed?'rawKeyDown':'keyUp',key:'Alt',code:'AltLeft',windowsVirtualKeyCode:18,modifiers:pressed?1:0});await settle();};
  let touchId=900,modifiers=0;
  const pointer=(type,p,kind,buttons=type==='mouseReleased'?0:1)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}],modifiers})
    :call('Input.dispatchMouseEvent',{type,...p,modifiers,button:'left',buttons,clickCount:1,pointerType:kind,force:buttons?.6:0});
  const tap=async(p,kind)=>{touchId++;await pointer('mousePressed',p,kind);await pause(40);await pointer('mouseReleased',p,kind);await settle();await pause(60);};
  const drag=async(from,to,kind,steps=12)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',from,kind);await pause(30);
    for(let i=1;i<=steps;i++){await pointer('mouseMoved',{x:from.x+(to.x-from.x)*i/steps,y:from.y+(to.y-from.y)*i/steps},kind);await settle();}
    await pointer('mouseReleased',to,kind);await settle();
  };
  const menuRow=label=>`[...document.querySelectorAll('.panel-context-menu:popover-open button')].find(b=>b.querySelector('.menu-label')?.textContent===${JSON.stringify(label)})`;
  const onBar=selector=>`(n=>!!n&&!n.closest('.canvas-action-bar-item,.canvas-action-bar-completion').hidden)(document.querySelector('${bar} ${selector}'))`;
  const viaMore=async(path,kind)=>{
    await tap(await middle(`${bar} .canvas-action-bar-more`),kind);
    for(const label of path){
      await wait(`!!${menuRow(label)}`);
      await tap(await evaluate(`(r=>({x:r.x+r.width/2,y:r.y+r.height/2}))(${menuRow(label)}.getBoundingClientRect())`),kind);
    }
  };
  const pressBar=async(id,kind)=>{
    await wait(`!!document.querySelector('${bar} [data-command="${id}"]')&&${discBar}`);
    if(await evaluate(onBar(`[data-command="${id}"]`)))return tap(await middle(`${bar} [data-command="${id}"]`),kind);
    await viaMore([await evaluate(`${command(id)}.label`)],kind);
  };
  const pick=async(label,kind)=>{
    const choice='[data-toolbar-choice="selection-source"]';
    await wait(`!!document.querySelector('${bar} ${choice}')&&${discBar}`);
    if(!await evaluate(onBar(choice)))return viaMore(['Source',label],kind);
    await tap(await middle(`${bar} ${choice} > button`),kind);
    await wait(`!!document.querySelector('.toolbar-choice-menu')`);
    const entries=await evaluate(`[...document.querySelectorAll('.toolbar-choice-menu > button')].map(b=>[b.textContent.trim(),b.getAttribute('role')])`);
    assert.deepEqual(entries,[['Reference layers','menuitemradio'],['Editing layer','menuitemradio']],`${kind}: the Source dropdown`);
    await tap(await middle(`.toolbar-choice-menu > button:nth-child(${entries.findIndex(([text])=>text===label)+1})`),kind);
    await wait(`!document.querySelector('.toolbar-choice-menu')`);
    await wait(`document.querySelector('${bar} ${choice} .toolbar-choice-label')?.textContent===${JSON.stringify(label)}`);
  };
  const sample=async(p,test,label)=>{
    if(await tool()!=='pick_visible')await invoke('eyedropper');
    const at=await screen(p);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:at.x+1,y:at.y,buttons:0,pointerType:'mouse'});
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...at,buttons:0,pointerType:'mouse'});await settle();
    let last;
    for(const end=Date.now()+10000;Date.now()<end;await pause(100)) {
      last=await evaluate('(p=>p?Array.from(p.rgba):null)(layerApp.state().color_picker.preview)');
      if(test(last))return;
    }
    assert.fail(`${label}: ${JSON.stringify(last)}`);
  };
  const cloning=async()=>{
    if(await tool()==='pick_visible')await invoke('eyedropper');
    await wait(`layerApp.state().brush.tool==='clone'&&layerApp.state().layer_tools.tool==='paint'&&layerApp.app.brush_ready()`);
  };
  const blue=rgba=>!!rgba&&rgba[2]-rgba[0]>.4&&rgba[2]>.7;
  const paper=rgba=>!!rgba&&rgba.slice(0,3).every(v=>v>.9);
  const disc=()=>evaluate(`(a=>layerApp.state().canvas_bar?.context.kind==='clone_source'&&a?[(a[0]+a[2])/2,(a[1]+a[3])/2]:null)(layerApp.state().canvas_bar?.anchor)`);
  const near=(a,b,within)=>!!a&&Math.abs(a[0]-b[0])<=within&&Math.abs(a[1]-b[1])<=within;
  const awaitDisc=async(expected,within,label)=>{
    for(const end=Date.now()+5000;Date.now()<end;await pause(50))if(near(await disc(),expected,within)&&await evaluate(discBar))return;
    assert.fail(`${label}: the disc is at ${JSON.stringify(await disc())}, not ${JSON.stringify(expected)}`);
  };
  const showBar=async(at,kind,label)=>{
    await tap(await screen(at),kind);
    await wait(discBar,5000).catch(()=>assert.fail(`${label}: a ${kind} tap on the disc at ${at} shows its bar`));
    return disc();
  };
  const hideBar=async(at,kind)=>{
    await tap(await screen(at),kind);
    await wait(`layerApp.state().canvas_bar?.context.kind!=='clone_source'`,5000);
  };
  let target;
  const painted=()=>evaluate(`String(layerApp.state().layers.find(l=>String(l.id)==='${target}').paint_revision)`);
  const stroke=async(from,to,kind)=>{
    const before=await painted();
    await drag(await screen(from),await screen(to),kind);
    await wait(`String(layerApp.state().layers.find(l=>String(l.id)==='${target}').paint_revision)!=='${before}'`,10000);
  };
  const dragDisc=async(from,to,kind)=>{
    const before=await painted(),view=await camera();
    await drag(await screen(from),await screen(to),kind);
    await pause(200);
    assert.equal(await painted(),before,`${kind}: dragging the disc paints nothing`);
    assert.deepEqual((await camera()).t,view.t,`${kind}: dragging the disc never pans the canvas`);
  };
  const capture=async name=>{
    const b=await rect(bar),d=await screen(await disc());
    const x=Math.max(0,Math.min(b.x,d.x-40)-16),y=Math.max(0,Math.min(b.y,d.y-40)-16);
    const clip={x,y,width:Math.max(b.x+b.width,d.x+40)+16-x,height:Math.max(b.y+b.height,d.y+40)+16-y,scale:1};
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  const added=[];
  try {
    await send({type:'set_color_sample_size',width:1});
    await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
    if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
    await invoke('fit_canvas');
    const [width,height]=await size(),at=(x,y)=>[width*x,height*y];
    const newLayer=async()=>{
      await send({type:'layer',action:{op:'new',group:false,clipped:false}});
      const id=await evaluate('String(layerApp.state().layer_tools.editing_layer.id)');
      added.push(id);return id;
    };
    const reference=await newLayer();
    await invoke('rectangle_select');
    await drag(await screen(at(.1,.2)),await screen(at(.35,.8)),'mouse',6);
    await wait('layerApp.state().layer_tools.has_selection');
    await send({type:'set_color',rgba:BLUE});await invoke('fill_selection');await invoke('deselect');
    target=await newLayer();
    await invoke('use_reference_below');
    await wait(`!!layerApp.state().layers.find(l=>String(l.id)==='${reference}')?.reference`);
    await invoke('clone');
    await send({type:'set_brush_size',value:48});
    await cloning();
    assert.deepEqual(await Promise.all(['selection_reference','clone_aligned','clone_flip_horizontal','clone_reset_offset'].map(selected)),[true,true,false,false],
      'Clone Stamp copies the reference layers below, aligned and unflipped');
    await evaluate(`window.cloneKeys=[];for(const type of ['keydown','keyup'])window.addEventListener(type,e=>{if(e.key==='Alt')cloneKeys.push([type,e.defaultPrevented])})`);

    const s1=at(.15,.5);
    await alt(true);
    await wait(`${command('clone_source_arm')}.selected`,5000);
    const before=await painted();
    modifiers=1;await tap(await screen(s1),'mouse');modifiers=0;
    await alt(false);
    await wait(`!${command('clone_source_arm')}.selected`,5000);
    assert.deepEqual(await evaluate('cloneKeys'),[['keydown',true],['keyup',true]],'the page keeps Alt from the browser');
    assert.equal(await painted(),before,'Alt-click paints nothing');
    assert.ok(near(await showBar(s1,'mouse','mouse'),s1,1.5),'Alt-click sets the source');
    assert.equal(await enabled('clone_reset_offset'),false,'Reset Offset waits for an offset');
    for(const name of ['light','dark']){await send({type:'set_theme',theme:name});await pause(300);await capture(`clone-bar-${name}`);}
    await send({type:'set_theme',theme});
    await pick('Editing layer','mouse');
    assert.deepEqual([await selected('selection_editing'),await selected('selection_reference')],[true,false],'Source ▾ chooses the editing layer');
    await pick('Reference layers','mouse');
    assert.equal(await selected('selection_reference'),true,'Source ▾ chooses the reference layers again');
    const moved=at(.17,.42),followed=at(.27,.42);
    await dragDisc(s1,moved,'mouse');
    await awaitDisc(moved,2,'the mouse drags the disc');
    await hideBar(moved,'mouse');
    await stroke(at(.6,.42),at(.7,.42),'mouse');
    assert.ok(near(await showBar(followed,'mouse','mouse after a stroke'),followed,3),'an aligned source follows the mouse stroke');
    assert.equal(await enabled('clone_reset_offset'),true,'an aligned stroke keeps an offset');
    await sample(at(.62,.42),blue,'the mouse stroke copies the reference');
    await invoke('undo');
    await sample(at(.62,.42),paper,'one undo removes the mouse stroke');
    await cloning();
    await showBar(followed,'mouse','mouse after undo');
    await pressBar('clone_reset_offset','mouse');
    await wait(`!${command('clone_reset_offset')}.enabled`,5000);
    await pressBar('clone_aligned','mouse');
    await wait(`!${command('clone_aligned')}.selected`,5000);
    await hideBar(followed,'mouse');
    await stroke(at(.6,.62),at(.68,.62),'mouse');
    assert.ok(near(await showBar(followed,'mouse','mouse after an unaligned stroke'),followed,1.5),'a source that is not aligned stays at the disc');
    await sample(at(.62,.62),blue,'a stroke that is not aligned starts copying at the disc');
    await invoke('undo');
    await sample(at(.62,.62),paper,'one undo removes it');
    await cloning();
    await showBar(followed,'mouse','mouse before Aligned');
    await pressBar('clone_aligned','mouse');
    await wait(`${command('clone_aligned')}.selected`,5000);
    for(const on of [true,false]) {
      await pressBar('clone_flip_horizontal','mouse');
      await wait(`${command('clone_flip_horizontal')}.selected===${on}`,5000);
    }
    await hideBar(followed,'mouse');

    const touched=at(.2,.5);
    await dragDisc(followed,touched,'touch');
    assert.notEqual(await evaluate(`layerApp.state().canvas_bar?.context.kind??null`),'clone_source','a drag is not a tap');
    assert.ok(near(await showBar(touched,'touch','touch'),touched,2),'a finger drags the disc');
    for(const on of [true,false]) {
      await pressBar('clone_flip_vertical','touch');
      await wait(`${command('clone_flip_vertical')}.selected===${on}`,5000);
    }
    await pick('Editing layer','touch');
    await pick('Reference layers','touch');
    const untouched=await painted();
    await alt(true);
    modifiers=1;await tap(await screen(at(.6,.3)),'touch');modifiers=0;
    await alt(false);
    await awaitDisc(touched,1,'Alt with a finger never sets the source');
    await drag(await screen(at(.6,.3)),await screen(at(.7,.35)),'touch');
    await pause(200);
    assert.equal(await painted(),untouched,'a finger never paints with Clone Stamp');
    assert.ok(near(await disc(),touched,1),'a finger elsewhere never moves the source');
    await hideBar(touched,'touch');

    await send({type:'open_settings',page:'input'});
    const click=async selector=>{await wait(`!!document.querySelector(${JSON.stringify(selector)})`);await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();};
    await click('[data-trigger="pen.button.primary"]');
    if(await evaluate(`document.querySelector('#pen-button-same').checked`))await click('#pen-button-same');
    await click('#pen-button-action-retouching');
    await click('[id="action-command.CloneSourceArm"]');
    assert.equal(await evaluate("layerApp.state().settings.pen_buttons['pen.button.primary'].retouching"),'command.CloneSourceArm');
    await send({type:'close_settings'});
    await cloning();
    const s3=at(.18,.35),p3=await screen(s3);
    const pen=(type,p,button,buttons)=>call('Input.dispatchMouseEvent',{type,...p,button,buttons,clickCount:1,pointerType:'pen',force:buttons&1?.6:0});
    await pen('mouseMoved',p3,'none',0);
    await pen('mousePressed',p3,'right',2);await settle();
    await wait(`${command('clone_source_arm')}.selected`,5000);
    const beforePen=await painted();
    await pen('mousePressed',p3,'left',3);await pause(40);await pen('mouseReleased',p3,'left',2);await settle();
    await pen('mouseReleased',p3,'right',0);await settle();
    await wait(`!${command('clone_source_arm')}.selected`,5000);
    assert.equal(await painted(),beforePen,'the side button and a tap paint nothing');
    assert.ok(near(await showBar(s3,'pen','pen'),s3,1.5),'the side button and a pen tap set the source');
    await hideBar(s3,'pen');
    const s4=at(.15,.45);
    await alt(true);
    await wait(`${command('clone_source_arm')}.selected`,5000);
    modifiers=1;await tap(await screen(s4),'pen');modifiers=0;
    await alt(false);
    assert.ok(near(await showBar(s4,'pen','pen after Alt'),s4,1.5),'Alt and a pen tap set the source');
    const penMoved=at(.17,.4),last=at(.27,.55);
    await dragDisc(s4,penMoved,'pen');
    await awaitDisc(penMoved,2,'the pen drags the disc');
    await hideBar(penMoved,'pen');
    for(const y of [.45,.6])await stroke(at(.6,y),at(.7,y),'pen');
    assert.ok(near(await showBar(last,'pen','pen after two strokes'),last,3),'an aligned source follows both pen strokes');
    for(const y of [.45,.6])await sample(at(.62,y),blue,`the pen stroke at ${y} copies the reference`);
    await invoke('undo');
    await sample(at(.62,.6),paper,'undo removes the last pen stroke');
    await sample(at(.62,.45),blue,'and only the last one');
    await invoke('undo');
    await sample(at(.62,.45),paper,'each pen stroke is one undo step');
    await cloning();
    await showBar(last,'pen','pen after undo');
    await pressBar('clone_reset_offset','pen');
    await wait(`!${command('clone_reset_offset')}.enabled`,5000);
    console.log(`PASS clone (${device?'tablet':'desktop'}): Alt (kept from the browser) and a side button bound to Set Source set the source with mouse and pen, never with a finger; the disc drags at once with mouse, touch and pen without painting or panning; a tap shows its bar, whose Source ▾, Aligned, Flip and Reset Offset work; aligned and unaligned strokes copy the reference in one undo step each; captures in ${directory}`);
  } catch(error) {
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error('Clone state',await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector('#status').textContent,
      tool:layerApp.state().layer_tools.tool,brush:layerApp.state().brush.tool,bar:layerApp.state().canvas_bar,keys:window.cloneKeys},(_,v)=>typeof v==="bigint"?String(v):v)`));
    throw error;
  } finally {
    await send({type:'open_settings',page:'input'});
    await send({type:'preferences',action:{type:'reset_trigger',trigger:'pen.button.primary'}});
    await send({type:'close_settings'});
    if(await tool()==='pick_visible')await send({type:'invoke',command:'eyedropper'});
    for(const id of added.reverse())await send({type:'layer',action:{op:'delete',id:Number(id)}});
    await send({type:'invoke',command:'brush'});
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme});
  }
}
