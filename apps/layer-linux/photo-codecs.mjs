// Validate and stage the replaceable native photo decoder libraries. The build
// recipe and corresponding upstream sources travel with the package.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, realpathSync } from "node:fs";
import { join, resolve, sep } from "node:path";

const digest = path => createHash("sha256").update(readFileSync(path)).digest("hex");
export function verifyPhotoCodecs(root) {
  const prefix = resolve(process.env.CAPY_PHOTO_CODEC_PREFIX || join(root, "target/photo-codecs/prefix"));
  if (!existsSync(join(prefix, "manifest.json"))) {
    throw new Error("Build the photo codec bundle first: python3 tools/build/photo-codecs.py --fetch");
  }
  const manifest = JSON.parse(readFileSync(join(prefix, "manifest.json"), "utf8"));
  const lock = JSON.parse(readFileSync(join(root, "tools/build/photo-codecs.json"), "utf8"));
  if (JSON.stringify(manifest.dependencies) !== JSON.stringify(lock)
      || manifest.recipe_sha256 !== digest(join(root, "tools/build/photo-codecs.py"))
      || manifest.bridge_sha256 !== digest(join(root, "crates/layer-color/src/photo/heif_bridge.c"))
      || manifest.patch_sha256 !== digest(join(root, "vendor/libheif-source-profile.patch"))) {
    throw new Error("The photo codec bundle is stale; rebuild it before packaging");
  }
  const lib = realpathSync(join(prefix, "lib"));
  const files = readdirSync(lib).filter(name => name.includes(".so"));
  files.push("capy-hdr-codec", "hdr-codec-abi");
  if (digest(join(lib,"capy-hdr-codec")) !== manifest.hdr_worker_sha256
      || readFileSync(join(lib,"hdr-codec-abi"),"utf8") !== "1\n"
      || digest(join(prefix,"share/doc/capycanvas-photo-codecs/hdr_codec.cpp")) !== digest(join(root,"crates/layer-color/src/photo/hdr_codec.cpp"))) {
    throw new Error("The HDR codec worker is missing or stale; rebuild the bundle");
  }
  if (execFileSync(join(lib,"capy-hdr-codec"),["--version"],{encoding:"utf8"}).trim() !== "capy-hdr-codec 1") throw new Error("Unsupported HDR codec worker");
  for (const name of files) {
    if (!realpathSync(join(lib, name)).startsWith(lib + sep)) throw new Error("Photo codec symlink leaves the bundle");
  }
  for (const [name, expected] of Object.entries(manifest.libraries)) {
    if (name !== name.split(/[\\/]/).pop() || digest(join(lib, name)) !== expected) {
      throw new Error(`Photo codec checksum mismatch: ${name}`);
    }
  }
  for (const [name, spec] of Object.entries(lock)) {
    const archive = join(prefix, "share/doc/capycanvas-photo-codecs/sources", spec.archive);
    if (digest(archive) !== spec.sha256) throw new Error(`Missing corresponding codec source: ${name}`);
  }
  // Actual loading checks dependencies and both decoding backends, not names.
  execFileSync("python3", ["-c", `
import ctypes, sys
lib = ctypes.CDLL(sys.argv[1])
lib.capy_photo_version.restype = ctypes.c_char_p
lib.capy_photo_avif_version.restype = ctypes.c_char_p
assert lib.capy_photo_abi() == 3
assert lib.capy_photo_version().decode() == sys.argv[2]
assert lib.capy_photo_avif_version().decode() == sys.argv[3]
assert lib.capy_photo_decoder(1) and lib.capy_photo_decoder(4)
`, join(lib, "libcapy_photo.so.1"), lock.libheif.version, lock.libavif.version], { stdio: "inherit" });
  return { prefix, files };
}

export function stagePhotoCodecs(bundle, output) {
  const library = join(output, "lib/capycanvas/photo");
  mkdirSync(library, { recursive: true });
  for (const name of bundle.files) {
    cpSync(join(bundle.prefix, "lib", name), join(library, name), { verbatimSymlinks: true });
  }
  const docs = join(output, "share/doc/capycanvas-photo-codecs");
  cpSync(join(bundle.prefix, "share/doc/capycanvas-photo-codecs"), docs, { recursive: true, verbatimSymlinks: true });
  cpSync(join(bundle.prefix, "manifest.json"), join(docs, "manifest.json"));
}
