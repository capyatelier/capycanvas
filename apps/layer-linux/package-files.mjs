// Generated-package ownership and payload checks, independent of a GTK build.
import { existsSync, lstatSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export function preparePackageOutput(output) {
  const marker = join(output, ".capy-package");
  if (existsSync(output)) {
    if (!lstatSync(output).isDirectory() || !existsSync(marker) || !lstatSync(marker).isFile())
      throw new Error("Refusing to overwrite an unmarked native package directory");
    // A generated directory can contain obsolete codecs from an older build.
    rmSync(output, { recursive: true });
  }
  mkdirSync(output, { recursive: true });
  writeFileSync(marker, "Generated Capy Canvas native package (build in progress)\n");
  return marker;
}

export function verifyPortablePhotoPackage(output) {
  for (const path of ["lib/capycanvas/photo", "share/doc/capycanvas-photo-codecs"])
    if (existsSync(join(output, path))) throw new Error(`Obsolete photo codec bundle in package: ${path}`);
  const visit = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (/^(?:capy-hdr-codec|hdr-codec-abi)$|^lib(?:capy_photo|heif|de265|dav1d|avif|aom|uhdr|turbojpeg|jpeg)(?:\.|-)/.test(entry.name))
        throw new Error(`Native photo codec payload in package: ${entry.name}`);
      if (entry.isDirectory()) visit(join(directory, entry.name));
    }
  };
  visit(output);
}
