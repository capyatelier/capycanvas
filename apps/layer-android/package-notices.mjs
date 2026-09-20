// Build-time notices for the Rust code actually linked into the selected APK ABIs.
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { dependencyNotices } from "../../tools/build/dependency-notices.mjs";

const app = dirname(fileURLToPath(import.meta.url)), root = resolve(app, "../..");
const [output, ...abis] = process.argv.slice(2);
if (!output || !abis.length) throw new Error("Usage: node package-notices.mjs OUTPUT ABI...");
const targets = abis.map(abi => {
  const target = { "arm64-v8a": "aarch64-linux-android", "x86_64": "x86_64-linux-android",
    "armeabi-v7a": "armv7-linux-androideabi", "x86": "i686-linux-android" }[abi];
  if (!target) throw new Error(`Unknown Android ABI: ${abi}`);
  return target;
});
const licensing = JSON.parse(execFileSync(process.env.LAYER_CARGO_ABOUT || "cargo-about", [
  "generate", "--locked", "--fail", "--manifest-path", join(app, "native/Cargo.toml"),
  "--config", join(root, "tools/build/about.toml"), ...targets.flatMap(target => ["--target", target]),
  "--format", "json",
], { cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] }));
const html = dependencyNotices(licensing.licenses);
const sysroot = execFileSync("rustc", ["--print", "sysroot"], { encoding: "utf8" }).trim();
mkdirSync(output, { recursive: true });
cpSync(join(sysroot, "share/doc/rust/COPYRIGHT.html"), join(output, "rust-toolchain-notices.html"));
writeFileSync(join(output, "dependency-licenses.html"), '<!doctype html><meta charset="utf-8"><title>Rust dependency licenses</title>'
  + '<style>body{max-width:70rem;margin:2rem auto;font:16px system-ui}pre{white-space:pre-wrap}</style>'
  + '<h1>Rust dependency licenses</h1>' + html);
