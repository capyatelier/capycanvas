import assert from "node:assert/strict";
import {mkdir, writeFile} from "node:fs/promises";

export async function checkTooltips({call,evaluate,settle}) {
  const saved=await evaluate("({workspace:layerApp.state().workspace,settings:layerApp.state().settings})");
  const wait=ms=>evaluate(`new Promise(r=>setTimeout(r,${ms}))`);
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await wait(240);};
  const rect=selector=>evaluate(`document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect().toJSON()`);
  const visible=()=>evaluate("document.querySelector('#hover-tooltip').matches(':popover-open')");
  const move=async(p,device="pen",buttons=0)=>{await call("Input.dispatchMouseEvent",{type:"mouseMoved",...p,pointerType:device,button:buttons?"left":"none",buttons});await settle();};
  const away=async()=>{await move({x:720,y:600});await settle();};
  const hover=async(selector,device="pen")=>{const r=await rect(selector);await move({x:r.x+r.width/2,y:r.y+r.height/2},device);await wait(620);assert.equal(await visible(),true,`${device} ${selector}: tooltip appears`);return r;};
  const dir=process.env.LAYER_TEST_ARTIFACTS||"/tmp/capy-hover-tooltips-web";await mkdir(dir,{recursive:true});
  const viewportWidth=await evaluate("innerWidth");
  const shot=async name=>{const s=await call("Page.captureScreenshot",{format:"png"});await writeFile(`${dir}/${name}.png`,Buffer.from(s.data,"base64"));};
  const fixture=structuredClone(saved.workspace),tabs=(id,panels)=>({kind:"tabs",id,panels,active:panels[0],tab_style:"icon"});
  Object.assign(fixture.layout,{bands:[{id:40,edge:"left",extent:252,root:tabs(41,["brushes","sizes"])},{id:42,edge:"right",extent:252,root:tabs(43,["layers","properties"])},{id:44,edge:"top",extent:36,root:tabs(45,["toolbar"])}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],next_id:Math.max(46,fixture.layout.next_id)});
  fixture.zen_mode=false;
  try {
    for(const theme of ["dark","light"])for(const device of ["mouse","pen"]) {
      await send({type:"restore_workspace",workspace:fixture});await send({type:"set_theme",theme});
      for(const source of ["toolbar","column","drawer"]) {
        if(source==="column")await send({type:"customize",action:{type:"set_column_collapsed",group:43,collapsed:true}});
        if(source==="drawer")await send({type:"customize",action:{type:"toggle_column_drawer",group:43,panel:"layers"}});
        const selector={toolbar:'.toolbar-controls [data-tile] > button:not(:disabled)',column:'.column-tab[data-panel=properties]',drawer:'.content-drawer .layer-footer button[aria-label="New layer"]'}[source];
        await away();const r=await hover(selector,device),t=await rect("#hover-tooltip");
        assert.ok(Math.abs(t.y-r.bottom-4)<1,"Tooltip is below the element with a 4px gap");
        const center=r.x+r.width/2-t.width/2;
        assert.ok(Math.abs(t.x-Math.max(8,Math.min(center,viewportWidth-t.width-8)))<1,"Centered where possible, clamped near side edges");
        const style=await evaluate("(()=>{const s=getComputedStyle(document.querySelector('#hover-tooltip'));return [s.color,s.backgroundColor,s.padding,s.borderRadius,s.fontSize,s.fontWeight,s.pointerEvents]})()");
        assert.deepEqual(style.slice(0,4),["rgb(255, 255, 255)","rgba(0, 0, 6, 0.8)","6px 10px","9px"]);
        assert.equal(style[4],await evaluate("getComputedStyle(document.body).fontSize"));
        assert.deepEqual(style.slice(5),["400","none"]);
        assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).title`),"","Native title cannot overlap the custom tooltip");
        assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).getAttribute('aria-describedby').includes('hover-tooltip')`));
        await shot(`${theme}-${device}-${source}`);await away();assert.equal(await visible(),false);
        assert.ok(await evaluate(`document.querySelector(${JSON.stringify(selector)}).title`),"Title is restored after leaving");
      }
    }
    // Real pen hover over a dynamic layer action preserves its keyboard hint.
    await send({type:"restore_workspace",workspace:fixture});
    const layer='.layer-footer button[aria-label="New layer"]';await hover(layer);
    assert.equal(await evaluate("document.querySelector('#hover-tooltip').textContent"),await evaluate("layerApp.app.action_tooltip('New layer',{type:'layer',action:{op:'new',group:false,clipped:false}})"));
    await away();
    // An edge fixture also covers long text and tooltips inside native DOM modals.
    await evaluate("(()=>{const d=document.createElement('dialog');d.id='tooltip-fixture';d.innerHTML='<button title=\"Hover details\">Target</button>';document.body.append(d);d.showModal();Object.assign(d.style,{position:'fixed',inset:'auto',margin:'0',right:'0',bottom:'0',padding:'0',width:'max-content',height:'max-content'});})()");
    const selector='#tooltip-fixture button';
    let r=await hover(selector),t=await rect('#hover-tooltip');
    assert.ok(t.bottom<=r.top-3,"Bottom-edge tooltips flip above the control");
    await evaluate("document.querySelector('#tooltip-fixture button').title='Long hover details '.repeat(30)");await settle();
    t=await rect('#hover-tooltip');assert.ok(t.width<=320&&t.x>=8&&t.right<=viewportWidth-8,`Long text wraps inside the viewport: ${JSON.stringify(t)}`);
    await evaluate("document.querySelector('#tooltip-fixture button').title=''");await settle();
    assert.equal(await visible(),false,"Clearing a hint dismisses obsolete text");
    assert.equal(await evaluate("document.querySelector('#tooltip-fixture button').title"),"");
    await evaluate("document.querySelector('#tooltip-fixture button').title='Hover details'");
    for(const cancel of ["key","press","scroll","blur","removed"]) {
      await away();await hover(selector);
      if(cancel==="key") {
        await call("Input.dispatchKeyEvent",{type:"keyDown",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
        await call("Input.dispatchKeyEvent",{type:"keyUp",key:"Escape",code:"Escape",windowsVirtualKeyCode:27});
      } else if(cancel==="press") {
        r=await rect(selector);const p={x:r.x+r.width/2,y:r.y+r.height/2};
        await call("Input.dispatchMouseEvent",{type:"mousePressed",...p,pointerType:"pen",button:"left",buttons:1,clickCount:1});
        assert.equal(await visible(),false,"Pen contact immediately dismisses the tooltip");
        await call("Input.dispatchMouseEvent",{type:"mouseReleased",...p,pointerType:"pen",button:"left",buttons:0,clickCount:1});
      } else if(cancel==="removed")await evaluate("document.querySelector('#tooltip-fixture button').remove()");
      else await evaluate(cancel==="blur"?"window.dispatchEvent(new Event('blur'))":"document.dispatchEvent(new Event('scroll'))");
      await settle();assert.equal(await visible(),false,`${cancel} dismisses tooltip`);
      if(cancel==="key")await evaluate("document.querySelector('#tooltip-fixture').showModal()");
    }
    await evaluate("document.querySelector('#tooltip-fixture').remove()");
    const toolbar='.toolbar-controls [data-tile] > button:not(:disabled)';r=await rect(toolbar);
    await call("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[{id:1,x:r.x+r.width/2,y:r.y+r.height/2}]});await wait(620);
    assert.equal(await visible(),false,"Touch holds keep their existing menu and never show a hover tooltip");
    await call("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});await away();
    console.log("PASS: mouse/pen tooltips, Adwaita style in both themes, centered below/edge clamping/flip/wrapping, drawers/modals, dynamic shortcuts, contact/scroll/Escape/blur/removal, touch exclusion");
  } finally {
    await evaluate("document.querySelector('#tooltip-fixture')?.remove();window.dispatchEvent(new Event('blur'))");
    await send({type:"restore_workspace",workspace:saved.workspace});await send({type:"restore_settings",settings:saved.settings});
  }
}
