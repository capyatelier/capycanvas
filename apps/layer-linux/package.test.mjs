import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { execFileSync, spawnSync } from "node:child_process";
import { preparePackageOutput } from "./package-files.mjs";

test("store metadata covers the current release and every language in both themes", () => {
  const metadata = JSON.parse(execFileSync("python3", ["-c", `
import json, sys, xml.etree.ElementTree as ET
root = ET.parse(sys.argv[1]).getroot()
lang = "{http://www.w3.org/XML/1998/namespace}lang"
print(json.dumps({"releases": [r.get("version") for r in root.findall("releases/release") if r.find("description") is not None],
    "screenshots": [{"default": s.get("type") == "default", "environment": s.get("environment"), "caption": s.findtext("caption"),
        "images": [{"language": i.get(lang, "en"), "localized": lang in i.attrib, "url": i.text, "width": i.get("width"), "height": i.get("height"), "type": i.get("type")}
            for i in s.findall("image")]} for s in root.findall("screenshots/screenshot")]}))
`, new URL("./art.capycanvas.CapyCanvas.metainfo.xml", import.meta.url).pathname], { encoding: "utf8" }));
  const version = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8").match(/^version = "([^"]+)"$/m)[1];
  const languages = [...readFileSync(new URL("../../crates/layer-ui/src/localization_languages.rs", import.meta.url), "utf8")
    .matchAll(/\("[^"]+", "([^"]+)"/g)].map(match => match[1]);
  assert.ok(metadata.releases.includes(version));
  assert.equal(metadata.screenshots.length, 6);
  assert.equal(metadata.screenshots.filter(s => s.default).length, 1);
  assert.ok(metadata.screenshots[0].default);
  const scenes = new Set();
  for (const screenshot of metadata.screenshots) {
    assert.ok(screenshot.caption);
    const canonical = language => ({ pt_BR: "pt-BR", pt: "pt-BR", zh_CN: "zh-Hans", zh: "zh-Hans", zh_TW: "zh-Hant", zh_HK: "zh-Hant", zh_MO: "zh-Hant" })[language] ?? language;
    assert.deepEqual(screenshot.images.slice(0, languages.length).map(image => canonical(image.language)), languages);
    assert.deepEqual(screenshot.images.slice(languages.length).map(image => image.language), ["pt", "zh", "zh_HK", "zh_MO"]);
    assert.match(screenshot.environment, /^gnome(?::dark)?$/);
    const theme = screenshot.environment === "gnome:dark" ? "dark" : "light";
    const scene = screenshot.images[0].url.match(/\/(paint|sketch|photo)-(light|dark)\.png$/)[1];
    scenes.add(`${scene}-${theme}`);
    for (const image of screenshot.images) {
      if (!["pt", "zh", "zh_HK", "zh_MO"].includes(image.language))
        assert.equal(image.language, ({ "pt-BR": "pt_BR", "zh-Hans": "zh_CN", "zh-Hant": "zh_TW" })[canonical(image.language)] ?? image.language);
      assert.equal(image.localized, image.language !== "en");
      assert.equal(image.url, `https://capycanvas.art/store/gtk/${canonical(image.language).toLowerCase()}/${scene}-${theme}.png`);
      assert.deepEqual([image.type, image.width, image.height], ["source", "2486", "1686"]);
    }
  }
  assert.equal(scenes.size, 6);
  assert.equal(new Set(metadata.screenshots.flatMap(s => s.images.map(image => image.url))).size, 90);
  assert.match(metadata.screenshots[0].images[0].url, /\/en\/paint-light\.png$/);
});

test("desktop locale filtering selects localized store images and preserves English fallback", t => {
  const result = spawnSync("/usr/bin/python3", ["-c", `
import json, pathlib, sys
try:
    import gi
    gi.require_version("Xmlb", "2.0")
except (ImportError, ValueError):
    sys.exit(77)
from gi.repository import Xmlb, GLib
xml = pathlib.Path(sys.argv[1]).read_text()
for locale, language in [("en_US", "en"), ("ja_JP", "ja"), ("zh_CN", "zh-hans"), ("zh_TW", "zh-hant"), ("zh_HK", "zh-hant"), ("zh_MO", "zh-hant"), ("zh_SG", "zh-hans"), ("ko_KR", "ko"),
        ("es_ES", "es"), ("pt_BR", "pt-br"), ("pt_PT", "pt-br"), ("id_ID", "id"), ("fr_FR", "fr"), ("de_DE", "de"), ("ru_RU", "ru"),
        ("th_TH", "th"), ("vi_VN", "vi"), ("tr_TR", "tr"), ("it_IT", "it"), ("zz_ZZ", "en")]:
    builder = Xmlb.Builder()
    for variant in GLib.get_locale_variants(locale + ".UTF-8"):
        builder.add_locale(variant)
    builder.add_locale("C")
    source = Xmlb.BuilderSource()
    source.load_xml(xml, 0)
    builder.import_source(source)
    silo = builder.compile(Xmlb.BuilderCompileFlags.SINGLE_LANG)
    images = silo.query("component/screenshots/screenshot/image", 0)
    assert len(images) == 6, (locale, len(images))
    assert all("/" + language + "/" in image.get_text() for image in images), locale
`, new URL("./art.capycanvas.CapyCanvas.metainfo.xml", import.meta.url).pathname], { encoding: "utf8" });
  if (result.status === 77) return t.skip("XMLB introspection is not installed");
  assert.equal(result.status, 0, result.stderr || result.error?.message);
});

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "capy-native-package-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test("staging only clears an owned generated directory", t => {
  const root = fixture(t), output = join(root, "package");
  mkdirSync(output);
  writeFileSync(join(output, "keep"), "unrelated content");
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(output, "keep"), "utf8"), "unrelated content");
  writeFileSync(join(output, ".capy-package"), "Generated Capy Canvas native package\n");
  preparePackageOutput(output);
  assert.ok(!existsSync(join(output, "keep")));
  assert.ok(existsSync(join(output, ".capy-package")));
});

test("staging refuses a package symlink and preserves its target", t => {
  const root = fixture(t), target = join(root, "target"), output = join(root, "package");
  preparePackageOutput(target);
  writeFileSync(join(target, "keep"), "content");
  symlinkSync(target, output);
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(target, "keep"), "utf8"), "content");
});
