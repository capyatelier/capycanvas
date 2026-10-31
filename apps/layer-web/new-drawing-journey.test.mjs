import {dropdownChoice,clickDropdownChoice,captureDropdowns} from './dropdown-fixture.mjs';
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {createDrawing} from './new-drawing-fixture.mjs';

const dialog='#new-drawing-dialog[open]';

export async function checkNewDrawing({call,evaluate,settle}) {
  const directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/new-drawing-dialog/web';
  await mkdir(directory,{recursive:true});
  const wait=(condition,timeout=30000)=>evaluate(`new Promise((resolve,reject)=>{const end=performance.now()+${timeout};function poll(){try{if(${condition})resolve(true);else if(performance.now()>end)reject(Error(${JSON.stringify(condition)}+": "+JSON.stringify({tabs:layerApp.app.document_tabs(0),file:layerApp.state().document_file,documentsBusy:layerApp.documents.busy(),brushReady:layerApp.app.brush_ready(),workspace:JSON.parse(layerApp.app.workspace_view()),newDrawing:layerApp.state().new_drawing,requests:layerApp.state().requests,error:layerApp.state().host_error,startup:layerApp.startupTimes},(_,value)=>typeof value==="bigint"?String(value):value)));else setTimeout(poll,20);}catch(e){reject(e)}}poll();})`);
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const view=()=>evaluate('JSON.parse(JSON.stringify(layerApp.state().new_drawing))');
  const drawer=async()=>(await view())?.drawer??null;
  const node=selector=>`document.querySelector(${JSON.stringify(selector)})`;
  const text=selector=>evaluate(`${node(selector)}?.textContent??null`);
  const press=async(find,button='left')=>{
    const r=await evaluate(`(()=>{const n=${find};n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const type of ['mouseMoved','mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...r,button:type==='mouseMoved'?'none':button,buttons:type==='mousePressed'?(button==='right'?2:1):0,clickCount:1});
    await settle();
  };
  const click=selector=>press(node(selector));
  const doubleClick=async find=>{
    const r=await evaluate(`(()=>{const n=${find};n.scrollIntoView({block:'nearest'});const r=n.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    for(const clickCount of [1,2])for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,...r,button:'left',buttons:type==='mousePressed'?1:0,clickCount});
    await settle();
  };
  const key=async(key,code=key,keyCode=0)=>{for(const type of ['keyDown','keyUp'])await call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:keyCode});await settle();};
  const type=async(selector,value)=>{
    await evaluate(`(()=>{const n=${node(selector)};n.focus();n.select();})()`);
    await call('Input.insertText',{text:value});await settle();
  };
  const choose=async(selector,label)=>{
    await evaluate(dropdownChoice(selector,label,'label'));
    await settle();
  };
  const menuItem=label=>`[...document.querySelectorAll('${dialog} .new-drawing-menu:popover-open button')].find(n=>n.firstChild.textContent===${JSON.stringify(label)})`;
  const capture=async name=>{
    await settle();await pause(320);
    await writeFile(`${directory}/${name}.png`,Buffer.from((await call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  };
  const open=async()=>{
    await wait("!layerApp.documents.busy()&&layerApp.state().commands.find(c=>c.id==='new_document').enabled");
    await evaluate(`layerApp.dispatch({type:'invoke',command:'new_document'})`);
    await wait(`!!document.querySelector('${dialog}')&&!!layerApp.state().new_drawing`);await settle();
  };
  const slid=async id=>{await wait(`document.querySelector('#${id}').classList.contains('open')&&!document.querySelector('#${id}').hidden`);await pause(300);};
  const shut=id=>wait(`document.querySelector('#${id}').hidden`);
  await wait('window.layerApp && layerApp.startupTimes.complete!==null');
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy');
  const startup=await evaluate(`(()=>{const s=layerApp.state(),height=Math.round(screen.height*(devicePixelRatio||1));return{width:s.tabs[0].width,height:s.tabs[0].height,expected:[Math.round(height*4/3),height],diameter:s.brush.diameter,rgba:s.colors.foreground.rgba};})()`);
  assert.deepEqual([startup.width,startup.height],startup.expected,'The startup sheet uses the display height at 4:3');
  assert.equal(startup.diameter,9,'A fresh user starts with Pencil at 9 px');
  for(const channel of startup.rgba.slice(0,3))assert.ok(Math.abs(channel-46/255)<.000001,'The startup drawing color is 30% lightness');
  for(const theme of ['light','dark']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:'${theme}'})`);await settle();
    if(theme==='light')await capture('startup-light');
    const tabs=await evaluate('layerApp.app.document_tabs(0).tabs.length');
    await open();
    const opened=await view();
    assert.equal(opened.untitled,await evaluate('layerApp.app.document_tabs(0).upcoming_untitled'),'The next untitled name opens the dialog');
    assert.equal(await evaluate(`${node('#new-drawing-name')}.placeholder`),opened.untitled,'Name shows the next untitled name');
    assert.equal(await text('#new-drawing-caption'),`${opened.page.preset} · ${opened.page.orientation_label}`,'The preview caption names the preset and orientation');
    assert.equal(await text('#new-drawing-capacity'),opened.page.capacity);
    assert.equal(await evaluate(`${node('#new-drawing-capacity')}.title`),opened.page.memory,'The capacity line explains the layer memory');
    assert.equal(await text('#new-drawing-size'),opened.page.size);
    const layout=await evaluate(`(()=>{const box=s=>document.querySelector(s).getBoundingClientRect(),w=box('#new-drawing-width'),h=box('#new-drawing-height'),p=box('#new-drawing-preview'),o=box('#new-drawing-orientation'),f=box('.new-drawing-form');
      return{equal:Math.abs(w.width-h.width)<.5,height:[w.height,h.height,box('#new-drawing-name').height,box('#new-drawing-unit').height,box('#new-drawing-resolution-field').height],preview:[Math.round(p.width),Math.round(p.height)],right:Math.round(f.right-o.right)}})()`);
    assert.deepEqual(layout,{equal:true,height:[34,34,34,34,34],preview:[232,250],right:0},`Main page geometry ${JSON.stringify(layout)}`);
    await capture(`${theme}-1-main`);
    await captureDropdowns({call,evaluate,settle},dialog,directory,`${theme}-main`);
    const nextUnit=opened.units.findIndex(unit=>unit.value!==opened.setup.unit);
    await click('#new-drawing-unit');
    assert.equal(await evaluate(`document.querySelector('.panel-context-menu:popover-open').closest('dialog')===document.querySelector('${dialog}')`),true,'The menu belongs to the active modal');
    await key('Escape','Escape',27);await clickDropdownChoice({call,evaluate,settle},'#new-drawing-unit',nextUnit);
    assert.equal((await view()).setup.unit,opened.units[nextUnit].value,'A real Chrome row click changes the modal value');
    assert.equal(await evaluate(`document.querySelector('.panel-context-menu').parentElement.id`),'workspace','Selecting returns the menu to the workspace');
    await choose('#new-drawing-unit',opened.units.find(unit=>unit.value===opened.setup.unit).label);
    await click('#new-drawing-unit');await key('Escape','Escape',27);
    assert.equal(await evaluate(`!!document.querySelector('${dialog}')&&document.activeElement.id==='new-drawing-unit'&&!document.querySelector('.panel-context-menu:popover-open')`),true,'Escape closes only the menu and focuses its trigger');

    await click('#new-drawing-resolution-choices');await wait(`!!document.querySelector('${dialog} .new-drawing-menu:popover-open')`);
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('${dialog} .new-drawing-menu button')].map(n=>[n.firstChild.textContent,n.lastChild.textContent,n.classList.contains('selected')])`),
      opened.resolutions.map(r=>[r.label,r.purpose,r.selected]),'Common resolutions list their purposes');
    await capture(`${theme}-1b-resolution-menu`);
    await press(menuItem(opened.resolutions.find(r=>r.value===150).label));
    assert.equal((await view()).setup.resolution,150,'A common resolution sets the resolution');
    assert.equal(await evaluate(`${node('#new-drawing-resolution-field input')}.value`),'150');

    await choose('#new-drawing-unit',opened.units.find(u=>u.value==='px').label);
    assert.equal((await view()).setup.unit,'px','The unit dropdown switches to pixels');
    await type('#new-drawing-width input','1200');
    assert.equal((await view()).values[0],1200,'Typed width follows the typing');
    await key('Enter','Enter',13);
    await click('#new-drawing-link');assert.equal((await view()).linked,true,'The link toggle links width and height');
    assert.equal(await evaluate(`${node('#new-drawing-link')}.getAttribute('aria-pressed')`),'true');
    const ratio=(await view()).values[1]/(await view()).values[0];
    await type('#new-drawing-width input','600');await key('Enter','Enter',13);
    assert.equal(Math.round((await view()).values[1]),Math.round(600*ratio),'Linked sizes keep their proportions');
    await click('#new-drawing-link');
    await type('#new-drawing-height input','400');await key('Enter','Enter',13);
    assert.deepEqual((await view()).values,[600,400],'Unlinked height changes alone');
    await click('#new-drawing-portrait');
    assert.deepEqual([(await view()).values,(await view()).page.orientation],[[400,600],'portrait'],'Portrait turns the sheet');
    await type('#new-drawing-resolution-field input','ppi');await key('Enter','Enter',13);
    assert.equal(await evaluate(`${node('#new-drawing-resolution-field .number-control')}.classList.contains('error')`),true,'Malformed resolution text is refused in place');
    await key('Escape','Escape',27);
    assert.deepEqual([!!await evaluate(`document.querySelector('${dialog}')`),await evaluate(`${node('#new-drawing-resolution-field input')}.value`)],[true,'150'],'Escape in a number restores it and keeps the dialog');
    await choose('#new-drawing-depth',opened.depths.find(d=>d.value==='F16').label);
    assert.equal(await evaluate(`${node('#new-drawing-blending')}.disabled`),true,'Float depths fix the blending');
    assert.equal(await evaluate(`${node('#new-drawing-blending')}.closest('label').title`),(await view()).blending_note);
    await choose('#new-drawing-depth',opened.depths.find(d=>d.value==='U8').label);
    await choose('#new-drawing-space',opened.spaces.find(s=>s.value==='DisplayP3').label);
    assert.equal((await view()).setup.color.space,'DisplayP3');

    await click('#new-drawing-presets-button');await slid('new-drawing-browser');
    assert.equal(await evaluate('document.activeElement.id'),'new-drawing-search','Presets focus the search');
    assert.equal(await evaluate(`document.querySelector('.new-drawing-main').inert`),true,'Drawers disable the page');
    await capture(`${theme}-2-presets`);
    let browser=await drawer();
    assert.equal(browser.kind,'presets');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('#new-drawing-tabs button')].map(n=>[n.textContent,n.getAttribute('aria-selected')==='true'])`),browser.tabs.map(t=>[t.label,t.selected]));
    await call('Input.insertText',{text:'chalk'});await settle();
    browser=await drawer();
    assert.equal(browser.query,'chalk','Search filters the presets');
    assert.equal(await evaluate(`[...document.querySelectorAll('#new-drawing-tabs button')].every(n=>n.disabled)`),true,'Tabs wait while searching');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('#new-drawing-presets .new-drawing-tile-name')].map(n=>n.textContent)`),browser.tiles.map(t=>t.name));
    await click('#new-drawing-save-as');
    assert.ok((await drawer()).save,'Save As opens naming over the filtered presets');
    await key('Escape','Escape',27);
    assert.equal((await drawer()).save??null,null,'Escape leaves naming first');
    assert.equal((await drawer()).query,'chalk','Leaving naming retains the preset search');
    await key('Escape','Escape',27);
    browser=await drawer();
    assert.equal(browser.kind,'presets','Clearing search keeps the browser open');
    assert.equal(browser.query,'','Escape clears the search before closing its drawer');
    assert.equal(await evaluate("document.querySelector('#new-drawing-search').value"),'','The search field follows the shared Back result');
    assert.equal(browser.tabs_enabled,true,'Clearing search re-enables preset tabs');
    await key('Escape','Escape',27);await shut('new-drawing-browser');
    assert.ok(await view(),'Closing the browser keeps the New drawing page');
    await click('#new-drawing-presets-button');await slid('new-drawing-browser');
    const pastel=(await drawer()).tabs.findIndex(t=>t.label==='Pastel');
    await click(`#new-drawing-tabs [data-new-drawing-tab="${pastel}"]`);
    browser=await drawer();assert.equal(browser.tabs[pastel].selected,true,'A tab shows its presets');
    const chalkboard=browser.tiles.findIndex(t=>t.name==='Chalkboard');
    await click(`#new-drawing-presets [data-new-drawing-preset="${chalkboard}"]`);
    browser=await drawer();
    assert.equal(browser.tiles[chalkboard].selected,true,'Clicking a preset picks it');
    assert.equal(await text('#new-drawing-summary-name'),'Chalkboard','The footer summarizes the pick');
    await capture(`${theme}-3-presets-pastel`);
    await click('#new-drawing-save-as');
    assert.equal(await evaluate('document.activeElement.id'),'new-drawing-save-name','Save As names the preset');
    await call('Input.insertText',{text:'Studio wall'});await settle();
    assert.equal((await drawer()).save.name,'Studio wall');
    await capture(`${theme}-4-save-as`);
    await click('#new-drawing-save-confirm');
    browser=await drawer();
    const own=browser.tiles.findIndex(t=>t.own&&t.name==='Studio wall');
    assert.ok(own>=0&&browser.tiles[own].selected&&browser.tabs.find(t=>t.selected).label==='Your presets','The saved preset is picked under Your presets');
    await press(node(`#new-drawing-presets [data-new-drawing-preset="${own}"]`),'right');
    await wait(`!!document.querySelector('${dialog} .new-drawing-menu:popover-open')`);
    await press(menuItem((await view()).copy.rename));
    assert.equal((await drawer()).save?.name,'Studio wall','Rename starts naming the preset');
    await type('#new-drawing-save-name','Studio chalk');
    await choose('#new-drawing-save-group','Pastel');
    assert.equal(await text('#new-drawing-save-confirm'),(await drawer()).save.confirm);
    await click('#new-drawing-save-confirm');
    browser=await drawer();
    const renamed=browser.tiles.findIndex(t=>t.own&&t.name==='Studio chalk');
    assert.ok(renamed>=0&&browser.tabs.find(t=>t.selected).label==='Pastel','Renaming moves the preset to its category');
    await press(node(`#new-drawing-presets [data-new-drawing-preset="${renamed}"]`),'right');
    await wait(`!!document.querySelector('${dialog} .new-drawing-menu:popover-open')`);
    await press(menuItem((await view()).copy.delete));
    assert.equal((await evaluate('(layerApp.state().settings.new_document.setups??[]).length')),0,'Delete removes the preset');
    browser=await drawer();
    const sketch=browser.tabs.findIndex(t=>t.label==='Sketch');
    await click(`#new-drawing-tabs [data-new-drawing-tab="${sketch}"]`);
    const kraft=(await drawer()).tiles.findIndex(t=>t.name==='Kraft paper');
    await doubleClick(node(`#new-drawing-presets [data-new-drawing-preset="${kraft}"]`));
    await shut('new-drawing-browser');
    assert.equal((await view()).page.preset,'Kraft paper','Double-clicking a preset selects it');
    assert.equal(await evaluate('document.activeElement.id'),'new-drawing-presets-button','Closing Presets returns to its button');

    await click('#new-drawing-preview');await slid('new-drawing-browser');
    await click('#new-drawing-select');await shut('new-drawing-browser');
    assert.equal((await view()).page.preset,'Kraft paper','The preview opens Presets and Select keeps the pick');

    await click('#new-drawing-presets-button');await slid('new-drawing-browser');
    await click(`#new-drawing-tabs [data-new-drawing-tab="${(await drawer()).tabs.findIndex(t=>t.label==='Pastel')}"]`);
    await doubleClick(node(`#new-drawing-presets [data-new-drawing-preset="${(await drawer()).tiles.findIndex(t=>t.name==='Chalkboard')}"]`));
    await shut('new-drawing-browser');
    const board=await view();
    assert.deepEqual([board.page.preset,board.setup.paper.relief,board.page.relief],['Chalkboard',50,'50 %'],'Chalkboard starts at 50% texture visibility');
    const textureTile=()=>evaluate(`(()=>{const box=s=>document.querySelector(s).getBoundingClientRect(),line=document.querySelector('#new-drawing-texture-relief'),name=box('#new-drawing-texture .new-drawing-tile-label'),r=box('#new-drawing-texture-relief'),t=box('#new-drawing-texture');
      return{text:line.hidden?null:line.textContent,below:line.hidden||r.top>=name.bottom-.5&&r.bottom<=t.bottom,height:Math.round(t.height)}})()`);
    const tile=await textureTile();
    assert.deepEqual(tile,{text:'50 %',below:true,height:44},`The texture tile shows its visibility below the name and keeps its height ${JSON.stringify(tile)}`);
    await press(node('#new-drawing-title'));
    await capture(`${theme}-5-chalkboard`);

    await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
    let paper=await drawer();
    assert.equal(paper.tiles[0].id??null,null,'No paper comes first');
    assert.equal(await evaluate(`!!document.querySelector('#new-drawing-papers [data-new-drawing-paper="0"] svg')`),true,'No paper shows its icon');
    const reliefField='#new-drawing-paper-relief-field input';
    const paperDrawer=await evaluate(`(()=>{const grid=document.querySelector('#new-drawing-papers'),tile=grid.lastElementChild,field=document.querySelector('#new-drawing-paper-relief-field'),box=n=>Math.round(n.getBoundingClientRect().height);
      return{last:tile.id,caption:tile.querySelector('.texture-caption').textContent,icon:!!tile.querySelector('.paper-preview svg'),checked:tile.getAttribute('aria-checked'),footerImport:!!document.querySelector('#new-drawing-paper-drawer footer #new-drawing-paper-import'),
        label:document.querySelector('#new-drawing-paper-relief .new-drawing-relief-label').textContent,value:field.querySelector('input').value,unit:field.querySelector('.new-drawing-affix').textContent,heights:[box(field),box(document.querySelector('#new-drawing-paper-use'))],disabled:field.querySelector('input').disabled}})()`);
    assert.deepEqual(paperDrawer,{last:'new-drawing-paper-import',caption:board.copy.import,icon:true,checked:null,footerImport:false,label:board.copy.relief,value:'50',unit:board.copy.relief_unit,heights:[34,34],disabled:false},
      `Import… is the last tile and Texture visibility shows the sheet's value ${JSON.stringify(paperDrawer)}`);
    assert.deepEqual([paper.relief.value,paper.relief.enabled],[50,true]);
    await capture(`${theme}-6-paper`);
    const stubPicker=()=>evaluate(`(()=>{window.paperPickers??=0;const click=HTMLInputElement.prototype.click;HTMLInputElement.prototype.click=function(){
      if(this.type!=='file'||this.accept!=='image/*')return click.call(this);
      HTMLInputElement.prototype.click=click;window.paperPickers++;this.dispatchEvent(new Event('cancel'));};return window.paperPickers;})()`);
    const marked=paper.tiles.findIndex(t=>t.selected);
    const markedTiles=()=>evaluate(`[...document.querySelectorAll('#new-drawing-papers [aria-checked=true]')].map(n=>n.dataset.newDrawingPaper??n.id)`);
    let pickers=await stubPicker();
    await click('#new-drawing-paper-import');
    await wait(`window.paperPickers===${pickers+1}`);await settle();
    assert.deepEqual([(await drawer()).tiles.findIndex(t=>t.selected),await markedTiles()],[marked,[String(marked)]],'A click on Import… opens the image picker and keeps the marked paper');
    await evaluate(`${node(`#new-drawing-papers [data-new-drawing-paper="${marked}"]`)}.focus()`);
    await key('End','End',35);
    assert.equal(await evaluate('document.activeElement.id'),'new-drawing-paper-import','End reaches Import…');
    pickers=await stubPicker();
    await key('Enter','Enter',13);
    await wait(`window.paperPickers===${pickers+1}`);await settle();
    assert.deepEqual([(await drawer()).tiles.findIndex(t=>t.selected),await markedTiles()],[marked,[String(marked)]],'Enter on Import… opens the image picker and keeps the marked paper');

    await click('#new-drawing-papers [data-new-drawing-paper="0"]');
    assert.equal((await drawer()).relief.enabled,false);
    assert.deepEqual(await evaluate(`(()=>{const row=document.querySelector('#new-drawing-paper-relief');return[row.classList.contains('disabled'),row.querySelector('input').disabled,getComputedStyle(row).opacity]})()`),[true,true,'0.5'],'No paper disables Texture visibility');
    await capture(`${theme}-6b-no-paper`);
    await evaluate(`(()=>{const body=document.querySelector('#new-drawing-paper-drawer .new-drawing-drawer-body');body.scrollTop=body.scrollHeight;})()`);
    await capture(`${theme}-6c-import`);
    const cold=paper.tiles.findIndex(t=>t.id==='cold_press');
    await click(`#new-drawing-papers [data-new-drawing-paper="${cold}"]`);
    assert.equal(await evaluate(`${node(`#new-drawing-papers [data-new-drawing-paper="${cold}"]`)}.getAttribute('aria-checked')`),'true','Clicking a paper picks it');
    assert.equal(await evaluate(`${node(reliefField)}.disabled`),false,'A paper enables Texture visibility');
    await type(reliefField,'150');
    await click('#new-drawing-paper-use');await shut('new-drawing-paper-drawer');
    let sheet=await view();
    assert.deepEqual([sheet.setup.paper.texture,sheet.setup.paper.relief,sheet.page.relief],['cold_press',150,'150 %'],'Use Texture applies the paper and its typed visibility');
    assert.equal(await text('#new-drawing-texture .new-drawing-tile-label'),sheet.page.texture);
    assert.equal((await textureTile()).text,'150 %');

    await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
    assert.equal(await evaluate(`${node(reliefField)}.value`),'150','The drawer opens at the sheet visibility');
    await type(reliefField,'250');
    await click('#new-drawing-paper-cancel');await shut('new-drawing-paper-drawer');
    assert.equal((await view()).setup.paper.relief,150,'Cancel discards the visibility');
    await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
    assert.equal(await evaluate(`${node(reliefField)}.value`),'150','Reopening shows the kept visibility');
    await evaluate(`(()=>{const entry=${node(reliefField)};entry.focus();entry.value='120';document.querySelector('#new-drawing-paper-use').click();})()`);
    await shut('new-drawing-paper-drawer');
    sheet=await view();
    assert.deepEqual([sheet.setup.paper.relief,sheet.page.relief],[120,'120 %'],'Use Texture commits text still being typed');
    if(theme==='light') {
      await evaluate(`(async()=>{
        const canvas=new OffscreenCanvas(64,48),context=canvas.getContext('2d');
        for(let y=0;y<48;y++)for(let x=0;x<64;x++){const v=(x*7+y*3)%256;context.fillStyle='rgb('+v+','+v+','+v+')';context.fillRect(x,y,1,1);}
        const file=new File([await canvas.convertToBlob({type:'image/png'})],'Linen scan.png',{type:'image/png'});
        const click=HTMLInputElement.prototype.click;
        HTMLInputElement.prototype.click=function(){
          if(this.type!=='file'||this.accept!=='image/*')return click.call(this);
          HTMLInputElement.prototype.click=click;
          const transfer=new DataTransfer();transfer.items.add(file);this.files=transfer.files;this.dispatchEvent(new Event('change'));
        };
      })()`);
      await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
      await click('#new-drawing-paper-import');
      await wait(`layerApp.state().new_drawing?.drawer?.tiles?.some(t=>t.label==='Linen scan'&&t.selected)`);
      assert.equal(await evaluate(`document.querySelector('#new-drawing-papers [aria-checked=true] .texture-caption').textContent`),'Linen scan','Import… adds the paper and picks it');
      await key('Escape','Escape',27);await shut('new-drawing-paper-drawer');
      assert.equal((await view()).setup.paper.texture,'cold_press','Escape leaves the paper unchanged');
    }

    await click('#new-drawing-paper-color');await slid('new-drawing-color-drawer');
    await wait(`document.querySelector('#new-drawing-color-drawer .color-wheel').width>0`);
    let colors=await drawer();
    assert.equal(colors.kind,'paper_color');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('#new-drawing-color-drawer .color-copy, #new-drawing-color-drawer .color-pick:not([hidden]), #new-drawing-color-drawer .color-current:not([hidden])')].length`),0,'Paper color hides copy, the eyedropper and the current patch');
    assert.equal(await text('#new-drawing-color-drawer .color-editor-title'),(await view()).copy.paper_color);
    assert.equal(await text('#new-drawing-color-drawer .new-drawing-palette-head span:last-child'),colors.count,'Paper colors shows its count');
    const kraftColor=colors.palette.findIndex(c=>c.name==='Kraft');
    await click(`#new-drawing-paper-color-tiles [data-new-drawing-paper-color="${kraftColor}"]`);
    colors=await drawer();
    assert.equal(colors.palette[kraftColor].selected,true,'A paper color tile sets the color');
    assert.equal(await evaluate(`${node(`#new-drawing-paper-color-tiles [data-new-drawing-paper-color="${kraftColor}"]`)}.classList.contains('selected')`),true);
    await capture(`${theme}-7-paper-color`);
    await click('#new-drawing-color-drawer .color-hex .color-value');
    await call('Input.insertText',{text:'#E07080'});await key('Enter','Enter',13);
    colors=await drawer();
    assert.equal(colors.can_add,true,'A new color can join the paper colors');
    const count=colors.palette.length;
    await click('#new-drawing-add-paper-color');
    assert.equal((await drawer()).palette.length,count+1,'+ adds the color');
    await press(node(`#new-drawing-paper-color-tiles [data-new-drawing-paper-color="${count}"]`),'right');
    await wait(`!!document.querySelector('${dialog} .new-drawing-menu:popover-open')`);
    await press(menuItem((await view()).copy.delete));
    assert.equal((await drawer()).palette.length,count,'Delete removes the paper color');
    await click('#new-drawing-color-mode-transparent');
    colors=await drawer();
    assert.equal(colors.transparent,true,'Transparent clears the sheet');
    assert.deepEqual(await evaluate(`(()=>{const d=document.querySelector('#new-drawing-color-drawer');return[d.querySelector('.color-hex .color-value').textContent,d.querySelector('.color-new').classList.contains('new-drawing-clear'),getComputedStyle(d.querySelector('.color-editor-left')).opacity]})()`),
      [(await view()).copy.transparent,true,'0.4'],'Transparent dims the editor and clears the New patch');
    await capture(`${theme}-8-transparent`);
    await click('#new-drawing-color-use');await shut('new-drawing-color-drawer');
    assert.equal((await view()).page.paper_color,(await view()).copy.transparent,'Use Color keeps the transparent sheet');
    assert.equal(await evaluate('document.activeElement.id'),'new-drawing-paper-color');

    await click('#new-drawing-unit');await evaluate(`document.querySelector('#new-drawing-cancel').click()`);await settle();
    await wait(`!document.querySelector('${dialog}')&&!layerApp.state().new_drawing&&!layerApp.state().requests.some(r=>r.kind.type==='document')`);
    assert.equal(await evaluate(`document.querySelector('.panel-context-menu').parentElement.id==='workspace'&&!document.querySelector('.panel-context-menu:popover-open')`),true,'Closing a dialog with a menu open returns it to the workspace');
    assert.equal(await evaluate('layerApp.app.document_tabs(0).tabs.length'),tabs,'Cancel creates nothing');

    await open();
    await key('Escape','Escape',27);
    await wait(`!document.querySelector('${dialog}')&&!layerApp.state().requests.some(r=>r.kind.type==='document')`);
    assert.equal(await evaluate('layerApp.app.document_tabs(0).tabs.length'),tabs,'Escape on the page cancels');

    await open();
    const name=`Harbor study ${theme}`;
    await type('#new-drawing-name',name);
    assert.equal((await view()).name,name);
    await choose('#new-drawing-unit',(await view()).units.find(u=>u.value==='px').label);
    await type('#new-drawing-width input','640');
    await type('#new-drawing-height input','480');
    await click('#new-drawing-create');
    await wait(`!document.querySelector('${dialog}')&&!layerApp.documents.busy()&&layerApp.app.document_tabs(0).tabs.length===${tabs+1}`,60000);
    const created=await evaluate('(t=>({title:t.tabs.find(tab=>String(tab.id)===String(t.selected)).title,size:[layerApp.state().tabs[0].width,layerApp.state().tabs[0].height]}))(layerApp.app.document_tabs(0))');
    assert.deepEqual(created,{title:name,size:[640,480]},'Create opens a named drawing at the typed size');
  }
  await evaluate('layerApp.documents.autosave()');
  const reloadedFrom=await evaluate('performance.timeOrigin');
  await call('Page.reload',{ignoreCache:true});
  for(;;)try{await wait(`performance.timeOrigin!==${reloadedFrom}&&window.layerApp?.startupTimes.complete!=null`,60000);break;}
    catch(error){if(!/navigated|context|Cannot find/i.test(String(error)))throw error;}
  await wait('JSON.parse(layerApp.app.workspace_view())?.ready&&!JSON.parse(layerApp.app.workspace_view()).busy&&!layerApp.documents.busy()',60000);
  await open();
  await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
  await wait(`layerApp.state().new_drawing.drawer.tiles.some(t=>t.label==='Linen scan')`);
  const stored=(await drawer()).tiles.findIndex(t=>t.label==='Linen scan');
  await click(`#new-drawing-papers [data-new-drawing-paper="${stored}"]`);await click('#new-drawing-paper-use');await shut('new-drawing-paper-drawer');
  const before=await evaluate('layerApp.app.document_tabs(0).tabs.length');
  await evaluate(createDrawing({width:320,height:240,paper:null}));
  await wait(`layerApp.app.document_tabs(0).tabs.length===${before+1}&&!layerApp.documents.busy()&&layerApp.app.brush_ready()`,60000);
  await evaluate(`layerApp.dispatch({type:'select_layer',id:layerApp.state().layers.find(l=>l.label==='Paper').id})`);await settle();
  const texture="layerApp.state().layer_properties.controls.find(c=>c.key==='texture')";
  await wait(`!!${texture}?.paper`);
  assert.equal(await evaluate(`${texture}.paper.tiles[${texture}.value.value].label`),'Linen scan','A drawing on a stored library paper loads that paper');
  const width=await evaluate('innerWidth'),height=await evaluate('innerHeight');
  await call('Emulation.setDeviceMetricsOverride',{width:640,height:860,deviceScaleFactor:1,mobile:false});await settle();
  try {
    await open();
    const narrow=await evaluate(`(()=>{const p=document.querySelector('#new-drawing-preview').getBoundingClientRect(),f=document.querySelector('.new-drawing-form').getBoundingClientRect();return{stacked:p.bottom<=f.top,width:Math.round(p.width)}})()`);
    assert.deepEqual(narrow,{stacked:true,width:348},'Narrow windows stack the preview above the form');
    await capture('narrow');
    await click('#new-drawing-texture');await slid('new-drawing-paper-drawer');
    const footer=()=>evaluate(`(()=>{const f=document.querySelector('#new-drawing-paper-drawer footer'),label=f.querySelector('.new-drawing-relief-label'),row=document.querySelector('#new-drawing-paper-relief').getBoundingClientRect(),cancel=document.querySelector('#new-drawing-paper-cancel').getBoundingClientRect();
      return{fits:f.scrollWidth<=f.clientWidth,clear:row.right<=cancel.left,ellipsized:label.scrollWidth>label.clientWidth,buttons:[...f.querySelectorAll(':scope > button')].map(n=>Math.round(n.getBoundingClientRect().height))}})()`);
    assert.deepEqual(await footer(),{fits:true,clear:true,ellipsized:false,buttons:[34,34]},'The narrow paper footer keeps Texture visibility clear of the buttons');
    await capture('narrow-paper');
    await call('Emulation.setDeviceMetricsOverride',{width:420,height:860,deviceScaleFactor:1,mobile:false});await settle();
    assert.deepEqual(await footer(),{fits:true,clear:true,ellipsized:true,buttons:[34,34]},'Texture visibility ellipsizes before the footer buttons shrink');
    await capture('narrower-paper');
    await click('#new-drawing-paper-cancel');await shut('new-drawing-paper-drawer');
    await click('#new-drawing-cancel');await wait(`!document.querySelector('${dialog}')`);
  } finally {
    await call('Emulation.setDeviceMetricsOverride',{width,height,deviceScaleFactor:1,mobile:false});await settle();
  }
  console.log('PASS: New drawing opens with the next untitled name, edits size, unit, link, orientation and resolution, browses, searches, saves, renames and deletes presets, picks and imports paper from the last tile, sets, cancels and commits texture visibility, creates on a stored library paper, edits, adds, deletes and clears paper colors, cancels and creates a named drawing in both themes');
}
