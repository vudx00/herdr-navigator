# Fuzzy Search Relevance and Navigation

## Plan

- [x] Confirm the repository is clean and create `fix/fuzzy-search-relevance`.
- [x] Trace candidate construction, fuzzy scoring, sorting, selection, and key dispatch.
- [x] Define deterministic relevance rules that prioritize the best matching entry and use shorter paths as a tie-breaker without hiding valid fuzzy matches.
- [x] Add regression coverage for the reported `work` ordering and result-to-preview association.
- [x] Implement the minimal ranking correction.
- [x] Bind Alt+J and Alt+K to down/up movement in all navigator input modes while preserving existing arrows and Vim-mode bindings.
- [x] Exercise focused behavior, then run formatting, tests, linting, and the build.
- [x] Record the verified results below and commit the completed fix.

## Review

Root cause: the matcher scored one concatenated source/title/path/search string. Ordered fuzzy characters could cross field boundaries (for example, `wor` in `word_freq` plus `k` in a later path segment), while equal scores fell back to alphabetical titles instead of shorter paths.

Implemented:

- Precompute and score independent normalized fields, including title, basename, full path, path components, metadata, source, and aliases.
- Rank exact, prefix, and substring field matches ahead of generic fuzzy matches; prefer higher-value fields, then matcher score, then shorter paths.
- Keep result indexes and scores aligned through the existing candidate vector.
- Bind Alt+J and Alt+K as always-active down/up commands and expose them in help/footer hints.

Verified:

- Both new fuzzy regression tests failed before the fix and pass for `nucleo`, `skim`, and `simple` afterward.
- The Alt+J/Alt+K handler regression failed before the binding and passes afterward in search mode.
- `cargo fmt --check`, 82 tests, strict Clippy, and `cargo build` pass.
- `cargo run --quiet -- list` starts the production binary and collects the live Navigator sources successfully.

---

# Herdr Pane Alt Navigation Override

## Plan

- [x] Confirm Herdr intercepts direct `Alt-J`/`Alt-K` focus bindings before forwarding input to normal panes.
- [x] Confirm plugin popups receive terminal input before Herdr evaluates global pane-navigation bindings.
- [x] Change the main Navigator entrypoint from a normal overlay pane to a full-size modal popup.
- [x] Require the oldest Herdr release that supports plugin popups.
- [x] Remove obsolete overlay focus/reuse code and update regression coverage.
- [x] Record the correction lesson and update user-facing documentation.
- [x] Validate the linked manifest, open/close the popup, and exercise Alt navigation.
- [x] Run formatting, tests, strict Clippy, and release build.
- [x] Commit, merge, push, clean up, and relink the installed plugin.

## Review

Root cause: Herdr evaluates direct pane-focus bindings before forwarding keys to normal overlay, split, tab, or zoomed panes. The prior TUI test proved Navigator handled Alt-J/Alt-K only after receipt; it did not prove the host delivered those events.

Implemented:

- Changed the main `picker` manifest entrypoint to a 100% by 100% modal plugin popup, which Herdr routes before global direct bindings.
- Raised `min_herdr_version` to 0.7.4, the first release with plugin popup placement.
- Removed obsolete overlay-pane discovery/focus logic; the persistent side picker remains a normal split.
- Added a parsed-manifest regression and recorded the host-routing lesson.

Verified:

- The popup manifest regression failed before the fix and passes afterward.
- The existing Alt-J/Alt-K search-mode regression passes.
- Herdr 0.7.5 accepted and linked the popup manifest with no reload diagnostics.
- The installed `herdr-navigator.open` action launched the modal popup successfully; it remained active until manually closed.
- `cargo fmt --check`, 82 tests, strict Clippy, and the release build pass.
