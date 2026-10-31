import {chooseDropdown} from './dropdown-fixture.mjs';
// An in-page script that fills the open New drawing dialog through its
// controls and, unless `create` is false, presses Create. Sizes are in
// pixels; `paper` is 'white', 'transparent' or null to keep the dialog's
// paper color; `relief` is the Texture visibility in percent or null to keep
// the dialog's.
export function createDrawing({width = 2048, height = 1536, resolution = null, space = null, depth = null, paper = 'white', relief = 0, name = null, create = true} = {}) {
  return `(async()=>{
    const end=performance.now()+30000,dialog=()=>document.querySelector('#new-drawing-dialog[open]'),view=()=>layerApp.state().new_drawing;
    while(!dialog()||!view()){if(performance.now()>end)throw Error('New drawing did not open');await new Promise(r=>setTimeout(r,20));}
    const choose=(id,key,value)=>{const index=view()[key].findIndex(c=>c.value===value);if(index<0)throw Error('No '+key+' '+value);(${chooseDropdown.toString()})('#'+id,String(index));};
    const type=(selector,text)=>{const n=dialog().querySelector(selector);n.focus();n.value=String(text);n.dispatchEvent(new Event('input',{bubbles:true}));n.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));};
    const press=selector=>{const n=dialog().querySelector(selector);if(!n||n.disabled)throw Error('Cannot press '+selector);n.click();};
    choose('new-drawing-unit','units','px');
    type('#new-drawing-width input',${width});type('#new-drawing-height input',${height});
    ${resolution == null ? '' : `type('#new-drawing-resolution-field input',${resolution});`}
    ${space == null ? '' : `choose('new-drawing-space','spaces',${JSON.stringify(space)});`}
    ${depth == null ? '' : `choose('new-drawing-depth','depths',${JSON.stringify(depth)});`}
    ${paper == null ? '' : `press('#new-drawing-paper-color');press(${JSON.stringify(paper === 'white' ? '#new-drawing-paper-color-tiles [data-new-drawing-paper-color="0"]' : '#new-drawing-color-mode-transparent')});press('#new-drawing-color-use');`}
    ${relief == null ? '' : `press('#new-drawing-texture');type('#new-drawing-paper-relief-field input',${relief});press('#new-drawing-paper-use');if(view().setup.paper.relief!==${relief})throw Error('New drawing kept relief '+view().setup.paper.relief);`}
    ${name == null ? '' : `const field=dialog().querySelector('#new-drawing-name');field.value=${JSON.stringify(name)};field.dispatchEvent(new Event('input',{bubbles:true}));`}
    const setup=view().setup;
    if(setup.unit!=='px'||view().values[0]!==${width}||view().values[1]!==${height})throw Error('New drawing kept '+JSON.stringify(view().values));
    if(!${create})return setup;
    press('#new-drawing-create');
    while(view()){if(performance.now()>end)throw Error('New drawing did not create');await new Promise(r=>setTimeout(r,20));}
    return setup;
  })()`;
}
