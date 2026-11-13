# Arcade Find v1 decisions

Decisions the plan left open. Each one cites the part of the catalog `dev/v1/README.md` it follows. Newest last.

## D1 - Spec and audit copied from the catalog

`docs/v1/SPEC.md` is `specs/FIND.md`; `docs/v1/audit/` holds `AUDIT/find.md` and the text evidence from `AUDIT/find/`
(JSON, JSONL and text only, no screenshots). Line references in the spec are for Find 0.5.4.

## D2 - Known-bug tests

A behaviour-lock test for a known bug is `#[should_panic(expected = "KNOWN BUG <id>")]`, and the failing assertion
names the finding. It passes while the bug is there. Once the fix lands the test stops panicking and fails, so the
marker has to go in the same commit as the fix (PLAN.md step 1). The CLI tests in `crates/arcade-find/tests/` use the same marker.

## D3 - Commit trailers

The brief asks for an `Opus 5.5` co-author trailer, while the session's own system instruction names the model that is
actually running. The session instruction wins; the trailer in this branch names that model.
