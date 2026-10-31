import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkSearchField({call,evaluate,settle},selector,name) {
  const settings=await evaluate('layerApp.state().settings'),directory=process.env.LAYER_TEST_ARTIFACTS??'artifacts/ui/search-fields';
  const value=await evaluate(`document.querySelector(${JSON.stringify(selector)}).value`);assert.ok(value,'Search field contains text');
  const restore=theme=>evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:${JSON.stringify(JSON.stringify({...settings,theme}))}})`);
  await mkdir(directory,{recursive:true});
  try {
    for(const theme of ['light','dark']) {
      await restore(theme);await settle();
      await evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`);
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).value`),value);
      const point=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n.getBoundingClientRect(),s=getComputedStyle(n),rule=[...document.styleSheets].flatMap(sheet=>[...sheet.cssRules]).find(rule=>rule.selectorText==='input[type="search"]::-webkit-search-cancel-button')?.style;return{x:r.right-parseFloat(s.paddingRight)-parseFloat(s.getPropertyValue('--panel-icon-size'))/2,y:r.y+r.height/2,appearance:rule?.appearance,mask:rule?.maskImage,color:rule?.backgroundColor,size:[rule?.width,rule?.height],focused:document.activeElement===n,theme:document.body.dataset.theme}})()`);
      assert.equal(point.focused,true);assert.equal(point.theme,theme);assert.equal(point.appearance,'none');assert.ok(point.mask.includes('layer-close-symbolic.svg'));assert.equal(point.color,'currentcolor');assert.deepEqual(point.size,['var(--panel-icon-size)','var(--panel-icon-size)']);
      for(const state of ['focused','hovered']) {
        if(state==='hovered')await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:point.x,y:point.y,button:'none'});
        const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}-${theme}-${state}.png`,Buffer.from(data,'base64'));
      }
      for(const type of ['mousePressed','mouseReleased'])await call('Input.dispatchMouseEvent',{type,x:point.x,y:point.y,button:'left',buttons:type==='mousePressed'?1:0,clickCount:1});await settle();
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).value`),'','The search clear control keeps its native input action');
      await call('Input.insertText',{text:value});await settle();
      assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).value`),value);
    }
  } finally {await restore(settings.theme);await settle();}
}

export async function checkCheckboxes(evaluate,selector) {
  const boxes=await evaluate(`[...document.querySelector(${JSON.stringify(selector)}).querySelectorAll('input[type="checkbox"]:not(.settings-switch)')].filter(n=>n.checkVisibility()).map(n=>{const s=getComputedStyle(n);return{appearance:s.appearance,size:[s.width,s.height],radius:s.borderTopLeftRadius,checked:n.checked,mask:getComputedStyle(n,'::after').maskImage}})`);
  assert.ok(boxes.length,'The journey checks visible checkboxes');
  for(const box of boxes){assert.equal(box.appearance,'none');assert.deepEqual(box.size,['16px','16px']);assert.equal(box.radius,'4px');if(box.checked)assert.notEqual(box.mask,'none');}
}

export async function captureCheckboxes({call,evaluate,settle},directory,name) {
  const settings=await evaluate('layerApp.state().settings');
  const restore=theme=>evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:${JSON.stringify(JSON.stringify({...settings,theme}))}})`);
  try {for(const theme of ['light','dark']){await restore(theme);await settle();assert.equal(await evaluate('document.body.dataset.theme'),theme);await checkCheckboxes(evaluate,'dialog[open]');const {data}=await call('Page.captureScreenshot',{format:'png'});await writeFile(`${directory}/${name}-${theme}.png`,Buffer.from(data,'base64'));}}
  finally {await restore(settings.theme);await settle();}
}

export function glassCss(color) {
  const alpha=Math.round(color[3]*255);
  return `rgb(${color.slice(0,3).map(v=>alpha?Math.round(v*alpha)*255/alpha:0).join(' ')} / ${alpha/255})`;
}

export async function checkTextEntry(evaluate,selector,{height=34,padding=9,reference='.panel .dropdown:not(.layer-blend):not(#properties-add-filter)',capture}={}) {
  const settings=await evaluate('layerApp.state().settings');
  const restore=theme=>evaluate(`layerApp.dispatch({type:'restore_saved_settings',saved:${JSON.stringify(JSON.stringify({...settings,theme}))}})`);
  try {
    for(const theme of ['light','dark']) {
      await restore(theme);
      await evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
      const entries=await evaluate(`(()=>{
        const ref=getComputedStyle(document.querySelector(${JSON.stringify(reference)})),font=getComputedStyle(document.body);
        return [...document.querySelectorAll(${JSON.stringify(selector)})].map(n=>{
          const s=getComputedStyle(n),r=n.getBoundingClientRect();
          return {visible:n.checkVisibility(),textarea:n.tagName==='TEXTAREA',height:r.height,padding:parseFloat(s.paddingLeft),
            actual:[s.borderTopWidth,s.borderTopLeftRadius,s.cornerShape,s.backgroundColor,s.color,s.fontFamily,s.fontSize],
            expected:['0px',ref.borderTopLeftRadius,ref.cornerShape,ref.backgroundColor,ref.color,font.fontFamily,font.fontSize]};
        });
      })()`);
      assert.ok(entries.length,selector);
      for(const entry of entries) {
        assert.equal(entry.visible,true,`${theme} ${selector} is visible`);
        assert.deepEqual(entry.actual,entry.expected,`${theme} ${selector} uses editor text styling`);
        assert.ok(entry.textarea?entry.height>=height:entry.height===height,`${theme} ${selector} height: ${entry.height}`);
        assert.equal(entry.padding,padding,`${theme} ${selector} padding`);
      }
      await capture?.(theme);
    }
  } finally {await restore(settings.theme);}
}
