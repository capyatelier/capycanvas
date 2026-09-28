# Apple clients

Start with the [Apple guide](../../docs/development/apple.md) and the
[porting guide](../../docs/APPLE_PORTING_GUIDE.md).

- `CapyCanvas.xcodeproj` is generated: edit `scripts/project.py`, rerun it and
  commit the result.
- Build and test on an Apple Silicon Mac; only the Rust bridge tests run
  elsewhere. Every change must build for both macOS and iPadOS.
