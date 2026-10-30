import assert from 'node:assert/strict';

export function glassCss(color) {
  const alpha=Math.round(color[3]*255);
  return `rgb(${color.slice(0,3).map(v=>alpha?Math.round(v*alpha)*255/alpha:0).join(' ')} / ${alpha/255})`;
}

export async function checkTextEntry(evaluate,selector,{height=34,padding=9,reference='.panel select',capture}={}) {
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
