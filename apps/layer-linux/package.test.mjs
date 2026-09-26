import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { preparePackageOutput } from "./package-files.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "capy-native-package-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

test("staging only clears an owned generated directory", t => {
  const root = fixture(t), output = join(root, "package");
  mkdirSync(output);
  writeFileSync(join(output, "keep"), "unrelated content");
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(output, "keep"), "utf8"), "unrelated content");
  writeFileSync(join(output, ".capy-package"), "Generated Capy Canvas native package\n");
  preparePackageOutput(output);
  assert.ok(!existsSync(join(output, "keep")));
  assert.ok(existsSync(join(output, ".capy-package")));
});

test("staging refuses a package symlink and preserves its target", t => {
  const root = fixture(t), target = join(root, "target"), output = join(root, "package");
  preparePackageOutput(target);
  writeFileSync(join(target, "keep"), "content");
  symlinkSync(target, output);
  assert.throws(() => preparePackageOutput(output), /unmarked/);
  assert.equal(readFileSync(join(target, "keep"), "utf8"), "content");
});
