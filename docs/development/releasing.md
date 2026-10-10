# Releasing

[Developer guide](README.md)

Use this guide to release Linux, Web, Android, Windows, macOS and iPadOS from
one tested source tag. Run commands from the repository root. Platform guides
provide test and build details; the release sequence lives here.

## Distribution channels

| Platform | Release channel today | What the agent must finish |
| --- | --- | --- |
| Linux x86_64 | Signed Flatpak download and project-hosted update repository | Publish the GitHub Release, deploy Flatpak updates, verify installation and updating. Flathub app publication is not configured by these workflows. |
| Web | Downloadable static/PWA ZIP and `editor.capycanvas.art` | Publish the ZIP and deploy the editor from the same tag; verify the live revision and offline/update journeys. |
| Android ARM64 | Direct Play-signed APK and working Play internal testing | Verify both installation paths. Closed testing (`alpha`) may be awaiting Google review; check its state before promotion. Production Play rollout is outside this procedure. |
| Windows x64 | Signed installer and portable ZIP | Verify both downloads. Microsoft Store is not set up; retain the generated MSIX and report Store submission as deferred. |
| macOS Apple silicon | Signed, notarized DMG | Verify download, Gatekeeper, installation and updating. The workflow does not submit a Mac App Store build. |
| iPadOS | Working TestFlight beta | Verify the exact build and its availability to the intended tester group. Beta review and production App Store submission are separate actions. |

Check current account and review states before each release. A public beta link,
successful upload or previous release does not prove that this build is available
to testers. Store setup and production launch are separate tasks; complete the
configured channels and report deferred channels explicitly.

## Publishing a release

### 1. Check access and readiness

Use the assigned worktree and install the [Git hooks](../COMMIT_GUIDE.md).
Confirm authorized Actions access to `capyatelier/capycanvas`,
`capyatelier/capycanvas-release` and `capyatelier/capycanvas-web`, approval access
to the protected `release` environment, and the configured
[signing credentials](#signing-credentials). Confirm Play upload/internal testing
access and App Store Connect upload/TestFlight access. Check existing reviews
before changing either testing channel.

**Pushing a release tag has store side effects.** It signs packages, uploads to
Play, rolls out internal testing, and uploads the iPad build to App Store Connect.
Every platform job must succeed before CI creates the draft GitHub Release.
Missing Play or Apple upload access blocks that draft; missing Microsoft Store
setup does not. There is no platform-skip input. Restore missing required access
before tagging; an unsigned build is not a substitute for signed distribution.

Run the [checks for the changes](testing.md), the
[publication checks](publication.md#separate-binary-release-gate) and affected
[performance measurements](../PERFORMANCE_TARGETS.md) on reference hardware.
Arrange private test profiles and [reserved devices](devices.md) for final
package verification on every host. Keep sanitized evidence under
`artifacts/release/<tag>/`; record the tag, commit, workflow runs, package hashes,
journeys, failures and each channel's actual state.

### 2. Prepare one version and source commit

Choose an unused version under the [version rules](#versions), edit the workspace
version in `Cargo.toml`, then regenerate derived files:

```bash
cargo update --workspace --offline
python3 apps/layer-apple/scripts/project.py
```

Add the matching `<release>` with notes in words a painter knows to
`apps/layer-linux/art.capycanvas.CapyCanvas.metainfo.xml`. The workflow shares
these notes with Google Play, which allows
[500 Unicode characters per language](https://support.google.com/googleplay/android-developer/answer/9859348).
Keep any compatibility warning within that limit; validation runs before platform
jobs and before a direct Play upload. Commit these files together. Fetch and
rebase onto `origin/main`, rerun checks, and land the tested
commit on `origin/main` under the [commit guide](../COMMIT_GUIDE.md).
Set these variables to that version and commit, not a later moving `main`:

```bash
CAPY_RELEASE_VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml)
CAPY_RELEASE_TAG="v$CAPY_RELEASE_VERSION"
CAPY_RELEASE_COMMIT=$(git rev-parse HEAD)
gh run list --repo capyatelier/capycanvas --workflow ci.yml \
  --commit "$CAPY_RELEASE_COMMIT" --json databaseId,status,conclusion,url
```

Wait for CI on that commit to pass. Optional build rehearsal:
`gh workflow run release.yml --repo capyatelier/capycanvas --ref main`.
This is unsigned only when dispatched on a branch; dispatching on a release tag
performs the same signing and store uploads as pushing it. Confirm the rehearsal's
source SHA, since `main` can advance. It does not publish or verify user journeys,
and the unsigned iPad archive is not retained as a downloadable artifact.

### 3. Tag and collect the signed candidate

```bash
git tag -a "$CAPY_RELEASE_TAG" "$CAPY_RELEASE_COMMIT" -m "Capy Canvas $CAPY_RELEASE_VERSION"
git push origin "$CAPY_RELEASE_TAG"
gh run list --repo capyatelier/capycanvas --workflow release.yml \
  --branch "$CAPY_RELEASE_TAG" --json databaseId,headSha,status,conclusion,url
```

Approve the protected environment when required. Follow the run for this tag and
commit until all jobs finish. Use `gh run view <run-id> --repo
capyatelier/capycanvas` to inspect results. If a job fails, follow
[recovery](#recovering-a-failed-release) before retrying uploads.

CI creates a draft with the [package outputs](#release-workflow), `SHA256SUMS`
and provenance attestations. Confirm every expected asset is present, download
to a fresh directory, and check its hashes:

```bash
gh release download "$CAPY_RELEASE_TAG" --repo capyatelier/capycanvas \
  --dir "artifacts/release/$CAPY_RELEASE_TAG/downloads"
(cd "artifacts/release/$CAPY_RELEASE_TAG/downloads" && sha256sum --check SHA256SUMS)
```

### 4. Verify the exact packages and beta build

On each supported host, test first launch, painting, import, save/reopen, export,
restart, installation and updating from the previous release with fixture
drawings. Exercise UI in light and dark themes. Use private profiles and test
identities under [testing](testing.md) and [devices](devices.md); never replace a
user's installation or use their drawings. Record any limitation in verifying
the production identity rather than treating a rebuilt test package as the
exact download.

Verify Android's downloaded APK without Google services and Play internal
installation separately ([Android checks](#android-apk)). On macOS check the
downloaded DMG's signature and Gatekeeper acceptance; on Windows check signatures
of the executables inside the ZIP and of the installer, plus installation/update
journeys under [Windows packaging](windows.md#package). Test the Web ZIP's offline
and update journeys under [Web packaging](web-packaging.md#preview-and-test). Verify the
TestFlight build number derived from this version on a reserved iPad.

For an intentional pre-release format change, follow [AGENTS.md](../../AGENTS.md):
include the limitation in release and beta notes, preserve original fixture
files, and tell testers to export flattened PNGs before updating. Report old-file
editing/recovery separately from current-format save/reopen. A failed package
check needs a fix and new version before publication.

### 5. Complete the configured store channels

Check Play Console for the exact internal version and any closed-track review.
If that version is already in review, record the pending state and leave the
review running. If a newer version needs promotion and no review is pending, use
the
[Android promotion procedure](#android-apk). An internal-only release can finish
with closed testing reported as awaiting review or setup; never cancel a review
to force the release through.

Complete the [TestFlight procedure](#store-submission-is-separate-from-upload)
for the intended tester group. Keep Microsoft Store deferred until its setup and
submission are requested; its unsigned MSIX is for Partner Center, not a signed
direct download. Report beta review as submitted, awaiting review or available,
according to the actual state. Do not claim store production availability.

### 6. Publish and deploy all public surfaces

Publish the tested draft through GitHub Releases or:

```bash
gh release edit "$CAPY_RELEASE_TAG" --repo capyatelier/capycanvas --draft=false
```

With immutable releases enabled, assets and tag cannot change afterward. Keep
this a stable GitHub Release: Flatpak updates deploy only from the latest
published release that is neither a draft nor a prerelease, even while mobile
distribution remains in beta.

Wait for `flatpak-publish.yml`, verify the public reference and repository
signatures, and test installation and updates from the preceding Flatpak release
in an isolated environment ([Linux Flatpak](#linux-flatpak)). Then deploy the
[website and online editor](#website-and-online-editor). Website deployment is
a required release step; the release and editor workflows do not refresh it.

### 7. Report completion by channel

Verify live downloads and release notes, the editor's source commit, the Flatpak
update source, Play internal installation and TestFlight tester availability.
Report the version/tag and evidence, each delivered channel, pending reviews,
deferred stores, failures and unverified journeys. Required checks or deployments
still failing make the release incomplete; Microsoft Store awaiting setup and
closed testing awaiting review must stay visible in the report.

## Store submission is separate from upload

For iPad, wait for App Store Connect processing, select the exact version/build
in TestFlight, complete test information and export-compliance questions, and
add it to the intended external group. Follow Apple's
[external-testing steps](https://developer.apple.com/help/app-store-connect/test-a-beta-version/invite-external-testers/)
to submit for beta review or start testing when eligible. Verify the beta review
state and tester availability separately. An uploaded build or membership in an
internal group does not confirm external distribution. TestFlight beta review
and production App Store submission are separate actions.

With authorized API-key access, the same steps can use App Store Connect's API:
set the shared notes as
[`betaBuildLocalizations.whatsNew`](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-betabuildlocalizations),
submit a
[`betaAppReviewSubmission`](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-betaappreviewsubmissions),
and [attach the existing beta group](https://developer.apple.com/documentation/appstoreconnectapi/post-v1-builds-_id_-relationships-betagroups).
Read the exact build with `buildBetaDetail` and `betaGroups` included to verify
its state and group membership. `WAITING_FOR_BETA_REVIEW` means review is pending;
`IN_BETA_TESTING` means beta distribution is enabled. Preserve existing groups
and testers.

For Windows, CI produces the MSIX but does not submit it to Microsoft Store.
The Store is currently awaiting setup; retain the package without submitting it.
When Store submission is requested and the account is ready, upload it through
an authorized Partner Center session and verify submission status there.
Azure signing access alone does not establish Partner Center
access. If access is unavailable, report Store submission as incomplete; the
published signed installer is a separate delivery channel.

## Website and online editor

The marketing site reads published GitHub Releases when it builds. After the
release is public, dispatch its workflow:

```bash
gh workflow run deploy.yml --repo capyatelier/capycanvas-web
```

The editor is deployed separately from the tested source tag:

```bash
gh workflow run deploy.yml --repo capyatelier/capycanvas-release -f ref="$CAPY_RELEASE_TAG"
```

Wait for the site's build, browser checks and Pages deployment, and for both
the editor's `deploy.yml` and triggered `pages.yml`. Verify the public
[downloads](https://capycanvas.art/download/) and
[release notes](https://capycanvas.art/download/past-versions/), including any
compatibility warning. Verify [the editor](https://editor.capycanvas.art/)
against the deployed repository's `release/source.json`: its `commit` must match
`CAPY_RELEASE_COMMIT`. Compare the live `index.html` and Wasm hashes with
`release/manifest.json` in that repository. Pages caches can temporarily serve an
older deployment after Actions succeeds; check the live
content before reporting the update complete. A release asset alone does not
update either site.

## Recovering a failed release

If an application check requires a code fix, discard the unpublished draft,
fix `main` and release the next patch version. Never move a tag or reuse a
version. A delivery or store-setup failure can be retried with the preserved,
unchanged build. Download retained artifacts with `gh run download <run-id>
--repo capyatelier/capycanvas --dir artifacts/release/<tag>/recovered` and follow
the [Android recovery steps](#android-apk). Inspect completed store uploads before
rerunning any job: rerunning the successful Apple iPad job repeats the same
version/build upload, and the workflow has no duplicate-upload recovery for it.
Rerun only failed jobs when safe, rather than the whole release workflow.

For a published release, repeat delivery with the unchanged assets: use
`gh workflow run flatpak-publish.yml --repo capyatelier/capycanvas -f tag=<tag>`
for the latest stable release, or repeat the
[website/editor deployment](#website-and-online-editor).
These retries do not require a new application version. Recheck live results and
record any channel still blocked; never replace immutable assets or move the tag.

## Access and public documentation

The [security policy](../../SECURITY.md) covers private vulnerability reporting
and repository protections. Security automation opens findings and update pull
requests; publication still follows the testing and signing steps in this guide.

Read current workflows before releasing. Obtain missing access privately from
the maintainer; never request credentials in an issue or commit.

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
- The AppStream release entry's `<description>` holds the release notes: a short
  paragraph and a list of what changed, in words a painter knows. Linux software
  centers show it, the GitHub Release starts with it, and the release workflow
  refuses a version without it.

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

Dispatching on a branch checks unsigned builds and saves downloadable artifacts
for every platform except iPad. Dispatching on a tag or pushing it follows the
signed release path. CI checks that the tag names the workspace version and its
commit belongs to `main`, and runs `cargo deny`. All platform jobs must pass
before the draft release job runs; neither store processing nor tester
availability is checked by that job.

### Android APK

CI downloads and verifies a standalone Play-signed APK that opens without Google
services or Play licensing checks. The
[Android distribution guide](android.md#distribution-apk-verification) describes
protection settings, signing verification and local checks.

The Android job saves the `android-bundle` artifact before uploading to Play,
which consumes the version number. It saves the verified APK separately as
`android-apk` before internal rollout. If a later step fails, recover these
artifacts from that workflow run instead of rebuilding or uploading the same
version. `android_publish.py upload` checks Play's version and SHA-256 against
the saved AAB and reuses an identical existing upload. Both upload and promotion
refuse to cancel changes already in review. Retry APK download with
`android_apk.py download`; retry internal rollout with the promotion workflow
below. A code change still needs a new version.

After a failed Play commit, inspect current bundles and tracks before retrying
the upload with the saved AAB. For an Android-only CI retry, commit that original
AAB to Play first: the upload script then refuses a rebuilt AAB with a different hash.
Use `gh run rerun <run-id> --repo capyatelier/capycanvas --job <android-job-id>`.
Preserve and hash-check the failed attempt's `android-bundle` before removing
that specific Actions artifact to allow the retry to save an artifact with the
same name. Keep successful Apple jobs out of the retry.

After testing the final internal build, check closed-track setup and review state.
When promotion is needed and no changes are in review, run **Promote Android
testing release** from its release tag with `track=alpha`. This promotes the
existing bundle to the `alpha` closed track, preserving release notes, and submits
the edit for review:

```bash
gh workflow run android-promote.yml --repo capyatelier/capycanvas \
  --ref "$CAPY_RELEASE_TAG" -f track=alpha
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

### Linux Flatpak

`packaging/flatpak/build.sh` builds against `org.gnome.Platform//50` and
`org.gnome.Sdk//50`, reusing `apps/layer-linux/package.mjs` and its patched GTK
runtime. `packaging/flatpak/export.sh` exports `art.capycanvas.CapyCanvas` on the
`stable` branch. Tagged CI builds sign the application commits and repository
summary with the release key and add the reference and repository archive; the
key's public half is `packaging/flatpak/release-key.asc`.

For local unsigned builds and runtime requirements, use the
[Linux packaging guide](linux.md#flatpak-runtime-and-portals).

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
checks that the certificate has a valid private key before building. Signing
expressions reference each secret by name so the runner receives only explicitly
referenced secrets. A missing platform credential fails instead of selecting the
other platform's identity. The iPad archive reuses an Apple Development identity;
App Store export uses cloud
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
