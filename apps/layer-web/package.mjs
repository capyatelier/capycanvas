// Build-only static distribution. No npm bundler, deployment, or Git writes.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = dirname(fileURLToPath(import.meta.url));
const root = resolve(web, "../..");
const digest = (data) => createHash("sha256").update(data);
const read = (path) => readFileSync(path, "utf8");
const escape = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
const page = (title, body) => `<!doctype html><html lang="en"><meta charset="utf-8"><title>${title}</title>
<style>body{max-width:70rem;margin:2rem auto;padding:0 1rem;font:16px system-ui}pre{white-space:pre-wrap}</style><h1>${title}</h1>${body}</html>`;

export function dependencyNotices(licenses) {
  return licenses.map((license) => {
    // Workspace licensing is included separately with the branding exception.
    const crates = license.used_by.map((used) => used.crate).filter((crate) => crate.source);
    if (!crates.length) return "";
    if (!license.source_path || /<year>|<copyright holders>/i.test(license.text))
      throw new Error(`Missing original license notice: ${crates.map((crate) => crate.name).join(", ")}`);
    return `<section><h2>${escape(license.name)}</h2><ul>${crates.map((crate) => `<li>${escape(crate.name)} ${escape(crate.version)}</li>`).join("")}</ul><pre>${escape(license.text)}</pre></section>`;
  }).join("\n");
}

export function filesIn(directory, prefix = "") {
  return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, "en")).flatMap((entry) => {
    const path = prefix + entry.name;
    if (entry.isDirectory()) return filesIn(join(directory, entry.name), path + "/");
    if (!entry.isFile()) throw new Error(`Not a regular package file: ${path}`);
    return [path];
  });
}

export function checkRuntime(data, name) {
  const text = data.toString("latin1");
  if (/\/(?:home|Users)\/[^\s/]+|[A-Z]:\\Users\\|-----BEGIN (?:\w+ )?PRIVATE KEY-----/.test(text)) {
    throw new Error(`Private path or key in runtime asset: ${name}`);
  }
}

function replaceRequired(text, from, to) {
  if (!text.includes(from)) throw new Error(`Missing package reference: ${from}`);
  return text.replaceAll(from, to);
}

export function fingerprintAssets(directory) {
  const files = filesIn(directory), names = {};
  const publish = (path, data = readFileSync(join(directory, path))) => {
    checkRuntime(data, path);
    const extension = extname(path);
    const name = `${path.slice(0, -extension.length)}.${digest(data).digest("hex").slice(0, 20)}${extension}`;
    writeFileSync(join(directory, name), data);
    rmSync(join(directory, path));
    names[path] = name;
  };
  // Our small, explicit graph: artwork/Wasm first, then CSS, glue and app.
  // Hash final bytes, after rewriting dependencies; no bundler or runtime fetch.
  const modules = ["preferences.js", "gpu.js", "customization.js", "numeric.js", "layers.js", "effects.js", "pkg/layer_web.js", "app.js"];
  for (const path of files) {
    if (path.endsWith(".js") && !modules.includes(path))
      throw new Error(`Add the new module to the package dependency order: ${path}`);
    if (!path.endsWith(".js") && path !== "style.css") publish(path);
  }
  const css = read(join(directory, "style.css")).replace(/url\((["']?)([^"')]+)\1\)/g, (reference, _quote, path) => {
    if (/^(data:|https?:|\/|#)/.test(path)) return reference;
    const name = names[path.replace(/^\.\//, "")];
    if (!name) throw new Error(`Missing CSS asset: ${path}`);
    return `url(${JSON.stringify(name)})`;
  });
  publish("style.css", css);
  publish("preferences.js");
  publish("gpu.js");
  publish("customization.js");
  publish("numeric.js");
  publish("layers.js");
  publish("effects.js");
  publish("pkg/layer_web.js", replaceRequired(read(join(directory, "pkg/layer_web.js")),
    "'layer_web_bg.wasm'", JSON.stringify(basename(names["pkg/layer_web_bg.wasm"]))));
  let app = read(join(directory, "app.js"));
  for (const path of modules.slice(0, -1))
    app = replaceRequired(app, `from "./${path}"`, `from "./${names[path]}"`);
  const artwork = Object.fromEntries(Object.entries(names).filter(([path]) => /^(icons|brush-previews)\//.test(path)));
  app = replaceRequired(app, "const assetPaths = {};", `const assetPaths = ${JSON.stringify(artwork)};`);
  publish("app.js", app);
  return names;
}

export function writeWorker(directory, template = read(join(web, "sw.js"))) {
  const files = filesIn(directory).filter((path) => !["sw.js", ".capy-package"].includes(path)).map((path) => ({
    path, integrity: `sha256-${digest(readFileSync(join(directory, path))).digest("base64")}`,
  }));
  // Worker-only changes also need a distinct cache: a failed update must never
  // delete or overwrite the still-active worker's cache.
  const version = digest(template + "\0" + JSON.stringify(files)).digest("hex");
  writeFileSync(join(directory, "sw.js"), template
    .replace('"__CAPY_VERSION__"', JSON.stringify(version))
    .replace("__CAPY_FILES__", JSON.stringify(files)));
  writeFileSync(join(directory, ".capy-package"), version + "\n");
  return { version, files };
}

function run(command, args, options = {}) {
  return execFileSync(command, args, { cwd: root, stdio: "inherit", ...options });
}
function tool(name, override) {
  const candidates = override ? [override] : [name, join(process.env.CARGO_HOME || join(homedir(), ".cargo"), "bin", name)];
  for (const candidate of candidates) {
    try { run(candidate, ["--version"], { stdio: "ignore" }); return candidate; } catch {}
  }
  throw new Error(`Missing ${name}; see docs/web-packaging.md for build prerequisites`);
}

export function packageWeb() {
  const resvg = tool("resvg", process.env.LAYER_RESVG);
  const about = tool("cargo-about", process.env.LAYER_CARGO_ABOUT);
  const output = join(root, "dist/capycanvas");
  if (existsSync(output) && !existsSync(join(output, ".capy-package"))) {
    throw new Error("Refusing to replace dist/capycanvas without its package marker");
  }
  mkdirSync(join(root, "target"), { recursive: true });
  const staging = mkdtempSync(join(root, "target/capy-web-package-"));
  const site = join(staging, "site"), runtime = join(site, "assets");
  mkdirSync(runtime, { recursive: true });
  try {
    const sysroot = run("rustc", ["--print", "sysroot"], { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] }).trim();
    if (!existsSync(join(sysroot, "share/doc/rust/COPYRIGHT.html")))
      throw new Error("Missing toolchain notices; run rustup component add rust-docs");
    if (process.env.RUSTFLAGS && !process.env.CARGO_ENCODED_RUSTFLAGS)
      throw new Error("Use CARGO_ENCODED_RUSTFLAGS for extra package flags so paths with spaces remain intact");
    const cargoHome = resolve(process.env.CARGO_HOME || join(homedir(), ".cargo"));
    // Encoded flags preserve spaces in paths. Cargo rebuilds affected artifacts;
    // stripping DWARF alone cannot remove file!()/panic paths embedded as data.
    const flags = [
      ...(process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f") || []),
      `--remap-path-prefix=${homedir()}=/build-home`,
      `--remap-path-prefix=${cargoHome}=/cargo`,
      `--remap-path-prefix=${sysroot}=/rust`,
      `--remap-path-prefix=${root}=/capycanvas`,
    ];
    run("bash", [join(web, "build.sh"), join(runtime, "pkg"), "web-release"], {
      env: { ...process.env, CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f") },
    });
    for (const path of filesIn(join(runtime, "pkg"))) {
      if (path.endsWith(".d.ts")) rmSync(join(runtime, "pkg", path));
    }
    for (const path of ["app.js", "preferences.js", "gpu.js", "customization.js", "numeric.js", "layers.js", "effects.js", "style.css"])
      cpSync(join(web, path), join(runtime, path));
    for (const directory of ["icons", "brush-previews"]) {
      mkdirSync(join(runtime, directory));
      for (const path of readdirSync(join(web, directory)).sort()) {
        if (path.endsWith(directory === "icons" ? ".svg" : ".png"))
          cpSync(join(web, directory, path), join(runtime, directory, path));
      }
    }
    // Shared brand geometry and mid-gray background at every icon size.
    const mark = read(join(web, "icons/layer-zen-looking-up-symbolic.svg"))
      .replace('width="24" height="24"', 'x="96" y="96" width="320" height="320" color="#f6f5f4"');
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><rect width="512" height="512" rx="76.8" fill="#767676"/>${mark}</svg>`;
    for (const size of [32, 180, 192, 512]) {
      // File input avoids renderer stdin/EOF stalls in constrained build hosts.
      // Apple masks artwork itself; other platforms retain rounded corners.
      const source = join(staging, "icon.svg");
      writeFileSync(source, size === 180 ? svg.replace(' rx="76.8"', "") : svg);
      run(resvg, ["--resources-dir", web, "--width", String(size), "--height", String(size), source, join(runtime, `icon-${size}.png`)]);
    }
    const names = fingerprintAssets(runtime);
    const asset = (path) => `assets/${names[path]}`;
    // Apple's out-of-page icon lookup also needs a conventional stable URL.
    // A query fingerprint refreshes explicit links while older saved URLs still
    // work after deployment removes a previous release's hashed asset files.
    cpSync(join(runtime, names["icon-180.png"]), join(site, "apple-touch-icon.png"));
    const appleIcon = `./apple-touch-icon.png?v=${names["icon-180.png"].split(".")[1]}`;

    const notices = ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md"];
    for (const path of notices) cpSync(join(root, path), join(site, path));
    const licensing = JSON.parse(run(about, ["generate", "--locked", "--fail", "--manifest-path", join(web, "Cargo.toml"),
      "--config", join(web, "about.toml"), "--format", "json"], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] }));
    writeFileSync(join(site, "dependency-licenses.html"), page("Dependency licenses",
      '<p>Wasm dependencies, including build-time crates. <a href="./licenses.html">Application and toolchain notices</a></p>' + dependencyNotices(licensing.licenses)));
    // Preserve the exact installed toolchain's notices, rather than guessing
    // which embedded standard-library/compiler-builtins notices can be omitted.
    cpSync(join(sysroot, "share/doc/rust/COPYRIGHT.html"), join(site, "rust-toolchain-notices.html"));
    writeFileSync(join(site, "licenses.html"), page("Capy Canvas licenses",
      '<p><a href="./">Back to drawing</a> · <a href="dependency-licenses.html">Rust dependency notices</a> · <a href="rust-toolchain-notices.html">Rust toolchain notices</a></p>' +
      notices.map((path) => `<h2>${path}</h2><pre>${escape(read(join(root, path)))}</pre>`).join("\n")));
    writeFileSync(join(site, "manifest.webmanifest"), JSON.stringify({
      id: "./", name: "Capy Canvas", short_name: "Capy Canvas",
      description: "A GPU-powered drawing workspace.", start_url: "./", scope: "./",
      display: "standalone", background_color: "#333333", theme_color: "#333333",
      icons: [192, 512].map((size) => ({ src: asset(`icon-${size}.png`), sizes: `${size}x${size}`, type: "image/png", purpose: "any" })),
    }, null, 2) + "\n");
    const metadata = `<link rel="manifest" href="./manifest.webmanifest" />
    <link rel="apple-touch-icon" sizes="180x180" type="image/png" href="${appleIcon}" />
    <meta name="apple-mobile-web-app-title" content="Capy Canvas" />
    <link rel="license" href="./licenses.html" />`;
    writeFileSync(join(site, "index.html"), read(join(web, "index.html"))
      .replace('href="data:,"', `type="image/png" sizes="32x32" href="${asset("icon-32.png")}"`)
      .replace('href="style.css"', `href="${asset("style.css")}"`)
      .replace('src="app.js"', `src="${asset("app.js")}"`)
      .replace("<!-- Packager inserts install metadata here; development never registers a worker. -->", metadata)
      .replace("</body>", `<script>addEventListener("load", () => { if ("serviceWorker" in navigator) navigator.serviceWorker.register("./sw.js", {updateViaCache: "none"}).catch(console.error); });</script>\n  </body>`));
    writeFileSync(join(site, ".nojekyll"), "");
    const { files } = writeWorker(site);
    mkdirSync(dirname(output), { recursive: true });
    const previous = join(staging, "previous");
    if (existsSync(output)) renameSync(output, previous);
    try { renameSync(site, output); }
    catch (error) {
      if (existsSync(previous)) renameSync(previous, output);
      throw error;
    }
    console.log(`Static PWA: ${output} (${files.length} precached files)`);
    return output;
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) packageWeb();
