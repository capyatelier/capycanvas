import { FakeElement as SharedElement } from "./fake-dom.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { chooseExport, SDR_FORMATS } from "./export-controls.js";
import { exportFormats } from "./documents.js";

class FakeElement extends SharedElement {
  constructor(tag, className = "", text = "") {
    super();
    this.tagName = tag.toUpperCase(); this.className = className; this.textContent = text ?? "";
    this.children = []; this.parentNode = null; this.attributes = new Map(); this.listeners = {};
    this.hidden = false; this.disabled = false; this.value = ""; this.style = {};
  }
  get options() { return this.children.filter(n => n.tagName === "OPTION"); }
  replaceChildren(...nodes) { this.children = []; this.append(...nodes); }
  closest(selector) { for (let n = this; n; n = n.parentNode) if (n.tagName === selector.toUpperCase()) return n; return null; }
  querySelectorAll(selector) {
    const tags = selector.split(",").map(s => s.trim().toUpperCase()), found = [];
    const walk = node => { for (const child of node.children) { if (tags.includes(child.tagName)) found.push(child); walk(child); } };
    walk(this); return found;
  }
  reportValidity() { return true; }
  click() { for (const listener of this.listeners.click ?? []) listener({ target: this }); }
}

const element = (tag, className, text) => new FakeElement(tag, className, text);
const button = (text, action, className = "") => { const node = element("button", className, text); node.addEventListener("click", action); return node; };
const WEBP_LIMIT = "WebP export is limited to 16,384 pixels per side. Fit the size within that or choose another format.";
const base = {
  format: "Png", profile: { name: "sRGB", profile: { Builtin: "Srgb" } }, depth: "U16", background: "Preserve",
  encoding: { conversion: { intent: "RelativeColorimetric", black_point_compensation: false }, dither: "None" },
  jpeg_quality: 90, size: "Original", resolution: "Master", metadata: { keep: "All", remove_location: true },
};
const METADATA_CHOICES = [["All", "All"], ["CopyrightContact", "Copyright & Contact"], ["None", "None"]];

function fakeApp({ extent = [20000, 400], photo = false } = {}) {
  const calls = { drafts: [], validated: [], rendered: 0 };
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
    export_form: () => ({ profiles: [base.profile], metadata: photo }),
    export_presets: async () => ({ names: ["Web", "Print", "Archive", "Last"], recipe: base, index: 0 }),
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
  const result = chooseExport({ app, element, button, id: 7, gpuOperation: run => run(),
    dialog: (title, build) => new Promise(resolve => { form = element("form"); build(form, resolve); }) });
  while (!form) await new Promise(resolve => setImmediate(resolve));
  const labelled = name => form.querySelectorAll("select,input").find(n => n.getAttribute("aria-label") === name);
  const pressed = name => form.querySelectorAll("button").find(n => n.textContent === name);
  const error = form.children.find(n => n.className === "error-message");
  return { form, result, labelled, pressed, error };
}

test("WebP joins the SDR formats with its file type", () => {
  assert.deepEqual(SDR_FORMATS.map(([id]) => id), ["Png", "Tiff", "Jpeg", "Webp"]);
  assert.deepEqual(SDR_FORMATS.at(-1), ["Webp", "WebP · lossless"], "the shared format name");
  assert.deepEqual(exportFormats.Webp, ["webp", "image/webp", "WebP image"]);
  for (const [id] of SDR_FORMATS) assert.ok(exportFormats[id], `${id} has a file type`);
});

test("choosing WebP drafts 8-bit output through the shared recipe", async () => {
  const app = fakeApp({ extent: [800, 600] });
  const dialog = await openDialog(app);
  const format = dialog.labelled("Format"), depth = dialog.labelled("Bit depth");
  assert.deepEqual(format.options.map(o => [o.value, o.textContent]), SDR_FORMATS);
  format.value = "Webp"; format.onchange();
  assert.deepEqual(app.calls.drafts.at(-1), { type: "format", value: "Webp" });
  assert.equal(format.value, "Webp");
  assert.equal(depth.value, "U8");
  assert.deepEqual(depth.options.filter(o => !o.disabled).map(o => o.value), ["U8"], "WebP is 8-bit only");
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
  format.value = "Webp"; format.onchange();
  dialog.pressed("Preview Output").click();
  assert.equal(dialog.error.textContent, WEBP_LIMIT);
  dialog.pressed("Choose File…").click();
  assert.equal(dialog.error.textContent, WEBP_LIMIT);
  assert.equal(app.calls.rendered, 0, "the refusal comes from validation, not the renderer");
  dialog.labelled("Pixel size").value = "Fit";
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
  assert.deepEqual(metadata.options.map(o => [o.value, o.textContent]), METADATA_CHOICES, "labels come from the shared view");
  assert.equal(metadata.value, "All");
  assert.equal(location.checked, true, "location is removed by default");
  assert.equal(metadata.closest("label").hidden, false);
  assert.equal(location.closest("label").hidden, false);
  assert.equal(note.hidden, true);
  metadata.value = "CopyrightContact"; metadata.onchange();
  assert.deepEqual(app.calls.drafts.at(-1), { type: "metadata", value: { keep: "CopyrightContact", remove_location: true } });
  assert.equal(location.closest("label").hidden, true, "Copyright & Contact never keeps a location");
  metadata.value = "All"; metadata.onchange();
  location.checked = false; location.onchange();
  assert.deepEqual(app.calls.drafts.at(-1), { type: "metadata", value: { keep: "All", remove_location: false } });
  const format = dialog.labelled("Format");
  format.value = "Jpeg"; format.onchange();
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
  range.value = "exr"; range.onchange();
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
