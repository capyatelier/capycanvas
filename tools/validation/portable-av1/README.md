# Rust AV1 portability qualification

This standalone workspace exercises the patched upstream `rav1d` 1.1.0 with
assembly disabled and both 8-bit and 10/12-bit decoding enabled. It is the AV1
decoder qualification step for the shared AVIF migration; application container,
color, gain-map, memory admission and cancellation integration remains separate.

The fixtures are synthetic 64 × 32, full-range identity-matrix 4:4:4 frames.
For channel `c` and pixel `i`, the source sample is
`(i * [37,17,7][c] + [19,301,151][c]) & ((1 << depth) - 1)`.
The driver checks dimensions, depth, and every decoder plane sample against
that formula, accounting for the G/B/R identity plane order and byte stride.
It selects one thread, limits the frame to 2048 pixels, and releases packet,
picture and context handles. This is a fixture driver, not the production API.

From the repository root:

```sh
CARGO_TARGET_DIR=/tmp/capy-portable-av1-target cargo test --locked --release \
  --manifest-path tools/validation/portable-av1/Cargo.toml
CARGO_TARGET_DIR=/tmp/capy-portable-av1-target cargo build --locked --release \
  --manifest-path tools/validation/portable-av1/Cargo.toml --target wasm32-unknown-unknown
python3 tools/validation/portable-av1/browser.py \
  /tmp/capy-portable-av1-target/wasm32-unknown-unknown/release/capy_portable_av1_check.wasm
CARGO_TARGET_DIR=/tmp/capy-portable-av1-target cargo check --locked \
  --manifest-path tools/validation/portable-av1/Cargo.toml --target aarch64-linux-android
```

The browser script uses a temporary Chrome profile, requires successful checks
for all three depths, and rejects any WebAssembly host imports. `--chrome`
selects a browser executable if it is not on PATH. Native and Chrome 152 checks
passed on Linux; Android compilation passed. Actual Android decoder operation
and browser application integration are still pending.

The checked-in OBU fixtures were encoded losslessly by AOM 3.13.3 through
libavif 1.3.0. Reproduce them using the matching libavif header/library, with C
used only as an independent encoder oracle:

```sh
mkdir -p /tmp/capy-av1-fixtures
cc -std=c11 -O2 -Wall -Wextra -Werror -I/path/to/libavif/include/avif \
  tools/validation/portable-av1/generate.c -l:libavif.so.16 -o /tmp/capy-av1-generate
/tmp/capy-av1-generate /tmp/capy-av1-fixtures
python3 tools/validation/portable-av1/extract.py /tmp/capy-av1-fixtures \
  tools/validation/portable-av1/fixtures
```

`extract.py` reads only the structure emitted by this fixture generator. It is
not an application AVIF parser. Regeneration matched these hashes byte for byte:

| Fixture | Bytes | SHA-256 |
| --- | ---: | --- |
| p3-8bit.obu | 6627 | `c2d46f0869161537dfaf22a8da1758ec019b1a63efe7af32652f7ad9cc9f0b1e` |
| p3-10bit.obu | 5623 | `7fcf1ace5428399bf97f4e334e39581d04ce003ec69167935ad6ce8061a8cb44` |
| p3-12bit.obu | 5306 | `1effcf44517741579ba89289e7e2b2ec5fe004c229f635b9538c3206c0a1b2f0` |
