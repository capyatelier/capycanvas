# Apple native foundation

The Apple C ABI wraps `layer-host`, the same session facade used by Android.
UIKit and AppKit retain their native surfaces and own one serial render executor.
All calls, including input, snapshots and destruction, run on that executor.
The UI thread captures samples and the displayed camera revision, then submits
owned buffers. Packed input and GPU work never traverse the JSON UI transport.

`CapyApple.h` documents ownership, packed sample units, return values and borrowed
versus owned strings. Errors and Rust unwinds are contained at operation boundaries.
Metal detach drops the surface while keeping the shared document and renderer
alive for attachment to a replacement layer. The platform must retain its layer
through detach. Frame costs currently measure five **CPU** stages; they do not
claim GPU timing, drawable presentation or input-to-present latency.

Foundation validation:

```sh
cargo test -p layer-host --lib
cargo run -p layer-host --example inventory
cargo check -p layer-android --target aarch64-linux-android
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
  cargo build -p layer-apple --target aarch64-apple-ios
```

This is the shared-host/Metal foundation milestone. Native editor integration,
complete input sensor handling, persistence, visual comparisons and measured
hardware acceptance remain required before the iPad app is complete.
