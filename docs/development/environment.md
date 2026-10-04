# Development environment

[Developer guide](README.md)

Several people and coding agents work on this repository at the same time, often
on one machine. This guide covers the shared resources they must not collide on:
worktrees, build directories, ports and test state. Tablets, VMs and Apple
hardware are in [devices](devices.md).

## Worktrees and build directories

- Use the git worktree named by the user or provided as the task's working
  directory. It is already isolated; do not create a nested or replacement
  worktree for a routine task.
  Create one only when none is assigned or the user requests one. Another session
  may be building, testing or committing in any other checkout, including the
  main one.
- Put worktrees and build directories under `/home`, for example
  `~/.cache/capy/<name>`. `/tmp` is a tmpfs whose per-user quota every session
  shares; a few Cargo `target/` directories fill it, and then every session's
  builds and tool output fail. If a worktree must live in `/tmp`, set
  `CARGO_TARGET_DIR` and the Gradle build directory under `/home`.
- Delete scratch worktrees and build copies you created when you finish; keep
  the user's assigned worktree. Never delete, reset or clean another session's
  worktree, branch, build or files.
- Set `CARGO_TERM_COLOR=never` when output goes to a log or an agent.

## Build profiles

Each entry point picks a Cargo profile. Profiles keep separate artifacts in the
target directory: the first build of a profile compiles its dependencies, and
returning to a cached profile reuses them. Prefer the entry point's profile for
repeated edit/run builds.

| Command | Profile |
| --- | --- |
| `apps/layer-linux/run.sh`, `apps/layer-web/run.sh` and `build.sh`, Android debug APKs | `dev-perf`: release optimization, incremental, line tables |
| `cargo test` for shared crates | `test` (unoptimized) |
| GTK native tests, `tools/performance/workspace-motion.sh`, `gtk-raster.sh` | `release` |
| Apple and Windows Debug builds | `dev` |
| Apple and Windows Release builds, Android release and benchmark APKs, the GTK package | `release` |
| Web package (`apps/layer-web/package.mjs`) | `web-release`: ThinLTO, one codegen unit |

`CAPY_RUST_PROFILE` overrides the GTK and Web scripts; Android also accepts
`-PcapyRustProfile`. Compare performance only between builds of the same profile.
`dev-perf` keeps release optimization and 16 codegen units while caching
incremental workspace compilation. A shared crate edit still recompiles its
consumers. `cargo check --locked -p layer-linux` avoids code generation and linking
when checking Rust changes. `CAPY_RUST_PROFILE=dev` selects an unoptimized app for
UI development; use the required optimized profile for frame measurements.
Keep profile overrides, compiler flags and the target directory stable to reuse
the same cache.

## Compilation and linking targets

Client implementations compile as Rust libraries so Cargo can start them when
shared dependency metadata is ready, while dependency code generation continues.
GTK's executable calls `layer_linux::run`. Web, Android, Apple and Windows build
their `layer-<platform>-link` package to produce the Wasm module, JNI library,
static archive or DLL. Each linking package reexports its implementation crate;
artifact names and native exports stay the same. The platform build scripts select
these packages. Keep crate tests and `cargo check` on the implementation packages.

`tools/build/profile-rust-incremental.py` times cached rebuilds after a temporary
edit to `layer-core`'s sRGB decode threshold, then restores the source. Run it on
an otherwise idle machine in your own worktree. To measure the GTK launcher profile:

```bash
python3 tools/build/profile-rust-incremental.py \
  --platforms gtk --profile dev-perf --modes configured
```

The report separates cache warm-up, unchanged builds and real edits. Increasing
codegen units or reducing optimization can shorten compilation, but changes the
generated code; qualify frame performance before changing the launcher default.

## Test state and processes

- Never run tests against a user's real settings, workspaces, documents or browser
  profile. Native tests and fixtures take a fresh `CAPY_STORAGE_DIR`
  ([test storage](../internals/storage.md#test-storage)), development builds use
  their own app identity, and Web checks use their own Chrome profile and origin.
- Inject native input only into the private display that
  `tools/performance/workspace-motion.sh` and `gtk-raster.sh` start, never into
  the desktop session.
- Close only processes you started. Match them by something unique to your run,
  such as your worktree path, never by a bare program name that also matches
  other sessions' apps.
- `RUST_TEST_THREADS` defaults to 4 in [`.cargo/config.toml`](../../.cargo/config.toml)
  because renderer tests create a GPU device each; see [testing](testing.md#shared-rust).

## Ports and environment variables

| Variable | Used by | Default |
| --- | --- | --- |
| `LAYER_WEB_PORT` | `apps/layer-web/run.sh` dev server | 4173 |
| `LAYER_WEB_URL` | `apps/layer-web/test.mjs` target | `http://127.0.0.1:4173/` |
| `CHROME` | `apps/layer-web/test.mjs` | `google-chrome` |
| `CAPY_CHROME` | `tools/visual/` captures | macOS Chrome path |
| `CAPY_ANDROID_SERIAL` | Android scripts and docs | `emulator-5554` in `run.sh` |
| `CAPY_RUST_PROFILE` | GTK and Web scripts | `dev-perf` |
| `CAPY_STORAGE_DIR` | Native clients: one private folder for all stored files; a relative name is inside the app's temporary folder | Platform folders |

The package preview uses port 4174 and `workspace-motion.sh web` uses 4179.
When another session may be serving on the same machine, pick a port nobody
else uses. `tools/devices/devices.py run` exports a per-worktree `CAPY_WEB_PORT`
and `CAPY_CDP_PORT` for this; use them for dev servers and forwarded tablet
Chrome sessions.
