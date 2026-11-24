import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import bench_fixtures


class EnsureFixturesTest(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        self.runs = 0
        self.generator = patch.object(bench_fixtures, 'generator', return_value=Path('bench_fixtures'))
        for patcher in [patch.object(bench_fixtures, 'FIXTURES', self.root), patch.object(bench_fixtures, 'GIGAPIXEL_FIXTURES', self.root / 'gigapixel'),
                        self.generator, patch('bench_fixtures.subprocess.run', side_effect=self.generate),
                        patch.object(bench_fixtures, 'jpeg', side_effect=lambda png, path: path.write_text(f'jpeg of {png.read_text()}'))]:
            patcher.start()
            self.addCleanup(patcher.stop)

    def generate(self, command, **_):
        self.runs += 1
        names = [Path(name).with_suffix('.png').name if name.endswith('.jpg') else name for name in bench_fixtures.GIGAPIXEL.values()] \
            if command[2:] == ['gigapixel'] else [f'{name}.capy' for name in bench_fixtures.NAMES]
        for name in names:
            Path(command[1], name).write_text(f'{Path(name).stem} {self.runs}')

    def manifest(self):
        return json.loads((self.root / '64x48' / 'manifest.json').read_text())

    def test_generates_missing_fixtures_and_records_their_hashes(self):
        paths = bench_fixtures.ensure_fixtures(64, 48)
        self.assertEqual(paths, {name: self.root / '64x48' / f'{name}.capy' for name in ('heavy', 'vector')})
        self.assertEqual(self.manifest(), {path.name: bench_fixtures.sha256(path) for path in paths.values()})
        self.assertEqual(sorted(os.listdir(self.root / '64x48')), ['heavy.capy', 'manifest.json', 'vector.capy'])

    def test_keeps_fixtures_whose_hashes_match(self):
        bench_fixtures.ensure_fixtures(64, 48)
        bench_fixtures.ensure_fixtures(64, 48)
        self.assertEqual(self.runs, 1)

    def test_regenerates_only_changed_or_unrecorded_fixtures(self):
        paths = bench_fixtures.ensure_fixtures(64, 48)
        paths['vector'].write_text('edited')
        bench_fixtures.ensure_fixtures(64, 48)
        self.assertEqual([paths['heavy'].read_text(), paths['vector'].read_text()], ['heavy 1', 'vector 2'])
        manifest = self.manifest()
        del manifest['heavy.capy']
        (self.root / '64x48' / 'manifest.json').write_text(json.dumps(manifest))
        bench_fixtures.ensure_fixtures(64, 48)
        self.assertEqual([paths['heavy'].read_text(), paths['vector'].read_text(), self.runs], ['heavy 3', 'vector 2', 3])
        self.assertEqual(self.manifest(), {path.name: bench_fixtures.sha256(path) for path in paths.values()})

    def test_gigapixel_set_builds_in_its_own_patched_tree_and_is_kept_once_hashed(self):
        paths = bench_fixtures.ensure_gigapixel()
        self.assertEqual(paths, {name: self.root / 'gigapixel' / file for name, file in
                                 [('giga-square', 'giga-square.jpg'), ('giga-panorama', 'giga-panorama.png'), ('giga-layers', 'giga-layers.capy')]})
        self.assertEqual(paths['giga-square'].read_text(), 'jpeg of giga-square 1')
        self.assertEqual(sorted(os.listdir(self.root / 'gigapixel')), ['giga-layers.capy', 'giga-panorama.png', 'giga-square.jpg', 'manifest.json'])
        bench_fixtures.generator.assert_called_once_with(bench_fixtures.GIGAPIXEL_SOURCE, bench_fixtures.GIGAPIXEL_PATCH)
        self.assertNotEqual(bench_fixtures.GIGAPIXEL_SOURCE, bench_fixtures.SOURCE)
        self.assertTrue(bench_fixtures.GIGAPIXEL_PATCH.is_file())
        self.assertEqual(bench_fixtures.fixture('giga-layers', 64, 48), paths['giga-layers'])
        self.assertEqual(self.runs, 1)
        self.assertEqual(bench_fixtures.fixture('heavy', 64, 48), self.root / '64x48' / 'heavy.capy')
        self.assertEqual([bench_fixtures.file_name(name) for name in ('giga-square', 'giga-panorama', 'giga-layers', 'vector')],
                         ['giga-square.jpg', 'giga-panorama.png', 'giga-layers.capy', 'vector.capy'])


if __name__ == '__main__':
    unittest.main()
