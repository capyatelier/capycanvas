// Generated-package ownership checks, independent of a GTK build.
import { existsSync, lstatSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export function preparePackageOutput(output) {
  const marker = join(output, ".capy-package");
  if (existsSync(output)) {
    if (!lstatSync(output).isDirectory() || !existsSync(marker) || !lstatSync(marker).isFile())
      throw new Error("Refusing to overwrite an unmarked native package directory");
    rmSync(output, { recursive: true });
  }
  mkdirSync(output, { recursive: true });
  writeFileSync(marker, "Generated Capy Canvas native package (build in progress)\n");
  return marker;
}
