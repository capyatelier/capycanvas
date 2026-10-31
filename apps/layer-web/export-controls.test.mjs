import {dropdownFactory,dropdownChoices,chooseDropdown} from './dropdown-test-fixture.mjs';
import { bindCopy,refreshCopy,refreshBindings } from "./localization.js";
import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { chooseExport, chooseProfileLibrary, localizeProfileLibraries, SDR_FORMATS } from "./export-controls.js";
import { exportFormats } from "./documents.js";

const ownerDocument={createTextNode:nodeValue=>({nodeType:3,nodeValue:String(nodeValue)})};
class FakeElement extends SharedElement {
  constructor(tag, className = "", text = "") {
    super();
    this.tagName = tag.toUpperCase(); this.className = className; this.ownerDocument=ownerDocument;this.children = []; this.parentNode = null; this.attributes = new Map(); this.listeners = {};
    this.hidden = false; this.disabled = false; this.value = ""; this.style = {};this.textContent=text??"";
  }
  showModal(){this.open=true;}
  close(){this.open=false;for(const listener of this.listeners.close??[])listener();}
  remove(){if(this.parentNode)this.parentNode.children.splice(this.parentNode.children.indexOf(this),1);this.parentNode=null;}
  get firstChild(){return this.children[0];}
  get textContent(){return this.children.map(node=>node.nodeType===3?node.nodeValue:node.textContent).join("");}
  set textContent(value){this.replaceChildren(this.ownerDocument.createTextNode(value));}
  insertBefore(node,before){node.parentNode=this;this.children.splice(before?this.children.indexOf(before):this.children.length,0,node);return node;}
  getContext(){return {putImageData:image=>{this.image=image;}};}
  set ariaLabel(value){this.setAttribute("aria-label",value);}
  get ariaLabel(){return this.getAttribute("aria-label");}
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.tagName === selector.toUpperCase()) return n; return null; }
  querySelectorAll(selector) {
    const tags = selector.split(",").map(s => s.trim().toUpperCase()), found = [];
    const walk = node => { for (const child of node.children??[]) { if (tags.includes(child.tagName)) found.push(child); walk(child); } };
    walk(this); return found;
  }
  reportValidity() { return true; }
  click() { for (const listener of this.listeners.click ?? []) listener({ target: this }); }
}

const element = (tag, className, text) => {const node=new FakeElement(tag,className,typeof text==="function"?text():text);if(typeof text==="function")bindCopy(node,text);return node;};
const button = (text, action, className = "") => { const node = element("button", className, text); node.addEventListener("click", action); return node; };
const numberField = (control, label, changed) => {
  const root = element("div"), entry = element("input"); root.append(entry); root.entry = entry;
  root.setAttribute = (name, value) => entry.setAttribute(name, value);
  root.update = value => { entry.value = String(value); };
  root.cancelEditing = () => { entry.composing = false; };
  root.commit = () => {
    if (entry.composing || !/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)$/.test(entry.value)) return false;
    const value = Number(entry.value);
    if (!Number.isFinite(value) || value < control.min || value > control.max) return false;
    changed(value); return true;
  };
  return root;
};
const WEBP_LIMIT = "WebP export is limited to 16,384 pixels per side. Fit the size within that or choose another format.";
const base = {
  format: "Png", profile: { name: "sRGB", profile: { Builtin: "Srgb" } }, depth: "U16", background: "Preserve",
  encoding: { conversion: { intent: "RelativeColorimetric", black_point_compensation: false }, dither: "None" },
  jpeg_quality: 90, size: "Original", resolution: "Master", metadata: { keep: "All", remove_location: true },
};
const METADATA_CHOICES = [["All", "All"], ["CopyrightContact", "Copyright & Contact"], ["None", "None"]];

const catalog = readFileSync(new URL("../../assets/locales/en/color-features.ftl", import.meta.url), "utf8") + "\n" + readFileSync(new URL("../../assets/locales/en/common.ftl", import.meta.url), "utf8");
const source = readFileSync(new URL("../../crates/layer-ui/src/color_feature_copy.rs", import.meta.url), "utf8");
function sharedCopy(name) {
  const implementation = source.split(`impl ${name} {`)[1].split("\n}")[0];
  const copy = {};
  for (const [, field, id] of implementation.matchAll(/([a-z_]+): localizer\.text\(MessageId::([A-Z_0-9]+)\)/g)) {
    const key = id.toLowerCase().replaceAll("_", "-");
    const label = catalog.match(new RegExp(`^${key} = (.*)$`, "m"));
    assert.ok(label, `${key} exists in the shared English catalog`);
    copy[field] = label[1];
  }
  copy.common = { cancel: "Cancel", done: "Done", apply: "Apply" };
  return copy;
}
const exportCopy = sharedCopy("ExportCopy");
const sdrLabels = SDR_FORMATS.map(id => [id, ({Png:exportCopy.format_png,Tiff:exportCopy.format_tiff,Jpeg:exportCopy.format_jpeg,Webp:exportCopy.format_webp})[id]]);

function fakeApp({ extent = [20000, 400], photo = false } = {}) {
  const calls = { drafts: [], validated: [], rendered: 0, preferences: [], reads:0 };
  let language="en";const text=value=>language==="en"?value:`${language}:${value}`;const copy=()=>Object.fromEntries(Object.entries(exportCopy).map(([key,value])=>[key,typeof value==="string"?text(value):value]));
  const metadataCopy=(format,keep)=>({label:text("Metadata"),choices:METADATA_CHOICES.map(([value,label])=>({value,label:text(label)})),remove_location:text("Remove location"),location:format!=="Exr"&&keep==="All",available:format!=="Exr",note:format==="Exr"?text("OpenEXR keeps no camera or copyright details. Choose another format to keep them."):null});
  const draft = (recipe, action) => {
    const format = action.type === "format" ? action.value : recipe.format;
    const metadata = action.type === "metadata" ? action.value : recipe.metadata;
    const webp = format === "Webp", exr = format === "Exr";
    calls.drafts.push(action);
    return {
      recipe: { ...recipe, format, metadata, depth: webp ? "U8" : recipe.depth },
      formats: ["Png", "Tiff", "Jpeg", "Webp"], depths: webp ? ["U8"] : ["U8", "U16"],
      backgrounds: ["Preserve", "White", "Black"], dithers: ["None", "Stochastic8"],
      metadata: {
        label: "Metadata", choices: METADATA_CHOICES.map(([value, label]) => ({ value, label })),
        remove_location: "Remove location", location: !exr && metadata.keep === "All", available: !exr,
        note: exr ? "OpenEXR keeps no camera or copyright details. Choose another format to keep them." : null,
      },
    };
  };
  return {
    calls,
    language(next){language=next;},
    localize_export_presets: view => ({...view,names:view.names.map((name,index)=>index<4?text(["Web","Print","Archive","Last"][index]):name)}),
    profile_name_copy: name => name || text("Embedded profile"),
    export_profile_caption_copy: caption => caption.type==="original"?`${text("Original:")} ${caption.name}`:caption.type==="embedded"?text("Embedded profile"):caption.name,
    color_feature_error_copy: error => error?.color_feature_error?text(error.color_feature_error):String(error),
    export_metadata_copy: metadataCopy,
    export_preview_copy: (format,gainmap,clipped) => text(`${format}:prepared:${gainmap}:${clipped}`),
    export_form: () => ({ profiles: [base.profile], metadata: photo, copy: exportCopy, numeric: { dimension: { min: 1, max: 32768 }, ppi: { min: 1, max: 65535 }, quality: { min: 1, max: 100 } } }),
    export_copy: copy,
    profile_copy: () => sharedCopy("ProfileCopy"),
    export_presets: async action => { calls.reads++;if (action?.type !== "get") calls.preferences.push(action); return { names: ["Web", "Print", "Archive", "Last"], recipe: base, index: 0 }; },
    document_color: () => ({ space: "Srgb", depth: "U8" }),
    export_draft: draft,
    export_validate: recipe => {
      calls.validated.push(recipe.format);
      if (recipe.format === "Webp" && extent.some(v => v > 16384) && recipe.size === "Original") throw WEBP_LIMIT;
      return recipe;
    },
    export_image: async () => { calls.rendered++; return {}; },
    capture_control: () => ({ cancel() {}, free() {}, cancelled: () => false }),
  };
}

async function openDialog(app) {
  let form;
  const result = chooseExport({ app, element, button, dropdown:dropdownFactory({element,button}), numberField, id: 7, gpuOperation: run => run(),
    dialog: (title, build) => new Promise(resolve => { form = element("form"); build(form, resolve); }) });
  while (!form) await new Promise(resolve => setImmediate(resolve));
  const labelled = name => form.querySelectorAll("button,input").find(n => n.getAttribute("aria-label") === name);
  const pressed = name => form.querySelectorAll("button").find(n => n.textContent === name);
  const error = form.children.find(n => n.className === "error-message");
  return { form, result, labelled, pressed, error };
}

test("WebP joins the SDR formats with its file type", () => {
  assert.deepEqual(SDR_FORMATS, ["Png", "Tiff", "Jpeg", "Webp"]);
  assert.deepEqual(sdrLabels.at(-1), ["Webp", "WebP · lossless"], "the shared format name");
  assert.deepEqual(exportFormats.Webp, ["webp", "image/webp"]);
  for (const id of SDR_FORMATS) assert.ok(exportFormats[id], `${id} has a file type`);
});

test("choosing WebP drafts 8-bit output through the shared recipe", async () => {
  const app = fakeApp({ extent: [800, 600] });
  const dialog = await openDialog(app);
  const format = dialog.labelled("Format"), depth = dialog.labelled("Bit depth");
  assert.deepEqual(dropdownChoices(format).map(o => [o.command, o.label]), sdrLabels);
  chooseDropdown(format,"Webp");
  assert.deepEqual(app.calls.drafts.at(-1), { type: "format", value: "Webp" });
  assert.equal(format.value, "Webp");
  assert.equal(depth.value, "U8");
  assert.deepEqual(dropdownChoices(depth).filter(o => o.enabled).map(o => o.command), ["U8"], "WebP is 8-bit only");
  assert.equal(dialog.labelled("Quality").closest("label").hidden, true, "lossless WebP has no quality");
  dialog.pressed("Choose File…").click();
  const choice = await dialog.result;
  assert.equal(choice.recipe.format, "Webp");
  assert.equal(app.calls.rendered, 0);
});

test("a WebP too large for its encoder is refused before any rendering", async () => {
  const app = fakeApp();
  const dialog = await openDialog(app);
  const format = dialog.labelled("Format");
  chooseDropdown(format,"Webp");
  dialog.pressed("Preview Output").click();
  assert.equal(dialog.error.textContent, WEBP_LIMIT);
  dialog.pressed("Choose File…").click();
  assert.equal(dialog.error.textContent, WEBP_LIMIT);
  assert.equal(app.calls.rendered, 0, "the refusal comes from validation, not the renderer");
  chooseDropdown(dialog.labelled("Pixel size"),"Fit");
  dialog.pressed("Choose File…").click();
  const choice = await dialog.result;
  assert.deepEqual(choice.recipe.size, { Fit: { bounds: [2048, 2048], enlarge: false } });
  assert.ok(app.calls.validated.filter(f => f === "Webp").length >= 3);
});

test("a photo's Metadata row drafts the kept fields and location through the shared recipe", async () => {
  const app = fakeApp({ extent: [800, 600], photo: true });
  const dialog = await openDialog(app);
  const metadata = dialog.labelled("Metadata"), location = dialog.labelled("Remove location");
  const note = dialog.form.children.find(n => n.className === "export-metadata-note");
  assert.deepEqual(dropdownChoices(metadata).map(o => [o.command, o.label]), METADATA_CHOICES, "labels come from the shared view");
  assert.equal(metadata.value, "All");
  assert.equal(location.checked, true, "location is removed by default");
  assert.equal(metadata.closest("label").hidden, false);
  assert.equal(location.closest("label").hidden, false);
  assert.equal(note.hidden, true);
  chooseDropdown(metadata,"CopyrightContact");
  assert.deepEqual(app.calls.drafts.at(-1), { type: "metadata", value: { keep: "CopyrightContact", remove_location: true } });
  assert.equal(location.closest("label").hidden, true, "Copyright & Contact never keeps a location");
  chooseDropdown(metadata,"All");
  location.checked = false; location.onchange();
  assert.deepEqual(app.calls.drafts.at(-1), { type: "metadata", value: { keep: "All", remove_location: false } });
  const format = dialog.labelled("Format");
  chooseDropdown(format,"Jpeg");
  assert.equal(location.checked, false, "a format change keeps the metadata choice");
  dialog.pressed("Choose File…").click();
  const choice = await dialog.result;
  assert.deepEqual(choice.recipe.metadata, { keep: "All", remove_location: false });
});

test("OpenEXR explains that it keeps no metadata, and drawings show no Metadata row", async () => {
  const app = fakeApp({ extent: [800, 600], photo: true });
  app.document_color = () => ({ space: "Srgb", depth: "F32" });
  const dialog = await openDialog(app);
  const range = dialog.labelled("Dynamic range");
  chooseDropdown(range,"exr");
  const note = dialog.form.children.find(n => n.className === "export-metadata-note");
  assert.equal(dialog.labelled("Metadata").closest("label").hidden, true);
  assert.equal(dialog.labelled("Remove location").closest("label").hidden, true);
  assert.equal(note.hidden, false);
  assert.match(note.textContent, /^OpenEXR keeps no camera or copyright details\./);
  const drawing = await openDialog(fakeApp({ extent: [800, 600] }));
  assert.equal(drawing.labelled("Metadata").closest("label").hidden, true, "a new drawing has no photo metadata");
  assert.equal(drawing.labelled("Remove location").closest("label").hidden, true);
  assert.equal(drawing.form.children.find(n => n.className === "export-metadata-note").hidden, true);
});

test("export refuses unfinished numeric text before validating or rendering", async () => {
  const app = fakeApp({ extent: [800, 600] }), dialog = await openDialog(app);
  const size = dialog.labelled("Pixel size"); chooseDropdown(size,"Fit");
  const width = dialog.labelled(exportCopy.maximum_width);
  const before = app.calls.validated.length;
  for (const literal of ["bad", "Infinity", "２０４８"]) {
    width.value = literal;
    dialog.pressed("Choose File…").click(); dialog.pressed("Preview Output").click();
    dialog.pressed(exportCopy.save_preset).click(); dialog.pressed(exportCopy.update_preset).click();
    assert.equal(app.calls.validated.length, before); assert.equal(app.calls.rendered, 0);
    assert.equal(app.calls.preferences.length, 0);
  }
  width.value = "1024"; width.composing = true;
  dialog.pressed("Choose File…").click();
  dialog.pressed(exportCopy.save_preset).click(); dialog.pressed(exportCopy.update_preset).click();
  assert.equal(app.calls.validated.length, before);
  assert.equal(app.calls.preferences.length, 0);
  width.composing = false; dialog.pressed("Choose File…").click();
  assert.deepEqual((await dialog.result).recipe.size.Fit.bounds, [1024, 2048]);
});

test("language publication retains export drafts and projects bounded labels without storage or validation work",async()=>{
  const app=fakeApp({extent:[800,600],photo:true}),dialog=await openDialog(app);
  const controls=dialog.form.querySelectorAll("input,button"),format=dialog.labelled("Format"),destination=dialog.labelled("Destination"),metadata=dialog.labelled("Metadata");
  const captions=[format,destination,metadata].map(node=>node.children[0]);
  const size=dialog.labelled("Pixel size");chooseDropdown(size,"Fit");
  const width=dialog.labelled(exportCopy.maximum_width),name=dialog.labelled(exportCopy.preset_name);
  width.value="２０４８ unfinished";width.selectionStart=2;width.selectionEnd=7;width.composing=true;name.value="İı ไทย Tiếng Việt { $name } 🎨";
  const before={drafts:app.calls.drafts.length,validated:app.calls.validated.length,rendered:app.calls.rendered,reads:app.calls.reads};
  app.language("tr");refreshCopy(app);dialog.form.localize();refreshBindings();
  assert.deepEqual(dialog.form.querySelectorAll("input,button"),controls);assert.deepEqual([format,destination,metadata].map(node=>node.children[0]),captions);
  assert.equal(width.value,"２０４８ unfinished");assert.equal(width.selectionStart,2);assert.equal(width.selectionEnd,7);assert.equal(width.composing,true);assert.equal(name.value,"İı ไทย Tiếng Việt { $name } 🎨");
  assert.equal(format.ariaLabel,"tr:Format");assert.equal(dropdownChoices(format)[0].label,`tr:${exportCopy.format_png}`);assert.equal(dropdownChoices(destination)[0].label,"tr:Web");assert.equal(metadata.ariaLabel,"tr:Metadata");assert.equal(dropdownChoices(metadata)[1].label,"tr:Copyright & Contact");
  assert.deepEqual({drafts:app.calls.drafts.length,validated:app.calls.validated.length,rendered:app.calls.rendered,reads:app.calls.reads},before);
  dialog.form.querySelectorAll("button").find(node=>node.textContent==="Cancel").click();await dialog.result;
});

test("pending and prepared export captions follow publication without recapturing or replacing previews",async t=>{
  const previous=globalThis.ImageData;globalThis.ImageData=class{constructor(data,width,height){Object.assign(this,{data,width,height});}};t.after(()=>{globalThis.ImageData=previous;});
  const app=fakeApp({extent:[800,600]});let resolve;
  app.export_image=()=>{app.calls.rendered++;return new Promise(done=>{resolve=done;});};
  const dialog=await openDialog(app);dialog.pressed("Preview Output").click();
  app.language("fr");refreshCopy(app);dialog.form.localize();refreshBindings();
  assert.equal(app.calls.rendered,1);assert.equal(dialog.form.children.filter(node=>node.tagName==="P").at(-1).textContent,`fr:${exportCopy.preparing_comparison}`);
  const image={extent:[1,1],pixels:[255,0,0,255]};resolve({previews:[image,image],clipped_channels:0});await new Promise(done=>setImmediate(done));
  const canvases=dialog.form.querySelectorAll("canvas"),figures=dialog.form.querySelectorAll("figure"),pixels=canvases.map(node=>node.image);
  assert.equal(canvases.length,2);assert.equal(dialog.form.children.filter(node=>node.tagName==="P").at(-1).textContent,"fr:Png:prepared:false:false");
  const before={drafts:app.calls.drafts.length,validated:app.calls.validated.length,reads:app.calls.reads};
  app.language("tr");refreshCopy(app);dialog.form.localize();refreshBindings();
  assert.deepEqual(dialog.form.querySelectorAll("canvas"),canvases);assert.deepEqual(dialog.form.querySelectorAll("figure"),figures);assert.deepEqual(canvases.map(node=>node.image),pixels);assert.equal(app.calls.rendered,1);
  assert.equal(canvases[0].ariaLabel,`tr:${exportCopy.artwork_preview}`);assert.equal(figures[1].querySelectorAll("figcaption")[0].textContent,`tr:${exportCopy.output}`);assert.equal(dialog.form.children.filter(node=>node.tagName==="P").at(-1).textContent,"tr:Png:prepared:false:false");
  assert.deepEqual({drafts:app.calls.drafts.length,validated:app.calls.validated.length,reads:app.calls.reads},before);
  dialog.form.querySelectorAll("button").find(node=>node.textContent==="Cancel").click();await dialog.result;
});

test("retained export refusals project the raw shared reason without repeating validation",async()=>{
  const app=fakeApp({extent:[800,600]}),dialog=await openDialog(app);let validations=0;
  app.export_validate=()=>{validations++;throw {color_feature_error:"ExportDimensions"};};
  dialog.pressed("Choose File…").click();assert.equal(dialog.error.textContent,"ExportDimensions");
  app.language("vi");refreshCopy(app);dialog.form.localize();refreshBindings();assert.equal(dialog.error.textContent,"vi:ExportDimensions");assert.equal(validations,1);assert.equal(app.calls.rendered,0);
  dialog.form.querySelectorAll("button").find(node=>node.textContent==="Cancel").click();await dialog.result;
});


test("retained original-profile choice reprojects its descriptor while profile bytes and literal layer names remain unchanged",async()=>{
  const app=fakeApp({extent:[800,600]}),literal="İı ไทย Tiếng Việt { $name } 🎨",profile={name:literal,channels:"Rgb",profile:{Icc:[0,1,2,3]}};
  const originalForm=app.export_form;app.export_form=()=>({...originalForm(),profiles:[base.profile,profile],profile_captions:[{type:"literal",name:"sRGB"},{type:"original",name:literal}]});
  const dialog=await openDialog(app),select=dialog.labelled("Output profile"),caption=select.children[0];chooseDropdown(select,"1");
  assert.equal(dropdownChoices(select)[1].label,`Original: ${literal}`);const bytes=profile.profile.Icc,before=app.calls.drafts.length;
  app.language("tr");refreshCopy(app);dialog.form.localize();refreshBindings();
  assert.equal(select.children[0],caption);assert.equal(dropdownChoices(select)[1].label,`tr:Original: ${literal}`);assert.equal(select.value,"1");assert.equal(profile.name,literal);assert.equal(profile.profile.Icc,bytes);assert.equal(app.calls.drafts.length,before);assert.equal(app.calls.reads,1);assert.equal(app.calls.rendered,0);
  dialog.form.querySelectorAll("button").find(node=>node.textContent==="Cancel").click();await dialog.result;
});

test('fit enlargement follows the visible option into the validated export recipe',async()=>{
  const app=fakeApp(),dialog=await openDialog(app);
  const size=dialog.labelled(exportCopy.pixel_size);chooseDropdown(size,'Fit');
  const enlarge=dialog.labelled(exportCopy.enlarge);assert.equal(enlarge.closest('label').hidden,false);enlarge.checked=true;
  dialog.pressed(exportCopy.choose_file).click();
  const chosen=await dialog.result;assert.equal(chosen.recipe.size.Fit.enlarge,true);
});


test('profile-library search uses shared metadata matching and Escape waits for its active write',async t=>{
  const before=globalThis.document,body=element('body');globalThis.document={body};t.after(()=>globalThis.document=before);
  const entries=Array.from({length:8},(_,index)=>({id:String(index),entry_copy_source:{id:String(index)},visible:true}));
  let release,language='en';const queries=[];
  const app={profile_copy:()=>({search:'Search profiles',library_title:'Profiles',library_help:'Help',remove:'Remove',show:'Show',hide:'Hide',import:'Import',common:{done:'Done'}}),language_tag:()=>language,
    profile_entry_copy:entry=>({name:`${language}:profile-${entry.id}`,details:'Details'}),
    profile_library_matches:(entry,query)=>{queries.push([entry.id,query]);return !query||entry.id===query;},
    profile_library:async(type)=>{if(type==='list')return entries;if(type==='remove')await new Promise(done=>release=done);},color_feature_error_copy:String};
  const result=chooseProfileLibrary({app,element,button,manage:true});await new Promise(done=>setImmediate(done));
  const root=body.children.find(node=>node.tagName==='DIALOG'),search=root.querySelectorAll('input')[0];assert.equal(search.hidden,false);
  search.value='3';search.dispatchEvent({type:'input'});assert.equal(root.querySelectorAll('h3').length,1);assert.equal(root.querySelectorAll('h3')[0].textContent,'en:profile-3');assert.ok(queries.some(([id,query])=>id==='3'&&query==='3'));
  language='fr';localizeProfileLibraries();assert.equal(root.querySelectorAll('h3')[0].textContent,'fr:profile-3');assert.equal(search.value,'3');
  root.querySelectorAll('button').find(node=>node.textContent==='Remove').click();
  const cancel={type:'cancel',defaultPrevented:false,preventDefault(){this.defaultPrevented=true;}};root.dispatchEvent(cancel);assert.equal(cancel.defaultPrevented,true);assert.equal(root.open,true);
  release();await new Promise(done=>setImmediate(done));assert.equal(root.querySelectorAll('button').find(node=>node.textContent==='Done').disabled,false);
  root.querySelectorAll('button').find(node=>node.textContent==='Done').click();assert.equal(await result,null);assert.equal(body.children.some(node=>node.tagName==='DIALOG'),false);
});
