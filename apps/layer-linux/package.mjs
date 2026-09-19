// Native package with the pinned startup-safe GTK and photo codecs.
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { stagePhotoCodecs, verifyPhotoCodecs } from "./photo-codecs.mjs";
import { stageGtkRuntime } from "./gtk-runtime.mjs";

const app = dirname(fileURLToPath(import.meta.url)), root = resolve(app, "../..");
const output = join(root, "dist/capycanvas-linux"), marker = join(output, ".capy-package");
if (existsSync(output) && !existsSync(marker)) throw new Error("Refusing to overwrite an unmarked native package directory");
const photoCodecs = verifyPhotoCodecs(root);
// Mark ownership before staging dependencies so a failed first build can be
// retried without treating our partial output as an unrelated directory.
mkdirSync(output, { recursive: true });
writeFileSync(marker, "Generated Capy Canvas native package (build in progress)\n");
const gtkRuntime = stageGtkRuntime(root, output);
const flags = [...(process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f") || []),
  `--remap-path-prefix=${homedir()}=/build-home`, `--remap-path-prefix=${root}=/capycanvas`];
const records = execFileSync("cargo", ["build", "--locked", "--release", "-p", "layer-linux", "--message-format=json"], {
  cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"],
  env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f") },
}).trim().split("\n").map(line => JSON.parse(line));
const binary = records.find(r => r.reason === "compiler-artifact" && r.target.name === "layer-linux" && r.executable)?.executable;
const generated = records.find(r => r.reason === "build-script-executed" && r.package_id.includes("/layer-linux#"))?.out_dir;
if (!binary || !generated) throw new Error("Cargo did not report the native binary and icon");
const files = [
  [binary, "bin/capycanvas-bin"],
  [join(app, "launch.sh"), "bin/capycanvas"],
  [join(app, "art.capycanvas.CapyCanvas.desktop"), "share/applications/art.capycanvas.CapyCanvas.desktop"],
  [join(app, "art.capycanvas.CapyCanvas.xml"), "share/mime/packages/art.capycanvas.CapyCanvas.xml"],
  [join(generated, "art.capycanvas.CapyCanvas.svg"), "share/icons/hicolor/scalable/apps/art.capycanvas.CapyCanvas.svg"],
  ...readdirSync(join(root, "assets/filters")).filter(name => /\.(json|wgsl)$/.test(name))
    .map(name => [join(root, "assets/filters", name), `bin/filters/${name}`]),
  ...["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md"].map(name => [join(root, name), `share/doc/capycanvas/${name}`]),
];
for (const [source, relative] of files) { const target = join(output, relative); mkdirSync(dirname(target), { recursive: true }); cpSync(source, target); }
stagePhotoCodecs(photoCodecs, output);
chmodSync(join(output, "bin/capycanvas"), 0o755);
execFileSync("strip", ["--strip-debug", join(output, "bin/capycanvas-bin")]);
writeFileSync(join(output, "share/doc/capycanvas-gtk/manifest.json"), JSON.stringify(gtkRuntime, null, 2) + "\n");
execFileSync("desktop-file-validate", [join(output, "share/applications/art.capycanvas.CapyCanvas.desktop")]);
writeFileSync(marker, "Generated Capy Canvas native package\n");
console.log(`Native package: ${output}\nRun: ${join(output, "bin/capycanvas")}`);
