# Packaging and releases

`Cargo.toml` workspace version is the single version source. The shortcut
sheet records that version and `scripts/check-version.py` verifies the build.
`scripts/package` builds the shipped AppImage, universal DMG or Inno Setup
installer for the current platform. CI exercises installation and removal
for all three, plus AppImage repair and re-execution.

## Release flow

Stable is never built: it is a verified release candidate promoted byte for
byte. Keep the version bump and dated changelog in the implementation PR and
merge with a merge commit only after all checks and visual review pass. Then:

1. Dispatch "Tag a release" (`tag.yml`) with `tag=v<version>-rc.<n>` and the
   full 40-character merge SHA; do not push tags manually. It creates the tag
   and starts "Release" (`release.yml`) on it, which runs CI, verifies the three
   installers (`scripts/prepare-release.py`) and publishes them through Link's
   `app-publish` as a hidden prerelease on the `test` channel, with the Inno
   installer's switches in `arcade-release.json`.
2. Run every exit gate against those published files (`verify-release.yml`
   checks the assets, checksums and manifest of a published tag).
3. Dispatch "Promote" (`promote.yml`) with the rc tag. Link's `app-promote`
   republishes the same files as `v<version>` with a signed `manifest.json`,
   downloads them again and compares every hash.

"Nightly" (`nightly.yml`, daily) republishes main CI's installers as a signed
`v<version>-nightly.*` prerelease when main has changed and its CI is green.

The release contains three installers, `arcade-release.json` and
`SHA256SUMS.txt`. Download the files through the REST asset API, check every
checksum and manifest size/hash, and verify the stable tag points at the
expected merge SHA. Confirm `releases/latest` and the real catalog repo.
Use Tools 0.4.1's real update scheduler with the prior installed Find version
and verify every selected platform download before declaring the release
complete. A published release alone is insufficient evidence.
