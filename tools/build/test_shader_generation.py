"""Check build-time shader cache invalidation without compiling the renderer."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
INPUTS = (
    "crates/layer-render-wgpu/src/shader.wgsl",
    "crates/layer-render-wgpu/Cargo.toml",
    "crates/layer-core/src/color.rs",
    "crates/layer-shader-cache-key/src/lib.rs",
    "crates/layer-shader-cache-key/build.rs",
    "crates/layer-shader-cache-key/Cargo.toml",
    "Cargo.lock",
    "assets/filters/example.wgsl",
    "assets/filters/example.json",
)


class ShaderGenerationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.temp.cleanup)
        cls.script = Path(cls.temp.name) / "shader-generation"
        subprocess.run(["rustc", "--edition=2024",
                        str(ROOT / "crates/layer-shader-cache-key/build.rs"),
                        "-o", str(cls.script)], check=True)

    def setUp(self):
        self.fixture = tempfile.TemporaryDirectory()
        self.addCleanup(self.fixture.cleanup)
        self.root = Path(self.fixture.name)
        for name in INPUTS:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name)

    def generation(self, root=None):
        root = root or self.root
        output = subprocess.check_output([str(self.script)], text=True, env={
            **os.environ,
            "CARGO_MANIFEST_DIR": str(root / "crates/layer-shader-cache-key"),
        })
        for name in INPUTS:
            if (root / name).exists():
                self.assertIn(f"cargo:rerun-if-changed={root / name}\n", output)
        return output.split("cargo:rustc-env=CAPY_SHADER_GENERATION=")[1].strip()

    def test_every_input_invalidates_and_restoring_reuses_generation(self):
        original = self.generation()
        for name in INPUTS:
            with self.subTest(input=name):
                path = self.root / name
                path.write_text(name + " changed")
                self.assertNotEqual(original, self.generation())
                path.write_text(name)
                self.assertEqual(original, self.generation())

    def test_location_independent_and_content_based(self):
        copied = self.root / "copy"
        shutil.copytree(self.root, copied, ignore=shutil.ignore_patterns("copy"))
        self.assertEqual(self.generation(), self.generation(copied))
        for name in INPUTS:
            (self.root / name).touch()
        self.assertEqual(self.generation(), self.generation(copied))

    def test_directory_membership_invalidates(self):
        original = self.generation()
        path = self.root / "assets/filters/example.wgsl"
        renamed = path.with_name("renamed.wgsl")
        path.rename(renamed)
        self.assertNotEqual(original, self.generation())
        renamed.rename(path)
        self.assertEqual(original, self.generation())
        path.unlink()
        self.assertNotEqual(original, self.generation())
        path.write_text("assets/filters/example.wgsl")
        extra = path.with_name("added.wgsl")
        extra.write_text("new shader")
        self.assertNotEqual(original, self.generation())


if __name__ == "__main__":
    unittest.main()
