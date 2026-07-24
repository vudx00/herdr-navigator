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
