# Packaging and releases

`Cargo.toml` workspace version is the single version source. The shortcut
sheet records that version and `scripts/check-version.py` verifies the build.
`scripts/package` builds the shipped AppImage, universal DMG or Inno Setup
installer for the current platform. CI exercises installation and removal
for all three, plus AppImage repair and re-execution.

## Release flow

Keep the version bump and dated changelog in the implementation PR. Merge
with a merge commit only after all checks and visual review pass. Dispatch
`tag.yml` with `tag=v<version>` and the full 40-character merge SHA; do not
push tags manually. Then dispatch `release.yml` on that tag with
`channel=stable` and `draft=false`. Workflow-created tags do not trigger
another workflow automatically.

The first use of this flow needs a draft beta rehearsal on main before the
old tag-push publisher is removed. Dispatch `release.yml` with `channel=beta`
and `draft=true`. Beta metadata uses `nightly`, because the manifest schema
accepts only `stable` and `nightly`. The beta stays private for the owner.

The release contains three installers, `arcade-release.json` and
`SHA256SUMS.txt`. Download the files through the REST asset API, check every
checksum and manifest size/hash, and verify the stable tag points at the
expected merge SHA. Confirm `releases/latest` and the real catalog repo.
Use Tools 0.4.1's real update scheduler with the prior installed Find version
and verify every selected platform download before declaring the release
complete. A published release alone is insufficient evidence.
