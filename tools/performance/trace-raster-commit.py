#!/usr/bin/env python3
"""Build isolated parent/current raster-commit phase probes; never edit the checkout.

Run the printed executables with CAPY_TRACE_COMMIT_PHASES=1. They report frame
encoding/submission and capture metadata/allocation/encoding/submission in us.
Newer renderers report CM2_COPY for the validated copy helper; CM2_FRAME
capture_us still includes the complete raster-index and capture work.
Tracing includes setup and warm-up; use the benchmark's measured window when
analyzing results. The probes are for diagnosis, not final latency qualification.
"""
import argparse
import io
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


def instrument(root):
    p=root/'crates/layer-render-wgpu/src/lib.rs';s=p.read_text()
    old='        let started = self.telemetry.enabled.then(web_time::Instant::now);'
    assert s.count(old)==1
    s=s.replace(old,'''        let cm2_trace = packet.dab_batches.iter().any(|b| b.stroke_end)
            && std::env::var_os("CAPY_TRACE_COMMIT_PHASES").is_some();
        let cm2_start = cm2_trace.then(std::time::Instant::now);
'''+old)
    old='''        self.uploads.finish(&encoder);
        self.telemetry.end(&mut encoder);
        let submission = encoder.submit(&self.queue);'''
    assert s.count(old)==1
    s=s.replace(old,'''        let cm2_encoded = cm2_start.map(|_| std::time::Instant::now());
'''+old+'''
        let cm2_submitted = cm2_start.map(|_| std::time::Instant::now());''')
    old='        self.commit_rasters(packet.layers)?;'
    assert s.count(old)==1
    s=s.replace(old,old+'''
        let cm2_captured = cm2_start.map(|_| std::time::Instant::now());''')
    old='''        self.refresh_storage_metrics();
        if let Some(started) = started {'''
    assert s.count(old)==1
    s=s.replace(old,'''        self.refresh_storage_metrics();
        if let Some(start) = cm2_start {
            let encoded=cm2_encoded.unwrap();let submitted=cm2_submitted.unwrap();let captured=cm2_captured.unwrap();
            eprintln!("CM2_FRAME encode_us={} submit_us={} capture_us={} post_us={}",
                (encoded-start).as_micros(),(submitted-encoded).as_micros(),(captured-submitted).as_micros(),captured.elapsed().as_micros());
        }
        if let Some(started) = started {''')
    p.write_text(s)
    p=root/'crates/layer-render-wgpu/src/raster.rs';s=p.read_text()
    if ' fn capture_tiles(' in s:
        p.write_text(instrument_native_capture(s))
        return
    a=s.index('    pub fn capture_raster(');b=s.index('    /// Restore changed pages',a)
    f=s[a:b];old='        let (textures, watercolor) = self.raster_textures(target);'
    f=f.replace(old,'''        let cm2_start = std::env::var_os("CAPY_TRACE_COMMIT_PHASES").is_some().then(std::time::Instant::now);
'''+old,1)
    f=f.replace('''        let mut encoder = self
''','''        let cm2_meta = cm2_start.map(|_| std::time::Instant::now());
        let mut cm2_alloc_us = 0;
        let mut encoder = self
''',1)
    f=f.replace('''            let buffer = self
                .raster_buffers
                .take(&self.device, size.next_power_of_two());''','''            let cm2_alloc = cm2_start.map(|_| std::time::Instant::now());
            let buffer = self.raster_buffers.take(&self.device, size.next_power_of_two());
            cm2_alloc_us += cm2_alloc.map_or(0, |t| t.elapsed().as_micros());''',1)
    f=f.replace('''        let submission = self.queue.submit([encoder.finish()]);''','''        let cm2_copied = cm2_start.map(|_| std::time::Instant::now());
        let command = encoder.finish();
        let cm2_finished = cm2_start.map(|_| std::time::Instant::now());
        let submission = self.queue.submit([command]);
        let cm2_submitted = cm2_start.map(|_| std::time::Instant::now());''',1)
    f=f.replace('''        Ok(Some(capture))''','''        if let Some(start) = cm2_start {
            let meta=cm2_meta.unwrap();let copied=cm2_copied.unwrap();let finished=cm2_finished.unwrap();let submitted=cm2_submitted.unwrap();
            eprintln!("CM2_CAPTURE bytes={total} tiles={} meta_us={} encode_us={} alloc_us={cm2_alloc_us} finish_us={} submit_us={} publish_us={}",copies.len(),
                (meta-start).as_micros(),(copied-meta).as_micros(),(finished-copied).as_micros(),(submitted-finished).as_micros(),submitted.elapsed().as_micros());
        }
        Ok(Some(capture))''',1)
    s=s[:a]+f+s[b:];p.write_text(s)


def instrument_native_capture(source):
    begin = source.rfind('\n', 0, source.index(' fn capture_tiles(')) + 1
    end = source.index('    /// Restore changed pages', begin)
    body = source[begin:end]
    old = '    ) -> Result<RasterCapture, GpuRasterError> {'
    assert body.count(old) == 1
    body = body.replace(old, old + """
        let cm2_start = std::env::var_os("CAPY_TRACE_COMMIT_PHASES").is_some().then(std::time::Instant::now);
        let mut cm2_alloc_us = 0u128;
""")
    old = '        let mut encoder = self\n'
    assert body.count(old) == 1
    body = body.replace(old, '        let cm2_meta = cm2_start.map(|_| std::time::Instant::now());\n' + old)
    for size in ['allocation', 'STATUS_BYTES']:
        old = f'            let buffer = self.raster_buffers.take(&self.device, {size});'
        assert body.count(old) == 1
        body = body.replace(old, '            let cm2_alloc = cm2_start.map(|_| std::time::Instant::now());\n' + old +
                            '\n            cm2_alloc_us += cm2_alloc.map_or(0, |t| t.elapsed().as_micros());')
    old = '        let submission = self.queue.submit([encoder.finish()]);'
    assert body.count(old) == 1
    body = body.replace(old, """        let cm2_copied = cm2_start.map(|_| std::time::Instant::now());
        let command = encoder.finish();
        let cm2_finished = cm2_start.map(|_| std::time::Instant::now());
        let submission = self.queue.submit([command]);
        let cm2_submitted = cm2_start.map(|_| std::time::Instant::now());""")
    assert body.count('        Ok(RasterCapture {') == 1
    body = body.replace('        Ok(RasterCapture {', '        let capture = RasterCapture {')
    tail = '        })\n    }\n\n'
    assert body.endswith(tail)
    body = body[:-len(tail)] + """        };
        if let Some(start) = cm2_start {
            let meta=cm2_meta.unwrap();let copied=cm2_copied.unwrap();let finished=cm2_finished.unwrap();let submitted=cm2_submitted.unwrap();
            eprintln!("CM2_COPY bytes={staging_bytes} tiles={} meta_us={} encode_us={} alloc_us={cm2_alloc_us} finish_us={} submit_us={} mapping_us={}",copies.len(),
                (meta-start).as_micros(),(copied-meta).as_micros(),(finished-copied).as_micros(),(submitted-finished).as_micros(),submitted.elapsed().as_micros());
        }
        Ok(capture)
    }

"""
    return source[:begin] + body + source[end:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent", required=True)
    parser.add_argument("--current", default="HEAD")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for label, revision in [("parent", args.parent), ("current", args.current)]:
        commit = subprocess.check_output(["git", "rev-parse", "--verify", "--end-of-options", revision + "^{commit}"], cwd=repo, text=True).strip()
        root = Path(tempfile.mkdtemp(prefix="capy-raster-trace-"))
        raw = subprocess.check_output(["git", "archive", commit], cwd=repo)
        with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
            archive.extractall(root, filter="data")
        instrument(root)
        with (output / (label + "-build.log")).open("w") as log:
            subprocess.run(["cargo", "build", "--offline", "--release", "-p", "layer-bench", "--bin", "gpu-bench", "--target-dir", str(repo / "target")], cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True)
        binary = output / (label + "-gpu-bench")
        shutil.copy2(repo / "target/release/gpu-bench", binary)
        (output / (label + "-revision.txt")).write_text(commit + "\n" + str(root) + "\n")
        print(f"{label}: {commit} {binary}", flush=True)


if __name__ == "__main__":
    main()
