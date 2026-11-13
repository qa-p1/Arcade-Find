# Arcade Find v1 progress

Detail for `qa-p1/Arcade-catalog` > `dev/v1/PROGRESS.md`. Newest first.

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
