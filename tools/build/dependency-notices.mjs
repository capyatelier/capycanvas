// Shared build-time rendering of exact Rust dependency license notices.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const digest = data => createHash("sha256").update(data);
const escape = text => text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

export function dependencyNotices(licenses) {
  return licenses.map((license) => {
    // Workspace licensing is included separately with the branding exception.
    // Patched third-party crates are local paths too and retain their notices.
    let crates = license.used_by.map((used) => used.crate).filter((crate) => crate.source
      || (crate.manifest_path && resolve(crate.manifest_path).startsWith(join(root, "vendor") + sep)));
    let original = "";
    // This published archive has neither notices nor repository metadata, so
    // cargo-about cannot fetch its git clarification. Preserve the exact upstream
    // Zlib alternative, checked against the recorded publication revision.
    if (!license.source_path) {
      const pinned = [
        ["zune-core", "0.4.12", "f8fbb123d5ed04441e8324a555bfcda0cb1bd28f"],
        ["zune-inflate", "0.2.54", "69502ce83fdfecdd0beefd677e2abb3781b29d98"],
      ];
      for (const [name, version, revision] of pinned) {
        const missing = crates.filter(crate => crate.name === name);
        if (!missing.length) continue;
        if (missing.some(crate => crate.version !== version || crate.license !== "MIT OR Apache-2.0 OR Zlib"))
          throw new Error(`Revalidate the original ${name} notice for this release`);
        const notice = readFileSync(join(root, "tools/build/licenses/zune-core-0.4.12-ZLIB.txt"));
        // Both pinned publication revisions contain this identical original.
        if (digest(notice).digest("hex") !== "7fa429541e55b1509909e058f2d21a37467e4958ec713b357f6e0cf9dc4ee352")
          throw new Error(`Original ${name} notice checksum differs`);
        const source = `https://github.com/etemesi254/zune-image/blob/${revision}/LICENSE-ZLIB`;
        original += `<section><h2>Zlib License</h2><p>${name} ${version} · <a href="${source}">Original notice</a></p><pre>${escape(notice.toString("utf8"))}</pre></section>`;
        crates = crates.filter(crate => crate.name !== name);
      }
    }
    if (!crates.length) return original;
    if (!license.source_path || /<year>|<copyright holders>/i.test(license.text))
      throw new Error(`Missing original license notice: ${crates.map((crate) => crate.name).join(", ")}`);
    return original + `<section><h2>${escape(license.name)}</h2><ul>${crates.map((crate) => `<li>${escape(crate.name)} ${escape(crate.version)}</li>`).join("")}</ul><pre>${escape(license.text)}</pre></section>`;
  }).join("\n");
}
