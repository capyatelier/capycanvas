import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { preparePackageOutput, verifyPortablePhotoPackage } from "./package-files.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "capy-native-package-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test("staging clears old codecs only from an owned generated directory", t => {
  const root = fixture(t), output = join(root, "package");
  mkdirSync(output);
  writeFileSync(join(output, "keep"), "unrelated content");
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(output, "keep"), "utf8"), "unrelated content");
  writeFileSync(join(output, ".capy-package"), "Generated Capy Canvas native package\n");
  mkdirSync(join(output, "lib/capycanvas/photo"), { recursive: true });
  writeFileSync(join(output, "lib/capycanvas/photo/libcapy_photo.so.1"), "old codec");
  preparePackageOutput(output);
  assert.ok(!existsSync(join(output, "keep")));
  assert.ok(!existsSync(join(output, "lib")));
  assert.ok(existsSync(join(output, ".capy-package")));
  verifyPortablePhotoPackage(output);
});

test("staging refuses a package symlink and preserves its target", t => {
  const root = fixture(t), target = join(root, "target"), output = join(root, "package");
  preparePackageOutput(target);
  writeFileSync(join(target, "keep"), "content");
  symlinkSync(target, output);
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(target, "keep"), "utf8"), "content");
});

test("payload validation rejects old bundles and relocated codec binaries", t => {
  const output = fixture(t);
  mkdirSync(join(output, "lib/capycanvas/photo"), { recursive: true });
  assert.throws(() => verifyPortablePhotoPackage(output), /Obsolete/);
  rmSync(join(output, "lib"), { recursive: true });
  for (const name of ["capy-hdr-codec", "libcapy_photo.so.1", "libheif.so.1", "libjpeg.so.62"]) {
    writeFileSync(join(output, name), "codec");
    assert.throws(() => verifyPortablePhotoPackage(output), /Native photo codec/);
    rmSync(join(output, name));
  }
  writeFileSync(join(output, "libgtk-4.so.1"), "platform UI");
  verifyPortablePhotoPackage(output);
});
