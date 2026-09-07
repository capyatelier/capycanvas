# Source publication and licensing

Capy Canvas's original software and non-brand assets use `MIT OR Apache-2.0`:
recipients choose either license. Every workspace crate inherits this declaration
from the root manifest, and the About page reads that package metadata.
Keep both license texts, [third-party notices](../THIRD_PARTY_NOTICES.md), and
the separate [branding terms](../BRANDING.md). The names and designated capybara
mark are not included in the software license grant.

The MIT option enables broader downstream reuse, including GPLv2 projects, as
discussed in [Graphite's relicensing proposal](https://github.com/GraphiteEditor/Graphite/pull/4208).
It does not permit importing GPL-covered implementations into our permissively
licensed source. Nor does it change Apache-only dependency terms; downstream
consumers must assess their actual dependency graph. See
[Apache's compatibility guidance](https://www.apache.org/licenses/GPL-compatibility.html).

## Included assets and provenance

| Material | Origin and distribution terms |
| --- | --- |
| Application Rust, WGSL, web UI and tests | Project implementations, MIT OR Apache-2.0; the Oklab conversion exception is attributed separately. |
| `apps/layer-web/icons/*.svg`, except the Zen mark | Original generic vector assets, MIT OR Apache-2.0. No GNOME icon set is vendored. |
| `apps/layer-web/icons/layer-zen-symbolic.svg` | Owner-contributed capybara mark. The owner confirmed authorship of the source artwork; it has the separate Capy Canvas Branding License. |
| `assets/brushes/*.pgm` | First-party numeric brush masks introduced during brush-engine development; not a redistributed commercial or GPL brush pack. |
| `apps/layer-web/brush-previews/*.png` | Runtime assets rendered from project presets by `crates/layer-bench/src/previews.rs`. Regenerate with `cargo run --release -p layer-bench -- --brush-previews`. No external reference images or fonts are used by that generator. |
| Oklab conversion functions | Adaptation of Björn Ottosson's MIT reference; exact notice and local modifications are recorded in `THIRD_PARTY_NOTICES.md`. |

Runtime controls may use system fonts and system-provided GTK resources; those
resources are not copied into this source checkout. Research documents link to
upstream documentation, papers and implementations. A reference is not a license
to copy its code, figures, screenshots, textures or other artwork.

## Publication checks

The initial source audit reviewed the file inventory, asset provenance and PNG
metadata, source attributions, dependency licenses, secret-like strings, personal
paths, hardware fingerprints and generated outputs. Automated scans and source
review reduce risk; they are not a legal guarantee of ownership or noninfringement.
The branding policy reserves rights but does not register trademarks or establish
their availability; obtain legal review before relying on it for enforcement.

Initial audit, 2026-09-07: `cargo-deny` 0.20.2 accepted all 201 locked third-party
Rust packages; Gitleaks 8.30.1 found no secrets in the exported publication tree.
The 48 runtime PNGs contained only IHDR/IDAT/IEND chunks, with no embedded text
metadata. Personal-path, old-origin and hardware-fingerprint scans were clean.
All 115 non-ignored Rust tests, Clippy, the Wasm check and seven launcher tests
passed. Eight display-dependent native tests are outside this source-only audit.

Repeat these checks before publishing:

1. Review `git diff --cached` and `git ls-files`. Do not include local credentials,
   absolute personal paths, browser profiles, machine-specific logs, screenshots,
   vendored third-party trees or build outputs. Scan the exact staged tree for
   secrets, not just the working directory. Inspect binary metadata and provenance
   whenever an asset changes.
2. Install `cargo-deny` and run `cargo deny --locked check licenses sources`.
   The allowlist checks build/dev dependencies and all platform targets. Missing
   licenses, unreviewed licenses and non-registry sources fail the gate. License
   expressions containing `OR` permit a choice; `AND` requires both terms.
3. Keep `Cargo.lock` tracked. Check dependency updates and preserve every required
   upstream notice; a successful allowlist check does not fulfill those obligations.
4. Run `cargo test --workspace --locked`, check the Wasm target, and run the
   [UI validation](ui-implementation.md#run) appropriate to any functional change.
5. Publish only the reviewed branch. When replacing history, preserve a private
   recovery copy outside the source checkout and verify that the public branch
   has only the intended root commit. New history does not erase old remotes,
   previously published commits or other people's clones. Rotate any exposed
   credential; a history rewrite alone cannot revoke it.

`artifacts/` is entirely local and ignored, including review PNGs, benchmark
reports, traces and historical baseline notes. Paths under that directory in
design documents describe local outputs, not files included in a fresh clone.
Generate fresh results with the documented test/benchmark commands; older
measurements are historical observations, not performance guarantees for another
machine. Build trees, generated Wasm bindings and local editor/agent state are
also ignored. The runtime brush previews above are the intentional exception
for generated images: both frontends need them without running a GPU generator
at startup.

## Separate binary-release gate

This audit prepares the source repository, not an executable release. Native
packages and web bundles must ship the required notices for their exact Rust,
toolchain and system dependencies. GTK/libadwaita and other system libraries
retain their own terms, including applicable LGPL requirements. Do not assume
that our MIT option relicenses them or satisfies redistribution obligations.
Review actual packaged contents before publishing binaries, WebAssembly bundles,
containers or vendored source archives.

The [web packager](web-packaging.md) now generates a separate static distribution
with Wasm dependency/toolchain notices, branding terms and runtime path checks.
Those generated files stay in ignored `dist/`, not in the source repository.
Review its output whenever dependencies or bundled assets change; native binary
distribution remains a separate gate.
