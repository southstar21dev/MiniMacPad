# Releasing

How MiniMacPad is versioned, signed and published, and how the app finds updates.

## Versions

MiniMacPad uses [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

- **Release tags** are the version with a `v`: `v1.2.0`. They are the only place the version lives; `Cargo.toml` stays at `0.0.0`.
- **Local builds** report the latest release tag with build metadata, for example `1.2.0+68.c5b81ac` (commit count and hash), which semver ignores when ordering, or `0.0.0+…` before the first release.
- **The build number** (`CFBundleVersion`) is derived from the version by [`scripts/version.sh`](../scripts/version.sh), so it always rises with it.

## The signing key (once)

Every release must be signed with the same key as the installed app, or the update is refused, and macOS would also forget the app's privacy permissions.

1. `scripts/create-signing-identity.sh` creates the self-signed "MiniMacPad Dev" code-signing identity, saves it in `~/.config/minimacpad/` (outside the repository) and imports it into your login keychain. **Back up `signing.p12` and `signing.password`: without them, installed copies can never be updated again.**
2. `scripts/upload-release-secrets.sh` stores it as the GitHub Actions secrets `MAC_SIGNING_P12_BASE64` and `MAC_SIGNING_P12_PASSWORD`, asking first.

The release workflow checks that every build carries this certificate before publishing it.

## Publishing

Releases are cut by [semantic-release](https://semantic-release.gitbook.io) from [Conventional Commits](https://www.conventionalcommits.org), the same way as in the other southstar21dev apps ([release workflow](../.github/workflows/release.yml), [config](../.releaserc.json)):

1. Merge `dev` into `master` through a pull request. Since the last release, `fix:` or `perf:` commits give a patch release, `feat:` a minor one, and `feat!:` or a `BREAKING CHANGE:` footer a major one. Anything else (`chore:`, `docs:`, `refactor:`, …) releases nothing.
2. A semantic-release dry run picks the version, and the app is built and signed with it.
3. Only when the build passes does semantic-release tag the commit `vX.Y.Z` and publish the GitHub release, with notes generated from the commits and `MiniMacPad-<version>-<build>-mac.zip` attached. `minimacpad-update.json`, a manifest with the version, build number and the file's SHA-256, is attached last. Nothing is committed back to `master`.

## How the app updates

The app asks GitHub for the repository's latest release when it opens, and when you choose **Check for Updates**. It ignores a release without a manifest (still building) and offers one whose version is above its own. Choosing **Update and Relaunch** downloads the zip, checks its SHA-256 against the manifest, checks that it is signed with the same certificate as the running app, swaps the app in place, refreshes the login item's copy of the background service, and reopens. The repository has to be public for this to work.
