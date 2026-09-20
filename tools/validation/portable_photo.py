"""Run the application Rust photo/ICC/storage smoke exports in headless Chrome.

Build with cargo build --release -p layer-color --example portable_smoke
--target wasm32-unknown-unknown, then supply the resulting .wasm path.
Uses the same wasm-bindgen packaging as the Web host; no host codec is supplied.
"""
import argparse
import base64
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("wasm", type=Path)
parser.add_argument("--chrome", default=shutil.which("google-chrome") or shutil.which("chromium"))
parser.add_argument("--wasm-bindgen", default=shutil.which("wasm-bindgen"))
args = parser.parse_args()
if not args.chrome:
    parser.error("Supply --chrome with a Chrome/Chromium executable")
if not args.wasm_bindgen:
    parser.error("Supply --wasm-bindgen with the version matching Cargo.lock")
with tempfile.TemporaryDirectory(prefix="capy-portable-photo-") as directory:
    root = Path(directory)
    subprocess.run([args.wasm_bindgen, str(args.wasm), "--target", "no-modules",
                    "--out-dir", str(root), "--out-name", "photo"], check=True)
    encoded = base64.b64encode((root / "photo_bg.wasm").read_bytes()).decode()
    glue = (root / "photo.js").read_text()
    html = root / "check.html"
    html.write_text('''<!doctype html><meta charset="utf-8"><body>pending<script>''' + glue + '''</script><script>
(async () => { try {
const bytes=Uint8Array.from(atob("''' + encoded + '''"), c=>c.charCodeAt(0));
const module=await WebAssembly.compile(bytes);
const instance=await wasm_bindgen({module_or_path:module});
const start=performance.now();
const color=instance.portable_smoke();
const avif=instance.portable_avif();
const avifExport=instance.portable_avif_export();
const imports=WebAssembly.Module.imports(module);
// Only wasm-bindgen's reference-table initialization is permitted. No browser
// image decoder, OS codec, clock, filesystem, or other service supplies pixels.
const allowed=imports.every(i=>i.kind==='function' && i.name==='__wbindgen_init_externref_table');
document.body.textContent=JSON.stringify({ok:color===1 && avif===3 && avifExport===2 && allowed, color, avif, avifExport,
  imports, ms:performance.now()-start,
  userAgent:navigator.userAgent});
} catch(e) { document.body.textContent='FAIL '+e.stack; } })();
</script>''')
    result = subprocess.run([
        args.chrome, "--headless", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
        "--no-first-run", "--no-default-browser-check", "--user-data-dir=" + str(root / "profile"),
        "--virtual-time-budget=15000", "--dump-dom", html.as_uri(),
    ], capture_output=True, text=True, timeout=45)
    print(result.stdout)
    if result.returncode or '"ok":true' not in result.stdout:
        print(result.stderr)
        raise SystemExit(1)
