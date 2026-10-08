# Security policy

## Report a vulnerability

Use [private vulnerability reporting](https://github.com/capyatelier/capycanvas/security/advisories/new)
for security problems in Capy Canvas or its release tooling. Include the affected
version or commit, platform, expected impact, and steps to reproduce. Use a
small synthetic document when a drawing or imported file is needed.

Keep vulnerability details out of public issues and pull requests until a fix
and disclosure have been coordinated. Do not include credentials, signing keys,
private artwork or other people's personal information in reports. Ordinary
bugs and feature requests belong in public issues.

Capy Canvas is in active pre-release development. Check the latest published
build when practical, and identify the affected version even if an older build
is required to reproduce the problem. Security fixes are developed on `main`
and delivered in a new release.

## Repository protections

Secret scanning, push protection, Dependabot security updates and private
vulnerability reporting are enabled in GitHub settings. The `Protect main
history` ruleset blocks deleting or force-pushing `main`; normal fast-forward
pushes remain available. The [CodeQL workflow](.github/workflows/codeql.yml)
scans pushes and pull requests to `main`, and runs weekly. It compiles Android
Kotlin and both Apple clients explicitly; the other supported languages use
analysis without a full build. Scanner findings are separate from app tests.
Keep advanced setup enabled: default setup cannot find the Android build.

Dependabot checks pinned GitHub Actions weekly through
[its configuration](.github/dependabot.yml). Review update pull requests and
their checks before merging. The [release guide](docs/development/releasing.md)
describes credential handling, signing and publication.
