# Releasing

[Developer guide](README.md)

## Access and public documentation

This is the publishing guide for maintainers and agents. Read the workflows
before a release and check live store status; previous release reports do not
establish the next release's state. Use an authorized GitHub session with access
to Actions and the protected `release` environment, plus the relevant store
accounts. CI uses the credentials named below. Ask the maintainer for missing
access through a private channel; do not ask them to paste credentials into an
issue or commit.

Keep workflow names, commands, public download URLs, credential variable names
and public verification keys here. Keep credential values, account identifiers,
tester identities, internal invitation links, local credential paths and signing
backups out of the repository, issues and release notes. Review staged changes
before committing. Ignored `artifacts/` and `*.local.md` can hold local test
evidence and handoff notes, but are not secure storage for credentials; sanitize
their contents before sharing them. A new agent obtains current private access
and tester details from the maintainer, not from repository history.

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
  results together. The entry's `<description>` holds the release notes: a
  short paragraph and a list of what changed, in words a painter knows. Linux
  software centers show it, the GitHub Release starts with it, and the release
  workflow refuses a version without it.

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
| Linux | `packaging/flatpak/build.sh` and `packaging/flatpak/export.sh` with GNOME Platform and SDK 50 | `capycanvas-<version>-linux-x86_64.flatpak`; on a tag also `capycanvas.flatpakref` and `capycanvas-<version>-flatpak-repository.tar.zst` |
| Web | `node apps/layer-web/package.mjs` | `capycanvas-<version>-web.zip` |
| Android | `./gradlew :app:bundleRelease -PcapyAbi=arm64-v8a` | `capycanvas-<version>-android.aab`; on a tag also a Play-signed `capycanvas-<version>-android.apk` without Play licensing checks |
| Windows | `package.ps1`, `package-msix.ps1`, `test-msix.ps1` and `package-installer.ps1` | Portable ZIP, setup program and Store MSIX |
| macOS, iPadOS | `apps/layer-apple/scripts/release.sh mac` and `ipad` | `capycanvas-<version>-macos-arm64.dmg`; the iPad build goes to App Store Connect |

Run it from the Actions tab to build unsigned packages as workflow artifacts.
Pushing a `v*` tag checks that the tag names the workspace version and is on
`main` and runs `cargo deny`, then signs with the protected `release`
environment, uploads the iPad build to TestFlight, verifies the Android download
and rolls out its bundle to Play's internal track, and creates a draft GitHub Release holding the release
notes, every download, `SHA256SUMS` and build provenance attestations.

### Android APK

The GitHub APK must open without Google Play or Google services. Play App Signing
preserves the signing identity used by existing installations; Automatic
protection adds an installer and licence check that prevents this use.

[`android_apk.py`](../../tools/build/android_apk.py) downloads the
[unprotected standalone APKs](https://developers.google.com/android-publisher/api-ref/rest/v3/generatedapks/list)
provided by Play. When protection is disabled, it can use the universal APK.
It checks the actual manifest and DEX for Play licensing and Google service code,
rejects split APKs, checks the package, version, Android 10 minimum and ARM64
renderer, and verifies the signature before publishing the download. The public
signing certificate in the script matches existing Android installations; an
upload-key signature is not a substitute. A signing-key change needs explicit
upgrade testing before changing this check.

APK verification uses Java 25 and Android Build Tools 37.0.0. Play's APK
Signature Scheme v3.2 signatures use ML-DSA; Java 17 cannot verify them. The
download step selects the runner's Java 25 through `JAVA_HOME_25_X64`. For local
verification, point `JAVA_HOME` at Java 25 and prepend `$JAVA_HOME/bin` to `PATH`.

The Android job saves the `android-bundle` artifact before uploading to Play,
which consumes the version number. It saves the verified APK separately as
`android-apk` before internal rollout. If a later step fails, recover these
artifacts from that workflow run instead of rebuilding or uploading the same
version. `android_publish.py upload` checks Play's version and SHA-256 against
the saved AAB and reuses an identical existing upload. Both upload and promotion
refuse to cancel changes already in review. Retry APK download with
`android_apk.py download`; retry internal
rollout with the promotion workflow below. A code change still needs a new
version.

After testing the final internal build, run **Promote Android testing release**
from its release tag with `track=alpha`. This promotes the existing bundle to
the `alpha` closed track, preserving release notes, and submits the edit for
review. For example:

```bash
gh workflow run android-promote.yml --ref v1.0.11 -f track=alpha
```

The workflow also accepts `track=internal` to retry internal rollout without
uploading again. It uses the protected `release` environment, refuses to replace
a newer testing build and fails if Google already has changes in review. It
does not silently cancel a review or fall back to a draft. An API commit does
not establish review approval or tester availability: check Play Console and
install through Play with an enrolled tester account. Keep tester lists and
group membership configured in Play Console.

For an app still classified as a draft, Play can reject closed-track rollout
with `Only releases with status draft may be created on draft app.` Internal
testing remains available. Finish the app setup in Play Console, select the
tested bundle for the closed release, save it, and send the changes for review
from Publishing overview. Reuse the uploaded bundle; this does not need a new
build or version. A saved draft alone is not a review submission.

Confirm the exact version code and track in Play Console's Publishing overview.
With authorized Publisher API access, `GET
/androidpublisher/v3/applications/{packageName}/tracks/alpha/releases` reports
`releaseLifecycleState`: `RELEASE_LIFECYCLE_STATE_DRAFT` is not submitted,
`RELEASE_LIFECYCLE_STATE_IN_REVIEW` confirms submission, and
`RELEASE_LIFECYCLE_STATE_PUBLISHED` confirms rollout. The edit's `completed`
status alone does not prove any of these review states. Do not resubmit or
cancel an existing review just to check its status.

For an internal installation check, obtain the invitation from Internal
testing → Testers in Play Console. The general closed-testing opt-in URL is
not a substitute. Use the same enrolled Google account in the browser and Play
Store, allow enrollment to propagate, then confirm the installed version and
Play installer on a [reserved device](devices.md). A successful sideload does
not verify the Play installation journey. Keep the invitation and tester account
out of public test reports.

If Play supplies only unprotected splits, turn off Automatic protection for the
release in Play Console before uploading its bundle. A per-release opt-out does
not disable protection for later releases. The workflow fails rather than
publishing a protected or incomplete APK. See Google's
[unprotected APK instructions](https://support.google.com/googleplay/android-developer/answer/10183279?hl=en).

Run the packaging regression checks without credentials or a device:

```bash
python3 -m unittest discover -s tools/build -p 'test_android_*.py'
python3 tools/build/android_apk.py verify 1.0.9 path/to/capycanvas-1.0.9-android.apk
```

Use the version of the APK being checked. Before release, test the exact download
on private Android installs with and without Google services: first launch
offline, painting, import, save/reopen, export, restart and updating an existing
installation without losing drawings. Google service independence does not
relax the [Android GPU requirements](android.md).

### Linux Flatpak

`packaging/flatpak/build.sh` builds against `org.gnome.Platform//50` and
`org.gnome.Sdk//50`, reusing `apps/layer-linux/package.mjs` and its patched GTK
runtime. `packaging/flatpak/export.sh` exports `art.capycanvas.CapyCanvas` on the
`stable` branch. Tagged CI builds sign the application commits and repository
summary with the release key and add the reference and repository archive; the
key's public half is `packaging/flatpak/release-key.asc`.

AppStream catalog generation runs with temporary `.Devel` build metadata so the
SDK's Glycin icon loader can run without a desktop portal during builds. The
exported application keeps `art.capycanvas.CapyCanvas` as its identity.

With Flatpak and its host SVG image loader installed (`librsvg2-common` on
Debian or Ubuntu), build and export a local unsigned bundle:

```bash
bash packaging/flatpak/build.sh
bash packaging/flatpak/export.sh
```

The local output is `dist/flatpak/capycanvas-<version>-linux-x86_64.flatpak`.
Unsigned builds do not produce the reference or repository archive and do not
configure the published update source. Native distribution builds can also use
the [Arch recipe](../../packaging/arch/README.md).

Users need their distribution's Flatpak package, a Wayland session and hardware
Vulkan support. The Flatpak runtime supplies toolkit dependencies; it does not
remove the [canvas requirements](linux.md#prerequisites).

Open the release's `capycanvas.flatpakref` in Fedora Software and choose Install,
or use `flatpak install capycanvas.flatpakref`, to download the application from
`https://capyatelier.github.io/capycanvas/flatpak/`. The standalone
signed `capycanvas-<version>-linux-x86_64.flatpak` bundle installs the same
application and records that repository as its update source. Both include the
public key so Flatpak can verify repository signatures. Software manages later
updates according to its update settings; the command-line equivalent is
`flatpak update`.

Generate store images with the [GTK capture helper](store-screenshots.md).
Artwork, recipes and published images belong in `capycanvas-web`; AppStream
references their public HTTPS URLs.

The sandbox grants the Wayland socket for windows, clipboard and input, and GPU
devices for rendering. The battery indicator reads the kernel's
`/sys/class/power_supply` data on a worker at startup and every 30 seconds;
Flatpak already exposes these files and their device targets read-only. It needs
no system-service or extra filesystem permission. System batteries are combined
by their energy capacities; peripheral batteries are excluded. Unavailable or
incomplete multi-battery readings hide the indicator. GTK uses the default portals
for file dialogs, selected-file access and opening links; the clock observes GNOME's time format through the
Settings portal. Settings, workspace libraries, recovery files and
caches use Flatpak's private application directories. The exported desktop entry
forwards files through the document portal when launched from a file manager.
Software's permission summary does not list these per-file portal grants as
access to the user's folders.

If opening a selected file fails with `Transport endpoint is not connected`,
check the document portal's FUSE mount with
`findmnt -T "$XDG_RUNTIME_DIR/doc"`; its filesystem type should be `fuse.portal`.
A running `xdg-document-portal` service can still have a disconnected or missing
mount. Close Flatpak applications before repairing the shared service with
`systemctl --user restart xdg-document-portal.service`, then reopen them so their
sandboxes receive the restored mount.

The repository archive contains the OSTree objects and metadata needed to serve
updates. The bundle and reference alone cannot supply a Flatpak repository.
`.github/workflows/flatpak-publish.yml` runs when a GitHub Release is published,
downloads its repository archive and deploys its contents under `flatpak/` on
GitHub Pages. It also accepts a tag through `workflow_dispatch` to repeat a
deployment. Only the latest published release that is neither a draft nor a
prerelease can deploy, so an older release cannot replace the update source.
Repository settings must select GitHub Actions as the Pages source and allow
`main` and `v*` tags in the `github-pages` environment's deployment policies.

### Signing credentials

The `release` environment holds the signing material; restrict it to `v*` tags
and require a maintainer's approval.

| Name | Kind | Use |
| --- | --- | --- |
| `ANDROID_UPLOAD_KEYSTORE`, `ANDROID_UPLOAD_KEYSTORE_PASSWORD` | Secrets | Base64 PKCS12 upload keystore with the alias `upload`, and its password |
| `APPLE_API_KEY` | Secret | Base64 App Store Connect team API key (`.p8`) with the Admin role, for cloud signing, upload and notarization |
| `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`, `APPLE_TEAM_ID` | Variables | That key's identifiers and the team |
| `APPLE_DEVELOPMENT_P12`, `APPLE_DEVELOPMENT_PASSWORD` | Secrets | Base64 PKCS12 Apple Development certificate with its private key, and its password, reused for iPad archives |
| `APPLE_DEVELOPER_ID_P12`, `APPLE_DEVELOPER_ID_PASSWORD` | Secrets | Base64 Developer ID Application certificate and its password; Xcode cannot cloud-sign Developer ID builds with an API key |
| `GCP_WORKLOAD_IDENTITY_PROVIDER`, `GCP_SERVICE_ACCOUNT` | Variables | The Google Cloud workload identity provider that trusts the `release` environment, and the service account it acts as; Play Console grants that account release access |
| `AZURE_CLIENT_ID`, `AZURE_TENANT_ID` | Variables | The Microsoft Entra app the Windows job signs in as through OIDC; it holds the Artifact Signing Certificate Profile Signer role and trusts the `release` environment |
| `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE` | Variables | The Azure Artifact Signing account's regional endpoint, its name and the Public Trust certificate profile |
| `FLATPAK_GPG_PRIVATE_KEY`, `FLATPAK_GPG_PASSPHRASE` | Secrets | ASCII-armored GPG private key, without base64 encoding, and its passphrase for signing Flatpak commits and the repository summary |
| `FLATPAK_GPG_FINGERPRINT` | Variable | The full fingerprint of the Flatpak release key; its public key is `packaging/flatpak/release-key.asc` |

Each Apple job imports its stored signing identity into a temporary keychain and
checks that the certificate has a valid private key before building. The iPad
archive reuses an Apple Development identity; App Store export uses cloud
distribution signing through the API key. Retain the development identity across
runs: disposable runners otherwise create certificates whose private keys are
lost when the job ends. Rotate the stored identity before it expires, and revoke
an old certificate only after confirming no active developer or build uses it.

Locally, `CAPY_UPLOAD_KEYSTORE` and `CAPY_UPLOAD_KEYSTORE_PASSWORD` sign the
Android bundle, and `CAPY_APPLE_TEAM`, `CAPY_APPLE_KEY`, `CAPY_APPLE_KEY_ID` and
`CAPY_APPLE_ISSUER` make `release.sh` sign, upload or notarize. On a tag, the
Windows job passes Azure Artifact Signing's `signtool` plugin to the packagers'
`-SignArguments` ([Windows](windows.md#portable-zip)), signing the app
executables and the setup program; the Store signs the MSIX.

Keep encrypted backups of the private signing material and its passwords in
maintainer-controlled storage outside the checkout. The secret names above
describe what must be backed up, not where those backups live. Reuse working
signing identities; do not generate or revoke certificates to troubleshoot an
unrelated publishing error. Ignore rules help prevent accidental staging but
do not make a credential safe to publish.

## Publishing a release

1. Bump the version on `main` and push a tag for that commit:
   `git tag -a v1.0.0 -m 'Capy Canvas 1.0.0' && git push origin v1.0.0`.
2. Test the exact draft downloads and the TestFlight build: the
   [checks](testing.md) and user journeys on every host, installation, updating
   from the previous release with existing drawings, and the
   [performance targets](../PERFORMANCE_TARGETS.md) on reference hardware.
   If the release crosses an intentional pre-release format change, follow the
   compatibility policy in [AGENTS.md](../../AGENTS.md). Warn testers before
   updating to export flattened PNGs and retain original drawing files; include
   the limitation in release and beta notes. Report incompatible old-file
   editing or recovery separately from current-format save/reopen results.
3. Confirm the internal-track installation, run the Android promotion workflow
   with `track=alpha`, upload the
   MSIX to Partner Center, and submit the TestFlight build for review.
4. Publish the draft. With immutable releases enabled, its assets and tag can no
   longer change. Wait for the Flatpak publishing workflow and verify that the
   published reference installs from the Pages repository and that Flatpak
   accepts its signatures. Check updates from the preceding Flatpak release
   when one exists.
5. Update the website and editor as described below, then verify the live
   download links, release notes and editor source revision.

If an application check requires a code fix, discard the unpublished draft,
fix `main` and release the next patch version. Never move a tag or reuse a
version. A delivery or store-setup failure can be retried with the preserved,
unchanged build; follow the Android recovery steps above. Record each channel's
actual state and any failed or unavailable checks in local release evidence.

### Store submission is separate from upload

For iPad, wait for App Store Connect processing, select the exact version/build
in TestFlight, complete test information and export-compliance questions, and
add it to the intended external group. Follow Apple's
[external-testing steps](https://developer.apple.com/help/app-store-connect/test-a-beta-version/invite-external-testers/)
to submit for beta review or start testing when eligible. Verify the beta review
state and tester availability separately. An uploaded build or membership in an
internal group does not confirm external distribution. TestFlight beta review
and production App Store submission are separate actions.

For Windows, CI produces the MSIX but does not submit it to Microsoft Store.
Upload it through an authorized Partner Center session and verify submission
status there. Azure signing access alone does not establish Partner Center
access. If access is unavailable, report Store submission as incomplete; the
published signed installer is a separate delivery channel.

### Website and online editor

The marketing site reads published GitHub Releases when it builds. After the
release is public, dispatch its workflow:

```bash
gh workflow run deploy.yml --repo capyatelier/capycanvas-web
```

The editor is deployed separately from a tested source tag. Replace the example
tag with the release being published:

```bash
gh workflow run deploy.yml --repo capyatelier/capycanvas-release -f ref=v1.0.11
```

Wait for the site's build, browser checks and Pages deployment, and for both
the editor's `deploy.yml` and triggered `pages.yml`. Verify the public
[downloads](https://capycanvas.art/download/) and
[release notes](https://capycanvas.art/download/past-versions/), including any
compatibility warning. Verify [the editor](https://editor.capycanvas.art/)
against the deployed repository's `release/source.json`. Pages caches can
temporarily serve an older deployment after Actions succeeds; check the live
content before reporting the update complete. A release asset alone does not
update either site.
