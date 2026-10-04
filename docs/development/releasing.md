# Releasing

[Developer guide](README.md)

## Versions

The `[workspace.package]` table in the root `Cargo.toml` holds the one application
version. Every crate inherits it, the About page shows it, and each host derives
its own numbers from it at build time:

| Value | Rule | For 1.0.0 |
| --- | --- | --- |
| Android `versionName`, Apple `CFBundleShortVersionString`, package file names | The version | `1.0.0` |
| Android `versionCode`, Apple `CFBundleVersion` | major × 1,000,000 + minor × 1,000 + patch | `1000000` |
| MSIX package version | major.minor.patch.0 | `1.0.0.0` |

- Versions are plain `MAJOR.MINOR.PATCH`, with a nonzero major part for the
  Microsoft Store and minor and patch parts below 1,000. Test status belongs to
  a distribution channel, never to the version.
- Every build that leaves the project, including a store test upload, gets a new
  version. Stores reject a reused number, and a skipped number is harmless.
- To change the version, edit the workspace version, then run
  `cargo update --workspace --offline` and `python3 apps/layer-apple/scripts/project.py`
  and commit the results together.
