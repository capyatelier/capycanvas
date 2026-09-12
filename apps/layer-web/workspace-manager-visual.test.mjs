import assert from "node:assert/strict";
import {mkdir,writeFile} from "node:fs/promises";

// Match native_workspace_manager_visual: fresh storage, three defaults, scale 1.
export async function checkWorkspaceManagerVisual({call,evaluate,settle}) {
  const output=process.env.LAYER_TEST_ARTIFACTS || '/tmp/capy-workspace-manager-visual/web';
  await mkdir(output,{recursive:true});
  const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
  const row='.workspace-row[data-id="builtin:workspace:painter"]';
  const move=async selector=>{
    const point=selector?await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`):{x:10,y:10};
    await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});await pause(200);await settle();
  };
  for(const theme of ['dark','light']) {
    await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}});layerApp.app.workspace_input(JSON.stringify({type:'open',page:'workspaces'}));null`);
    await pause(700);await settle();
    assert.equal(await evaluate('document.querySelector(".workspace-manager").open'),true);
    const samples={};
    for(const [state,selector] of [['normal',null],['row',`${row} .workspace-choice`],['grip',`${row} .workspace-grip`],['pin',`${row} .workspace-pin`],['options',`${row} .workspace-options`],['current','.workspace-row[data-id="builtin:workspace:illustrator"] .workspace-choice'],['new','.workspace-add'],['cancel','.workspace-manager footer button'],['disabled','.workspace-manager .suggested-action']]) {
      await move(selector);
      const shot=await call('Page.captureScreenshot',{format:'png'});
      await writeFile(`${output}/web-${theme}-${state}.png`,Buffer.from(shot.data,'base64'));
      const geometry=await evaluate(`(()=>{function record(e){const r=e.getBoundingClientRect(),s=getComputedStyle(e);return {tag:e.tagName,css:e.className,text:e.children.length?null:e.textContent,bounds:[r.x,r.y,r.width,r.height],hover:e.matches(':hover'),background:s.backgroundColor,color:s.color,font:s.font,opacity:s.opacity,children:[...e.children].map(record)};}return record(document.querySelector('.workspace-manager'));})()`);
      await writeFile(`${output}/web-${theme}-${state}.json`,JSON.stringify(geometry,null,2));
      samples[state]=await evaluate(`(()=>{const row=document.querySelector(${JSON.stringify(row)}),background=e=>getComputedStyle(e).backgroundColor;return {row:background(row),choice:background(row.querySelector('.workspace-choice')),grip:background(row.querySelector('.workspace-grip')),options:background(row.querySelector('.workspace-options')),primary:background(document.querySelector('.workspace-manager .suggested-action'))};})()`);
    }
    const normal=samples.normal;
    assert.notEqual(samples.row.row,normal.row,'hover highlights the whole row');
    for(const state of ['grip','pin','options']) assert.equal(samples[state].row,samples.row.row,`${state} shares the row hover surface`);
    for(const sample of Object.values(samples)) {
      assert.equal(sample.choice,'rgba(0, 0, 0, 0)','row text never gets a separate button background');
      assert.equal(sample.grip,'rgba(0, 0, 0, 0)','the handle never gets a button background');
      assert.equal(sample.primary,normal.primary,'hovering disabled confirmation preserves its color');
    }
    assert.equal(samples.row.options,normal.options);
    assert.notEqual(samples.options.options,normal.options,'options has its own button hover');
    const measured=await evaluate(`(()=>{const rect=s=>{const r=document.querySelector(s).getBoundingClientRect();return [r.x,r.y,r.width,r.height]};return {dialog:rect('.workspace-manager'),intro:rect('.workspace-intro'),list:rect('.workspace-list'),row:rect('.workspace-row'),footer:rect('.workspace-manager footer'),add:rect('.workspace-add'),focus:document.activeElement.className};})()`);
    assert.deepEqual(measured,{dialog:[490,250,460,500],intro:[508,314,424,36],list:[508,372,424,314],row:[508,372,424,56],footer:[508,698,424,34],add:[870,256,34,34],focus:'workspace-manager'},'GTK allocations and initial dialog focus');
    // Keyboard navigation still exposes focus, including the whole selected row.
    await call('Input.dispatchKeyEvent',{type:'keyDown',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
    await call('Input.dispatchKeyEvent',{type:'keyUp',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});
    assert.equal(await evaluate('document.activeElement.matches(".workspace-add:focus-visible")'),true);
    await evaluate(`document.querySelector(${JSON.stringify(`${row} .workspace-choice`)}).focus()`);
    assert.equal(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(row)})).outlineStyle`),'solid');
    // Touch mode must not retain a mouse hover while the list scrolls or drags.
    await move(`${row} .workspace-grip`);await evaluate('document.documentElement.dataset.touch=""');await settle();
    assert.equal(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(row)})).backgroundColor`),normal.row);
    await evaluate('delete document.documentElement.dataset.touch');
    // The pin and whitespace belong to the same previewable row surface.
    await move(`${row} .workspace-pin`);
    const point=await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(`${row} .workspace-pin`)}).getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await call('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',buttons:1,clickCount:1});
    await call('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',buttons:0,clickCount:1});
    await pause(500);await settle();
    assert.deepEqual(await evaluate('(()=>{const v=JSON.parse(layerApp.app.workspace_view());return [v.id,v.selected,v.enabled]})()'),['builtin:workspace:illustrator','builtin:workspace:painter',true],'clicking a status icon selects a preview without switching');
    await move('.workspace-manager .suggested-action');
    const background=await evaluate(`getComputedStyle(document.querySelector('.workspace-manager .suggested-action')).backgroundColor`);
    const color=background.match(/[\d.]+/g).map(Number);
    assert.ok(color[2]>color[1] && color[1]>color[0] && (color[3]??1)===1,`enabled confirmation remains blue on hover: ${background}`);
    await evaluate(`document.querySelector('.workspace-manager footer button').click()`);await pause(300);
  }
  console.log(`PASS: GTK manager geometry, row/button hover, disabled styling and keyboard focus in both themes. Captures: ${output}`);
}
