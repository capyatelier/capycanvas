"""Run the raw Rust AV1 Wasm fixture checks in an isolated headless Chrome."""
import argparse
import base64
import shutil
import subprocess
import tempfile
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("wasm", type=Path)
parser.add_argument("--chrome", default=shutil.which("google-chrome") or shutil.which("chromium"))
args = parser.parse_args()
if not args.chrome:
    parser.error("Supply --chrome with a Chrome/Chromium executable")
encoded = base64.b64encode(args.wasm.read_bytes()).decode()
with tempfile.TemporaryDirectory(prefix="capy-portable-av1-") as directory:
    root = Path(directory)
    html = root / "check.html"
    html.write_text('''<!doctype html><meta charset="utf-8"><body>pending<script>
try {
const bytes=Uint8Array.from(atob("''' + encoded + '''"), c=>c.charCodeAt(0));
const module=new WebAssembly.Module(bytes);
const instance=new WebAssembly.Instance(module,{});
const start=performance.now();
const depths=instance.exports.verify_all_depths();
document.body.textContent=JSON.stringify({ok:depths===3, depths,
  imports:WebAssembly.Module.imports(module), ms:performance.now()-start,
  userAgent:navigator.userAgent});
} catch(e) { document.body.textContent='FAIL '+e.stack; }
</script>''')
    result = subprocess.run([
        args.chrome, "--headless", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage",
        "--no-first-run", "--no-default-browser-check", "--user-data-dir=" + str(root / "profile"),
        "--dump-dom", html.as_uri(),
    ], capture_output=True, text=True, timeout=45)
    print(result.stdout)
    if result.returncode or '"ok":true' not in result.stdout or '"imports":[]' not in result.stdout:
        print(result.stderr)
        raise SystemExit(1)
