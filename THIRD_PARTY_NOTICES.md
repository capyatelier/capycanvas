# Third-party notices

Capy Canvas's original code and non-brand assets are licensed under
[MIT OR Apache-2.0](LICENSE), at the recipient's option. Third-party material
retains its own terms; our dual license does not relicense dependencies.
The project-owned capybara mark is a separate exception under [BRANDING.md](BRANDING.md).

## Oklab color conversion — MIT

The `linear_to_oklab` and `oklab_to_linear` functions in
`crates/layer-render-wgpu/src/material_brush.wgsl` adapt Björn Ottosson's
[reference implementation](https://bottosson.github.io/posts/oklab/).
The local implementation uses WGSL vectors, signed cube roots, and clamps
negative RGB output. We use the author's MIT license option and preserve the
[original notice](https://bottosson.github.io/misc/License.txt) below.

```text
Copyright (c) 2020 Björn Ottosson
Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Dependencies and binary releases

Cargo downloads dependencies separately; their sources and compiled libraries
are not vendored in this repository. `Cargo.lock` records exact versions and
checksums. [deny.toml](deny.toml) enforces the reviewed Rust license allowlist,
including build/dev dependencies and non-Linux targets. An allowed license is
not permission to omit its copyright notices or other distribution conditions.

The native frontend uses separately installed GTK, libadwaita and their system
dependencies. They are not relicensed to Apache-2.0. In particular,
[GTK](https://www.gtk.org/docs/architecture/) and
[libadwaita](https://gitlab.gnome.org/GNOME/libadwaita/-/blob/main/COPYING)
have LGPL obligations; using their public APIs is distinct from copying their
implementation or artwork into this repository.

The MIT option facilitates reuse of our own code in GPLv2 projects, but does not
guarantee that the entire dependency graph is GPLv2-compatible. Apache-only
dependencies must be assessed separately by downstream users.

Before distributing native executables, WebAssembly bundles, containers or
vendored source archives, collect notices and license texts for the exact
shipped dependency/toolchain artifacts. If shipping LGPL libraries, also satisfy
their applicable source, replacement/relinking and notice requirements. This
source audit is not a completed binary-distribution compliance package.
