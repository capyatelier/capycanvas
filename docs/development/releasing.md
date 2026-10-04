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
- To change the version, edit the workspace version, run
  `cargo update --workspace --offline` and `python3 apps/layer-apple/scripts/project.py`,
  add a matching `<release>` entry to
  `apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml`, and commit the
  results together. The AppImage build refuses a version without that entry.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request: the window-free
shared Rust suites, including the localization catalogs, the Wasm consumer check,
a check that the generated Xcode project matches `project.py`, the Git hook tests
and `cargo deny`. Hosted runners have no GPU and no tablet, so CI never replaces
the [checks for your change](testing.md).

## Release workflow

`.github/workflows/release.yml` builds every package from a clean checkout with
the same scripts developers run and the Rust version pinned in the workflow:

| Job | Script | Output |
| --- | --- | --- |
| Linux | `apps/layer-linux/appimage.sh` in an Arch Linux container | `capycanvas-<version>-linux-x86_64.AppImage` and its `.zsync` file |
| Web | `node apps/layer-web/package.mjs` | `capycanvas-<version>-web.zip` |
| Android | `./gradlew :app:bundleRelease -PcapyAbi=arm64-v8a` | `capycanvas-<version>-android.aab` |
| Windows | `package.ps1`, `package-msix.ps1`, `test-msix.ps1` and `package-installer.ps1` | Portable ZIP, setup program and Store MSIX |
| macOS, iPadOS | `apps/layer-apple/scripts/release.sh mac` and `ipad` | `capycanvas-<version>-macos-arm64.dmg`; the iPad build goes to App Store Connect |

Run it from the Actions tab to build unsigned packages as workflow artifacts.
Pushing a `v*` tag checks that the tag names the workspace version and is on
`main`, signs with the protected `release` environment, uploads the iPad build to
TestFlight and creates a draft GitHub Release holding every download,
`SHA256SUMS` and build provenance attestations.

### Linux AppImage

`appimage.sh` runs as root in a disposable Arch Linux container, which ships the
GTK and libadwaita versions the app needs. It installs the
[Arch recipe's](../../packaging/arch/README.md) dependencies, runs the native
packager, installs the result into `/usr`, and lets a pinned `quick-sharun` from
[Anylinux AppImages](https://github.com/pkgforge-dev/Anylinux-AppImages) bundle
every library, including glibc, the Vulkan loader and Mesa. NVIDIA's proprietary
driver always comes from the host. The AppImage carries the project and GTK
notices, the license texts and versions of every bundled Arch package, and
update information for AppImage updaters. It does not yet include the
corresponding sources of its LGPL libraries, which publishing it requires. To
build one locally:

```bash
podman run --rm -v "$PWD":/src:Z -w /src docker.io/library/archlinux:latest \
  bash apps/layer-linux/appimage.sh
```

### Signing credentials

The `release` environment holds the signing material; restrict it to `v*` tags
and require a maintainer's approval.

| Name | Kind | Use |
| --- | --- | --- |
| `ANDROID_UPLOAD_KEYSTORE`, `ANDROID_UPLOAD_KEYSTORE_PASSWORD` | Secrets | Base64 PKCS12 upload keystore with the alias `upload`, and its password |
| `APPLE_API_KEY` | Secret | Base64 App Store Connect team API key (`.p8`) with the Admin role, for cloud signing, upload and notarization |
| `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`, `APPLE_TEAM_ID` | Variables | That key's identifiers and the team |
| `APPLE_DEVELOPER_ID_P12`, `APPLE_DEVELOPER_ID_PASSWORD` | Secrets | Base64 Developer ID Application certificate and its password; Xcode cannot cloud-sign Developer ID builds with an API key |
| `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` | Variables | The Microsoft Entra app the Windows job signs in as through OIDC; it holds the Artifact Signing Certificate Profile Signer role and trusts the `release` environment |
| `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE` | Variables | The Azure Artifact Signing account's regional endpoint, its name and the Public Trust certificate profile |

Locally, `CAPY_UPLOAD_KEYSTORE` and `CAPY_UPLOAD_KEYSTORE_PASSWORD` sign the
Android bundle, and `CAPY_APPLE_TEAM`, `CAPY_APPLE_KEY`, `CAPY_APPLE_KEY_ID` and
`CAPY_APPLE_ISSUER` make `release.sh` sign, upload or notarize. On a tag, the
Windows job passes Azure Artifact Signing's `signtool` plugin to the packagers'
`-SignArguments` ([Windows](windows.md#portable-zip)), signing the app
executables and the setup program; the Store signs the MSIX.

## Publishing a release

1. Bump the version on `main` and push a tag for that commit:
   `git tag -a v1.0.0 -m 'Capy Canvas 1.0.0' && git push origin v1.0.0`.
2. Test the exact draft downloads and the TestFlight build: the
   [checks](testing.md) and user journeys on every host, installation, updating
   from the previous release with existing drawings, and the
   [performance targets](../PERFORMANCE_TARGETS.md) on reference hardware.
3. Upload the AAB to Google Play and the MSIX to Partner Center, publish the web
   ZIP through the hosting repository, and submit the TestFlight build for review.
4. Publish the draft. With immutable releases enabled, its assets and tag can no
   longer change.
5. If a check fails, delete the draft, fix `main` and release the next patch
   version. Never move a tag or reuse a version.
