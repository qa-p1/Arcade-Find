# Arcade Find v1 progress

Detail for `qa-p1/Arcade-catalog` > `dev/v1/PROGRESS.md`. Newest first.

## 2026-11-14 (cloud session, branch `claude/sharp-cori-29w5eg`)

- Step 6 (6e2fc47): rc-only releases through Link `app-publish` (Inno), `promote.yml`, `nightly.yml`; the stage job
  keeps Find's own installer checks. CI saves Rust caches only on main pushes (f7d2b38).
- Step 2, untangle (in progress): module dependency graph scripted from `crate::` paths. Done: one `identity` module
  (D11: `dev.arcade.Find` was defined twice; installer and tray imported Settings for the tagline), one `session`
  module (D12: three Wayland checks, two Hyprland detections, two `hyprctl -j` runners). Kept on purpose: two cycles
  (D13). Left: the Settings window (1,810 lines) and `ui::model`; then the comments and tests extras.
- Step 4, dead code: `cargo machete` finds no unused dependency; `clippy -D warnings` (dead code included) is clean.
- Tested: `cargo +1.88.0 clippy --workspace --all-targets -D warnings`, `cargo +1.88.0 test --workspace` (all pass).

## 2026-11-14 (Link v1.0.7)

- PR #18 CI fully green at e7d1403 (9 of 9 checks, incl. the headless sway + Xvfb end-to-end run, after its two
  scripts were moved to the new Escape ladder and focus rule: 7cece31, e7d1403).
- Find pins Arcade Link `v1.0.7` (Cargo, design kit via `scripts/sync-design.py`, the three runner clones in ci.yml,
  THIRD_PARTY_NOTICES). It lists `arcade-find` in `legacy_desktop_ids`, so 1.0.7's installer retires that entry.

## 2026-11-13 (step 3: fixes, PR #18)

Every FIND.md finding is fixed on `claude/tender-feynman-3vt7dg` ([PR #18](https://github.com/qa-p1/Arcade-Find/pull/18)),
each with a regression test; the step-1 lock markers are gone (no `KNOWN BUG` left).

| Finding | Commit | Test |
|---|---|---|
| FD-1, FD-5, FD-6 (CLI) | 1f65db6 | `tests/cli_search.rs` exit-code matrix, `json_failures_exit_nonzero`, `an_invalid_filter_next_to_a_valid_term_is_an_error`, `limit_zero_is_rejected`, `an_offline_limit_above_a_thousand_is_honoured` |
| FD-3 (newline names) | deb2785 | `content::tests::finds_files_whose_name_has_a_newline` (needs `rg`) |
| FD-7, FD-2, FD-4 (overlay) | bdd8ebd | `escape_ladder_clears_the_query_before_closing`, `escape_hint_says_clear_while_text_is_typed`, `focus_loss_never_closes_the_popup`, `twenty_rows_fit_a_1080p_screen` |
| FD-8 + memory FD1 | 51c69a0 | `watch::tests::a_rescan_clears_a_stale_watch_error`, `index::tests::shrink_to_fit_returns_spare_capacity` |
| FD-9, FD-10, RD-FIND-2/3/4 | cda00b4 | `the_hyprland_section_is_gated_on_hyprland`, `nested_roots_are_refused_and_parents_replace_children`, `drives_are_named_by_label_then_device`, `short_scans_show_milliseconds` |

Left for the next steps: move to Link >= 1.0.7 (fleet "One launcher per app"; also brings `runtime::trim` for W0 and
the unpacked runtime U1), untangle (step 2), dead code, memory budget measurement, CI targets, Round 5, release.
The FD-2 and FD-4 behaviour on a real compositor and drag and drop are for the desktop harness (`tests/desktop/`).

## 2026-11-10 (code-writing pass)

Step 0 (brief): done. See the commits below as they land.
