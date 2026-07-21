# Follow-up items from the picker hot-path review

Measured per-collector cost against a real Herdr install (release build, 312
entries, 3 roots at depth 3):

| collector | cost |
| --- | --- |
| `collect_zoxide` | 15.5 ms |
| `collect_roots` | 11.6 ms cold / 6.6 ms warm |
| `collect_workspaces` | 10.5 ms |
| `fetch_agents` | 8.1 ms |
| `collect_sessions` | 7.4 ms |

Collectors now run concurrently, so wall-clock startup is the slowest one
(~15 ms of the 41 ms total), not their sum.

## Doing

- [x] `walk_dirs`: one `read_dir` pass instead of `is_dir` + `canonicalize` +
      three `exists` probes per directory
- [x] `walk_dirs`: re-walk a directory first reached with less depth left
- [x] Surface a malformed `config.toml` instead of silently using defaults
- [x] Precompute the spinner's "any agent working" flag at refresh
- [x] Cache `home()` for the render path

## Not doing (and why)

- **Disk-backed root cache.** `root_cache_seconds` only applies within one
  process, so the overlay picker rescans on every open. But the walk is 11.6 ms
  against a 15.5 ms critical path, so caching it buys ~0 ms of wall clock while
  adding staleness for new directories. Reducing the walk's syscalls is the
  better trade; revisit only if a deep-root setup shows the walk dominating.
- **Windowing `draw_list` to visible rows.** ~2 ms/frame at 3000 entries now
  that `pin_key` is cheap. Manual windowing has to reproduce ratatui's scroll
  handling around group headers and two-line rows; the risk outweighs 2 ms.
- **`Result<_, String>` to a typed error enum.** Wide churn across every call
  site for no behavioural change.

## Review

- `walk_dirs` drops from ~6 syscalls per directory to `read_dir` alone when
  `follow_symlinks` is false (the default). Old and new walkers timed in the
  same process over the same roots: 6.1 ms -> 4.15 ms (-32%), finding an
  identical set of directories. The saving is per-directory, so it grows with
  root size. Wall-clock startup is unchanged here because the walk sits behind
  zoxide's 15.5 ms on the critical path.
- Depth-truncation fix: a directory reached first from a shallow root no longer
  blocks a deeper root from scanning past it.
- Invalid `config.toml` now reports the parse error to stderr and as a Herdr
  notification rather than silently reverting to defaults.
