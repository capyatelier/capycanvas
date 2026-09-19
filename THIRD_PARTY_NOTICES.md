# Third-party notices

Capy Canvas's original code and non-brand assets are licensed under
[MIT OR Apache-2.0](LICENSE), at the recipient's option. Third-party material
retains its own terms; our dual license does not relicense dependencies.
The project-owned capybara mark is a separate exception under [BRANDING.md](BRANDING.md).

## Native HEIF/AVIF photo decoders

GTK packages include dynamically loaded libheif 1.23.4 and libde265 1.1.3
(LGPL-3.0-or-later), plus libavif 1.4.2 and dav1d 1.5.3 (BSD-2-Clause). Their original licenses,
corresponding source archives, pinned checksums, local patch and build recipe
are included in `share/doc/capycanvas-photo-codecs/` in the native package.
These shared libraries remain replaceable in `lib/capycanvas/photo/`.

The narrow Capy Canvas C bridge uses the project's MIT OR Apache-2.0 terms.
The libheif patch preserves source color metadata after RGB conversion and
retains libheif's original license. See [codec provenance](vendor/README.md#heifavif-source-color-preservation).

## Native HDR gain-map codecs

The Linux photo-codec bundle also includes libultrahdr 2.0.0 (Apache-2.0),
libaom 3.14.1 (BSD-2-Clause and the AOM patent license), and libjpeg-turbo 3.1.4.1
(IJG, BSD-3-Clause and zlib licenses for the applicable components). The pinned
manifest `tools/build/photo-codecs.json` lists exact archives, hashes and license
files. The bundle copies those files and corresponding source archives into
`share/doc/capycanvas-photo-codecs/`. The Capy codec process and bridge retain
our MIT OR Apache-2.0 license; linking does not relicense the codec libraries.

## Oklab color conversion — MIT

The `working_to_oklab` and `working_from_oklab` functions in
`crates/layer-render-wgpu/src/working_color.wgsl` adapt Björn Ottosson's
[reference implementation](https://bottosson.github.io/posts/oklab/).
The local implementation uses WGSL vectors, signed cube roots and document-primary
transforms. Native Float32 output preserves extended RGB. We use the author's
MIT license option and preserve the
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

## Okhsv color conversion — MIT

`crates/layer-ui/src/color/okhsv.rs` adapts Björn Ottosson's
[Okhsv reference implementation](https://bottosson.github.io/posts/colorpicker/).
It uses double-precision intermediates, three gamut-boundary refinement steps,
explicit neutral/black handling, and caches hue-only terms for field rendering.

```text
Copyright (c) 2021 Björn Ottosson
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

## Output dithering hash — public domain

`crates/layer-color/src/icc/output/quantize.rs` adapts the SplitMix64 mixing
function from [Sebastiano Vigna's 2015 reference](https://prng.di.unimi.it/splitmix64.c).
The author dedicates the code to the public domain and permits use, copying,
modification and distribution. The source is supplied without warranty. We use
pixel coordinates instead of mutable generator state for repeatable output.

## fasteval — MIT

Numeric expressions use `fasteval` 0.2.4, downloaded as a Cargo dependency.

```text
MIT License

Copyright (c) 2019 Christopher Sebastian

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

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

The Android frontend uses AndroidX/Jetpack Compose and
[AndroidSVG 1.4](https://github.com/BigBadaboom/androidsvg), under Apache-2.0.
AndroidSVG (copyright Paul LeBeau, Cave Rock Software Ltd.) renders the existing
project-owned UI icons; no external brush artwork or icon set is imported.
The generated Gradle wrapper is provided by Gradle under Apache-2.0. Android SDK,
NDK and emulator packages are development tools installed separately, not vendored
or included as source assets. Android binary releases must collect notices for
their exact Maven and native dependencies as well as the notices below.

The published wgpu 30.0.1 `wgpu`, `wgpu-hal` and `wgpu-types` crates are included
under [vendor](vendor/README.md) with a local Vulkan color pass-through patch.
They retain their MIT OR Apache-2.0 licenses and copyright notices; each package
contains `LICENSE.MIT` and `LICENSE.APACHE`.

Cargo downloads other dependencies separately. `Cargo.lock` records exact
versions and registry checksums. [deny.toml](deny.toml) enforces the reviewed Rust license allowlist,
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

## Skia RWTMO tone mapping

The reference-white tone mapping implementation in
`crates/layer-core/src/color/hdr/sdr.rs` and
`crates/layer-render-wgpu/src/hdr_mapping.wgsl`, and the independent reference
in `tools/validation/rwtmo_reference.cpp`, are adapted from
Skia `src/codec/SkHdrAgtm.cpp`, commit
`bc94efd2229aad1048edbf892a6b2e7db28b22c4`.

Copyright 2025 Google LLC.

Copyright (c) 2011 Google Inc. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of the copyright holder nor the names of its
    contributors may be used to endorse or promote products derived
    from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## OpenEXR Rust codec dependencies

The Float32 interchange reader/writer uses `exr` 1.74.0 (BSD-3-Clause),
`lebe` 0.5.3 (BSD-3-Clause), `bit_field` 0.10.3 (MIT option), and
`zune-inflate` 0.2.54 (MIT OR Apache-2.0 OR Zlib). Cargo.lock pins these versions.

### exr-1.74.0

```text
# Licenses

This Rust implementation of the OpenEXR image format
and the exrs crate are not affiliated with the
[OpenEXR project](https://www.openexr.com/) or the
[ACADEMY SOFTWARE FOUNDATION](https://www.aswf.io/).


## The OpenEXR Image Format: [BSD-3-Clause](https://github.com/AcademySoftwareFoundation/openexr/blob/master/LICENSE.md)
Copyright (c) Contributors to the OpenEXR Project. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## The `exrs` Project, and independent Rust implementation of the official OpenEXR Image Format: Also [BSD-3-Clause](https://github.com/AcademySoftwareFoundation/openexr/blob/master/LICENSE.md)
Copyright (c) Contributors to the exrs Project. All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### lebe-0.5.3

```text
Copyright (c) 2022 Contributors to the lebe Project. All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
this list of conditions and the following disclaimer in the documentation
and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors
may be used to endorse or promote products derived from this software without
specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### bit_field-0.10.3

```text
The MIT License (MIT)

Copyright (c) 2016 Philipp Oppermann

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

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
