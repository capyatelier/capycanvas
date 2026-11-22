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
        for patcher in [patch.object(bench_fixtures, 'FIXTURES', self.root),
                        patch.object(bench_fixtures, 'generator', return_value=Path('bench_fixtures')),
                        patch('bench_fixtures.subprocess.run', side_effect=self.generate)]:
            patcher.start()
            self.addCleanup(patcher.stop)

    def generate(self, command, **_):
        self.runs += 1
        for name in bench_fixtures.NAMES:
            Path(command[1], f'{name}.capy').write_text(f'{name} {self.runs}')

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


if __name__ == '__main__':
    unittest.main()
