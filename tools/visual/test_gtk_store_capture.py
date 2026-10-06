import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("gtk_store_capture", Path(__file__).with_name("gtk-store-capture.py"))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="capy-store-wrapper-test-")
        self.root = Path(self.temporary.name)
        self.source = self.root / "art.png"
        self.source.write_bytes(b"original source")
        self.recipe = self.root / "recipe.json"
        self.scene = dict(id="paint", source="art.png", workspace="illustrator", steps=[{"type": "set_color", "rgba": [0, .5, 1, 1]}])

    def tearDown(self):
        self.temporary.cleanup()

    def write_recipe(self, scenes=None, **extra):
        self.recipe.write_text(json.dumps(dict(scenes=scenes or [self.scene], **extra)))

    def test_recipe_resolves_sources_and_preserves_native_steps(self):
        self.write_recipe()
        resolved, digest = capture.scene_recipe(self.recipe)
        self.assertEqual(resolved["source"], str(self.source))
        self.assertEqual(resolved["steps"], self.scene["steps"])
        self.assertEqual(len(digest), 64)
        self.assertEqual(self.source.read_bytes(), b"original source")

    def test_multiscene_requires_unique_explicit_selection(self):
        self.write_recipe([self.scene, dict(self.scene, id="photo", workspace="photographer")])
        with self.assertRaises(ValueError):
            capture.scene_recipe(self.recipe)
        self.assertEqual(capture.scene_recipe(self.recipe, "photo")[0]["id"], "photo")
        with self.assertRaises(ValueError):
            capture.scene_recipe(self.recipe, "missing")
        self.write_recipe([self.scene, self.scene])
        with self.assertRaises(ValueError):
            capture.scene_recipe(self.recipe, "paint")

    def test_recipe_rejects_unknown_fields_unsafe_ids_and_missing_sources(self):
        self.write_recipe(version=1)
        with self.assertRaises(ValueError):
            capture.scene_recipe(self.recipe)
        for change in [dict(id="../paint"), dict(workspace="unknown"), dict(steps={}), dict(unrecognized=True)]:
            self.write_recipe([dict(self.scene, **change)])
            with self.assertRaises(ValueError):
                capture.scene_recipe(self.recipe)
        self.write_recipe([dict(self.scene, source="missing.png")])
        with self.assertRaises(FileNotFoundError):
            capture.scene_recipe(self.recipe)

    def test_output_refuses_existing_content_and_symlink(self):
        self.root.joinpath("keep.txt").write_text("keep")
        with self.assertRaises(ValueError):
            capture.prepare_output(self.root)
        self.assertEqual(self.root.joinpath("keep.txt").read_text(), "keep")
        link = self.root / "link"
        link.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(ValueError):
            capture.prepare_output(link)
        output = capture.prepare_output(self.root / "output")
        self.assertTrue((output / "session/home").is_dir())
        with self.assertRaises(ValueError):
            capture.prepare_output(output)

    def test_size_bounds_and_cargo_test_executable_selection(self):
        capture.validate_size(1200, 800, 2)
        for values in [(0, 800, 2), (1200, 0, 2), (1200, 800, 0), (1200, 800, 5), (5000, 800, 2)]:
            with self.assertRaises(ValueError):
                capture.validate_size(*values)
        executable = self.root / "test-bin"
        executable.write_bytes(b"test binary")
        messages = "\n".join(json.dumps(dict(reason="compiler-artifact", target=dict(name=name), profile=dict(test=test), executable=str(executable)))
                             for name, test in [("unrelated", True), ("layer_linux", False), ("layer_linux", True)])
        self.assertEqual(capture.artifact_executable(messages), executable)
        with self.assertRaises(ValueError):
            capture.artifact_executable("no test artifact")

    def test_namespace_masks_home_runtime_and_binds_inputs_readonly(self):
        output = capture.prepare_output(self.root / "output")
        executable = self.root / "test-bin"
        args = capture.namespace(output, executable, [self.source], ["native-test"])
        home = ["--bind", str(output / "session/home"), str(Path.home().resolve())]
        self.assertIn(home, [args[i:i + 3] for i in range(len(args) - 2)])
        for source in [capture.ROOT, executable, self.source]:
            self.assertIn(["--ro-bind", str(source), str(source)], [args[i:i + 3] for i in range(len(args) - 2)])
        self.assertIn(["--tmpfs", "/run/user"], [args[i:i + 2] for i in range(len(args) - 1)])
        self.assertIn(["--setenv", "GSETTINGS_BACKEND", "memory"], [args[i:i + 3] for i in range(len(args) - 2)])
        self.assertEqual(args[-1], "native-test")
        self.assertNotIn("--bind", args[:3])

    def manifest_fixture(self):
        output = capture.prepare_output(self.root / "output")
        (output / "images").mkdir()
        image = output / "images/paint-en-light.png"
        image.write_bytes(b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", 2, 2) + bytes([8, 6, 0, 0, 0]) + b"\0" * 4)
        sidecar = output / "images/paint-en-light.json"
        sidecar.write_text(json.dumps(dict(dimensions=[2, 2], alpha=dict(transparent=1, partial=0, opaque=3, corners=[0, 0, 0, 0]))))
        job = dict(scenes=[self.scene], languages=["en"], themes=["light"], width=1200, height=800, scale=2, source_revision="a" * 40,
                   source_dirty=False, executable_sha256="c" * 64, executable_override=False, recipe_sha256="b" * 64)
        manifest = dict(job, supported_languages=["en", "ja"], captures=[dict(scene="paint", language="en", theme="light", image="images/paint-en-light.png", sidecar="images/paint-en-light.json")])
        (output / "capture.json").write_text(json.dumps(manifest))
        return output, job, manifest

    def test_manifest_checks_variant_provenance_and_alpha(self):
        output, job, manifest = self.manifest_fixture()
        self.assertEqual(capture.check_manifest(output, job), manifest)
        for key, value in [("recipe_sha256", "wrong"), ("executable_sha256", "wrong"), ("source_dirty", True), ("languages", ["ja"]), ("captures", [])]:
            (output / "capture.json").write_text(json.dumps(dict(manifest, **{key: value})))
            with self.assertRaises(ValueError):
                capture.check_manifest(output, job)
        (output / "capture.json").write_text(json.dumps(manifest))
        sidecar = output / manifest["captures"][0]["sidecar"]
        sidecar.write_text(json.dumps(dict(dimensions=[2, 2], alpha=dict(transparent=0, partial=0, opaque=4, corners=[255] * 4))))
        with self.assertRaises(ValueError):
            capture.check_manifest(output, job)

    def test_manifest_rejects_input_escape_and_missing_sidecar(self):
        output, job, manifest = self.manifest_fixture()
        manifest["captures"][0]["image"] = str(self.source)
        (output / "capture.json").write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            capture.check_manifest(output, job)
        manifest["captures"][0]["image"] = "images/paint-en-light.png"
        manifest["captures"][0]["sidecar"] = "images/missing.json"
        (output / "capture.json").write_text(json.dumps(manifest))
        with self.assertRaises(FileNotFoundError):
            capture.check_manifest(output, job)


if __name__ == "__main__":
    unittest.main()
