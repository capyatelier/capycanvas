// Native staging directory; no bundled GTK libraries or generated source assets.
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const app = dirname(fileURLToPath(import.meta.url)), root = resolve(app, "../..");
const output = join(root, "dist/capycanvas-linux"), marker = join(output, ".capy-package");
if (existsSync(output) && !existsSync(marker)) throw new Error("Refusing to overwrite an unmarked native package directory");
const flags = [...(process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f") || []),
  `--remap-path-prefix=${homedir()}=/build-home`, `--remap-path-prefix=${root}=/capycanvas`];
const records = execFileSync("cargo", ["build", "--release", "-p", "layer-linux", "--message-format=json"], {
  cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"],
  env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f") },
}).trim().split("\n").map(line => JSON.parse(line));
const binary = records.find(r => r.reason === "compiler-artifact" && r.target.name === "layer-linux" && r.executable)?.executable;
const generated = records.find(r => r.reason === "build-script-executed" && r.package_id.includes("/layer-linux#"))?.out_dir;
if (!binary || !generated) throw new Error("Cargo did not report the native binary and icon");
const files = [
  [binary, "bin/capycanvas"],
  [join(app, "art.capycanvas.CapyCanvas.desktop"), "share/applications/art.capycanvas.CapyCanvas.desktop"],
  [join(generated, "art.capycanvas.CapyCanvas.svg"), "share/icons/hicolor/scalable/apps/art.capycanvas.CapyCanvas.svg"],
  ...["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md"].map(name => [join(root, name), `share/doc/capycanvas/${name}`]),
];
for (const [source, relative] of files) { const target = join(output, relative); mkdirSync(dirname(target), { recursive: true }); cpSync(source, target); }
execFileSync("strip", ["--strip-debug", join(output, "bin/capycanvas")]);
execFileSync("desktop-file-validate", [join(output, "share/applications/art.capycanvas.CapyCanvas.desktop")]);
writeFileSync(marker, "Generated Capy Canvas native package\n");
console.log(`Native package: ${output}\nRun: ${join(output, "bin/capycanvas")}`);
