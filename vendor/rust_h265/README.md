# rust_h265

A pure Rust H.265 / HEVC video decoder.

> **Status:** functional. Main and Main 10 profile HEVC (8-bit and 10-bit
> 4:2:0) decodes end-to-end with CTU 16/32/64, I/P/B slices (including
> hierarchical B), WPP, tiles, dependent slice segments, SAO, deblocking,
> AQ (`cu_qp_delta`), scaling lists, sign-data hiding, weighted prediction,
> transform skip, transquant bypass, constrained intra prediction, and PCM.
> 127 tests pass. Byte-exact against FFmpeg on every in-tree fixture plus
> real 1080p Big Buck Bunny at x265 presets `ultrafast` / `medium` / `slow`
> in both 8-bit and 10-bit. No threading or SIMD yet — ~5× slower than
> single-threaded FFmpeg on 8-bit content, ~2.5× on 10-bit; see
> [`BENCHMARK.md`](BENCHMARK.md).

While working on `rust_media` it became clear that there isn't a sufficiently good open source software HEVC decoder that ships as a standalone library. FFmpeg has one, but it isn't split out. Most devices have hardware HEVC decoders, but a portable software fallback is still useful when you want one binary that runs anywhere.

A pure Rust H.264 decoder ([`rust_h264`](https://github.com/roticv/rust_h264)) already exists in this style. HEVC is a substantively different codec, not an extension of H.264, so this is a fresh implementation rather than a port.

## Design

- **Input:** Annex B bytestream (start code delimited `00 00 00 01` / `00 00 01`). HVCC (length-prefixed, used in MP4) is **not** supported — callers must convert to Annex B before feeding data to the decoder.
- **Streaming:** `Decoder::decode_nal(&[u8]) -> Result<Option<Frame>, DecodeError>` plus `flush()`. NAL units are fed incrementally and decoded frames are emitted as they become available, in **decode order** (callers re-sort by POC for display).
- **Performance:** The decoder aims to be fast, with FFmpeg's software HEVC decoder as the target benchmark. Current gap vs single-threaded FFmpeg: ~5× on 8-bit, ~2.5× on 10-bit real 1080p content. No NEON / SSE kernels yet.
- **Multi-bit-depth:** 8-bit and 10-bit (Main / Main 10 profile) via a generic `Pixel` trait. 12-bit infrastructure is in place but untested. Pixel planes are `PixelData::U8(Vec<u8>)` or `PixelData::U16(Vec<u16>)` — check `frame.bit_depth` to determine which.
- **Pure Rust, no `unsafe`** in the current codebase. `unsafe` will be reserved for SIMD paths once they land.

## Usage

The public API mirrors `rust_h264`:

```rust
use rust_h265::{Decoder, parse_annex_b};

let h265_data = std::fs::read("input.h265").unwrap();
let nals = parse_annex_b(&h265_data);
let mut decoder = Decoder::new();

for nal in &nals {
    match decoder.decode_nal(nal) {
        Ok(Some(frame)) => {
            // `frame` is a decoded YUV420 picture:
            //   frame.y, frame.u, frame.v  — PixelData (U8 or U16)
            //   frame.width, frame.height  — dimensions
            //   frame.bit_depth            — 8 or 10
            //   frame.pic_order_cnt        — display order index
            //
            // Access pixels:
            //   frame.y.as_u8()  -> Option<&[u8]>   (8-bit)
            //   frame.y.as_u16() -> Option<&[u16]>  (10-bit)
        }
        Ok(None) => {} // NAL consumed, no frame ready yet (e.g. VPS/SPS/PPS)
        Err(e) => eprintln!("decode error: {:?}", e),
    }
}
// Flush the last buffered frame
if let Some(frame) = decoder.flush() {
    // handle final frame
}
```

### Important: frame ordering

**`decode_nal` returns frames in decode order, not display order.** With B-frames, the decoder must buffer reference frames before it can decode the B-frames that depend on them. The output order differs from the intended display order.

To display frames correctly, sort by `pic_order_cnt` (POC). If the stream has multiple IDR boundaries (GOPs), also track IDR boundaries to avoid mixing frames from different GOPs:

```rust
use rust_h265::NalUnitType;

let mut idr_count: u32 = 0;
let mut frames = Vec::new();

for nal in &nals {
    // HEVC IDR pictures are nal_unit_type 19 (IDR_W_RADL) or 20 (IDR_N_LP).
    let is_idr = matches!(
        nal.nal_unit_type,
        NalUnitType::IdrWRadl | NalUnitType::IdrNLp,
    );

    if let Ok(Some(frame)) = decoder.decode_nal(nal) {
        // Push with the CURRENT idr_count — this frame belongs to the
        // previous picture, before the IDR boundary.
        frames.push((idr_count, frame));
    }

    // Increment AFTER decode_nal, because decode_nal returns the
    // PREVIOUS frame when it sees a new picture header. Incrementing
    // first would tag the last B-frame of the old GOP with the new
    // GOP's count and sort it incorrectly.
    if is_idr {
        idr_count += 1;
    }
}
if let Some(frame) = decoder.flush() {
    frames.push((idr_count, frame));
}

// Sort by (GOP, POC) for display order.
frames.sort_by_key(|(idr, f)| (*idr, f.pic_order_cnt));
```

### Common pitfall: IDR count timing

The most common mistake is incrementing `idr_count` **before** calling `decode_nal`. This causes the last frame of each GOP to be placed after the next IDR in display order, producing a visible glitch at every scene cut.

**Wrong:**
```rust
if is_idr {
    idr_count += 1;  // BUG: too early
}
let frame = decoder.decode_nal(nal)?;
// frame belongs to the OLD GOP but gets the NEW idr_count
```

**Correct:**
```rust
let frame = decoder.decode_nal(nal)?;
// Push frame with current idr_count first
if is_idr {
    idr_count += 1;  // After the previous frame is handled
}
```

## Differences from H.264

If you're coming from `rust_h264`, the major changes are not API-level — the public surface is essentially the same — but the codec internals are nearly entirely different. A few user-visible differences worth knowing:

- **NAL header is 2 bytes**, not 1. `nal_unit_type` is 6 bits, with VPS=32, SPS=33, PPS=34. IDR pictures are types 19 (`IDR_W_RADL`) and 20 (`IDR_N_LP`), not type 5.
- **VPS** (Video Parameter Set) exists in addition to SPS and PPS.
- **Block structure** is a quad-tree of Coding Tree Units (typically 64×64) instead of fixed 16×16 macroblocks.
- **More intra modes** (35: planar, DC, 33 angular) and larger transforms (up to 32×32, plus 4×4 DST for intra luma).
- **SAO** (Sample Adaptive Offset) is a new in-loop filter on top of deblocking.
- **Tiles, WPP, and slice segments** add picture-level parallelism options absent in H.264.

These do not change how you call the decoder; they change what the decoder has to implement internally.

## Tools

```sh
# Play an H.265 file in a window (press Escape to quit):
cargo run --release --example play -- input.h265 [--fps 30] [--loop]

# Decode to raw YUV in display order (8-bit: yuv420p, 10-bit: yuv420p10le):
cargo run --release --example dump_frames -- input.h265 out.yuv

# Throughput measurement on a single file:
cargo run --release --example bench_decode -- input.h265 --warmup 2 --repeat 10

# Real-world benchmark matrix (downloads Big Buck Bunny, transcodes with
# x265 at several presets in 8-bit and 10-bit, runs both rust_h265 and
# FFmpeg on each):
cargo run --release --example bench_realworld
```

## Testing

```sh
cargo test --release
```

127 tests covering the full feature matrix — 8-bit and 10-bit, CTU 16/32/64,
I/P/B slices, WPP, tiles, dependent slices, SAO, deblocking, transform skip,
transquant bypass, constrained intra, PCM, multi-slice, and more. All
in-tree fixtures are byte-exact against FFmpeg; tests that need >1 MB of
reference output (e.g. 1080p) use a SHA-256 hash of the decoded planes. The
`bench_realworld` example covers real 1080p Big Buck Bunny in both 8-bit
and 10-bit and requires `ffmpeg` and `x265` on `$PATH`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
