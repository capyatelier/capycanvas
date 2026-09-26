// Native GTK package. Photo codecs are compiled into the shared Rust core.
import { execFileSync } from "node:child_process";
import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { preparePackageOutput } from "./package-files.mjs";
import { dependencyNotices } from "../../tools/build/dependency-notices.mjs";
import { stageGtkRuntime } from "./gtk-runtime.mjs";

const app = dirname(fileURLToPath(import.meta.url)), root = resolve(app, "../..");
const output = join(root, "dist/capycanvas-linux"), marker = preparePackageOutput(output);
const about = process.env.LAYER_CARGO_ABOUT || "cargo-about";
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1];
const target = process.env.CARGO_BUILD_TARGET || host;
if (!target) throw new Error("Cannot determine the Rust package target");
const licensing = JSON.parse(execFileSync(about, ["generate", "--locked", "--fail", "--manifest-path", join(app, "Cargo.toml"),
  "--config", join(root, "tools/build/about.toml"), "--target", target, "--format", "json"],
  { cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] }));
const licenseHtml = dependencyNotices(licensing.licenses);
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
const docs = join(output, "share/doc/capycanvas");
writeFileSync(join(docs, "dependency-licenses.html"), '<!doctype html><meta charset="utf-8"><title>Rust dependency licenses</title>'
  + '<style>body{max-width:70rem;margin:2rem auto;font:16px system-ui}pre{white-space:pre-wrap}</style>'
  + '<h1>Rust dependency licenses</h1>' + licenseHtml);
const sysroot = execFileSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).trim();
const rustNotices = [
  join(sysroot, "share/doc/rust/COPYRIGHT.html"),
  // Arch's system toolchain relocates rustc's complete copyright notice.
  join(sysroot, "share/licenses/rust/COPYRIGHT.html.rustc"),
].find(existsSync);
if (!rustNotices) throw new Error(`Rust toolchain copyright notice not found under ${sysroot}`);
cpSync(rustNotices, join(docs, "rust-toolchain-notices.html"));
chmodSync(join(output, "bin/capycanvas"), 0o755);
execFileSync("strip", ["--strip-debug", join(output, "bin/capycanvas-bin")]);
writeFileSync(join(output, "share/doc/capycanvas-gtk/manifest.json"), JSON.stringify(gtkRuntime, null, 2) + "\n");
execFileSync("desktop-file-validate", [join(output, "share/applications/art.capycanvas.CapyCanvas.desktop")]);
writeFileSync(marker, "Generated Capy Canvas native package\n");
console.log(`Native package: ${output}\nRun: ${join(output, "bin/capycanvas")}`);
