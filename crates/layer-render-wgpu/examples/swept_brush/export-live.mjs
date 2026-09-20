// Strip unrelated app state/process dumps, retain every timing sample, compress.
// node export-live.mjs OUTPUT_DIR WIDE_BRUSH_JSON [WIDE_BRUSH_JSON ...]
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {basename, join} from 'node:path';
import {gzipSync} from 'node:zlib';
const [directory, ...paths] = process.argv.slice(2);
if (!directory || !paths.length) throw new Error('OUTPUT_DIR and input JSON required');
mkdirSync(directory, {recursive: true});
for (const path of paths) {
  const r = JSON.parse(readFileSync(path, 'utf8'));
  const keep = (object, keys) => Object.fromEntries(keys.map(k => [k, object[k]]));
  const clean = {...keep(r, ['device', 'algorithm', 'preset', 'pressure', 'turns_per_second']),
    diameter: r.state.brush.diameter,
    runs: r.runs.map(run => ({...keep(run, ['failure', 'camera', 'input_origin',
      'swept_counts', 'tracked_canvas_bytes', 'process_pss_bytes', 'process_mappings']),
      timeline: keep(run.timeline, ['frames', 'frame_fields', 'inputs', 'input_fields']),
      renderer_stats: keep(run.renderer_stats, ['samples', 'gpu_samples', 'rows'])}))};
  writeFileSync(join(directory, basename(path) + '.gz'), gzipSync(JSON.stringify(clean), {level: 9}));
}
