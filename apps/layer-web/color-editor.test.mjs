import {createDrawing} from './new-drawing-fixture.mjs';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function assertColorEditorCells(evaluate,selector='.color-dialog[open]') {
  const {groups,...facts}=await evaluate(`(()=>{
    const dialog=document.querySelector(${JSON.stringify(selector)}),body=dialog.querySelector('.color-editor-body'),outer=dialog.getBoundingClientRect();
    const visible=node=>node.getClientRects().length&&getComputedStyle(node).visibility!=='hidden';
    const groups=[...dialog.querySelectorAll('.color-rows,.color-editor-head')].filter(visible).map(grid=>({
      name:grid.className,cells:[...grid.children].filter(visible).map(node=>{const r=node.getBoundingClientRect();return{name:node.className,text:node.textContent.trim().slice(0,80),bounds:[r.x,r.y,r.width,r.height]};}).filter(cell=>cell.bounds[2]>0&&cell.bounds[3]>0)
    }));
    const swatches=[...dialog.querySelectorAll('.color-editor-head .color-pair > *')].filter(visible).map(node=>{const r=node.getBoundingClientRect();return[r.width,r.height];});
    return{split:!body.classList.contains('single-pane'),outer_width:outer.width,outer_bounds:[outer.x,outer.y,outer.width,outer.height],preview_width:body.querySelector('.color-pair').getBoundingClientRect().width,swatches,groups};
  })()`);
  for(const size of facts.swatches)assert.deepEqual(size,[54,48],'Color previews retain their native minimum');
  assert.ok(facts.preview_width>=54*facts.swatches.length-1,'The preview container does not clip its native-sized swatches');
  for(const group of groups)for(const cell of group.cells){
    const [x,,width]=cell.bounds,[left,,outerWidth]=facts.outer_bounds;
    assert.ok(x>=left-1&&x+width<=left+outerWidth+1,`Color Editor cell leaves the page: ${JSON.stringify({...facts,group:group.name,cell})}`);
  }
  for(const group of groups)for(let i=0;i<group.cells.length;i++)for(const right of group.cells.slice(i+1)){
    const left=group.cells[i],a=left.bounds,b=right.bounds;
    const overlap=[Math.min(a[0]+a[2],b[0]+b[2])-Math.max(a[0],b[0]),Math.min(a[1]+a[3],b[1]+b[3])-Math.max(a[1],b[1])];
    assert.ok(overlap[0]<=1||overlap[1]<=1,`Color Editor grid cells overlap: ${JSON.stringify({...facts,group:group.name,left,right,overlap})}`);
  }
  return {...facts,grids:groups};
}

export async function checkColorEditor({call,evaluate,settle}) {
  const output=process.env.LAYER_TEST_ARTIFACTS||'artifacts/color-editor/web';
  await mkdir(output,{recursive:true});
  const wait=(condition,timeout=10000)=>evaluate(`new Promise((resolve,reject)=>{const start=performance.now();function poll(){try{if(${condition})resolve(true);else if(performance.now()-start>${timeout})reject(Error(${JSON.stringify(condition)}));else setTimeout(poll,20);}catch(e){reject(e)}}poll();})`);
  const capture=async name=>writeFile(`${output}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const dialog='.color-dialog[open]';
  const text=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent??null`);
  const center=async selector=>evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
  const mouse=async(points,{release=true}={})=>{
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...points[0],button:'none'});
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...points[0],button:'left',buttons:1,clickCount:1});
    for(const p of points.slice(1))await call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,button:'left',buttons:1});
    if(release)await call('Input.dispatchMouseEvent',{type:'mouseReleased',...points.at(-1),button:'left',buttons:0,clickCount:1});
    await settle();
  };
  const click=async selector=>mouse([await center(selector)]);
  const key=async(key,code=key,keyCode=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:keyCode});await settle();};
  const type=async(selector,value)=>{
    await click(selector);await wait(`document.activeElement?.matches('.color-value-input[data-color-value="${await evaluate(`document.querySelector(${JSON.stringify(selector)}).dataset.colorValue`)}"]')`);
    await call('Input.insertText',{text:value});await key('Enter','Enter',13);
  };
  const rowValues=row=>evaluate(`[0,1,2].map(i=>document.querySelector('${dialog} .color-value[data-color-value="${row}-'+i+'"]').textContent)`);
  const foreground=()=>evaluate('JSON.stringify(layerApp.state().colors.foreground)');
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'})`);
  await evaluate(createDrawing({width:512,height:384,paper:'white',relief:0}));
  await wait('!layerApp.documents.busy()&&layerApp.app.brush_ready()');
  await call('Browser.grantPermissions',{origin:await evaluate('location.origin'),permissions:['clipboardReadWrite','clipboardSanitizedWrite']},null);
  try { for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);
    await evaluate(`layerApp.dispatch({type:'color',action:{op:'definition',color:{space:'Srgb',rgba:[1,1,1,1]}}})`);
    await evaluate(`layerApp.dispatch({type:'color',action:{op:'editor_memory',memory:{forms:['rgb','hsb','oklch'],search:''}}})`);
    await settle();
    const original=await foreground();
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
    await wait(`!!document.querySelector('${dialog}')`);await wait(`document.querySelector('${dialog} .color-wheel').width>0`);await settle();
    await assertColorEditorCells(evaluate);
    await capture(`editor-${theme}`);
    const layout=await evaluate(`(()=>{const d=document.querySelector('${dialog}'),box=s=>d.querySelector(s).getBoundingClientRect(),middle=r=>r.top+r.height/2,dialogBox=d.getBoundingClientRect(),title=box('.color-editor-title'),pair=box('.color-editor-head .color-pair');return{title:title.left+title.width/2-(dialogBox.left+dialogBox.width/2),pair:pair.height,pick:box('.color-pick').height,icon:box('.color-pick svg').width,centers:[box('.color-pick'),box('.color-hex .color-value')].map(r=>middle(r)-middle(pair)),copies:[...d.querySelectorAll('.color-copy')].map(b=>Math.round(b.getBoundingClientRect().right)),footer:Math.round(box('.suggested-action').right),overflow:d.querySelector('.color-editor-body').scrollWidth>d.querySelector('.color-editor-body').clientWidth}})()`);
    assert.ok(Math.abs(layout.title)<=1,`the title is centered ${JSON.stringify(layout)}`);
    assert.equal(layout.pair,layout.pick,`Current and New match the eyedropper height ${JSON.stringify(layout)}`);
    assert.equal(layout.icon,24);
    assert.ok(layout.centers.every(offset=>Math.abs(offset)<=.5),`the eyedropper and hex center on Current and New ${JSON.stringify(layout)}`);
    assert.equal(new Set(layout.copies).size,1,`copy buttons line up ${JSON.stringify(layout)}`);
    assert.equal(layout.overflow,false);
    const formName=row=>text(`${dialog} .color-format[data-color-format="${row}"] .current`);
    assert.deepEqual([await formName(0),await formName(1),await formName(2)],['RGB','HSB','OKLCH']);
    const size=()=>evaluate(`(()=>{const r=document.querySelector('${dialog}').getBoundingClientRect();return [r.width,r.height]})()`),opened=await size();
    for(const [row,forms] of [[0,['rgb_unit','linear_rgb','rgb']],[1,['hsl','hsb']],[2,['oklab','oklch']]]) for(const form of forms) {
      await click(`${dialog} .color-format[data-color-format="${row}"]`);await click(`${dialog} .color-format-menu [data-form="${form}"]`);
      assert.deepEqual(await size(),opened,`${form} keeps the dialog size`);
      if(form==='linear_rgb')await capture(`linear-${theme}`);
    }
    assert.deepEqual(await rowValues(0),['255','255','255']);
    await evaluate(`(()=>{const data=new DataTransfer();data.setData('text/plain','rgb(202 75 53)');document.querySelector('${dialog}').dispatchEvent(new ClipboardEvent('paste',{clipboardData:data,bubbles:true}))})()`);
    assert.equal(await text(`${dialog} .color-value[data-color-value="hex"]`),'#CA4B35');
    assert.deepEqual(await rowValues(0),['202','75','53']);
    await click(`${dialog} .color-copy[data-color-copy="0"]`);
    assert.equal(await evaluate('navigator.clipboard.readText()'),'rgb(202 75 53)');
    await click(`${dialog} .color-copy[data-color-copy="hex"]`);
    assert.equal(await evaluate('navigator.clipboard.readText()'),'#CA4B35');
    await click(`${dialog} .color-format[data-color-format="1"]`);
    await click(`${dialog} .color-format-menu [data-form="hsl"]`);
    assert.equal(await formName(1),'HSL');
    await type(`${dialog} .color-value[data-color-value="1-0"]`,'200');
    assert.equal(await text(`${dialog} .color-value[data-color-value="1-0"]`),'200°');
    await type(`${dialog} .color-value[data-color-value="0-0"]`,'lots');
    assert.ok(await evaluate(`document.querySelector('${dialog} .color-value-input[data-color-value="0-0"]').hasAttribute('aria-invalid')`));
    assert.ok(await evaluate(`document.querySelector('${dialog} .suggested-action').disabled`));
    assert.match(await text(`${dialog} .color-editor-error`),/Red/);
    assert.deepEqual(await size(),opened,'an error keeps the dialog size');
    await capture(`refused-${theme}`);
    await key('Escape','Escape',27);
    assert.ok(await evaluate(`!!document.querySelector('${dialog}') && !document.querySelector('${dialog} .suggested-action').disabled && !document.querySelector('${dialog} .color-editor-error').textContent`));
    const before=Number((await rowValues(0))[2]),start=await center(`${dialog} .color-value[data-color-value="0-2"]`);
    await mouse([start,{x:start.x,y:start.y-6},{x:start.x,y:start.y-20}]);
    const after=Number((await rowValues(0))[2]);
    assert.ok(after>before,`dragging up raises the number ${before} -> ${after}`);
    assert.ok(await evaluate(`document.querySelector('${dialog} .color-value-input[data-color-value="0-2"]').hidden`),'a drag never opens the field');
    await click(`${dialog} .color-current`);
    assert.equal(await text(`${dialog} .color-value[data-color-value="hex"]`),'#FFFFFF');
    const slide=async open=>{
      await evaluate(`window.sheetTops=[];(function sample(frame){const s=document.querySelector('${dialog} .color-sheet');if(s&&!s.hidden)sheetTops.push(s.getBoundingClientRect().top);if(frame<40)requestAnimationFrame(()=>sample(frame+1))})(0)`);
      await click(`${dialog} ${open?'.color-swatches':'.color-sheet-close'}`);
      await wait(open?`document.querySelector('${dialog} .color-sheet').getBoundingClientRect().top===document.querySelector('${dialog} .color-editor-page').getBoundingClientRect().top`:`document.querySelector('${dialog} .color-sheet').hidden`);
      const tops=await evaluate('sheetTops');
      assert.ok(tops.every((top,i)=>!i||(open?top<=tops[i-1]+1:top>=tops[i-1]-1))&&new Set(tops.map(Math.round)).size>=3,`the sheet slides ${open?'up':'down'} ${JSON.stringify(tops)}`);
    };
    await slide(true);
    await wait(`document.activeElement===document.querySelector('${dialog} .color-sheet input')`);
    const pairs=await evaluate(`(()=>{const d=document.querySelector('${dialog}'),box=n=>n.getBoundingClientRect(),size=n=>[box(n).width,box(n).height],mini=d.querySelector('.color-mini .color-pair'),main=d.querySelector('.color-editor-head .color-pair'),m=box(mini),cancel=box(d.querySelector('.color-cancel')),style=n=>[getComputedStyle(n).borderRadius,getComputedStyle(n,'::after').boxShadow];return{mini:size(mini),halves:[...mini.children].map(size),main:size(main),center:m.top+m.height/2-(cancel.top+cancel.height/2),style:style(mini),mainStyle:style(main),hex:box(d.querySelector('.color-mini-hex')).left-m.right}})()`);
    assert.deepEqual([pairs.mini,pairs.halves,pairs.main],[[76,34],[[38,34],[38,34]],[108,48]],`the footer Current|New pair takes the main pair's proportions at button height ${JSON.stringify(pairs)}`);
    assert.ok(Math.abs(pairs.center)<=.5,`the footer pair centers on the footer buttons ${JSON.stringify(pairs)}`);
    assert.deepEqual(pairs.style,pairs.mainStyle,'the footer pair keeps the main pair border and radius');
    assert.notEqual(pairs.style[1],'none','the pair border paints above both halves');
    assert.ok(pairs.hex>0,'the hex label follows the footer pair');
    const reach=await evaluate(`[document.querySelector('${dialog} .color-sheet-body').getBoundingClientRect().bottom,document.querySelector('${dialog} .color-sheet').getBoundingClientRect().bottom,document.querySelector('${dialog} .color-editor-page').getBoundingClientRect().bottom,document.querySelector('${dialog} .dialog-footer').getBoundingClientRect().top]`);
    assert.ok(Math.abs(reach[0]-reach[3])<=1,`the swatch list reaches the divider ${JSON.stringify(reach)}`);
    await call('Input.insertText',{text:'zzzz-no-color'});await settle();
    assert.ok(await evaluate(`!!document.querySelector('${dialog} .color-sheet-empty')`));
    await evaluate(`(()=>{const s=document.querySelector('${dialog} .color-sheet input');s.value='';s.dispatchEvent(new Event('input'))})()`);await settle();
    await capture(`sheet-${theme}`);
    await click(`${dialog} .palette-tile[data-color-tile="sheet"]`);
    assert.notEqual(await text(`${dialog} .color-value[data-color-value="hex"]`),'#FFFFFF');
    await evaluate(`(()=>{const s=document.querySelector('${dialog} .color-sheet input');s.value='Ink';s.dispatchEvent(new Event('input'))})()`);await settle();
    await slide(false);
    await click(`${dialog} .color-cancel`);
    await wait(`!document.querySelector('${dialog}')`);
    assert.equal(await foreground(),original,'Cancel keeps the paint');
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
    await wait(`!!document.querySelector('${dialog}')`);
    assert.equal(await formName(1),'HSL','row formats are remembered');
    await slide(true);
    assert.equal(await evaluate(`document.querySelector('${dialog} .color-sheet input').value`),'Ink','swatch search is remembered');
    await evaluate(`(()=>{const search=document.querySelector('${dialog} .color-sheet input');search.value='';search.dispatchEvent(new Event('input'))})()`);
    await key('Escape','Escape',27);
    await wait(`document.querySelector('${dialog} .color-sheet').hidden && !!document.querySelector('${dialog}')`);
    await key('Escape','Escape',27);
    await wait(`!document.querySelector('${dialog}')`);
    assert.equal(await foreground(),original,'Escape from the sheet then the dialog keeps the paint');
    await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
    await wait(`!!document.querySelector('${dialog}')`);
    await evaluate(`(()=>{const data=new DataTransfer();data.setData('text/plain','#CA4B35');document.querySelector('${dialog}').dispatchEvent(new ClipboardEvent('paste',{clipboardData:data,bubbles:true}))})()`);
    await click(`${dialog} .color-pick`);
    await wait(`!document.querySelector('${dialog}') && !document.querySelector('.color-strip').hidden && layerApp.app.color_preview().picker.editor`);
    const canvas=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect(),a=c.work_area;return{x:r.x+(a[0]+a[2]/2)*r.width/c.viewport[0],y:r.y+(a[1]+a[3]/2)*r.height/c.viewport[1]}})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...canvas,button:'none'});await settle();
    await wait(`document.querySelector('.color-strip-hex').textContent==='#FFFFFF'`);
    await capture(`strip-${theme}`);
    const edge=await evaluate(`(()=>{const r=document.querySelector('.color-strip').getBoundingClientRect();return{x:r.left-4,y:r.top+r.height/2,left:r.left}})()`);
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:edge.x,y:edge.y,button:'none'});
    await wait(`document.querySelector('.color-strip').getBoundingClientRect().left!==${edge.left}`);
    const hovered=await center('.color-strip');
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...hovered,button:'none'});
    await wait(`(r=>${hovered.x}<r.left||${hovered.x}>r.right||${hovered.y}<r.top||${hovered.y}>r.bottom)(document.querySelector('.color-strip').getBoundingClientRect())`);
    await click('.color-strip');
    await wait(`!!document.querySelector('${dialog}') && document.querySelector('.color-strip').hidden`);
    assert.equal(await text(`${dialog} .color-value[data-color-value="hex"]`),'#CA4B35','the strip goes back without a change');
    await click(`${dialog} .color-pick`);
    await wait(`!document.querySelector('${dialog}') && !document.querySelector('.color-strip').hidden`);
    await mouse([canvas]);
    await wait(`!!document.querySelector('${dialog}')`);
    assert.equal(await text(`${dialog} .color-value[data-color-value="hex"]`),'#FFFFFF','the picked color returns to the dialog');
    assert.equal(await foreground(),original,'picking only changes the draft');
    await type(`${dialog} .color-value[data-color-value="hex"]`,'#2F7B9C');
    await click(`${dialog} .suggested-action`);
    await wait(`!document.querySelector('${dialog}')`);
    assert.deepEqual(await evaluate('layerApp.state().colors.foreground.rgba.slice(0,3).map(v=>Math.round(v*255))'),[47,123,156]);
  }
  await evaluate(`layerApp.dispatch({type:'set_color',rgba:[0.2,0.4,0.8,1]})`);
  await evaluate(`layerApp.dispatch({type:'effect',action:{op:'insert',effect:'solid_color'}})`);await settle();
  const fillId=await evaluate(`String(layerApp.state().layers.find(l=>l.label==='Solid Color').id)`);
  const fillThumb=`[data-layer="${fillId}"] .layer-thumbnail`;
  const square=hex=>`(()=>{const c=document.querySelector('${fillThumb} canvas'),d=c.getContext('2d').getImageData(0,0,32,32).data,want=${JSON.stringify(hex)};for(let i=0;i<d.length;i+=4)for(let k=0;k<3;k++)if(Math.abs(d[i+k]-want[k])>2)return false;return true})()`;
  await wait(square([51,102,204]),15000);
  await click(fillThumb);
  await wait(`!!document.querySelector('${dialog}')`);
  assert.equal(await text(`${dialog} .color-value[data-color-value="hex"]`),'#3366CC','the fill thumbnail opens its color');
  await type(`${dialog} .color-value[data-color-value="hex"]`,'#D7263D');
  await click(`${dialog} .suggested-action`);
  await wait(`!document.querySelector('${dialog}')`);
  await wait(square([215,38,61]),15000);
  await capture('fill-thumbnail');
  await call('Emulation.setDeviceMetricsOverride',{width:640,height:900,deviceScaleFactor:1,mobile:false});await settle();
  await evaluate(`[...document.querySelectorAll('button[aria-label="Edit Color"]')].find(b=>b.getBoundingClientRect().width>0).click()`);
  await wait(`!!document.querySelector('${dialog}')`);await settle();
  await assertColorEditorCells(evaluate);
  await capture('narrow');
  const narrowHeader=await evaluate(`(()=>{const d=document.querySelector('${dialog}'),pick=d.querySelector('.color-pick'),hex=d.querySelector('.color-hex'),p=pick.getBoundingClientRect(),h=hex.getBoundingClientRect();return {pick:[p.x,p.y,p.width,p.height],hex:[h.x,h.y,h.width,h.height],target:document.elementFromPoint(p.x+p.width/2,p.y+p.height/2)?.closest('.color-pick')===pick};})()`);
  assert.equal(narrowHeader.target,true,`The narrow eyedropper has its full hit target: ${JSON.stringify(narrowHeader)}`);
  await click(`${dialog} .color-pick`);
  await wait(`!document.querySelector('${dialog}') && !document.querySelector('.color-strip').hidden`);await settle();
  const narrow=await evaluate(`(()=>{const c=layerApp.app.camera(),r=layerApp.canvas.getBoundingClientRect();return c.work_area[2]*r.width/c.viewport[0]})()`);
  const strip=await center('.color-strip');
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',...strip,button:'none'});
  await wait(`(r=>${strip.y}<r.top||${strip.y}>r.bottom||${strip.x}<r.left||${strip.x}>r.right)(document.querySelector('.color-strip').getBoundingClientRect())`);
  if(narrow<2*272+32)assert.ok(await evaluate(`document.querySelector('.color-strip').getBoundingClientRect().top>${strip.y}`),'a narrow work area uses a bottom corner');
  await capture('strip-narrow');
  await key('Escape','Escape',27);
  await wait(`!!document.querySelector('${dialog}')`);
  await click(`${dialog} .color-cancel`);
  await call('Emulation.clearDeviceMetricsOverride',{});await settle();
  } catch(error) { await capture('failure'); throw error; }
}
