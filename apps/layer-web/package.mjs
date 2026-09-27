// Build-only static distribution. No npm bundler, deployment, or Git writes.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, extname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { dependencyNotices } from "../../tools/build/dependency-notices.mjs";
export { dependencyNotices };

const web = dirname(fileURLToPath(import.meta.url));
const root = resolve(web, "../..");
const digest = (data) => createHash("sha256").update(data);
const read = (path) => readFileSync(path, "utf8");
const escape = (text) => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
const page = (title, body) => `<!doctype html><html lang="en"><meta charset="utf-8"><title>${title}</title>
<style>body{max-width:70rem;margin:2rem auto;padding:0 1rem;font:16px system-ui}pre{white-space:pre-wrap}</style><h1>${title}</h1>${body}</html>`;
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

// Relative references the browser resolves against a module: static, side-effect
// and dynamic imports, and URLs such as workers and Wasm. Bare specifiers and
// absolute URLs are not package files.
const moduleReferences = [
  /(\b(?:from|import)\s*\(?\s*)(["'])(\.\.?\/[^"'\n]*)\2/g,
  /(\bnew URL\(\s*)(["'])((?![a-zA-Z][\w+.-]*:|\/)[^"'\n]+)\2(?=\s*,\s*import\.meta\.url\s*\))/g,
];

// Hash final bytes after rewriting dependencies; no bundler required. The module
// graph is read from the modules, as the browser follows it, so new modules and
// imports package without changes here; an unresolved reference fails the build.
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
  for (const path of files) if (!path.endsWith(".js") && path !== "style.css") publish(path);
  const css = read(join(directory, "style.css")).replace(/url\((["']?)([^"')]+)\1\)/g, (reference, _quote, path) => {
    if (/^(data:|https?:|\/|#)/.test(path)) return reference;
    const name = names[path.replace(/^\.\//, "")];
    if (!name) throw new Error(`Missing CSS asset: ${path}`);
    return `url(${JSON.stringify(name)})`;
  });
  publish("style.css", css);
  // app.js looks up artwork, brush previews and Wasm by their development paths.
  const lookups = JSON.stringify(Object.fromEntries(Object.entries(names).filter(([path]) => path !== "style.css")));
  const sources = new Map(files.filter(path => path.endsWith(".js")).map(path => [path, read(join(directory, path))]));
  const target = (path, specifier) => posix.normalize(posix.join(posix.dirname(path), specifier));
  // Publish each module after its dependencies, so its references name final hashes.
  const done = new Set(), visiting = [];
  const visit = (path) => {
    if (done.has(path)) return;
    if (visiting.includes(path))
      throw new Error(`Module cycle cannot be fingerprinted: ${[...visiting.slice(visiting.indexOf(path)), path].join(" -> ")}`);
    visiting.push(path);
    for (const pattern of moduleReferences) for (const [, , , specifier] of sources.get(path).matchAll(pattern)) {
      const dependency = target(path, specifier);
      if (sources.has(dependency)) visit(dependency);
      else if (!names[dependency]) throw new Error(`${path} references missing ${specifier}`);
    }
    visiting.pop();
    let text = sources.get(path);
    for (const pattern of moduleReferences) text = text.replace(pattern, (_, prefix, quote, specifier) =>
      prefix + quote + specifier.slice(0, -basename(specifier).length) + basename(names[target(path, specifier)]) + quote);
    if (path === "app.js") text = replaceRequired(text, "const assetPaths = {};", `const assetPaths = ${lookups};`);
    publish(path, text);
    done.add(path);
  };
  for (const path of sources.keys()) visit(path);
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
  throw new Error(`Missing ${name}; see docs/development/web-packaging.md for build prerequisites`);
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
    // Stage every runtime module; fingerprinting follows their references.
    for (const path of [...readdirSync(web).filter(path => path.endsWith(".js") && path !== "sw.js"), "style.css"])
      cpSync(join(web, path), join(runtime, path));
    for (const directory of ["icons", "brush-previews"]) {
      mkdirSync(join(runtime, directory));
      for (const path of readdirSync(join(web, directory)).sort()) {
        if (path.endsWith(directory === "icons" ? ".svg" : ".png"))
          cpSync(join(web, directory, path), join(runtime, directory, path));
      }
    }
    const brand = read(join(web, "icons/layer-zen-looking-up-symbolic.svg"));
    for (const size of [32, 180, 192, 512]) {
      // Favicon and installation icons share the approved enlarged artwork.
      const markSize = 440;
      const inset = (512 - markSize) / 2;
      const mark = brand.replace('width="24" height="24"',
        `x="${inset}" y="${inset}" width="${markSize}" height="${markSize}" color="#f6f5f4"`);
      // Apple masks artwork itself; other platforms retain rounded corners.
      const corners = size === 180 ? "" : ' rx="76.8"';
      const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><rect width="512" height="512"${corners} fill="#767676"/>${mark}</svg>`;
      // File input avoids renderer stdin/EOF stalls in constrained build hosts.
      const source = join(staging, "icon.svg");
      writeFileSync(source, svg);
      run(resvg, ["--resources-dir", web, "--width", String(size), "--height", String(size), source, join(runtime, `icon-${size}.png`)]);
    }
    const names = fingerprintAssets(runtime);
    const asset = (path) => {
      if (!names[path]) throw new Error(`Missing package asset: ${path}`);
      return `assets/${names[path]}`;
    };
    // Apple's out-of-page icon lookup also needs a conventional stable URL.
    // A query fingerprint refreshes explicit links while older saved URLs still
    // work after deployment removes a previous release's hashed asset files.
    cpSync(join(runtime, names["icon-180.png"]), join(site, "apple-touch-icon.png"));
    const appleIcon = `./apple-touch-icon.png?v=${names["icon-180.png"].split(".")[1]}`;

    const notices = ["LICENSE", "LICENSE-MIT", "LICENSE-APACHE", "BRANDING.md", "THIRD_PARTY_NOTICES.md"];
    for (const path of notices) cpSync(join(root, path), join(site, path));
    const licensing = JSON.parse(run(about, ["generate", "--locked", "--fail", "--manifest-path", join(web, "Cargo.toml"),
      "--config", join(root, "tools/build/about.toml"), "--target", "wasm32-unknown-unknown", "--format", "json"], { encoding: "utf8", maxBuffer: 32 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] }));
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
      file_handlers: [{action: "./", accept: {"application/octet-stream": [".capy"], "image/png": [".png"], "image/jpeg": [".jpg", ".jpeg"], "image/tiff": [".tif", ".tiff"], "image/avif": [".avif"], "image/x-exr": [".exr"]}}],
      display: "standalone", background_color: "#333333", theme_color: "#333333",
      icons: [192, 512].map((size) => ({ src: asset(`icon-${size}.png`), sizes: `${size}x${size}`, type: "image/png", purpose: "any" })),
    }, null, 2) + "\n");
    // Discover the whole UI module graph from the HTML, instead of waiting for
    // successive import fetches through the service worker on each navigation.
    const preloads = Object.keys(names).filter(path => path.endsWith(".js") &&
      path !== "app.js" && !path.endsWith("-worker.js")).map(path =>
      `<link rel="modulepreload" href="${asset(path)}" />`).join("\n    ");
    const metadata = `${preloads}
    <link rel="manifest" href="./manifest.webmanifest" />
    <link rel="apple-touch-icon" sizes="180x180" type="image/png" href="${appleIcon}" />
    <meta name="apple-mobile-web-app-title" content="Capy Canvas" />
    <link rel="license" href="./licenses.html" />`;
    // Page scripts and styles name runtime files; the preload script's list and
    // the insertion points must still match, or the build fails.
    let html = read(join(web, "index.html")).replace(/\b(src|href)="([^":#]+)"/g,
      (reference, attribute, path) => names[path] ? `${attribute}="${asset(path)}"` : reference);
    for (const [from, to] of [
      ['href="data:,"', `type="image/png" sizes="32x32" href="${asset("icon-32.png")}"`],
      ['["icons.svg", "pkg/layer_web_bg.wasm"]', JSON.stringify([asset("icons.svg"), asset("pkg/layer_web_bg.wasm")])],
      ["<!-- Packager inserts install metadata here; development never registers a worker. -->", metadata],
      ["</body>", `<script>addEventListener("load", () => { if ("serviceWorker" in navigator) navigator.serviceWorker.register("./sw.js", {updateViaCache: "none"}).catch(console.error); });</script>\n  </body>`],
    ]) html = replaceRequired(html, from, to);
    for (const [, path] of html.matchAll(/\b(?:src|href)="(?![a-z][\w+.-]*:|#)([^"?#]+)/g))
      if (!existsSync(join(site, path))) throw new Error(`index.html references unpublished ${path}`);
    writeFileSync(join(site, "index.html"), html);
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
