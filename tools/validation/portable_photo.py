"""Run the application Rust photo/ICC/storage smoke exports in headless Chrome.

Build with cargo build --release -p layer-color --example portable_smoke
--target wasm32-unknown-unknown, then supply the resulting .wasm path.
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
args = parser.parse_args()
if not args.chrome:
    parser.error("Supply --chrome with a Chrome/Chromium executable")
encoded = base64.b64encode(args.wasm.read_bytes()).decode()
with tempfile.TemporaryDirectory(prefix="capy-portable-photo-") as directory:
    root = Path(directory)
    html = root / "check.html"
    html.write_text('''<!doctype html><meta charset="utf-8"><body>pending<script>
(async () => { try {
const bytes=Uint8Array.from(atob("''' + encoded + '''"), c=>c.charCodeAt(0));
const module=await WebAssembly.compile(bytes);
const instance=await WebAssembly.instantiate(module,{});
const start=performance.now();
const color=instance.exports.portable_smoke();
const avif=instance.exports.portable_avif();
document.body.textContent=JSON.stringify({ok:color===1 && avif===3, color, avif,
  imports:WebAssembly.Module.imports(module), ms:performance.now()-start,
  userAgent:navigator.userAgent});
} catch(e) { document.body.textContent='FAIL '+e.stack; } })();
</script>''')
    result = subprocess.run([
        args.chrome, "--headless", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
        "--no-first-run", "--no-default-browser-check", "--user-data-dir=" + str(root / "profile"),
        "--virtual-time-budget=15000", "--dump-dom", html.as_uri(),
    ], capture_output=True, text=True, timeout=45)
    print(result.stdout)
    if result.returncode or '"ok":true' not in result.stdout or '"imports":[]' not in result.stdout:
        print(result.stderr)
        raise SystemExit(1)
