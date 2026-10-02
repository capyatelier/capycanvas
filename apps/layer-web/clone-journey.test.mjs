import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {crc32,deflateSync} from 'node:zlib';

const BLUE=[.1,.3,.8,1];
const LIGHT=[199,184,158].map(v=>v/255),DARK=[107,92,77].map(v=>v/255),SCRATCH=[230,64,51].map(v=>v/255),DOT=[242,217,77].map(v=>v/255);
const bar='.canvas-action-bar';
const visible=`(b=>!!b&&!b.hidden&&!b.classList.contains('suppressed'))(document.querySelector('${bar}'))`;
const discBar=`layerApp.state().canvas_bar?.context.kind==='clone_source'&&${visible}`;

async function retouching({call,evaluate,settle,device,name,sampleSize}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??`artifacts/${name}/${device?'web-tablet':'web'}`;
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
  const key=async(key,code,vk,modifiers=0)=>{
    for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:vk,modifiers,...(type==='keyDown'?{text:key,unmodifiedText:key}:{})});
    await settle();
  };
  let touchId=900;
  const pointer=(type,p,kind,modifiers=0)=>kind==='touch'
    ?call('Input.dispatchTouchEvent',{type:{mousePressed:'touchStart',mouseMoved:'touchMove',mouseReleased:'touchEnd'}[type],touchPoints:type==='mouseReleased'?[]:[{id:touchId,x:p.x,y:p.y}],modifiers})
    :call('Input.dispatchMouseEvent',{type,...p,modifiers,button:'left',buttons:type==='mouseReleased'?0:1,clickCount:1,pointerType:kind,force:type==='mouseReleased'?0:.6});
  const tap=async(p,kind,modifiers=0)=>{touchId++;await pointer('mousePressed',p,kind,modifiers);await pause(40);await pointer('mouseReleased',p,kind,modifiers);await settle();await pause(60);};
  const press=async(from,to,kind,steps=12)=>{
    await wait('layerApp.app.brush_ready()');
    touchId++;await pointer('mousePressed',from,kind);await pause(30);
    for(let i=1;i<=steps;i++){await pointer('mouseMoved',{x:from.x+(to.x-from.x)*i/steps,y:from.y+(to.y-from.y)*i/steps},kind);await settle();}
  };
  const lift=async(to,kind)=>{await pointer('mouseReleased',to,kind);await settle();};
  const hover=async p=>{await call('Input.dispatchMouseEvent',{type:'mouseMoved',...await screen(p),buttons:0,pointerType:'mouse'});await settle();};
  const drag=async(from,to,kind,steps=12)=>{await press(from,to,kind,steps);await lift(to,kind);};
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
      if(test(last))return last;
    }
    assert.fail(`${label}: ${JSON.stringify(last)}`);
  };
  const shown=async(p,radius=5)=>{
    const at=await screen(p);
    const {data}=await call('Page.captureScreenshot',{format:'png',clip:{x:at.x-radius,y:at.y-radius,width:2*radius+1,height:2*radius+1,scale:1}});
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${data}';await image.decode();const c=new OffscreenCanvas(image.width,image.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(image,0,0);const rgba=x.getImageData(0,0,c.width,c.height).data,sum=[0,0,0];for(let i=0;i<rgba.length;i+=4)for(let j=0;j<3;j++)sum[j]+=rgba[i+j]/255;return sum.map(v=>v*4/rgba.length);})()`);
  };
  const ready=async brush=>{
    if(await tool()==='pick_visible')await invoke('eyedropper');
    await wait(`layerApp.state().brush.tool===${JSON.stringify(brush)}&&layerApp.state().layer_tools.tool==='paint'&&layerApp.app.brush_ready()`);
  };
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
  const added=[];
  const track=id=>{added.push(id);return id;};
  const newLayer=async()=>{
    await send({type:'layer',action:{op:'new',group:false,clipped:false}});
    return track(await evaluate('String(layerApp.state().layer_tools.editing_layer.id)'));
  };
  let target;
  const painted=()=>evaluate(`String(layerApp.state().layers.find(l=>String(l.id)==='${target}').paint_revision)`);
  const repainted=before=>wait(`String(layerApp.state().layers.find(l=>String(l.id)==='${target}').paint_revision)!=='${before}'`,10000);
  const stroke=async(from,to,kind)=>{
    const before=await painted();
    await drag(await screen(from),await screen(to),kind);
    await repainted(before);
  };
  const altTap=async(p,kind)=>{
    await alt(true);
    await wait(`${command('clone_source_arm')}.selected`,5000);
    const before=await painted();
    await tap(await screen(p),kind,1);
    await alt(false);
    await wait(`!${command('clone_source_arm')}.selected`,5000);
    assert.equal(await painted(),before,`${kind}: Alt and a tap paint nothing`);
  };
  const dragDisc=async(from,to,kind)=>{
    const before=await painted(),view=await camera();
    await drag(await screen(from),await screen(to),kind);
    await pause(200);
    assert.equal(await painted(),before,`${kind}: dragging the disc paints nothing`);
    assert.deepEqual((await camera()).t,view.t,`${kind}: dragging the disc never pans the canvas`);
  };
  const around=async(p,margin)=>{const s=await screen(p);return{x:s.x-margin,y:s.y-margin,width:2*margin,height:2*margin};};
  const capture=async(name,boxes)=>{
    const x=Math.max(0,Math.min(...boxes.map(b=>b.x))-16),y=Math.max(0,Math.min(...boxes.map(b=>b.y))-16);
    const clip={x,y,width:Math.max(...boxes.map(b=>b.x+b.width))+16-x,height:Math.max(...boxes.map(b=>b.y+b.height))+16-y,scale:1};
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png',clip})).data,'base64'));
  };
  const theme=await evaluate('layerApp.state().settings.theme ?? null'),sampleWidth=await evaluate('layerApp.state().color_picker.sample_width');
  const captureThemes=async(name,boxes)=>{
    for(const shade of ['light','dark']){await send({type:'set_theme',theme:shade});await pause(300);await capture(`${name}-${shade}`,await boxes());}
    await send({type:'set_theme',theme});
  };
  const click=async selector=>{await wait(`!!document.querySelector(${JSON.stringify(selector)})`);await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`);await settle();};
  const bindSideButton=async()=>{
    await send({type:'open_settings',page:'input'});
    await click('[data-trigger="pen.button.primary"]');
    if(await evaluate(`document.querySelector('#pen-button-same').checked`))await click('#pen-button-same');
    await click('#pen-button-action-retouching');
    await click('[id="action-command.CloneSourceArm"]');
    assert.equal(await evaluate("layerApp.state().settings.pen_buttons['pen.button.primary'].retouching"),'command.CloneSourceArm');
    await send({type:'close_settings'});
  };
  const sideButtonTap=async p=>{
    const pen=(type,button,buttons)=>call('Input.dispatchMouseEvent',{type,...p,button,buttons,clickCount:1,pointerType:'pen',force:buttons&1?.6:0});
    await pen('mouseMoved','none',0);
    await pen('mousePressed','right',2);await settle();
    await wait(`${command('clone_source_arm')}.selected`,5000);
    await pen('mousePressed','left',3);await pause(40);await pen('mouseReleased','left',2);await settle();
    await pen('mouseReleased','right',0);await settle();
    await wait(`!${command('clone_source_arm')}.selected`,5000);
  };
  const resetSideButton=async()=>{
    await send({type:'open_settings',page:'input'});
    await send({type:'preferences',action:{type:'reset_trigger',trigger:'pen.button.primary'}});
    await send({type:'close_settings'});
  };
  const fail=async label=>{
    await writeFile(`${directory}/failure.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    console.error(`${label} state`,await evaluate(`JSON.stringify({error:layerApp.state().host_error,notice:layerApp.state().notice,status:document.querySelector('#status').textContent,
      tool:layerApp.state().layer_tools.tool,brush:layerApp.state().brush.tool,bar:layerApp.state().canvas_bar,keys:window.cloneKeys},(_,v)=>typeof v==="bigint"?String(v):v)`));
  };
  const restore=async()=>{
    if(await tool()==='pick_visible')await send({type:'invoke',command:'eyedropper'});
    for(const id of added.reverse())await send({type:'layer',action:{op:'delete',id:Number(id)}});
    await send({type:'invoke',command:'brush'});
    await send({type:'set_color_sample_size',width:sampleWidth});
    await send({type:'set_theme',theme});
  };
  await send({type:'set_color_sample_size',width:sampleSize});
  await wait(`(()=>{[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent==='Keep for Later')?.click();return !document.querySelector('dialog[open]');})()`);
  if(await evaluate('layerApp.state().layer_tools.has_selection'))await invoke('deselect');
  await invoke('fit_canvas');
  return {directory,wait,pause,send,invoke,command,selected,enabled,tool,size,screen,rect,middle,alt,key,tap,press,lift,hover,drag,pick,pressBar,sample,shown,ready,
    disc,near,awaitDisc,showBar,hideBar,track,newLayer,painted,repainted,stroke,altTap,dragDisc,around,captureThemes,bindSideButton,sideButtonTap,resetSideButton,fail,restore,
    set target(id){target=id;}};
}

export async function checkClone({call,evaluate,settle,device=false}) {
  const h=await retouching({call,evaluate,settle,device,name:'clone',sampleSize:1});
  const {directory,wait,pause,send,invoke,command,selected,enabled,screen,tap,drag,pick,pressBar,sample,ready,disc,near,awaitDisc,showBar,hideBar,painted,stroke,dragDisc}=h;
  const blue=rgba=>!!rgba&&rgba[2]-rgba[0]>.4&&rgba[2]>.7;
  const paper=rgba=>!!rgba&&rgba.slice(0,3).every(v=>v>.9);
  try {
    const [width,height]=await h.size(),at=(x,y)=>[width*x,height*y];
    const reference=await h.newLayer();
    await invoke('rectangle_select');
    await drag(await screen(at(.1,.2)),await screen(at(.35,.8)),'mouse',6);
    await wait('layerApp.state().layer_tools.has_selection');
    await send({type:'set_color',rgba:BLUE});await invoke('fill_selection');await invoke('deselect');
    h.target=await h.newLayer();
    await invoke('use_reference_below');
    await wait(`!!layerApp.state().layers.find(l=>String(l.id)==='${reference}')?.reference`);
    await invoke('clone');
    await send({type:'set_brush_size',value:48});
    await ready('clone');
    assert.deepEqual(await Promise.all(['selection_reference','clone_aligned','clone_flip_horizontal','clone_reset_offset'].map(selected)),[true,true,false,false],
      'Clone Stamp copies the reference layers below, aligned and unflipped');
    await evaluate(`window.cloneKeys=[];for(const type of ['keydown','keyup'])window.addEventListener(type,e=>{if(e.key==='Alt')cloneKeys.push([type,e.defaultPrevented])})`);

    const s1=at(.15,.5);
    await h.altTap(s1,'mouse');
    assert.deepEqual(await evaluate('cloneKeys'),[['keydown',true],['keyup',true]],'the page keeps Alt from the browser');
    assert.ok(near(await showBar(s1,'mouse','mouse'),s1,1.5),'Alt-click sets the source');
    assert.equal(await enabled('clone_reset_offset'),false,'Reset Offset waits for an offset');
    await h.captureThemes('clone-bar',async()=>[await h.rect(bar),await h.around(await disc(),40)]);
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
    await ready('clone');
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
    await ready('clone');
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
    await h.altTap(at(.6,.3),'touch');
    await awaitDisc(touched,1,'Alt with a finger never sets the source');
    await drag(await screen(at(.6,.3)),await screen(at(.7,.35)),'touch');
    await pause(200);
    assert.equal(await painted(),untouched,'a finger never paints with Clone Stamp');
    assert.ok(near(await disc(),touched,1),'a finger elsewhere never moves the source');
    await hideBar(touched,'touch');

    await h.bindSideButton();
    await ready('clone');
    const s3=at(.18,.35);
    const beforePen=await painted();
    await h.sideButtonTap(await screen(s3));
    assert.equal(await painted(),beforePen,'the side button and a tap paint nothing');
    assert.ok(near(await showBar(s3,'pen','pen'),s3,1.5),'the side button and a pen tap set the source');
    await hideBar(s3,'pen');
    const s4=at(.15,.45);
    await h.altTap(s4,'pen');
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
    await ready('clone');
    await showBar(last,'pen','pen after undo');
    await pressBar('clone_reset_offset','pen');
    await wait(`!${command('clone_reset_offset')}.enabled`,5000);
    console.log(`PASS clone (${device?'tablet':'desktop'}): Alt (kept from the browser) and a side button bound to Set Source set the source with mouse and pen, never with a finger; the disc drags at once with mouse, touch and pen without painting or panning; a tap shows its bar, whose Source ▾, Aligned, Flip and Reset Offset work; aligned and unaligned strokes copy the reference in one undo step each; captures in ${directory}`);
  } catch(error) {
    await h.fail('Clone');
    throw error;
  } finally {
    await h.resetSideButton();
    await h.restore();
  }
}

export function png(width,height,pixel,channels=3) {
  const stride=width*channels+1,rows=Buffer.alloc(stride*height);
  for(let y=0;y<height;y++)for(let x=0;x<width;x++)rows.set(pixel(x,y),y*stride+1+x*channels);
  const chunk=(type,data)=>{
    const block=Buffer.alloc(data.length+12);
    block.writeUInt32BE(data.length,0);block.write(type,4,'latin1');data.copy(block,8);
    block.writeUInt32BE(crc32(block.subarray(4,data.length+8)),data.length+8);
    return block;
  };
  const header=Buffer.alloc(13);header.writeUInt32BE(width,0);header.writeUInt32BE(height,4);header[8]=8;header[9]=channels===4?6:2;
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
}

function texturedPhoto(width,height) {
  const [light,dark,scratch,dot]=[LIGHT,DARK,SCRATCH,DOT].map(c=>c.map(v=>Math.round(v*255)));
  return png(width,height,(x,y)=>{
    if(x>=width*.62&&x<width*.68&&Math.abs(y-height*.45)<6)return scratch;
    if(Math.hypot(x-width*.8,y-height*.72)<11)return dot;
    let h=Math.imul(x,0x9e3779b1)^Math.imul(y,0x85ebca77);
    h=Math.imul(h^h>>>15,0x2c1b3c6d);h^=h>>>12;
    const n=((h>>>0)%9-4)*2.5;
    return (x>=width*.05&&x<width*.4&&y>=height*.15&&y<height*.85?light:dark).map(v=>v+n);
  });
}

export async function checkHeal({call,evaluate,settle,device=false}) {
  const original=await evaluate('JSON.parse(layerApp.app.workspace_view()).id');
  const workspace=async id=>{
    await evaluate(`layerApp.app.workspace_input(${JSON.stringify(JSON.stringify({type:'switch',id}))});null`);
    await evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+30000;(function check(){const v=JSON.parse(layerApp.app.workspace_view());if(v.id===${JSON.stringify(id)}&&!v.busy&&!v.dirty)resolve();else if(performance.now()>end)reject(Error('workspace ${id}'));else setTimeout(check,50);})();})`);
    await settle();
  };
  await evaluate('window.healSettings=layerApp.state().settings');
  const h=await retouching({call,evaluate,settle,device,name:'heal',sampleSize:15});
  const {directory,wait,pause,send,invoke,command,selected,screen,alt,key,tap,press,lift,sample,shown,ready,disc,near,awaitDisc,showBar,hideBar,painted,repainted,stroke,dragDisc}=h;
  const like=(expected,within)=>rgba=>!!rgba&&expected.every((v,i)=>Math.abs(rgba[i]-v)<=within);
  const scratched=rgba=>!!rgba&&rgba[0]-rgba[1]>.3;
  const choose=async(brush,kind)=>{
    const tile=await evaluate(`layerApp.state().workspace.layout.panels.find(p=>p.id==='toolbar').content.tiles.find(t=>t.control?.command===${JSON.stringify(brush)}).id`);
    await tap(await h.middle(`.toolbar-controls[data-panel="toolbar"] [data-tile="${tile}"] > button`),kind);
    await ready(brush);
  };
  const brush=()=>evaluate('layerApp.state().brush.tool');
  const settings=()=>evaluate('layerApp.dispatch({type:"restore_settings",settings:healSettings});null').then(settle);
  try {
    await workspace('builtin:workspace:photographer');
    await invoke('fit_canvas');
    const [width,height]=await h.size(),at=(x,y)=>[width*x,height*y];
    await evaluate(`(()=>{const file=new File([Uint8Array.from(atob('${texturedPhoto(width,height).toString('base64')}'),c=>c.charCodeAt(0))],'Texture.png',{type:'image/png'});
      window.healPicker=window.showOpenFilePicker;window.showOpenFilePicker=async()=>[{name:file.name,kind:'file',async getFile(){return file}}];})()`);
    await invoke('import_image');
    await wait(`layerApp.state().canvas_bar?.context.kind==='placement'`);
    assert.deepEqual(await evaluate('layerApp.state().canvas_bar.anchor'),[0,0,width,height],'the photo covers the canvas');
    await invoke('apply_transform');
    await wait(`!layerApp.state().canvas_bar`);
    const photo=h.track(await evaluate('String(layerApp.state().layer_tools.editing_layer.id)'));
    h.target=await h.newLayer();
    await invoke('use_reference_below');
    await wait(`!!layerApp.state().layers.find(l=>String(l.id)==='${photo}')?.reference`);

    await choose('heal','mouse');
    await invoke('brush');
    const cycle=[];
    for(let i=0;i<3;i++){await key('s','KeyS',83);cycle.push(await brush());}
    assert.deepEqual(cycle,['clone','heal','spot_heal'],'S cycles Clone Stamp, Healing Brush and Spot Healing Brush');
    await send({type:'open_settings',page:'shortcuts'});
    await send({type:'preferences',action:{type:'select_keymap',id:'photoshop'}});
    await send({type:'close_settings'});
    await key('j','KeyJ',74);
    assert.equal(await brush(),'spot_heal','J chooses Spot Healing in the Photoshop keymap');
    await key('J','KeyJ',74,8);
    assert.equal(await brush(),'heal','Shift+J chooses Healing in the Photoshop keymap');
    await settings();
    await ready('heal');
    await send({type:'set_brush_size',value:120});
    assert.deepEqual(await Promise.all(['selection_reference','clone_aligned'].map(selected)),[true,true],'Healing copies the reference layers below, aligned');

    const lightArea=at(.25,.3),darkArea=at(.65,.33),scratch=at(.65,.45);
    const surroundings=await sample(darkArea,like(DARK,.05),'the darker area around the scratch');
    await sample(lightArea,like(LIGHT,.05),'the light textured area');
    let onScreen;
    const heal=async(kind,source)=>{
      await sample(scratch,scratched,`${kind}: the scratch before healing`);
      await ready('heal');
      onScreen??=Math.abs((await shown(lightArea))[0]-(await shown(darkArea))[0])>.2;
      const before=await painted(),from=await screen(at(.59,.45)),to=await screen(at(.71,.45));
      await press(from,to,kind);
      if(onScreen)assert.ok(like(LIGHT,.12)(await shown(scratch)),`${kind}: while the pen is down the stroke is the clone of the light area`);
      await lift(to,kind);
      await repainted(before);
      const healed=await sample(scratch,like(surroundings,.05),`${kind}: the healed stroke takes the tone of the darker area around it`);
      assert.ok(!like(LIGHT,.2)(healed),`${kind}: the healed stroke is not the clone`);
      await ready('heal');
      return [source[0]+width*.12,source[1]];
    };
    const undoHeal=async kind=>{
      await invoke('undo');
      await sample(scratch,scratched,`${kind}: one undo brings the scratch back`);
      await ready('heal');
    };

    const s1=at(.15,.45);
    await ready('heal');
    await h.altTap(s1,'mouse');
    assert.ok(near(await showBar(s1,'mouse','mouse'),s1,1.5),'Alt-click sets the healing source');
    await hideBar(s1,'mouse');
    const followed=await heal('mouse',s1);
    await undoHeal('mouse');
    assert.ok(near(await showBar(followed,'mouse','mouse after a stroke'),followed,3),'an aligned source follows the healing stroke');
    const moved=at(.17,.5);
    await dragDisc(followed,moved,'mouse');
    await awaitDisc(moved,2,'the mouse drags the disc');
    await hideBar(moved,'mouse');

    const touched=at(.2,.55);
    await dragDisc(moved,touched,'touch');
    assert.ok(near(await showBar(touched,'touch','touch'),touched,2),'a finger drags the disc');
    const untouched=await painted();
    await h.altTap(at(.6,.25),'touch');
    await awaitDisc(touched,1,'Alt with a finger never sets the source');
    await h.drag(await screen(at(.6,.25)),await screen(at(.7,.3)),'touch');
    await pause(200);
    assert.equal(await painted(),untouched,'a finger never paints with Healing');
    assert.ok(near(await disc(),touched,1),'a finger elsewhere never moves the source');
    await hideBar(touched,'touch');

    await h.bindSideButton();
    await ready('heal');
    const s3=at(.18,.35);
    await h.sideButtonTap(await screen(s3));
    assert.equal(await painted(),untouched,'the side button and a tap paint nothing');
    assert.ok(near(await showBar(s3,'pen','pen'),s3,1.5),'the side button and a pen tap set the healing source');
    await hideBar(s3,'pen');
    const s4=at(.14,.4);
    await h.altTap(s4,'pen');
    assert.ok(near(await showBar(s4,'pen','pen after Alt'),s4,1.5),'Alt and a pen tap set the healing source');
    const penMoved=at(.15,.45);
    await dragDisc(s4,penMoved,'pen');
    await awaitDisc(penMoved,2,'the pen drags the disc');
    await hideBar(penMoved,'pen');
    const penFollowed=await heal('pen',penMoved);
    assert.ok(near(await showBar(penFollowed,'pen','pen after a stroke'),penFollowed,3),'an aligned source follows the pen stroke');
    await h.captureThemes('heal',async()=>[await h.rect('.canvas-action-bar'),await h.around(await disc(),40),await h.around(scratch,90)]);
    await hideBar(penFollowed,'pen');
    await undoHeal('pen');

    await choose('spot_heal','touch');
    for(const id of ['clone_source_arm','clone_aligned','clone_flip_horizontal','clone_flip_vertical','clone_reset_offset'])
      assert.deepEqual(await evaluate(`(c=>[c.enabled,c.disabled_reason])(${command(id)})`),[false,'Spot Healing finds its own source'],`${id} with Spot Healing`);
    const quiet=await painted();
    await alt(true);
    await pause(300);
    assert.deepEqual(await evaluate(`[${command('clone_source_arm')}.selected,layerApp.state().notice??null,layerApp.state().host_error??null]`),[false,null,null],'Alt does nothing with Spot Healing');
    await alt(false);
    assert.equal(await painted(),quiet);
    await send({type:'set_brush_size',value:100});
    const dot=at(.8,.72),beside=await sample(at(.8,.62),like(DARK,.05),'the darker area around the dot');
    for(const kind of ['mouse','pen']) {
      await sample(dot,like(DOT,.08),`${kind}: the dot`);
      await ready('spot_heal');
      const [x,y]=dot;
      await stroke([x-12,y],[x+12,y],kind);
      await sample(dot,like(beside,.05),`${kind}: one stroke heals the dot into the texture around it`);
      if(kind==='pen') {
        await ready('spot_heal');
        await h.hover(at(.5,.2));
        await h.captureThemes('spot-heal',async()=>[await h.around(dot,90)]);
      }
      await invoke('undo');
      await sample(dot,like(DOT,.08),`${kind}: one undo brings the dot back`);
      await ready('spot_heal');
    }
    console.log(`PASS heal (${device?'tablet':'desktop'}): the Photo toolbar, S and the Photoshop keymap's J and Shift+J choose Healing and Spot Healing; Alt and a side button bound to Set Source set the healing source with mouse and pen, never with a finger; the disc drags with mouse, touch and pen; a healing stroke ${onScreen?'previews as the clone and ':''}heals into the darker area around it, and Spot Healing removes a dot, with mouse and pen, one undo step each; Spot Healing's source commands are disabled and Alt does nothing${onScreen?'':'; the live preview is unchecked because screenshots here omit WebGPU pixels'}; captures in ${directory}`);
  } catch(error) {
    await h.fail('Heal');
    throw error;
  } finally {
    await h.restore();
    await settings();
    await evaluate('if("healPicker" in window)window.showOpenFilePicker=window.healPicker;delete window.healPicker;delete window.healSettings');
    await workspace(original);
  }
}
