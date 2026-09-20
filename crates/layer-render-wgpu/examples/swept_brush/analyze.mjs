// node analyze.mjs artifacts/swept-brush/run1/samples.csv [...more paired runs]
import {readFileSync} from 'node:fs';

const paths = process.argv.slice(2);
if (!paths.length) throw new Error('Usage: node analyze.mjs CSV [CSV ...]');
function quantile(rows, key, q) {
  const values = rows.map(r => Number(r[key])).sort((a,b) => a-b);
  return values[Math.max(0, Math.ceil(q * values.length) - 1)];
}
// Keep runs separate: pooling medians across different GPU clock states can
// produce a ratio that represents neither run.
for (const path of paths) {
  const groups = new Map();
  const [header, ...lines] = readFileSync(path, 'utf8').trim().split('\n');
  const keys = header.split(',');
  for (const line of lines) {
    const row = Object.fromEntries(line.split(',').map((v, i) => [keys[i], v]));
    const key = [row.brush, row.diameter, row.path].join('/');
    const group = groups.get(key) ?? {};
    (group[row.variant] ??= []).push(row);
    groups.set(key, group);
  }
  console.log(`\n${path}\n`);
  console.log('| Brush/size/path | Contacts → segments | Contact GPU median | Simple GPU median | Merged GPU median | Merged p95 | Speedup |');
  console.log('| --- | ---: | ---: | ---: | ---: | ---: | ---: |');
  for (const [key, g] of groups) {
    if (!g.contact || !g.simple || !g.merged) throw new Error(`Incomplete pair: ${key}`);
    const a = quantile(g.contact, 'gpu_ms', .5);
    const b = quantile(g.simple, 'gpu_ms', .5);
    const c = quantile(g.merged, 'gpu_ms', .5);
    const p95 = quantile(g.merged, 'gpu_ms', .95);
    console.log(`| ${key} | ${g.contact[0].contacts} → ${g.merged[0].contacts} | ${a.toFixed(3)} ms | ${b.toFixed(3)} ms | ${c.toFixed(3)} ms | ${p95.toFixed(3)} ms | ${(a/c).toFixed(2)}× |`);
  }
}
