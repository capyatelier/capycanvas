import assert from "node:assert/strict";
import {mkdir, readdir, writeFile} from "node:fs/promises";
import {checkIconControls} from "./icon-controls.test.mjs";

// Run through Chrome's SVG renderer on desktop or tablet. The editor remains
// mounted behind an isolated fixture; no document or workspace is changed.
export async function checkIcons({call, evaluate, settle}) {
  const output = process.env.LAYER_TEST_ARTIFACTS || "artifacts/ui/icons/web";
  await mkdir(output, {recursive: true});
  await checkIconControls({call,evaluate,settle},output);
  const icons = (await readdir(new URL("./icons/", import.meta.url))).filter(n => n.endsWith(".svg")).sort();
  const commands = await evaluate("layerApp.state().commands.map(({id,icon})=>({id,icon}))");
  const catalog = await evaluate("layerApp.app.catalog().icons");
  for (const command of commands) {
    assert.ok(command.icon, `${command.id} needs an action icon`);
    assert.ok(catalog.includes(command.icon), `${command.id} must preload its icon`);
    assert.ok(icons.includes(`layer-${command.icon}-symbolic.svg`), `${command.id} SVG must ship`);
  }
  for (const [id, icon] of Object.entries({clear_layer:"clear",eraser:"eraser",delete_layer:"delete",scale_rotate:"transform",
    fit_canvas:"fit",select_all:"select-all",deselect:"deselect",invert_selection:"invert-selection",fill_selection:"fill-selection"})) {
    assert.equal(commands.find(c => c.id === id)?.icon, icon, id);
  }
  const mounted = await evaluate(`Array.from(document.querySelectorAll('svg[data-asset]'), svg=>({
    icon:svg.dataset.asset, width:svg.getBoundingClientRect().width,height:svg.getBoundingClientRect().height,
    hidden:svg.getAttribute('aria-hidden'),focusable:svg.getAttribute('focusable')})).filter(s=>s.width&&s.height)`);
  assert.ok(mounted.length > 20, "Production editor has mounted icons");
  for (const svg of mounted) {
    assert.ok(Math.abs(svg.width-svg.height)<.1, `${svg.icon} must not stretch`);
    assert.equal(svg.hidden,"true");
    assert.equal(svg.focusable,"false");
  }
  await writeFile(`${output}/commands.json`, JSON.stringify({commands,mounted},null,2));
  const saved = await evaluate("layerApp.state().settings.theme");
  const fixtures = [];
  try {
    for (const theme of ["light","dark"]) {
      await evaluate(`layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(theme)}})`);
      await settle();
      const editor = await call("Page.captureScreenshot",{format:"png"});
      await writeFile(`${output}/editor-${theme}.png`,Buffer.from(editor.data,"base64"));
    }
    await evaluate(`(async()=>{
      const frame=document.createElement('iframe');frame.id='icon-audit';frame.title='Icon rendering fixture';
      frame.style.cssText='position:fixed;left:0;top:0;width:576px;height:${Math.ceil(icons.length/12)*48}px;border:0;z-index:2147483647';
      document.body.append(frame);const d=frame.contentDocument;d.body.style.margin='0';
      for(const [index,file] of ${JSON.stringify(icons)}.entries()) {
        const response=await fetch('./icons/'+file);if(!response.ok)throw Error('Missing '+file);
        const svg=new DOMParser().parseFromString(await response.text(),'image/svg+xml').documentElement;
        if(svg.localName!=='svg')throw Error('Invalid '+file);svg.dataset.asset=file;
        const cell=d.createElement('div');cell.style.cssText='position:absolute;width:48px;height:48px;display:grid;place-items:center;left:'+(index%12*48)+'px;top:'+(Math.floor(index/12)*48)+'px';
        cell.append(d.importNode(svg,true));d.body.append(cell);
      }
    })()`);
    for (const theme of ["light","dark"]) for (const size of [16,24,32]) for (const state of ["normal","accent","disabled"]) {
      const fixture = {name:`${theme}-${size}-${state}`,theme,size,width:576,height:Math.ceil(icons.length/12)*48,
        foreground:state==="accent"?"#3584e4":theme==="light"?"#292a2d":"#f0f0f1",
        background:theme==="light"?"#fafafa":"#242629",opacity:state==="disabled"?.35:1,
        icons:icons.map(n=>n.replace(/\.svg$/,""))};
      fixture.scale = await evaluate("devicePixelRatio");
      await evaluate(`(()=>{const f=${JSON.stringify(fixture)},d=document.querySelector('#icon-audit').contentDocument;
        d.body.style.background=f.background;d.body.style.color=f.foreground;
        for(const svg of d.querySelectorAll('svg')){svg.style.width=svg.style.height=f.size+'px';svg.style.opacity=f.opacity;}
      })()`);
      await settle();
      const shot = await call("Page.captureScreenshot",{format:"png",clip:{x:0,y:0,width:fixture.width,height:fixture.height,scale:1}});
      await writeFile(`${output}/web-${fixture.name}.png`,Buffer.from(shot.data,"base64"));
      fixtures.push(fixture);
    }
    await writeFile(`${output}/fixtures.json`,JSON.stringify({schema:1,fixtures},null,2));
  } finally {
    await evaluate(`document.querySelector('#icon-audit')?.remove();layerApp.dispatch({type:'set_theme',theme:${JSON.stringify(saved)}})`);
    await settle();
  }
  console.log(`PASS: ${commands.length} command identities, ${mounted.length} mounted icons, ${icons.length} SVGs in 18 theme/size/state captures`);
}
