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

---

# N01 offline navigation contract

- [x] Created `feat/navigator-machine-panes` from main at `6b5ea9e6ce81a1771db7e535fb98b1b469118c81`. Preserved the three approved design docs. No publishing or N02+ implementation.
- [x] Moved the exact inherited 0.7.3 workspace/agent JSON values to fixtures. The parser test now supplies `/tmp` cwd mappings and documents the legacy terminal-target assertion for N05 replacement.
- [x] Added test-only inventory/collision fixtures with qualified expected identities, ancestry, actions, 6 workspaces/7 tabs/9 panes, two valid agent links, duplicate/invalid updates, naming states, and unknown owner paths. Added 42 capability/failure scenarios. No production navigation module or new dependency.
- [x] Captured only the six authorized offline version/schema/help commands, each bounded to 20 seconds, all exit 0 with empty stderr. Installed Herdr is 0.9.0, schema protocol 22/version 1. Stored verbatim help and 51 exact JSON Pointer excerpts, with original stdout SHA-256. Excerpts were compared back to the capture; inherited JSON values were compared to the baseline.
- [x] Added seven fixture-integrity tests, metadata allowlist, provenance README, and a named retain/adapt/replace regression matrix. Marked only N01 complete in the implementation plan.

## Verification

All Cargo checks used offline dependencies, temporary HOME/config/cache/data/state directories, preserved Rust toolchain/cache locations, cleared inherited Herdr routing variables, and a fail-closed `HERDR_BIN_PATH`. PATH also blocked real Herdr, SSH, curl, wget, and zoxide. Environment-mutating tests ran serially. No dependencies were fetched and no live session operation ran.

| Command | Timeout | Result |
| --- | --- | --- |
| `cargo test --offline navigation_test_support -- --test-threads=1` | 120 s | 7 passed |
| `cargo test --offline sources::tests::parses_herdr_workspace_and_agent_list_json -- --test-threads=1` | 60 s | 1 passed |
| `cargo fmt --check` | 30 s | Passed; archived baseline formatting also passed |
| `cargo clippy --offline -- -D warnings` | 120 s | Passed |
| `cargo test --offline -- --test-threads=1` | 180 s | 89 passed |
| `cargo build --offline --release` | 240 s | Passed |
| `git diff --check` | 20 s | Passed |

Cargo logs for the final run are in `/tmp/n01-checks-obc85zqy`. The first focused compile caught a helper lifetime error; the corrected helper passed all checks above. Fixture tests do not execute a parallel navigation implementation. They check data integrity and explicit expected-state coverage only.

Remaining limits belong to later tasks. Schema/help is not live transport or acknowledgement evidence. Durable server identity, explicit tab naming metadata, authorized client routing, and native-flow exports remain unverified or unavailable. The parser's legacy terminal target remains deliberately unchanged pending N05. Pane-close prohibition has no existing test; the matrix assigns that coverage to N06. No single-server or combined navigation feature is claimed by N01.

---

# N02 config translation and rollout foundation

- [x] Added serde and Rust defaults for `picker.presentation = "legacy"`, `sources.panes = false`, and inactive reserved `focus_cycle_key = "f6"` / `refresh_key = "f8"`. Unsupported merged/pane requests stay readable and produce deterministic stderr diagnostics at load. They do not send notifications or transport requests. Unknown presentation values keep the existing invalid-config fallback and notification policy, tested through a pure parser helper.
- [x] Sorted original custom filter names before alias/key resolution. First valid sorted aliases and first sorted custom key claims win; custom-over-default and existing command registry precedence remain intact. Added path/key/precedence diagnostics for invalid inputs, aliases, custom/default collisions, legacy actions, vim j/k, templates, reserved keys, and host-prefix conflicts. Default host remapping skips template/reserved claims, preserves explicit custom bindings, and warns when no fallback remains. Ctrl-A still remaps to Ctrl-G with default config.
- [x] Shared the existing registry and key matching between dispatch and pure config checks. F6/F8 add no command or help/footer hint. Ctrl-P/R/S, Tab, Ctrl-O, Alt-Enter, vim, and conditional F5 keep their meanings. No production edits to App or TUI.
- [x] Added three config fixtures and their translation README, updated both examples, and marked only N02 complete in the implementation plan. Inspected `herdr-plugin.toml`; no manifest, version, Source COUNT/index/order, identity, collector, action, UI, persistence, or dependency changes. Preserved N01 and the other design docs.

## Verification

Added 15 tests, nine config tests and six pure registry tests. Config checks never call `Config::load`, refresh, filesystem discovery, template loading, App construction, or Herdr. The existing serial suite still checks TUI handlers/rendering and CLI stubs.

All commands below ran through `/tmp/n02-checks-te9a7kto/run.py`. Its invocation is `python3 /tmp/n02-checks-te9a7kto/run.py <log-name> <timeout-seconds> <command>`. The runner uses `subprocess.run` with the named timeout and saves combined stdout/stderr to `<log-name>.log` in that directory.

The runner cleared inherited `HERDR_*` variables, then set `HERDR_BIN_PATH=/tmp/n02-checks-te9a7kto/bin/herdr`. HOME was `/tmp/n02-checks-te9a7kto/home`; XDG config/cache/data/state each used its own directory below that HOME. CARGO_HOME and RUSTUP_HOME remained `/home/void/.cargo` and `/home/void/.rustup` for installed offline dependencies and toolchains. PATH was `/tmp/n02-checks-te9a7kto/bin:/home/void/.cargo/bin:/usr/bin:/bin`. Fail-closed stubs for Herdr, SSH, curl, wget, and zoxide printed a blocked-command error and exited 125. No dependency downloads, live Herdr/SSH calls, or network verification ran.

| Exact command after the runner prefix | Log name | Timeout | Final result |
| --- | --- | --- | --- |
| `cargo fmt` | `fmt-write` | 30 s | Passed |
| `cargo test --offline config::tests -- --test-threads=1` | `config` | 120 s | 22 passed |
| `cargo test --offline keymap::tests -- --test-threads=1` | `keymap` | 120 s | 6 passed |
| `cargo fmt --check` | `fmt` | 30 s | Passed |
| `cargo clippy --offline -- -D warnings` | `clippy` | 120 s | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy-tests` | 120 s | Passed |
| `cargo test --offline -- --test-threads=1` | `tests` | 180 s | 104 passed |
| `cargo build --offline --release` | `release` | 240 s | Passed |
| `git diff --check` | `diff` | 20 s | Passed |

The initial config compile passed the 13 inherited config tests. An intermediate run passed 103 tests before adding the independent presentation/pane-request regression. The final counts above include that regression; no failed test or lint run remains.

Compatibility changes are limited to deterministic choices for already-ambiguous filter configurations, config diagnostics, and avoiding reserved/template keys during default host-prefix fallback. Valid existing keys and legacy behavior remain unchanged. A colliding custom filter can still shadow a legacy command because N02 preserves existing registry precedence rather than silently repairing user config. Diagnostics explain the winner.

Final independent review of N01 and N02 is still required before cleanup or publishing. No commit, merge, or push ran. N03 and later implementation remains untouched. Merged pin ordering, Query launch focus, compact owner rows, and Agent/pane translation are documented future work, not working features. U01 through U04, N19, N20, and the full combined rollout remain blocked by the public client contract and separately authorized integration verification. Machine New workspace still requires the native naming flow, not direct creation or synthetic keys.

---

# N03 intermediate qualified-domain foundation

This historical record describes the 122-test intermediate state before the bounded completion below. N03 was unchecked at that point. The completion record supersedes the missing-domain items and Quick Actions restriction below, but retains the later release blockers. N01/N02 changes and the intentional design docs remain intact. No config, key, manifest, dependency, or version change belongs to this task.

## Implemented decisions

- `model.rs` now has separate machine/session references inside `ServerScope`, validated workspace/tab/pane identities, and parent-kind/same-scope validation. Parent IDs stay opaque. The current collector supplies an invocation-ephemeral scope, not a durable server incarnation. The reported-value constructor is test-only until a transport can supply those values. The N01 collision fixture exercises the same domain equality and hashing with 12 distinct live keys.
- `Entry` keeps identity private and exposes mutable display/action metadata through `EntryData`. Equality and hashing use identity only. Live target ancestry does not change object identity. Removed the filesystem-derived key and `OnceCell`. `DestinationKind` is separate from legacy `Source`; Source COUNT, indexes, aliases, and order are unchanged. There is no Machine row constructor yet.
- Provider keys contain the process-host reference, provider namespace, provider ID, item ID, and configured action. Builtin and command-integration namespaces cannot collide. Project keys name the template file rather than cwd. Session keys follow the executed attach target; a configured session ignored by remote handoff does not invent a different target. Provider provenance is a set. Dedupe merges it only for identical identities.
- Added `navigation/mod.rs`. Each App constructs one context with an ephemeral scope and optional qualified picker-workspace hint. The process-host reference is process-local and not serializable. Neither it nor the context environment hint grants client authority. Successful refreshes reuse the App's scope. No PID, HOME, socket path, or route label becomes durable identity.
- `OwnedPath` separates process-owned `PathBuf` values from reported scope-qualified text with optional owner home. Only process paths cross the filesystem interface. Entry and workspace-reference paths are optional. Missing live cwd stays absent. Remote/session placeholders and command/quick-action HOME fallbacks are gone. Search and rendering use text. Reported `~`, `$HOME`, and paths equal to the process HOME stay literal unless an explicit owner home permits display shortening.
- Collectors, all provider constructors, App selection/marks/dedupe, workspace joins, reuse indexes, close protection, TUI paths, and debug-list paths use the foundation. `WorkspaceReuseKey` is separate from entry identity and includes server scope. Workspace references carry qualified identities rather than a duplicate bare action ID. Agent metadata now has pane identity and supplied workspace/tab ancestry; unlinked records are omitted. The legacy terminal-based agent dispatch remains pending N05/N06. Pane rows cannot supply workspace-close targets.

Touched files are `src/model.rs`, `src/paths.rs`, new `src/navigation/mod.rs`, `src/navigation/test_support.rs`, `src/sources.rs`, `src/integrations/command.rs`, `src/integrations/herdr_plus.rs`, `src/integrations/sessions.rs`, `src/app.rs`, `src/tui.rs`, `src/main.rs`, and this task record. The only N03 edit to test support exposes the existing collision fixture to domain tests.

## Compatibility limits and remaining work

- The current collector cannot prove process/server co-location. Production contexts therefore reject project/directory reuse-or-create, directory-template application, and process-owned quick-action invocation rather than treating unknown ownership as a missing workspace. Preview reports unavailable rather than Create. Existing preference rules and template bootstrap order remain tested with explicit test-only co-location evidence. This is a runtime compatibility loss, not unchanged behavior. Before release, N04/N06 must supply an evidence-backed relation or a supported owner-specific action. Reported paths cannot be locally canonicalized to make that relation appear true.
- Pins are invocation-only marks, retained across refresh. App no longer reads legacy pins into qualified selection or writes ephemeral identities to disk. Jump-back returns a qualification error without reading, deleting, or rewriting old history. Previous-workspace replay is disabled. Existing files remain available for N07 migration or rollback, but persisted pin replay/removal and jump-back are not working in this intermediate build. A production test verifies byte-for-byte preservation while marking and requesting jump-back.
- N04 still owns route capture, transport deadlines, failure distinctions, and scope invalidation after connection loss. The current refresh still swallows collector failures and cannot detect a restart that reuses IDs. An invocation token is not evidence of a real server incarnation. No connection/freshness framework was added without a consumer.
- Machine rows, current-client selection, authorized client context, and machine native naming-flow actions are absent. Reported identity fixtures do not authorize any action. N05 must validate ancestry against actual tab/pane inventory, collect ordinary panes, and replace terminal dispatch. N06 must finish action resolution/revalidation. The existing overlay error lifecycle remains a later release gate. No transport, all-pane collector, merged renderer, native prompt operation, or persistent migration was implemented.

At this intermediate point, the missing machine/context/invalidation domain data prevented N03 completion. The bounded completion below separates those domain requirements from N04 transport/ownership evidence, N06 creation restoration, and N07 persistence. Do not release this intermediate state as feature-compatible.

## Verification

Added 20 contract tests and replaced two obsolete unqualified jump-back replay tests with production rejection/state-preservation coverage. Adapted the existing pin test to invocation marks. The final suite has 122 tests. Tests cover fixture collision keys, identity mutation invariance, ancestry rejection, missing cwd, provider identity, process/foreign path separation, actual App search/dedupe/selection/reuse, rendering, cross-scope dispatch rejection, pane-close prohibition, and legacy-state preservation. Existing ranking engines, roots, shell quoting, subprocess limits, key handling, template parsing, and CLI-stub bootstrap tests pass.

Checks used `/tmp/n03-checks-YWeiRZ1v/run.py`, copied from the N02 runner. Invocation is `python3 /tmp/n03-checks-YWeiRZ1v/run.py <log-name> <timeout-seconds> <command>`. It clears inherited HERDR variables, uses temporary HOME and separate XDG config/cache/data/state directories, preserves `/home/void/.cargo` and `/home/void/.rustup`, and places fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. Environment-mutating tests run serially. Cargo used offline dependencies. No dependency downloads, live Herdr/SSH operations, private config operations, or network checks ran.

| Command after runner prefix | Log | Timeout | Result |
| --- | --- | --- | --- |
| `cargo fmt --check` | `fmt` | 30 s | Passed |
| `cargo clippy --offline -- -D warnings` | `clippy` | 120 s | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy-tests` | 120 s | Passed |
| `cargo test --offline -- --test-threads=1` | `tests` | 180 s | 122 passed |
| `cargo build --offline --release` | `release` | 240 s | Passed |
| `git diff --check` | `diff` | 20 s | Passed |

The N02 handoff reported a passing 104-test baseline. No pre-existing failure was found. Intermediate N03 compilation runs caught stale field references/imports and moved test values; an intermediate Clippy run caught the large live-identity enum variant. Those introduced errors were corrected, including boxing the live target. All final checks above pass. No test assertion failure remains. No commit, merge, push, or cleanup ran. Independent review remains required.

---

# N03 domain foundation complete

- [x] Added validated `MachineKey` and `MachineIdentity` construction. A selected session must belong to that machine. Machine equality and hashing use only the machine key, not session selection, incarnation, labels, or status. `EntryIdentity::Machine` and `DestinationKind::Machine` remain distinct from live kinds and provider sessions. The machine entry constructor supplies no path or workspace target and uses `UnavailableNativeWorkspace`. App rejects machine activation and close before any command, even if mutable action metadata changes. No production collector emits machine rows and no native prompt or direct creation operation was added.
- [x] Completed the small context model with optional qualified current-client selection, qualified picker ownership, and explicit unknown connection/freshness defaults. The environment workspace hint never sets client selection or path authority. Selection observations are data, not an authorized client route. Context setters reject foreign scopes; `LiveTarget` constructors reject missing, wrong-kind, and cross-scope ancestry. Actual inventory joins remain N05.
- [x] Added domain-only `invalidate_scope`. It preserves machine/session references, rotates the ephemeral incarnation, clears current selection and picker ownership, removes scope-local co-location evidence, and marks freshness stale while connection remains unknown. Tests prove old live/reuse keys differ, old targets cannot reenter context, and App rejects a retained old target before dispatch. Successful refresh keeps the scope stable and does not invent connection evidence. N04 must call invalidation from supported transport observations. No runtime restart detection, serialization, or persistent replay is claimed.
- [x] Restored ordinary-route Quick Actions by removing only the filesystem co-location gate on `InvokePluginAction`. Exact CLI-stub assertions require `plugin action invoke cloudmanic.herdr-plus.quick-actions`, with no path or extra call, using production `NavigationContext::new` and disabled notifications. Both success and nonzero exit are tested. Unknown-owner project/folder/template operations still reject; foreign paths still cannot reach local filesystem operations.
- [x] Marked only N03's domain foundation complete in the implementation plan. N04 ownership/transport invalidation, N05 inventory/exact targets, N06 action restoration, N07 persisted marks/jump-back, and N14 native-flow lifecycle remain later work. S01 explicitly cannot release while ordinary project/folder/template and persistence regressions remain unresolved.

This completion changed `src/model.rs`, `src/navigation/mod.rs`, `src/app.rs`, `src/tui.rs`, `docs/implementation-plan.md`, and `tasks/todo.md`. The TUI change is only the exhaustive unavailable-machine preview arm and removal of the obsolete Quick Actions unavailable preview. Source indexing, collectors, renderer layout, config, dependencies, and earlier changes remain untouched by this completion. Future-facing domain constructors/accessors have narrow dead-code allowances because production transports and machine collectors are not part of N03.

## Completion verification

Added 10 tests to the intermediate 122-test suite, for 132 total. Focused runs exercise production model/context/App/path types. New CLI stubs restore the binary environment and remove temporary files on panic as well as success.

Checks used `/tmp/n03-completion-zFG8OZxa/run.py`, copied from the recorded N03 runner. Invocation is `python3 /tmp/n03-completion-zFG8OZxa/run.py <log-name> <timeout-seconds> <command>`. It clears inherited `HERDR_*` variables, sets temporary HOME and separate XDG config/cache/data/state directories, preserves installed Cargo/Rustup locations, and places fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. Every Cargo dependency operation used `--offline`; environment-mutating tests ran serially. No downloads, live Herdr/network/SSH calls, or private-config inspection ran.

| Command after runner prefix | Log | Timeout | Final result |
| --- | --- | --- | --- |
| `cargo fmt` | `fmt-write` | 30 s | Passed |
| `cargo test --offline model::domain_tests -- --test-threads=1` | `domain` | 120 s | 7 passed |
| `cargo test --offline navigation::tests -- --test-threads=1` | `context` | 120 s | 3 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app` | 120 s | 29 passed |
| `cargo test --offline paths::ownership_tests -- --test-threads=1` | `paths` | 120 s | 3 passed |
| `cargo fmt --check` | `fmt` | 30 s | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy-tests` | 120 s | Passed |
| `cargo test --offline -- --test-threads=1` | `tests` | 180 s | 132 passed |
| `cargo build --offline --release` | `release` | 240 s | Passed |
| `git diff --check` | `diff` | 20 s | Passed |

The first new App focused run had two assertion failures in the recording stub. Shell `printf` repeated its record header for each argument. The corrected stub writes one complete argument record per invocation; the focused and full suites pass. No production assertion or lint failure remains. No commit, merge, push, or repository cleanup ran. Independent review remains required before N04 planning or publishing.


---

# N04 bounded ordinary-server adapter

- [x] Added `src/navigation/server.rs` with typed list reads and qualified exact focus. Raw requests use the documented newline-delimited JSON Unix socket transport, never an invented CLI command or JSON on Herdr stdin. Workspace/tab/pane focus uses only the exact target-kind ID. Snapshot negotiation is read-only and shares the mutation deadline. Only Herdr 0.9.0/protocol 22 enables the verified focus methods; version/protocol changes clear scope, and unsupported-method rejections affect only that kind.
- [x] Added frozen invocation/command specs, injectable limits, a recording runner, bounded process execution, and typed causes with NotSent/Rejected/Unknown mutation outcomes in `src/herdr.rs`. Production captures executable resolution, environment including absent route variables, cwd, and socket before dispatch. Unix process execution polls stdin/stdout/stderr, bounds memory, kills the process group, and reaps the direct child. No detached reader threads or mutation retries were added. Notifications remain outside JSON parsing.
- [x] Wired existing workspace/pane/agent reads through the adapter without implementing N05 inventory conversion. App handles failures before conversion and drops successful responses collected under the invalidated generation. Workspace-row and reuse focus use the exact raw adapter. Quick Actions keeps its existing bounded ordinary plugin-action command. Agent rows retain the explicit legacy terminal dispatch gap for N05/N06.
- [x] Connected transport uncertainty and observed version/protocol changes to scope invalidation. It clears picker/client target hints, co-location and capability observations. Healthy refresh preserves scope; later successful reads cannot requalify old results or restore cleared startup co-location. Structured rejection and unsupported operations do not rotate scope.
- [x] Recorded the parent-fetched versioned public transport/runtime evidence in `tests/fixtures/navigation/ordinary-server/0.9.0/transport-contract.md`. Added `synthetic-transport/README.md` and linked them from the fixture README. These are explanatory Markdown, not fabricated official fixtures or new entries in N01's enumerated manifest. N01 integrity tests still pass unchanged.
- [x] Marked only N04 complete in the implementation plan. No dependency, manifest, config, UX layout, history migration, N05 conversion, or full N06 action architecture was added.

Implementation files are `src/herdr.rs`, new `src/navigation/server.rs`, `src/navigation/mod.rs`, `src/model.rs`, `src/sources.rs`, `src/app.rs`, and startup capture in `src/main.rs`. Documentation changes are the implementation plan, this task record, the navigation fixture README, and the two transport Markdown files. `cargo fmt --all` also formatted pre-existing worktree changes. Earlier task changes remain present; no commit, merge, push, or cleanup ran.

## Trust and release limits

The documented unchanged managed-plugin runtime contract can establish ordinary process/server filesystem co-location when explicit socket and runtime markers are present. This is not authentication against an external caller who overrides the environment before launch, and never authorizes a remote client. Reported paths still cannot reach local filesystem operations. Unknown invocations remain blocked for process-owned reuse/create. After uncertainty, co-location remains cleared until a new invocation. N06 still must qualify creation/template behavior and revalidate displayed actions before release.

Version/protocol metadata has no durable server incarnation. A silent restart reusing the socket, version, and IDs remains undetectable. No persistent live-target replay or client-wide history is enabled. N07 persisted marks/jump-back and S01 failed-overlay lifecycle remain release gates. All-pane inventory and exact agent-row dispatch remain N05/N06. Combined navigation and native new-workspace operations were not implemented.

Linux is the tested platform. The implementation includes macOS Unix transport/process code, but macOS compilation/runtime still needs platform CI. Other platforms return explicit unsupported transport errors. A descendant deliberately leaving the command's process group cannot be killed by group cleanup; inherited pipes still cannot keep Navigator waiting beyond the polling deadline. OS spawn/cleanup and one bounded parser call cannot promise real-time preemption. Optional non-Herdr providers and custom sound playback remain outside this runner.

## Verification

Added 17 tests to the 132-test baseline, for 149 total. Nine synthetic socket tests cover exact target kinds, version negotiation/change, per-kind rejection, wrong owner/session/incarnation, raw request/response limits, malformed/conflicting/trailing/truncated data, ID correlation, hangs, and invalidation without retries. Six runner tests cover frozen routing and absence, recording, concurrent pipe draining, stdin, byte/time limits, failed spawn/exit, inherited pipes, group cleanup, and the managed-plugin trust contract. Two App tests exercise production refresh and focus paths with injected adapters. Existing Quick Actions and template/bootstrap tests still pass.

The first serial run found two stale test assumptions. The recording CLI stub emitted an envelope with no ID or discriminator, and the Quick Actions test expected the old untyped error text. The stub now emits the appropriate typed list envelope and the assertion checks the bounded runner's error. No failing assertion or lint remains.

Final checks used `/tmp/n04-final-verify.py`, with logs and isolation directories under `/tmp/n04-offline-__ulvcnu`. The runner clears inherited `HERDR_*`, sets a temporary HOME and separate XDG config/cache/data/state directories, retains `/home/void/.cargo` and `/home/void/.rustup` for installed offline dependencies/toolchains, and puts fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. `HERDR_BIN_PATH` names that temporary Herdr stub. Synthetic socket workers have bounded accept/read/write waits and remove their sockets after joining. No dependency downloads, live Herdr/network/SSH calls, private-config inspection, or real metadata requests ran.

Each final command had a 240-second outer timeout. Runtime transport tests inject much shorter limits where needed.

| Command | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo test --offline navigation::server::tests -- --test-threads=1` | `focused.log` | 9 passed |
| `cargo test --offline herdr::runner_tests -- --test-threads=1` | `runner.log` | 6 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app.log` | 31 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 149 passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

This completes N04's bounded ordinary-server implementation and offline acceptance, not S01 release qualification or combined navigation. Independent review remains required.

# N05 typed pane inventory and exact activation

- [x] Added `src/navigation/inventory.rs` with typed workspace/tab/pane/agent records and independent read outcomes. Each read retains its captured scope and observation time. Disabled, successful-empty, unavailable, and discarded-stale reads differ; unknown counts are `None`, not zero. Tab naming remains unknown under protocol 22. The N04 bounded adapter still owns transport, envelope checks, deadlines, and failure distinctions. It now checks every additional consumed title/name/session field before conversion.
- [x] Replaced production agent-row conversion with `join_inventory`. It checks actual workspace/tab membership and matching pane ancestry. Duplicate inventory identities and invalid agent links become diagnostics, not guessed targets. Each valid pane has one qualified identity and optional agent metadata. Highest agent revision wins; equal revisions use a deterministic typed-record ordering. A successful empty agent read removes metadata, and a failed optional read marks it unknown without retaining old metadata as current. A missing agent name does not remove a valid pane link; names remain optional and the pane title falls back without inventing an identity.
- [x] Added manual-label, agent-name, useful-reported-title, and `Pane N` title precedence. Useful reported titles exclude controls, blanks, identity placeholders, and generic unknown/untitled/pane/terminal values. Navigator assigns scope-local ordinals by pane identity, keeps them across refresh order changes, and clears them on scope change. Missing cwd stays absent; reported cwd remains opaque. Agent names, configured aliases, status, revision, and session remain metadata rather than parsed title/subtitle identity.
- [x] App retains the typed inventory and joined snapshot. `panes=false/agents=false` shows no panes; agent-only shows linked panes; pane-only shows every valid pane without enrichment; both shows each pane once with enrichment. Hiding workspace rows keeps their ancestry and reuse index. Ordinary panes use the existing Workspace bucket and enriched panes use Agent, with `DestinationKind::Pane` and pane-plus-agent provenance. Source COUNT, indexes, keys, configured sessions/remotes, and inactive F6/F8 remain unchanged.
- [x] Removed `FocusAgent` and `agent_target`. `FocusPane` has no alternate string target. App checks current scope, destination kind, available joined ancestry, and action kind before calling `navigation.focus`. Collected pane rows send exact raw `pane.focus` with `pane_id`; terminal IDs appear only as metadata. Pane Ctrl-X cannot close the pane or its parent workspace in either legacy source bucket. No agent-focus, directional-focus, create, or attach fallback was added.
- [x] Kept structured optional rejection separate from uncertainty. Agent rejection leaves valid panes usable and labels agent metadata unknown. Transport uncertainty still invalidates the invocation scope before conversion. Successful responses retain their old scope and become stale, never relabeled into the new generation. Focus uncertainty also marks the retained inventory stale.
- [x] Added the minimum legacy rendering needed for panes. Detailed pane rows have a workspace/tab/agent breadcrumb, and compact rows retain that metadata inline. Numeric tab `1` remains visible. Preview says exact pane focus and labels terminal IDs as metadata. Inventory failures and stale reads appear in the query area without requiring a selected row or preview. The all-pane unavailable warning is gone; its default is still false. Merged warnings, existing keys, and unsupported client/native-flow operations remain unchanged.
- [x] Added production-path fixture, parser, App refresh/socket, and renderer tests. The original six-workspace evidence remains six workspaces, seven tabs, and nine panes. The production join yields two enriched panes and no extra Agent destinations. Hypothetical owner/naming/agent fields remain separate from the inherited 0.7.3 values and offline 0.9.0 schema. The synthetic `waiting` status maps to unknown in the fixture adapter, not a fabricated protocol-22 status. Only N05 is newly checked in the implementation plan.

N05 implementation files are `src/navigation/inventory.rs`, `src/navigation/server.rs`, `src/navigation/mod.rs`, `src/model.rs`, `src/sources.rs`, `src/app.rs`, `src/tui.rs`, and `src/config.rs`. Existing provider constructors in `src/integrations/command.rs`, `src/integrations/herdr_plus.rs`, and `src/integrations/sessions.rs` now initialize absent agent metadata instead of an absent terminal target. Test support is in `src/navigation/test_support.rs` and inline modules. Documentation updates cover README, both example configs, the config fixture/comment, navigation fixture README, implementation plan, and this record. Earlier worktree changes remain present. No commit, merge, push, delegation, download, live Herdr, network, SSH, or private-config inspection ran.

## N05 verification

Added 13 tests to the 149-test baseline, for 162 total. Six new App tests exercise source toggles, metadata mutation and selection, optional rejection versus uncertainty, required inventory rejection, retained removed targets, and exact collected-pane activation. Four source tests exercise the six-workspace production conversion, title/ordinal rules, duplicate agent revisions/ties/removal, and invalid/duplicate ancestry. One runner-backed adapter test covers all four typed reads and consumed optional types. Two TUI tests cover pane metadata and inventory failures with no selected row. Existing tests now use the production join instead of a second test-only workspace projection.

The first compile found one generic inference error in the collector-panic fallback, fixed by returning a typed error before constructing each read. The first full run found four stale test assumptions: the recording CLI lacked a tab array, and three TUI fixtures described workspaces with directory actions. Those fixtures now match the consumed inventory and destination kinds. The old terminal assertion now checks exact qualified pane focus and session metadata. No production assertion or lint failure remains.

Final acceptance ran through `/tmp/n05-final-verify.py`, with logs and isolation directories under `/tmp/n05-offline-724i5d4z`. Each command had a 240-second outer timeout. The runner clears inherited `HERDR_*`, uses a temporary HOME and separate XDG directories, retains only the installed Cargo/Rust toolchains for offline dependencies, and places fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. `HERDR_BIN_PATH` points to that temporary stub. Tests inject recording runners or bounded temporary Unix sockets. No live service or network access was used.

| Command | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo test --offline navigation::server::tests -- --test-threads=1` | `focused.log` | 10 passed |
| `cargo test --offline sources::tests -- --test-threads=1` | `sources.log` | 11 passed |
| `cargo test --offline herdr::runner_tests -- --test-threads=1` | `runner.log` | 6 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app.log` | 37 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 162 passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

## N05 remaining gates

Independent reads are observations, not an atomic server snapshot. The picker still refreshes synchronously; N15 owns incremental scheduling. N06 still owns full action resolution and creation/template release qualification. N07 owns persistent marks and jump-back. S01 still blocks release on failed-overlay lifecycle, including keeping the overlay open after an action error. N08 and later own merged UI. Machine rows, combined client-authorized routing, and the native machine New workspace flow remain unavailable.

The N04 explicit Unix socket and verified 0.9.0/protocol 22 requirements still apply. A silent server restart reusing the route/version/IDs remains undetectable. Unknown process/server co-location remains blocked, and successful reads cannot restore it after uncertainty. Linux checks pass; macOS compilation/runtime still needs platform CI. This completes N05 offline acceptance, not S01 release or combined navigation qualification. Independent review remains required.

Post-evidence checks also passed for tracked changes and each N05-edited untracked file, with 10-second per-file bounds. A broader untracked-file check found whitespace in the original captured `pane-focus-help.txt`, which N01 preserves byte-for-byte. That raw evidence was not edited. The no-index check treats exit 1 with no diagnostic output as ordinary file differences, not a whitespace failure.


# N06 displayed actions and frozen dispatch

- [x] Added `src/actions.rs` for prepared process files, action resolution, shared descriptions, semantic comparison, and typed payload dispatch. The selected action has a primary or directory-template slot, exact target or provider request, execution owner, availability reason, and semantic observations. App assigns a local revision only when displayed semantics change. Status, spinner ticks, and observation times do not change that revision.
- [x] Successful TUI draws commit the displayed token. Keyboard activation submits that token through `activate_displayed`, which also accepts a retained token for a future input handler. Changing selection, revision, ancestry, config, target, cwd, label, command, timeout, or template contents rejects the activation before mutation. A changed action needs a new successful draw and activation. Dispatch consumes the token, so a successful or failed request cannot replay it.
- [x] Restored ordinary project/folder reuse and creation under N04's existing managed-plugin process/server relation. Project reuse chooses Project. Folder reuse chooses Dir, then Unknown, never Project. The explicit directory-template action chooses Dir, then any same-owner workspace, and retains its exception to `reuse_existing` and `create_missing`. Known matches, known unmatched inventory, unavailable inventory, and missing cwd remain distinct. Hidden workspace rows still populate reuse observations.
- [x] Prepared process literal/canonical paths outside rendering. Reported strings are compared only within their server scope and never passed to the process-file adapter. Retained template filenames are absolute under the captured process cwd. Activation loads the exact file once, compares its complete contents, and dispatches the parsed contents from that validation. Prepared templates share immutable data rather than copying their contents into each action payload.
- [x] Routed create, close, plugin invocation, template tab/pane operations, startup commands, and Herdr notifications through the same ordinary adapter. Existing first-root reuse, split defaults, labels, quoted commands, and creation of every tab/pane before startup commands remain intact. Failed follow-ons stop immediately. Errors report possible partial creation/application or an unknown mutation outcome. There is no automatic retry, fallback create, destructive rollback, or invented remote operation. Mutation uncertainty invalidates scope and co-location evidence.
- [x] Kept Attach and Run separate from live focus. Attach uses the captured executable, environment, and cwd. Local attach strips every `HERDR_*` variable; configured remote attach retains `--remote TARGET --handoff`. Neither receives the ordinary two-second request deadline. Run retains shell substitution quoting, process-group timeout cleanup, and configured timeouts including zero. A provider's `kind=server` or `kind=session` never changes Run into Focus or changes its execution host.
- [x] Preview and footer use the same resolved action description. Directory templates have their own action slot. Native machine New workspace remains explicitly unavailable. Ctrl-X cannot close panes, tabs, or machines. A separate retained close target survives confirmation, names the chosen workspace, revalidates before close, and protects the qualified picker-owning workspace. Unsupported Ctrl-X does not insert a query character.
- [x] Duplicate identities merge provenance only when action-relevant data agree. Conflicting data leave an unavailable diagnostic rather than silently choosing the first provider item.

N06 changed `src/actions.rs`, `src/app.rs`, `src/herdr.rs`, `src/integrations/command.rs`, `src/integrations/herdr_plus.rs`, `src/integrations/sessions.rs`, `src/keymap.rs`, `src/main.rs`, `src/model.rs`, `src/navigation/mod.rs`, `src/navigation/server.rs`, `src/tui.rs`, `docs/implementation-plan.md`, and this record. No dependency, manifest, config default, history format, merged layout, mouse handler, incremental collector, or native client implementation changed. All earlier worktree changes remain present. No commit, merge, push, delegation, download, private-config inspection, live Herdr, network, or SSH operation ran.

## N06 verification

Added 20 tests to the 162-test N05 baseline, for 182 total. Tests exercise actual App activation with recording runners and temporary Unix servers, process-owned temporary templates, the in-memory process-file adapter, and TestBackend rendering/key events. Cases cover Focus-to-Create, Create-to-Focus, replacement workspaces, pane reparenting, workspace visibility, unavailable inventory, unknown cwd, changed owner/config/label/command/timeout, template cwd/name/tabs/command/comments/disappearance/invalid content, token replay, duplicate conflicts, exact tab focus, close confirmation/revalidation, and partial template failures at each mutation stage. Tests also change global executable/environment/cwd/socket/session values after capture and verify that later template calls and notifications retain the original route. Attach command construction and process environment retain the same captured context.

The first compilation found a missing inventory error accessor and a borrowed environment iterator mismatch. Both were corrected. Updating the old activation tests required displayed-token setup and fresh-list replies. Strict Clippy found a large action enum; immutable shared templates fixed that without suppressing the lint. Final checks have no remaining compiler, lint, or test failure.

Final acceptance ran through `/tmp/n06-final-verify.py`. Logs are under `/tmp/n06-offline-effcrec_`. Each command had a 240-second outer timeout. The runner clears inherited `HERDR_*`, gives tests temporary HOME and XDG directories, retains the installed Cargo/Rust toolchains for offline dependencies, and puts fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. New ordinary mutations use injected recording runners and temporary Unix servers, never a live route.

| Command | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo test --offline actions::tests -- --test-threads=1` | `actions.log` | 3 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app.log` | 50 passed |
| `cargo test --offline integrations:: -- --test-threads=1` | `integrations.log` | 17 passed |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 24 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 182 passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

## N06 remaining gates

Ordinary list reads are independent observations, not an atomic snapshot or server-side compare-and-mutate. A target may change between preflight and mutation. Template/path preparation is synchronous, but never runs in draw/render. Silent server restarts that reuse the socket, version, and IDs remain undetectable. Unknown outcomes do not authorize retry or restore co-location. Partial template mutations remain in place for the user to inspect.

The TUI now returns to the picker after a narrow action error instead of exiting as success. This does not qualify all terminal cleanup, pending events, overlay ownership, or native handoff behavior. S01 and N13 remain release gates. N07 still owns persisted marks and jump-back. N08 and later still own the merged picker and responsive layout, so long descriptions can still truncate in the legacy footer. No production tab rows, machine actions, combined client view, or native naming flow were enabled. Existing exact-focus socket/version requirements and the untested macOS runtime gate remain. Only N06 is newly marked complete; this is not a release qualification.

## N07 partial implementation

N07 remains incomplete. This change restores durable provider pins and adds unresolved-record maintenance. It does not restore jump-back or persist live workspace/pane targets.

- [x] Added a version-1 `navigation-history.json` beside a stable `navigation-history.lock` in the existing plugin config directory. App freezes an absolute directory at construction; tests inject directories rather than changing environment paths. No state-directory switch or dependency was added.
- [x] Generated an installation-local provider preference owner. Keys include builtin/command namespace, provider ID, item, configured action, and folder/template or attach identity. Labels, reused workspace IDs, current status and ephemeral `EntryIdentity` are not serialized. Copying the document copies preferences only, never machine identity or action authorization.
- [x] Restored pins only onto fresh matching provider rows. Provider absence retains preferences without creating replacement rows. Pin changes persist before the visible mark and selection change. The existing sort and selection rules remain. Live marks stay in memory and cannot transfer after scope invalidation.
- [x] Imported every legacy value as unresolved provenance, with stable record IDs. A sole matching row, socket, HOME or picker context cannot infer an owner. `pinned-entries.json` and `jump-back-workspace` remain byte-for-byte unchanged. Explicit removal keeps exact source/value tombstones, so unchanged legacy input cannot resurrect records. Re-pinning a fresh provider row is separate from resolving or deleting an old record.
- [x] Added `history list` and `history remove RECORD_ID`. They load neither App nor config, capture no Herdr invocation and make no Herdr calls. JSON output escapes stored text. The directory and files must be owner-controlled.
- [x] Bounded input before parsing to 1 MiB, records including tombstones to 4096, and each string to 16 KiB. Strict serde shapes reject unknown fields, versions and types. Validation rejects duplicate IDs/keys/provenance and foreign provider owners. No live-target shape is accepted. Malformed, unavailable, unsafe or oversized input disables writes; it never becomes empty state.
- [x] Added Unix advisory transactions with a 250 ms lock-wait budget, reload inside the lock, unique mode-0600 temp files, flush/file sync, rename and directory sync. Directory traversal uses descriptor-relative no-follow opens. Symlinks, hard links, nonregular files and group/other-writable store files or directories are rejected. Writers never unlink the stable lock. No eviction occurs. Failures before rename preserve old bytes; failures after rename report uncertain durability and disable retries on that Store instance.
- [x] Classified successful dispatch as exact `OrdinaryFocus` or `ProviderCompleted`. Both remain ordinary completion evidence, not invoking-client transitions. N06 token consumption, action revalidation, frozen routing and notifications remain unchanged. No pre/post workspace-list inference, native-prompt recording or synthetic production authorization was added.
- [ ] Obtain durable live identity and authorized invoking-client previous/current targets before implementing workspace-transition recording or jump-back replay.
- [ ] Complete positive qualified-transition, client expiry/segregation and replay acceptance against a usable client contract. `qualified-history-cases.json` is a blocked checklist, not an implemented replay test suite. N07, S01 and full-feature release are not complete.

Changed for N07 only: `src/history.rs`, `src/app.rs`, `src/actions.rs`, `src/main.rs`, `src/model.rs`, four files under `tests/fixtures/history/`, `README.md`, `docs/implementation-plan.md`, and this record. Existing changes to paths, config, transport, collectors, rendering and other documentation remain untouched. No picker layout, mouse handling, client transport, native flow, dependency or manifest change was made. No commit, merge, push, delegation, download, private user-state access, live Herdr, network or SSH operation ran.

## N07 verification

Added 19 tests to the 182-test N06 baseline, for 201 total. Storage cases cover unchanged version-1 round trips, independent and copied preference owners, valid-looking/ambiguous/unknown/repeated/malformed/incomplete legacy inputs, stable IDs, removal tombstones, future/duplicate/oversized documents, malformed reload, unavailable files, capacity rejection, concurrent writers, bounded lock waits, unsafe file types and permissions, temp cleanup, and failures before/after rename. App cases cover restart, provider absence/return, display and reuse changes, folder/project/template/attach key differences, failed writes, selection retention and live-scope invalidation. Existing exact-tab and directory-creation tests now assert the narrow completion classification.

Initial storage tests used default temporary-file permissions under a group-writable umask. They correctly hit the owner-control rejection. The fixtures now create mode-0700 directories and mode-0600 files explicitly. A prior collision fixture marked a scope other than App's current ordinary scope; it now selects the current scope and retains the same dedupe/search/selection checks. Strict Clippy required boxing the exact-focus completion target. Final checks have no remaining compiler, lint or test failure.

Final acceptance ran through `/tmp/n07-final-verify.py`. Logs are under `/tmp/n07-offline-jj388mpg`. Commands had 240-second outer timeouts. The runner clears inherited `HERDR_*`, supplies temporary HOME/XDG directories and fail-closed Herdr/SSH/curl/wget/zoxide stubs, and retains installed Cargo/Rust toolchains for offline dependencies. CLI checks use a separate call-recording stub, invalid config contents and isolated mode-0700 state.

| Command | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo test --offline history::tests -- --test-threads=1` | `history.log` | 15 passed |
| `cargo test --offline actions::tests -- --test-threads=1` | `actions.log` | 3 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app.log` | 54 passed |
| `cargo test --offline integrations:: -- --test-threads=1` | `integrations.log` | 17 passed |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 24 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 201 passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |
| Release CLI list/repeat/concurrent process removals/tombstones/malformed rejection | `cli.log` | Passed, legacy bytes preserved, no config load or Herdr call |

## N07 remaining risks

The store assumes installation-local Unix filesystem and advisory-lock semantics. Sharing or copying it does not establish ownership of another machine's actions. Linux offline checks passed; macOS compilation/runtime and other filesystem behavior remain unqualified. Malformed legacy input blocks import and writes until the user repairs or moves it. Tombstones consume the documented capacity and are not evicted. If rename succeeds but directory sync fails, the saved preference may differ from the unchanged visible mark until the user reopens the store.

Herdr 0.9.0/protocol 22 still lacks durable server incarnation and invoking-client transition evidence. A silent same-socket/version restart remains undetectable. Provider preferences grant no right to execute; every activation still uses fresh N06 resolution and revalidation. Live marks and jump-back remain the uncompleted part of N07. No ordinary completion, workspace list or native prompt outcome substitutes for client authorization. S01 failed-overlay lifecycle and the separate native-flow/client gates remain unchanged.

## S01 partial early lifecycle safety

S01 remains unchecked. This work takes only its independent early terminal/action-loop fixes. It does not complete the N07 dependency or qualify a release.

- [x] Added one private terminal/input adapter and a cleanup guard in `src/tui.rs`. Production and fake terminals run the same draw, displayed-action commit, key dispatch, App activation and next-input loop. Failed draws cannot authorize an action.
- [x] Removed the blocking action-error acknowledgement. Failed activation remains inline in the overlay and side picker. Ordinary focus stays on the alternate screen. Failure causes no automatic retry, refresh or row relabel. N06 still requires a new draw and activation for changed action descriptions.
- [x] Kept successful overlay close and persistent refresh with identity restoration. Attach and Run still suspend the terminal, then resume on failure. A resume failure retains the action error alongside the terminal error.
- [x] Cleanup attempts raw disable, alternate exit and cursor restoration independently. Fault injection covers partial acquisition, initialize, draw, poll, read, suspension, close/update confirmation, resume and final cleanup. Earlier errors win over later cleanup failures.
- [x] Routed mark-error notifications through the frozen ordinary adapter. A notification failure does not hide the inline mark error or trigger a fallback notification. Open/close notification ownership remains unchanged.
- [x] Added the preview-independent `Server scope: owner unknown` label. It contains no socket path or route credential and does not call the process host Local.
- [ ] Complete N07 qualified live-target persistence and invoking-client history, live terminal/platform validation and independent final review. `RELEASE.md` explicitly holds publication. No N08 merged UI, N12 mouse, N13 asynchronous pending machine or native handoff was added.

Changed for this partial S01 work only: `src/tui.rs`, `src/app.rs`, `README.md`, `CHANGELOG.md`, `RELEASE.md`, `docs/implementation-plan.md`, and this record. App adds a displayed-action suspension query and makes its existing explicit history-directory constructor available within the crate for isolated loop tests. Earlier worktree changes remain present. No dependency, manifest, version, transport or history-format change was made. No commit, merge, push, delegation, download, live Herdr, network, SSH or private user-state access ran.

### S01 offline verification

Added 13 tests to the 201-test baseline, for 214 total. Both overlay and persistent modes exercise rejected focus, lost acknowledgement, malformed acknowledgement, timeout and unsupported exact focus. Each failure produces at most one exact `pane.focus` request with `pane_id`, preserves selection/title/query/filter/preview/input mode through redraw, leaves history bytes unchanged and accepts the next input. The recording runner allows only the expected inventory reads and one frozen notification, with no create, attach, fallback or failure refresh.

Other loop tests cover removed/reparented/stale/unavailable targets, changed focus-to-create descriptions without dispatch on redraw, successful overlay close and side refresh, failed process Run, a temporary fake interactive Attach, failed mark notification, close confirmation and independent terminal restoration failures. Existing N06 tests still cover consumed-token replay rejection. Fake F5 tests only decline confirmation or inject errors; no update installer runs.

`/tmp/s01-final-verify.py` used isolated HOME/XDG directories, fail-closed command stubs, cached offline Cargo dependencies, temporary state and synthetic Unix sockets. Each subprocess had a 240-second bound. Logs are under `/tmp/s01-final-offline-xijjky_r`.

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | Passed |
| `cargo check --offline` | Passed |
| Focused history / actions / App / integrations / TUI suites, serial | 15 / 3 / 54 / 17 / 37 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | Passed |
| `cargo test --offline -- --test-threads=1` | 214 passed |
| `cargo build --offline --release` | Passed, build only |
| `git diff --check` | Passed |

### S01 remaining acceptance

The tests record restoration attempts, not a real terminal's response. Live terminal behavior and macOS compilation/runtime still need platform validation. Long errors can still truncate in the legacy layout. Ordinary preflight is not atomic compare-and-mutate, and a silent restart at the same socket/version remains undetectable. Process-owned creation/templates remain conditional on managed-plugin co-location and fresh action revalidation; machine New workspace remains unavailable.

Provider preferences are not durable server/client identity. Live marks remain invocation-only, jump-back remains disabled and N07 stays partial. S01 and the release gate remain incomplete. This record does not authorize later dependency-gated work or claim a release.

## N08 offline browse state and flat projection

- [x] Retained the interrupted writer's pure `src/picker.rs` implementation and eight N08 tests in `src/tui.rs`. Added five focused tests instead of duplicating that coverage. N08 is complete at the data/state interface, not as an active merged UI.
- [x] Retained exact validated tab entries in `JoinedInventory`, including empty tabs. They do not enter legacy `App.entries`. App supplies structural disabled workspaces, eligible destinations, already-prepared action resolution, owner headings, marks and tab completeness without I/O. Protocol 22 naming remains Unknown, regardless of label `1`.
- [x] Kept destination, task-group and server/process-heading identities separate. Machine headings come only from explicit client fixture input. Ordinary scope never becomes Local. Process tasks use the known server only with proven co-location; configured remote Attach does not establish machine ownership. Command source Server still groups by its Action destination kind.
- [x] Kept expansion, browse selection/viewport, flat selection/viewport and current identity separate. Initialization opens headings and only known current ancestry. Compression requires one complete proven Unnamed tab. Flat requests retain caller eligibility/order and add no matching or ranking. Clearing query restores saved browse state without changing the caller's filter. Hidden or vanished anchors return no displayed replacement and never silently select a neighbor.
- [x] Task groups expose disclosure but no entry or action. Machine rows retain the unavailable NativeNewWorkspace action independently of expansion. Pins and previous marks neither duplicate destinations nor reorder machines, expand non-current workspaces or suppress current marks.
- [x] Kept the active consumer caller-owned in tests. No unused duplicate `App.picker` state, N09 ranking, N10 keyboard handler, N11 renderer, client transport or native operation was added. Temporary dead-code allowances name the pending consumer and are limited to this module, its App seam and required model accessors.

Approved scheduling exception beside N08 in the implementation plan permits independent offline fixture/UI development based on completed N06 model/actions and N07 provider preferences. Qualified live history remains blocked. This completes neither N07 nor S01 and does not authorize release. N09 through N18 keep their remaining dependencies.

N08 files are `src/picker.rs`, `src/picker/tests.rs`, `src/main.rs`, `src/model.rs`, `src/navigation/inventory.rs`, `src/navigation/test_support.rs`, `src/sources.rs`, `src/app.rs`, `src/tui.rs` tests, `tests/fixtures/navigation/README.md`, `docs/implementation-plan.md`, and this record. Earlier uncommitted work remains intact. No dependency, config default, Source index, storage format or public rollout changed.

### N08 verification

The completed suite has 227 tests, 13 more than the recorded 214-test S01 baseline. Eight N08 tests were already present in the interrupted work. Recovery added three picker tests and two App tests. Assertions cover six workspaces, seven tabs, nine panes, two machine rows, two agent metadata links, exact ancestry, no duplicate destinations, empty tabs, named `1`, compression completeness, all four collision scopes, session/incarnation changes, unknown current and vanished selections. App refresh uses a RecordingRunner before projection; repeated input/projection leaves call counts, history bytes and legacy view state unchanged.

An intermediate test compile omitted RunCommand notification fields; the corrected fixture passes. An overlapping refresh test underqueued agent responses and was removed after locating the existing production-path test. No failing test or lint remains.

`/tmp/n08-final-verify.py` clears inherited `HERDR_*`, supplies temporary HOME and separate XDG config/cache/data/state directories, preserves `/home/void/.cargo` and `/home/void/.rustup`, and places fail-closed Herdr/SSH/curl/wget/zoxide stubs first on PATH. Cargo uses offline dependencies. Every command has a 240-second outer timeout, and tests run serially. Final logs are under `/tmp/n08-offline-7yahvwa2`.

| Command | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo test --offline picker::tests -- --test-threads=1` | `picker.log` | 3 passed |
| `cargo test --offline sources::tests -- --test-threads=1` | `sources.log` | 11 passed |
| `cargo test --offline app::tests -- --test-threads=1` | `app.log` | 56 passed |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 45 passed |
| `cargo test --offline navigation_test_support -- --test-threads=1` | `fixtures.log` | 7 passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 227 passed |
| `cargo build --offline --release` | `release.log` | Passed, build only |
| `git diff --check` | `diff.log` | Passed |

### N09 ranking decision before implementation

Merged search compares best-field class, exact/prefix/substring/fuzzy quality, then engine score. Equal direct relevance favors a workspace. A pane is not stronger merely because it is a pane. Pins and source preferences break later ties only. Empty browse keeps N08 hierarchy and machine order. Legacy ranking and activation remain unchanged.

### N08 remaining limits

N10/N11 still must connect input and rendering to this state. Tests inspect group and machine action data, not new Enter behavior. N09 still owns filters and ranking; N15 owns full closed-target retention and incremental refresh. The projection does not measure latency or validate live terminal behavior. Legacy remains the default and merged requests retain the unavailable warning.

Fixtures grant no client authority. Ordinary tabs cannot claim explicit naming, current-client selection is unknown without evidence, native machine New workspace remains unavailable, and qualified live history/jump-back remain blocked. N07, S01, platform/runtime validation and independent final review remain incomplete. No delegation, commit, merge, push, dependency download, private state access, live Herdr, network or SSH operation ran during recovery.

## N09 query-preserving filters and field-aware search

- [x] Added independent Machine, Type and provenance state and setters. Setters and filter reset cannot edit the caller's query. Agents requires pane metadata; named tabs remain targets under All. Explicit machine equality never promotes ordinary server/process scope to a known machine.
- [x] Moved query parsing and operator predicates to `matcher.rs`. Bare `@`, `!agent`, `@workspace/status`, `/reported-path` and `#status` intersect. Repeated tokens can match separate allowed fields, but no field concatenation supplies a token match. Legacy App status priority and ranking remain active and unchanged.
- [x] Added typed direct, inherited and supporting search fields. Direct title, agent identity/name/display and each alias compete by their best match, not enumeration order. One qualified ancestry/owner lookup per search includes hidden workspaces. Paths use reported text and components only; missing cwd stays absent. Live legacy `search_terms` and breadcrumb subtitles cannot leak inherited text into direct fields.
- [x] Ranked by class, exact/prefix/substring/fuzzy quality, then engine score. Semantic ties use Workspace, Machine, Tab, Pane, Project, Folder, Session, Action order. Pins, overflow-safe source preference, label and stable input order follow. Source boosts cannot defeat relevance; pins cannot reorder browse machines.
- [x] Connected `App::picker_projection` to pure `picker::search_project` over a caller-owned input/state. App supplies the actual query and ranking config. Search derives eligibility and order before N08 projection. Filtered browse retains only needed structural ancestors, without actions or selection. Raw nonempty queries, including operator-only and whitespace queries, stay flat. Empty non-live type filters stay flat. Clearing a query preserves filters, expansion and browse anchors.
- [x] Added 14 tests. Production `FixtureClient` input passes through the real inventory join; expected fixture arrays do not produce search results. Tests cover all engines, operator/provenance intersections, configured remote Attach as Sessions and server-category Run as Actions, qualified collisions, hidden ancestry, multiword/Unicode aliases, best-field selection, hash-order determinism, extreme boosts and delayed pin ties.
- [x] After actual RecordingRunner App refresh, repeated query/filter changes and caller-owned selection movement cause no more calls or history writes. Legacy selection/filtered indices remain unchanged. Attempted activation still reports that no action was displayed, so search did not commit a token.
- [x] Preserved earlier uncommitted work. Changed only the N09 Rust implementation/test files and evidence below. No new module, JSON fixture, dependency, config default, key/menu, renderer or activation gate was added. The merged-request warning remains.

N09 files are `src/picker.rs`, `src/picker/tests.rs`, `src/matcher.rs`, `src/app.rs`, `src/model.rs`, `src/config.rs`, `src/tui.rs` tests, `docs/implementation-plan.md`, `tests/fixtures/config/README.md`, `tests/fixtures/navigation/README.md`, and this record.

### N09 verification

`/tmp/n09-final-verify.py` follows the N08 offline runner pattern. Each command has a 240-second limit. HOME, all XDG directories and test history use temporary paths. Cargo is offline. Fail-closed Herdr/SSH/network/provider stubs precede system commands. Fake routes use bounded deadlines. No private config, live Herdr, network, SSH, dependency download, delegation, commit, merge or push ran.

Final logs are under `/tmp/n09-offline-w70vpjx7`.

| Check | Log | Result |
| --- | --- | --- |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| Picker, matcher, sources, App, TUI and fixture serial tests | `picker.log`, `matcher.log`, `sources.log`, `app.log`, `tui.log`, `fixtures.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 241 passed, including all previous 227 |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

An earlier verification caught a test calling private config diagnostics. The test now checks the retained presentation request; the existing config warning regression stays unchanged. The final run includes the correction and strict all-target Clippy.

### N09 remaining limits

N09 completes only the pure search/filter state and App seam. N10/N11 still own active input and rendering. N15 owns refresh scheduling and closed-target retention. No live latency, terminal/platform behavior, native machine flow or client authority was qualified. Legacy rendering/filtering stays active. N07, S01, release qualification and independent review remain incomplete.

## N09 independent review backlog

Standards review `sa-2b8fa547` and specification review `sa-28fdeea0` both returned FAIL. Their duplicate report leaves five distinct findings. The review diff, including baseline `6b5ea9e` and untracked files, is under `/tmp/n09-review-Bmhe9T`. The earlier 241 passing tests did not establish a review pass.

| Finding | Severity | Status |
| --- | --- | --- |
| 1. Unknown picker ownership permits workspace close after scope invalidation and healthy refresh | High | Independently closed |
| 2. Discovered and configured Attach rows with the same identity conflict on path/title metadata | Medium | Independently closed |
| 3. Tests that construct `App::new` open the real history store without injected isolation | Medium | Independently closed |
| 4. Machine rows with `Source::Server` pass Ctrl-S provenance filtering | Medium | Independently closed |
| 5. Quick Actions has a process heading but a server-owned plugin payload | Medium | Independently closed, including the authorized TUI test update |

Final interim Standards review `sa-3a77d876` and specification review `sa-6d27834f` both returned PASS with no issues. They independently closed all five findings, the socket/deadline and Attach validation repairs, and the later managed-ambient ownership fixture finding. The repair records below retain the earlier failures and pending-review states as historical evidence. N10 has not started. N07 and S01 release blockers remain.

### Finding 1 repair

- The shared close guard rejects absent picker ownership and ownership from another scope. It preserves the existing picker-owning-workspace error and permits only a distinct workspace in the same scope. Existing workspace eligibility and current-scope checks remain.
- Close execution consumes its retained token, checks picker ownership before inventory revalidation, and checks it again immediately before mutation. Retaining a new close still clears any old token before rejection. The fix does not infer ownership from focus, inventory, client selection, or environment after invalidation.
- New App regressions use `App::with_history` with temporary directories and a frozen `RecordingRunner`. Actual refresh after invalidation returns fresh same-ID rows without restoring ownership. Neither the old picker workspace nor another workspace can close. Rejected retention clears the old description/token. A legitimately retained close loses authorization before revalidation and cannot replay.
- A new TUI regression uses `loop_app` and `TestState`. After invalidation and healthy refresh, Ctrl-X, redraw, and Escape produce no confirmation, close, notification, query change, selection/filter change, or history write. It covers both workspace rows and both confirmation settings.
- Three existing close fixtures now explicitly set a distinct same-scope picker owner. They still test changed-target rejection, one exact frozen close request with no retry, and confirmation failure/decline. No test helper assigns an owner globally.

Changed files are `src/app.rs`, `src/tui.rs`, and `tasks/todo.md`. Production navigation, transport, notifications, and key dispatch are unchanged. Fix-start copies are under `/tmp/n09-fix1-r5aMwa/start`; no baseline reset or unrelated repair ran.

### Finding 1 verification

The runner `/tmp/n09-fix1-r5aMwa/run.py` gives each command fresh temporary HOME, XDG config/cache/data/state/runtime, and TMPDIR directories. It clears inherited `HERDR_*` and `XDG_*`, preserves installed Cargo/Rustup caches, and sets offline Cargo mode. Fail-closed Herdr, Herdr Plus, SSH, SCP, curl, wget, and zoxide stubs precede system commands. Every command has a 240-second limit. Environment-sensitive tests run serially. No download, private history/config access, live Herdr, network, or SSH call ran.

Before the guard change, `red-regressions.log` records two assertion failures. App allowed close with unknown ownership, and TUI opened a confirmation prompt. `red-close.log` records the guard returning `None` for unknown ownership and retained execution returning the generic action-change error instead of the ownership error. The other five focused close tests passed. An earlier compile in `red-app.log` caught two missing test imports, corrected before these assertion runs.

After the guard change, `green-regressions.log` records both new refresh regressions passing. `green-close.log` records all seven focused close tests passing. The first App run in `app.log` caught another inherited assertion that permitted a foreign-scope owner. That assertion now requires the unverified-ownership error; `app-final.log` passes all 59 App tests.

Final logs are under `/tmp/n09-fix1-r5aMwa`. Each command below ran through `python3 /tmp/n09-fix1-r5aMwa/run.py <log-name> <command>`.

| Command | Log | Result |
| --- | --- | --- |
| `cargo test --offline app::tests -- --test-threads=1` | `app-final.log` | 59 passed |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 47 passed |
| `cargo test --offline navigation:: -- --test-threads=1` | `navigation.log` | 13 passed |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 244 passed, three more than the reviewed baseline |
| `cargo build --offline --release` | `release.log` | Passed, build only |
| `git diff --check` | `diff.log` | Passed |

The scoped diffs are `app-fix.diff`, `tui-fix.diff`, and `todo-fix.diff` in the same log directory. Only the two allowed Rust files were formatted. No commit, merge, push, cleanup of prior work, or N10 implementation ran. Passing checks do not close the review backlog. Finding 1 is repaired pending independent recheck; findings 2 through 5 remain pending, overall review remains FAIL, and N10 remains held.

### Finding 2 repair

- `push_unique` still requires full `EntryIdentity` equality. Only builtin `sessions` identities with configured action `attach`, both `Source::Session`, and equal `AttachSession { name, remote: None }` actions can differ in title/path. Project, template file, workspace ancestry, and tab ancestry must still agree. Other action and metadata comparisons remain strict.
- Compatible local duplicates retain the first title, subtitle, and path. The collector emits discovery before configuration, so discovery wins when present. Otherwise the first configured alias wins. This is deterministic collector-order precedence, not order independence. Dedupe merges provenance and adds duplicate titles and search terms without repeated search terms. Incoming and existing conflict diagnostics survive compatible and incompatible duplicates.
- A recording CLI regression uses `bin_path_lock`, a temporary executable with RAII environment restoration, and temporary `App::with_history` storage. Only the sessions source is enabled. The stub returns discovered `work` with a session directory and running status. Configuration adds `Work alias` targeting `work` with a unique tag. Real App refresh collects, deduplicates, prepares, and resolves one usable row. Both legacy search and N09 projection find the alias and tag. The row retains discovery path/status and resolves to process-owned local Attach targeting `work`.
- Committing the displayed token sends exactly `session attach work` once, without alias or path arguments. Replay sends nothing. Discovered, configured, and merged rows have the same durable pin key. A reconstructed App projects the pin after discovery disappears and only the configured alias remains. Changing the target does not transfer that pin.
- Retained-token tests change title, path, and target separately. Each rejects activation without dispatch until a new displayed token is committed. The old token remains unusable after that draw. Action equality and dispatch code were not relaxed.
- Countertests cover changed Attach names, local versus remote, Attach versus Run/OpenRemote, project/template differences, non-Attach display differences, provider ID/namespace/action-key differences, both source checks, distinct session/remote targets, live cross-scope identities, changed live ancestry, configured order, search-term deduplication, and incoming/existing conflict retention.

Only `src/app.rs` and `tasks/todo.md` changed for this repair. Fix-start copies and the prior tracked diff/status are under `/tmp/n09-fix2-oRqBDeQj/start`. No identity format, integration, history, action, transport, or owner-model change was made. Earlier uncommitted work remains intact.

### Finding 2 verification

The fresh runner `/tmp/n09-fix2-oRqBDeQj/run.py` copies the finding 1 isolation runner. Every command gets temporary HOME, XDG config/cache/data/state/runtime, and TMPDIR directories. It clears inherited `HERDR_*` and `XDG_*`, preserves installed Cargo/Rustup caches, and uses offline Cargo. Fail-closed Herdr, Herdr Plus, SSH, SCP, curl, wget, and zoxide stubs precede system commands. Environment-sensitive tests run serially. Commands have a 240-second limit. The temporary session CLI accepts only session list and the exact test Attach targets; all other commands fail closed, including App's unconditional inventory reads.

`red-collector.log` records the production regression failing before the fix at `action_conflict`, which contained the duplicate conflict diagnostic instead of `None`. `green-collector.log` records that same test passing after the fix. No compile or lint correction was needed. Eight new tests bring the previous 244-test baseline to 252.

All logs are under `/tmp/n09-fix2-oRqBDeQj`. Commands below use `python3 /tmp/n09-fix2-oRqBDeQj/run.py <log-name> <command>`.

| Command | Log | Result |
| --- | --- | --- |
| `cargo test --offline app::tests -- --test-threads=1` | `app-final.log` | 67 passed |
| `cargo test --offline integrations::sessions::tests -- --test-threads=1` | `sessions.log` | 3 passed |
| `cargo test --offline actions::tests -- --test-threads=1` | `actions.log` | 3 passed |
| `cargo test --offline history::tests -- --test-threads=1` | `history.log` | 15 passed |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full.log` | 252 passed |
| `cargo build --offline --release` | `release.log` | Passed, build only |
| `git diff --check` | `diff.log` | Passed |

Scoped diffs are `app-fix.diff` and `todo-fix.diff` in the log directory. Only `src/app.rs` was formatted. No delegation, download, private-config inspection, live Herdr, network, SSH, commit, merge, push, or N10 implementation ran.

Finding 2 is repaired pending independent recheck. Overall review remains FAIL with findings 3, 4, and 5 pending. N07 and S01 release blockers remain. N10 stays held. The required commit comes only after all review passes and before N10, not during this repair.

### Finding 3 repair

- Added test-only `App::for_test` with a private `TempHistory` guard. Each App owns an absolute temporary directory allocated with PID plus an atomic counter. Allocation retries existing names at most 128 times without touching collisions. Unix directory creation sets mode 0700 atomically. The helper opens the real Store and requires a healthy diagnostic. Moving the App retains its directory; normal drop and panic unwind remove only its owned directory. App itself has no Drop implementation, so production callers can still move out entries.
- Replaced all 32 ordinary `App::new` calls, 10 in App tests and 22 in TUI tests. `test_app` still explicitly sets co-location. Explicit `with_history` persistence, session CLI and terminal-loop fixtures remain unchanged. The helper changes no environment variables and acquires no binary-path test lock. Production `App::new`, both main call sites, and `src/history.rs` remain unchanged.
- Added tests for distinct directories and provider owners, real pin writes and reopen, moved-App lifetime, independent cleanup, and unwind cleanup. The subprocess regression runs the actual ordinary suite at default parallelism, without its own parent test, under a private sentinel config directory. A second fresh process tests HOME fallback without `HERDR_PLUGIN_CONFIG_DIR`, including the cached home path. Snapshots compare every sentinel filename and byte, so imports, lock files and temporary files fail the test.
- An ignored, guarded positive control validates its temporary environment, marker, paths and fail-closed executables before calling unchanged production `App::new`. It must import the waiting legacy pin, leave legacy bytes intact, and create the lock. Plain `cargo test -- --ignored` returns before construction when the marker is absent. These fixtures are test data, not authority evidence.
- Subprocesses get a frozen, filtered environment rather than a parent environment swap. HOME, XDG config/cache/data/state/runtime, TMPDIR and cwd are temporary. PATH blocks Herdr, Herdr Plus, SSH, SCP, curl, wget and zoxide. Child output goes to files, not pipes. Each child has a 40-second deadline and a guard that kills its process group and reaps it before directory cleanup on timeout or unwind. Successful runs also assert that ordinary App directories and sentinel roots were removed.

### Finding 3 verification

Fix-start snapshots are in `/tmp/n09-fix3-EYF65TYn/start`. Scoped diffs are `app-fix.diff`, `tui-fix.diff`, and `todo-fix.diff` in that directory's parent. `cleanup-scope.log` records no App or sentinel directory left under any check's TMPDIR, unchanged explicit `with_history` call counts, and byte-for-byte preservation of the separately reproduced Attach race test. The fresh `run.py` copies the finding 2 isolation runner. Every command used offline Cargo, temporary HOME/XDG/TMPDIR and fail-closed executables. Installed Cargo/Rustup caches remain available. Each runner command has a 240-second outer bound; formatting, check and diff also use 30-, 120- and 20-second limits. Only `src/app.rs`, `src/tui.rs`, and this record changed for this repair. No dependencies, production storage changes, private config/history access, live Herdr/network/SSH operation, commit, merge, push or N10 work ran.

The pre-fix regression was added before replacing constructors. `red-sentinel.log` records all 254 ordinary child tests passing while the parent fails because ordinary App construction imports the waiting pin and creates `navigation-history.lock`. The only affected history was the temporary sentinel. `green-sentinel-final.log` passes both path modes and both production positive controls after replacement. Cleanup checks cover normal return and unwind.

| Command or check | Log | Result |
| --- | --- | --- |
| Exact `app::tests::ambient_history::ordinary_tests_leave_ambient_history_untouched` | `red-sentinel.log` | Expected failure before constructor replacement |
| Same exact regression after replacement | `green-sentinel-final.log` | Passed, including default-parallel children and production controls |
| `cargo test --offline app::tests -- --test-threads=1` | `app-recheck.log` | 70 passed, 1 ignored |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 47 passed |
| `cargo test --offline history::tests -- --test-threads=1` | `history.log` | 15 passed |
| `cargo test --offline -- --ignored` | `ignored-guard.log` | 1 passed without opening production history |
| `cargo test --offline -- --test-threads=1` | `full-serial-final.log` | 255 passed, 1 ignored |
| `cargo test --offline` | `full-parallel-last.log` | 255 passed, 1 ignored; earlier failures remain recorded below |
| `cargo fmt --check` | `fmt-final.log` | Passed |
| `cargo check --offline` | `check-final.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy-final.log` | Passed |
| `cargo build --offline --release` | `release-final.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

Parallel execution is not consistently green. The initial App and full-parallel checks ran alongside other checks. `app.log`, `full-parallel.log`, and the later independent `full-parallel-recheck.log` report `navigation::server::tests::uncertain_ack_invalidates_scope_and_capabilities_without_retry` receiving NotSent instead of Unknown, with a synthetic socket accept timeout. That test has an 80 ms deadline. The first full-parallel run also failed `tui::tests::failed_focus_keeps_overlay_and_side_picker_usable_without_replay_or_history_writes` after a synthetic inventory deadline. Two default-parallel runs without the sentinel parent passed in `parallel-without-sentinel.log` and `parallel-without-sentinel-2.log`; those socket failures were not reproduced separately without concurrent suites. No deadline was changed or assertion suppressed.

`full-parallel-env-frozen.log` exposed a separate failure in the unchanged `tui::tests::failed_interactive_attach_resumes_and_never_replays_on_redraw`, with a missing call log after a fail-closed executable ran. That test swaps `HERDR_BIN_PATH` without `bin_path_lock`. A separate five-test reproduction, without the sentinel test, ran it beside the existing environment-mutating runner/context tests. Attempt 3 failed its expected-error assertion. The script and evidence are `reproduce-attach-race.py` and `attach-race-reproduction.log`. This race remains outside finding 3's storage-isolation repair. The sentinel's byte comparison passed before these child-status failures. Serial success does not resolve the parallel race.

Finding 3 is repaired pending independent recheck, based on the red/green sentinel, real Store controls and cleanup tests. This does not claim that ordinary tests avoid all ambient reads; agent-sort config reads remain, so verification still requires temporary HOME/XDG. Findings 4 and 5 remain pending. Overall review remains FAIL, N10 stays held, and N07/S01 release blockers remain. The user-requested commit waits for all review passes.

### Finding 4 repair

- Added an identity guard before `Entry::has_provenance`'s compatibility source shortcut. A discovered `EntryIdentity::Machine` rejects `Source::Server`. The machine constructor still has its legacy Server category, empty provider provenance, Machine identity and unavailable native-workspace action. No provider-only restriction replaces the compatibility shortcut.
- Added one picker regression through `FixtureClient::sample().picker_input()`, its existing production inventory join and action resolution, and `search_ids`. For nucleo, skim and simple, query `void` returns the void machine and its descendants under All without provenance, exactly that machine under Machines, and no destinations under either type with Server provenance. Explicit `void-dev` ownership gives the same results; local ownership excludes them. Empty-query Machines plus Server has no selectable destination. Independent setters preserve other filters, query bytes, browse selection/viewport and expansion. Clearing Server restores the machine on the same state. Existing unknown-owner rejection and merged-provider checks remain unchanged.
- Extended constructor-only integration assertions. Configured remotes still have Server provenance, Session destination kind and the exact existing OpenRemote target. Command `server` and `remote-terminal` items still have Server and Integration provenance, Action destination kind and exact RunCommand text. Session-category compatibility checks remain. No new test executes these actions.

### Finding 4 verification

Fix-start snapshots of all tracked and untracked files are in `/tmp/n09-fix4-wnheycou/start`; the original status and tracked diff are beside them. Only `src/model.rs`, `src/picker/tests.rs`, test sections in `src/integrations/sessions.rs` and `src/integrations/command.rs`, and this record changed. The directory contains per-file fix-start diffs and `scope.log`. Existing work is preserved.

Commands use `python3 /tmp/n09-fix4-wnheycou/run.py <log-name> <command>`. The runner copies finding 3's isolation setup, clears inherited HERDR/XDG variables, creates temporary HOME/XDG/TMPDIR, preserves installed Cargo/Rustup caches and places fail-closed Herdr, Herdr Plus, SSH, SCP, curl, wget and zoxide stubs first on PATH. Cargo runs offline. Each invocation has a 240-second bound. Checks ran sequentially, not alongside other checks launched by this repair.

| Command or check | Log | Result |
| --- | --- | --- |
| Exact new picker regression before guard | `red-picker.log` | Expected failure at All plus Server, nucleo |
| Exact domain provenance regression before guard | `red-domain.log` | Expected failure at machine Server rejection |
| `cargo test --offline model::domain_tests -- --test-threads=1` | `green-domain.log` | 8 passed |
| `cargo test --offline picker::tests -- --test-threads=1` | `picker-final.log` | 13 passed, including all three engines |
| `cargo test --offline integrations:: -- --test-threads=1` | `integrations.log` | 17 passed |
| `cargo fmt --check` | `fmt-final-state.log` | Passed |
| `cargo check --offline` | `check-final.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy-final-state.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full-serial-final.log` | 256 passed, 1 ignored |
| `cargo test --offline` | `full-parallel-final.log` | 255 passed, 1 failed, 1 ignored; details below |
| `cargo build --offline --release` | `release-final.log` | Passed, build only |
| `git diff --check` | `diff.log` | Passed |

The first post-guard picker run in `green-picker.log` caught an incorrect extra test assertion. The existing `browse` helper supplies all eligible IDs directly to projection rather than computing filter eligibility. That assertion was removed; the empty-query assertion uses production `search_ids` instead. `clippy.log` caught two unnecessary test clones, replaced with `std::slice::from_ref`. Final focused tests and strict lint pass.

Parallel checks remain red. `full-parallel.log` failed the sentinel parent because its ordinary child failed `tui::tests::failed_focus_keeps_overlay_and_side_picker_usable_without_replay_or_history_writes` after a synthetic inventory deadline and socket accept timeout. `full-parallel-recheck.log` repeated that child failure and also failed `navigation::server::tests::uncertain_ack_invalidates_scope_and_capabilities_without_retry` in the parent suite. These match the previously recorded socket failures. The final run failed the sentinel parent because its ordinary child failed unchanged `herdr::runner_tests::process_limits_and_inherited_pipes_have_absolute_deadlines`, which received Timeout instead of StdoutLimit at `src/herdr.rs:666`. This last failure was not previously recorded in finding 3 and has not been independently reproduced. All three tests pass in the serial suite. No timeout or assertion was weakened, and no runner, socket, TUI or sentinel implementation changed. The separately reproduced Attach environment race from finding 3 also remains unresolved.

Finding 4 is repaired pending independent recheck based on the red/green provenance tests and unchanged provider contracts. This does not claim a green parallel suite. Overall review remains FAIL with finding 5 pending. N10 remains held and N07/S01 release blockers remain. No new repository file, dependency, collector, transport, history, keymap, renderer or ownership change was made. No delegation, download, private-config access, live Herdr/network/SSH operation, commit, merge or push ran. The required commit still waits for all review passes and must precede N10.

### Finding 5 owner projection repair

- `App::picker_input` resolves the primary action once per entry. Only non-live Action destinations use that resolved owner for placement. Server actions retain their resolved scope. Process actions retain the existing co-location-aware process heading. Live targets, projects, folders and Attach sessions keep their existing placement. An unavailable folder can still have a server-owned action without moving its process-owned input row.
- Supported Quick Actions groups belong to the server. Enabled command integrations declare process Actions groups. Identical owner/group pairs collapse only when the existing co-location relation puts both under the same heading. Supported empty groups remain explicit. Projects and folders and Sessions support declarations are unchanged.
- Added `picker_action_owner_refresh_projection` through real `App::for_test`, `refresh`, `picker_input` and `picker_projection`. A recording runner supplies exactly workspace, pane and tab fixture reads on an unmanaged route with no socket. A bounded command collector logs one invocation and returns fixed JSON with `kind: server`. Quick Actions appears under server Actions, while the command remains under process Actions despite its Server source. The live workspace remains server-owned.
- The refresh regression launches only its exact test in a fresh child. It reuses the existing isolated-child runner and private-root setup, with temporary HOME/XDG/config/TMPDIR/cwd, fail-closed executables, a checked child marker, a 40-second deadline, process-group kill/reap and RAII cleanup. Only that child creates the HOME-based Quick Actions directory. No parent environment swap was added.
- Three repeated input/projection calls preserve runner specs, collector log bytes, history directory filenames/bytes, ambient sentinel bytes, legacy selection, source filter, query and filtered rows/scores. Companion tests cover all quick/command enabled combinations with and without proven co-location, supported empty groups, unavailable Quick Actions and Run owners, unchanged project/folder/Attach placement, and no invented machine or Local heading. Unavailability remains attached to the resolved action and grants no dispatch authority.

### Finding 5 verification and scope block

Fix-start copies and all-file hashes are in `/tmp/n09-fix5-ipea5scu/start`. Commands used `/tmp/n09-fix5-ipea5scu/run.py`, copied from the finding 4 runner. Each command had a 240-second bound, offline Cargo with installed caches, temporary HOME/XDG/TMPDIR and fail-closed executables. Checks ran sequentially. Only `src/app.rs` was formatted.

| Check | Log | Result |
| --- | --- | --- |
| Initial focused compile | `red-owner.log` | Failed because the new test tried to construct private Invocation fields; changed the test to use the existing invocation constructor |
| New tests before owner repair | `red-owner-compiled.log` | All three failed on process placement or missing server Actions support |
| `cargo test --offline picker_action_owner -- --test-threads=1` | `green-owner.log` | 3 passed after the repair |
| App picker tests | `focused-picker-app.log` | 4 passed |
| N09 tests | `focused-n09.log` | 2 passed |
| Ordinary Quick Actions dispatch test | `focused-ordinary-quick.log` | 1 passed |
| Picker tests | `focused-picker.log` | 13 passed |
| `cargo test --offline -- --test-threads=1` | `full-serial.log` | FAIL, 257 passed, 2 failed, 1 ignored |
| `cargo test --offline` | `full-parallel.log` | FAIL, 257 passed, 2 failed, 1 ignored |
| `cargo fmt --all -- --check` | `fmt-check.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff-check.log` | Passed |
| Fix-start hash and TMPDIR cleanup comparison | `scope-check.log` | Only `src/app.rs` and `tasks/todo.md` changed; no App or isolated-child directories leaked |

The scoped diffs are `app-fix.diff` and `todo-fix.diff` in the same log directory. Every other repository file, including `src/tui.rs`, matches its fix-start hash.

Both full suites fail `tui::tests::n08_provider_groups_use_explicit_ownership_and_flat_non_live_request` at `src/tui.rs:3133`. That existing test explicitly requires every provider destination, including a plugin action, to have a process owner. It also expects one process heading and three groups. Those expectations conflict with the repaired owner contract. The sentinel parent fails because its ordinary child runs the same stale test. `src/tui.rs` is outside this repair's allowed files and remains byte-for-byte unchanged. Updating this test requires scope authorization; the implementation does not retain the bug to satisfy the old assertion. No test was skipped or weakened.

The full-serial command also reports `tui::tests::failed_focus_keeps_overlay_and_side_picker_usable_without_replay_or_history_writes` failing inside the sentinel's default-parallel child after an inventory deadline and synthetic socket accept timeout. This is the previously recorded timing failure, not the stale owner assertion. The sentinel byte comparison passed before its child-status assertion, but the complete sentinel test did not pass. Neither full-suite failure was retried to obtain a green result.

Finding 5's owner mapping is repaired pending independent recheck based on the three red/green regressions. Its stale TUI test remains blocked by scope. Overall review stays FAIL and full-suite validation stays red. No commit, merge, push, delegation, download, private-config access, live Herdr/network/SSH operation or N10 implementation ran. Dispatch, action resolution, transports, persistence and legacy UI code are unchanged. The required commit must follow review PASS and precede N10. N07 and S01 remain independent release blockers.

### Finding 5 authorized TUI test completion

The parent authorized the unfinished test portion after inspecting the obsolete ownership assertions. This completion supersedes the TUI scope block above. Only `src/tui.rs` test `n08_provider_groups_use_explicit_ownership_and_flat_non_live_request` and this evidence record changed. Fix-start copies and the pre-edit scoped diff are in `/tmp/n09-fix5-tui-7v24IAk8`.

- The test locates each destination by identity and requires the plugin action's ordinary Server owner, with the project/folder fixture and configured remote still Process-owned. Each resolved action must match App's primary resolution.
- Collapsed browse must contain exactly the Server heading and Actions group, then the Process heading and Projects and folders and Sessions groups. Group identities, labels, disclosures and logical heading parents are explicit. Expanding the groups checks each destination's exact heading and task-group ancestry and its resolved action. Neither heading invents a machine or Local ownership.
- Sessions disclosure and Attach payload checks remain. The non-live request still produces three actionable depth-zero rows in caller-supplied reverse identity order, with no selection. The explicit machine co-location fixture and unavailable native-workspace assertions remain unchanged. A final recording-runner assertion checks that all projections made no calls. Other flat tests and actual legacy render tests are unchanged.

Commands used `python3 /tmp/n09-fix5-tui-7v24IAk8/run.py <log-name> <command>`, copied from the previous finding 5 runner. Each command had a 240-second bound. Cargo used offline dependencies and installed caches, temporary HOME/XDG/TMPDIR and fail-closed executables. Checks ran sequentially. Only `src/tui.rs` was formatted. Both full suites ran once, without retries.

| Command | Log | Result |
| --- | --- | --- |
| `cargo test --offline tui::tests::n08_provider_groups_use_explicit_ownership_and_flat_non_live_request -- --exact --test-threads=1` | `focused.log` | 1 passed |
| `cargo test --offline tui::tests -- --test-threads=1` | `tui.log` | 47 passed |
| `cargo fmt --all --check` | `fmt.log` | Passed |
| `cargo check --offline` | `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo test --offline -- --test-threads=1` | `full-serial.log` | 259 passed, 1 ignored |
| `cargo test --offline` | `full-parallel.log` | FAIL, 258 passed, 1 failed, 1 ignored |
| `git diff --check` | `diff.log` | Passed |

The parallel failure is the previously recorded `navigation::server::tests::uncertain_ack_invalidates_scope_and_capabilities_without_retry`. At `src/navigation/server.rs:962`, it received NotSent instead of Unknown. Its socket worker then reported a synthetic accept timeout at line 730. The repaired TUI test and sentinel parent passed in both full runs. No timing, environment-lock or production change was made. The earlier Attach race and process-limit failure remain unresolved even though neither reproduced in these runs.

Finding 5 is repaired pending independent recheck, including its test portion. Overall review remains FAIL pending recheck, and global validation remains blocked by the separate failures below. N10 stays held. The required commit must follow review PASS and precede N10. N07 and S01 remain release blockers. No delegation, download, private-config access, live Herdr/network/SSH operation, commit, merge or push ran.

### Separate validation follow-up

This follow-up is distinct from the five review findings. The tests-only socket/deadline and Attach repairs below address all three diagnosis items, pending independent recheck.

- [x] Diagnose the synthetic socket deadline failures in `navigation::server::tests::uncertain_ack_invalidates_scope_and_capabilities_without_retry` and `tui::tests::failed_focus_keeps_overlay_and_side_picker_usable_without_replay_or_history_writes`. The repair and retained red evidence are below.
- [x] Separate output-cap validation from the short process deadline in `herdr::runner_tests::process_limits_and_inherited_pipes_have_absolute_deadlines`. The original failure remains in `/tmp/n09-fix4-wnheycou/full-parallel-final.log`. The controlled reproduction below confirms competing test conditions, not a production error-precedence defect.
- [x] Repair the separately reproduced Attach environment race recorded in finding 3. The explicit fixture invocation and controlled interference regression are below.
- [x] Obtain independent standards and specification rechecks of all five findings and both validation repairs. Final interim reviews `sa-3a77d876` and `sa-6d27834f` both returned PASS with no issues.
- [ ] After both independent reviews pass, make the user-required commit before N10. No merge, push or release waiver is authorized.

Passing focused tests, static checks and the release build do not establish independent review PASS.

### Socket/deadline validation repair

Only the test module in `src/navigation/server.rs`, `src/herdr.rs` runner tests, affected helpers/assertions in `src/tui.rs` tests, and this record changed. Fix-start copies, the complete starting diff/status, diagnostic logs and final scoped diffs are in `/tmp/n09-deadline-6SgJC5iA`. Production source prefixes and all other tracked diffs match the fix-start snapshot. `src/app.rs`, the history sentinel, process polling, output caps, transport deadlines and error precedence are unchanged.

Ranked hypotheses, stated before controlled experiments:

1. Setup or negotiation consumes the 80 ms whole-operation budget before mutation. A delayed snapshot should return Timeout/NotSent; a withheld mutation acknowledgement should return Timeout/Unknown.
2. The fixture's separate accept timer and 200 ms Hang sleep create secondary failures. Keeping Hang connected until client disconnect should exercise the actual client deadline instead of malformed EOF.
3. The runner's 100 ms startup/output budget can expire before its infinite producer exceeds 512 bytes. Delayed finite output should reproduce Timeout with the short budget and the exact cap failure with the normal budget.

The initial full-parallel baseline passed 259 tests with 1 ignored. Ten isolated runs of each original affected test passed, 30/30. The first of two exact sentinel samples failed in its default-parallel fallback child; the second passed. The failed trace records two completed malformed-ack cases, then Timeout/NotSent at 81.34 ms with no recorded request. The worker recorded `session.snapshot` afterward and timed out waiting for mutation. This reproduces the reported failure before any fix. Both sample outcomes remain in `pre-sentinel-sample.log` and their numbered logs.

Controlled tests narrowed the causes. A 150 ms snapshot delay returned Timeout/NotSent at 80.17 ms. A finite 640-byte producer delayed by 150 ms returned Timeout at 105.57 ms under the 100 ms limit. The original Hang returned Malformed/Unknown at 204.61 ms with the normal two-second deadline. All three intended assertions failed in `minimized-red.log`. One overlay/malformed TUI case with the delayed snapshot failed with deadline exceeded/NotSent in `minimized-tui-red.log`. With the normal budget, the delayed snapshot reached mutation, the finite producer returned StdoutLimit, and the same TUI case passed. Those controls are in `minimized-delay-controls.log` and `minimized-tui-control.log`.

The natural process-cap failure did not recur in this pre-fix sample. The old log and controlled reproduction show that the test asked for a cap error while permitting a correct earlier deadline error. They do not prove the precise scheduling delay in the old run. No evidence required a production change.

Changes:

- The socket fixture waits for bounded worker readiness. It records each request before replying. Hang withholds its acknowledgement until client disconnect or teardown cancellation. A separate five-second fixture watchdog bounds accept/read/Hang work. Teardown cancels pending work, joins the worker and removes the socket. Missing expected requests still fail normal completion; a dedicated test checks cancellation and removal.
- Malformed acknowledgement cases use the existing normal two-second budget and require Malformed/Unknown. Separate end-to-end `NavigationContext::focus` tests require Timeout/Unknown after exactly snapshot plus pane mutation, and Timeout/NotSent after snapshot only. They assert elapsed client deadlines, bounded cleanup, scope invalidation, unknown capabilities for every kind, and stale-target rejection without retry.
- `loop_app` no longer imposes 80 ms on inventory and every subsequent action. The complete five-case, two-mode TUI matrix remains. The timeout case now requires the actual deadline error with Unknown, elapsed deadline, exact snapshot/focus requests, unchanged view/history and no replay.
- Runner deadline tests keep their short limits for silent children and inherited pipes. Separate stdout and stderr cap tests use finite 640-byte output, a 512-byte cap, delayed startup and the normal two-second budget. They require exact cap failures and Unknown. Request-limit/spawn tests require NotSent. Descendant cleanup also requires the exact timeout disposition and consumed deadline. No test accepts alternative failure outcomes.

All commands ran offline through `python3 /tmp/n09-deadline-6SgJC5iA/run.py <unique-log-name> <command>`. The runner uses temporary HOME/XDG/TMPDIR, cleared inherited Herdr routing, cached Cargo/Rustup locations and fail-closed Herdr/SSH/download-command stubs. Every invocation has a bound of at most 240 seconds. The final runner reserves time to kill the process group and reap it after timeout. No command timed out. Batches have fixed counts, retain every result and return failure if any sample fails. No suite-wide lock or serialization was added.

| Validation | Logs | Result |
| --- | --- | --- |
| Initial `cargo test --offline` | `baseline-full-parallel.log` | 259 passed, 1 ignored |
| Ten exact runs each of original socket, process-limit and failed-focus TUI tests | `pre-sample.log`, `pre-*-01.log` through `pre-*-10.log` | 30 passed |
| Two exact history-sentinel runs | `pre-sentinel-sample.log`, numbered sentinel logs | 1 failed, 1 passed |
| Minimized diagnostic tests and TUI case | `minimized-red.log`, `minimized-tui-red.log` | 3 failed, then 1 failed as intended |
| Normal-budget controls | `minimized-delay-controls.log`, `minimized-tui-control.log` | 2 passed, then 1 passed |
| Focused server, runner and exact TUI checks | `focused-*-corrected.log` | 13, 10 and 1 passed |
| Twenty exact runs each of nine affected tests | `post-socket-sample.log`, `post-runner-sample.log`, `post-tui-sample.log` and all numbered logs | 180 passed, 0 failed |
| Three `cargo test --offline` runs | `post-full-parallel-1.log` through `post-full-parallel-3.log` | Each 265 passed, 1 ignored |
| `cargo test --offline -- --test-threads=1` | `post-full-serial.log` | 265 passed, 1 ignored |
| Exact sentinel timing check | `sentinel-runtime.log` | 1 passed, 12.65 s test runtime |
| Default-parallel ordinary suite without the sentinel parent | `ordinary-child-runtime.log` | 264 passed, 1 ignored, 1 filtered; 5.93 s |
| `cargo fmt --check`, `cargo check --offline` | `fmt.log`, `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |
| Cleanup reruns of server, runner and all TUI tests | `cleanup-server.log`, `cleanup-runner.log`, `cleanup-tui.log` | 13, 10 and 47 passed |

The nine 20-run samples cover malformed acknowledgement, withheld acknowledgement, withheld negotiation, process deadlines, stdout cap, stderr cap, pre-spawn rejection, descendant cleanup and the full failed-focus TUI matrix. Each numbered log contains the exact test name and invocation. One intermediate compile error required marking the consumed cleanup-test socket unwind-safe. All three initial focused logs retain that same compile error; the corrected focused runs passed. Temporary diagnostic tests and tagged logging were removed before the fixed post-repair samples. No production cap/deadline removal mutation was performed.

The ordinary child-sized run took 5.93 seconds. The unchanged sentinel completed both child modes and its controls in 12.65 seconds, below its existing 40-second per-child bound. The three full-parallel suites took 14.30, 15.11 and 14.91 seconds. No sentinel timeout change is needed by these measurements.

The separate Attach environment race remained unresolved at the end of that batch. The follow-up below supersedes that open item. Independent review must still pass for this repair and every review finding. N10 remains held. Commit only after all reviews pass, before N10. No delegation, download, private-config access, live Herdr/network/SSH operation, commit, merge or push ran. The release build is not a release waiver; N07/S01 blockers remain.

### Attach test environment-race repair

Only `src/herdr.rs`, `src/tui.rs` test code and this task record changed. Fix-start source copies, worktree diff/status and hashes are in `/tmp/n09-attach-6b4QmB63/start`. This repair preserves the branch baseline `6b5ea9e`, all earlier uncommitted work, the App history sentinel and the socket/deadline fixtures.

The original red evidence remains in `/tmp/n09-fix3-EYF65TYn/attach-race-reproduction.log` and `full-parallel-env-frozen.log`. Before experiments, the ranked hypotheses were capture contamination, fixture cwd/lifetime, then dispatch recapture. The predictions were a decoy call if the swap precedes App construction, no path/lifetime failure with owned absolute fixtures, and a decoy call after construction only if production dispatch recaptures.

The controlled reproduction used the original TUI test path under `bin_path_lock` with RAII restoration. Intended executable A exits 7. Decoy B writes a distinct marker and exits 23. Selecting A, then B immediately before `loop_app`, failed twice with a missing intended call log and `decoy\nsession attach synthetic-only\n` in B's log. `controlled-red.log` and `controlled-red-repeat.log` retain both failures. Moving B's override after App construction passed both overlay and persistent modes with A's exact arguments and no decoy log. B remained selected through dispatch in that control, recorded in `controlled-after-app.log`. Captured diagnostic source copies remain beside the logs; no diagnostic logging remains in repository source.

The cause was test setup. The old test changed process-wide `HERDR_BIN_PATH` without the shared lock, then `loop_app` captured whatever executable was selected at that instant. Production dispatch correctly kept the captured route. A lock around production capture or serialization of the ordinary Attach test would hide this setup dependency rather than remove it.

- Added `#[cfg(test)] Invocation::for_test(executable, cwd, environment)`. It directly stores explicit fields, requires absolute executable/cwd paths, and sets no socket and unmanaged defaults. Existing fields remain private. Production capture, routing, process execution and earlier runner tests are unchanged.
- Added `loop_app_with_invocation` through the existing `OrdinaryServer::new` and `NavigationContext::with_server` interfaces. Existing `loop_app(state, socket)` callers retain their configuration, recording runner and normal limits. The Attach fixture replaces App's initial ambient context before action preparation or execution.
- The ordinary Attach test now uses absolute owned fixtures and an explicit invocation, with no environment swap or lock. Scripts use `/bin/sh` and shell builtins. The minimal explicit environment supplies quoted log paths and a non-routing token. A separate interference regression uses the shared environment lock and RAII to select B before App construction and keep B selected through dispatch.
- Both tests exercise overlay and persistent modes. They reject activation before display, then execute the real draw/token/Enter path. Assertions require exactly one `session attach synthetic-only`, exit 7 inline, terminal suspend/resume, unchanged selected view and error on Resize, usable Escape and final restoration, zero ordinary runner calls and zero decoy calls. The intended child must report its fixture cwd and explicit token. Fixture removal and restoration of the interference test's ambient executable are checked.

Commands ran through `python3 /tmp/n09-attach-6b4QmB63/run.py <unique-log-name> <command>`, copied from the earlier deadline runner. Each command uses temporary HOME/XDG/config/TMPDIR, cleared inherited Herdr routing, cached Cargo/Rustup locations and offline Cargo. PATH blocks Herdr, Herdr Plus, SSH, SCP, curl, wget and zoxide. Commands are bounded to at most 240 seconds with process-group cleanup and child reaping. Post-fix runs remove their isolation roots after checking for leftover TUI fixtures. The five-test sample has a ten-second bound per run, a fixed count of 20 and retains every result without stopping on failure. No random sleeps, retry-until-green loop, suite-wide lock, skip or sentinel timeout change was added.

| Validation | Logs | Result |
| --- | --- | --- |
| Exact repaired Attach test | `focused-attach.log` | 1 passed, both modes |
| Exact interference regression | `focused-interference.log` | 1 passed, both modes |
| Original five-test concurrent selection, `--test-threads=5`, 20 fixed runs | `fixed-five-batch.log`, `fixed-five-01.log` through `fixed-five-20.log` | 20/20 runs passed, 100 test executions |
| `cargo test --offline tui::tests` | `tui-suite.log` | 48 passed |
| `cargo test --offline herdr::runner_tests` | `runner-suite.log` | 10 passed |
| Three `cargo test --offline` runs | `full-parallel-1.log` through `full-parallel-3.log` | Each 266 passed, 1 existing ignored control |
| `cargo test --offline -- --test-threads=1` | `full-serial.log` | 266 passed, 1 existing ignored control |
| `cargo fmt --check`, `cargo check --offline` | `fmt.log`, `check.log` | Passed |
| `cargo clippy --offline --all-targets -- -D warnings` | `clippy.log` | Passed |
| `cargo build --offline --release` | `release.log` | Passed |
| `git diff --check` | `diff.log` | Passed |

The five selected tests are Attach, `capture_freezes_executable_cwd_environment_and_absence`, `frozen_route_covers_template_followons_notifications_attach_and_process_environment`, `managed_plugin_contract_requires_explicit_inherited_route_and_runtime_markers` and `environment_owner_hint_never_sets_client_selection_or_path_authority`. Every full suite includes the unchanged history sentinel, its nested ordinary children and its isolated controls. Parallel suite runtimes were 13.38, 11.97 and 14.32 seconds; serial was 25.17 seconds. No post-fix validation failed or timed out. The two intentional red results remain recorded.

Scope verification compares all tracked and untracked source/document hashes against fix-start. Only the three authorized files differ. Removing the test-only constructor makes `src/herdr.rs` byte-identical to fix-start. The TUI production prefix, failed-focus test, App sentinel and socket/deadline module are unchanged. No temporary diagnostic logs or fixture directories remain in repository source or the checked isolation roots.

All five findings and both validation repairs still require independent standards and specification rechecks. These passing checks do not establish review PASS or commit readiness. N10 stays held until reviews pass and the user-required commit is made. No delegation, dependency download, private-config/history access, live Herdr/network/SSH operation, commit, merge or push ran. N07/S01 and release qualification remain blocked.

### New managed-ambient ownership fixture finding

This is a separate finding from the prior five findings and the Attach repair. Standards review `sa-a8f9cf5a` found that ordinary ownership tests still captured managed route variables changed by sibling runner tests. The interim specification result `sa-a6eb0bc5` does not close this finding.

- [x] Reproduce the existing ownership assertions in isolated managed-environment children and repair their fixtures.
- [x] Run the fixed validation batches and preserve every result.
- [x] Obtain fresh independent standards and specification rechecks. Final interim Standards `sa-3a77d876` and specification `sa-6d27834f` both returned PASS with no issues, including this fixture repair. N10 has not started.

Fix-start copies, status, the full starting diff and hashes are in `/tmp/n09-ownership-zimuurk5/start`. Only `src/navigation/mod.rs`, `src/paths.rs`, `src/app.rs`, `src/tui.rs` test code and this record changed. The branch baseline remains `6b5ea9e`; all earlier uncommitted work is preserved.

The new `app::tests::ambient_history::ordinary_ownership_tests_ignore_managed_ambient_route` regression uses the existing isolated-child helper. Its opt-in managed environment sets a synthetic plugin ID/root, `HERDR_ENV=1`, a synthetic workspace hint and an absent socket below the temporary runtime directory. HOME, XDG, TMPDIR, config and cwd remain temporary; the executable is the fail-closed stub. Existing helper callers retain their old environment behavior, process-group cleanup and 40-second child bound.

Each case runs as a separate exact child. The parent collects all eight results before asserting success, including the guarded production-constructor control. Before the fixture changes, that control passed and all seven existing tests failed in `ownership-red.log`. These were assertion failures, not compilation failures or deadline failures. The regression took 0.13 seconds after compilation.

| Existing test | Recorded red symptom |
| --- | --- |
| `navigation::tests::context_targets_require_current_scope_and_valid_ancestry` | Unexpected process/server co-location |
| `paths::ownership_tests::process_paths_keep_filesystem_access_but_do_not_imply_server_ownership` | Process path unexpectedly received a reuse key |
| `app::tests::unknown_process_server_owner_blocks_create_and_template_before_any_io` | `unowned canonicalization` panic |
| `tui::tests::foreign_paths_render_as_reported_text_and_unknown_ownership_never_says_create` | Missing ownership-unavailable preview |
| `app::tests::picker_task_groups_have_no_actions_and_provider_sources_do_not_invent_machines` | Destination owner differed from the expected owner |
| `app::tests::ordinary_route_quick_actions_need_no_filesystem_colocation_and_propagate_failure` | Unexpected process/server co-location |
| `navigation::tests::environment_owner_hint_never_sets_client_selection_or_path_authority` | Unexpected process/server co-location |

The ranked predictions were that explicit unmanaged invocations would remove route contamination, explicit context fields would remove inherited picker hints, and clearing the managed contract would restore the owner-hint test's unmanaged premise. The same isolated regression passed after those test-only changes in 0.12 seconds.

Changes:

- Added test-only `NavigationContext::unmanaged` with `Invocation::for_test`, absolute temporary fixture paths, an empty environment and `RecordingRunner`. `with_server` now initializes every context field instead of recapturing through `Self::new`. Test-only `reported` and `colocated` use the explicit fixture rather than an inherited route. They do not inherit picker ownership or client selection.
- Updated the scope/ancestry test, process-path ownership test, unknown-owner App preparation test and TUI preview test to use explicit unmanaged contexts. `inventory_app` keeps its existing recording runner and uses an explicit unmanaged invocation rooted in its owned history fixture.
- Quick Actions still run the real exit-code stub through `ProcessRunner`. The invocation now names the fixture executable/cwd explicitly and has an empty environment. Exact plugin arguments, success and nonzero-exit assertions remain unchanged.
- The owner-hint test still exercises production `NavigationContext::new` under the existing environment lock. It saves, clears and restores the socket, environment marker, plugin ID/root and context hint with RAII. No new global lock or suite serialization was added.
- The guarded child control proves production `NavigationContext::new` still recognizes the managed contract and qualified owner hint without granting client selection. It only captures state, never executes the stub or connects to the absent socket. It also checks that all three explicit context fixtures have no inherited route, picker owner or client selection; only `colocated` grants its explicit test evidence.

Commands ran through `python3 /tmp/n09-ownership-zimuurk5/run.py <unique-log-name> <command>`, copied unchanged from the Attach repair. The runner uses offline Cargo, cached toolchains, temporary HOME/XDG/config/TMPDIR, filtered Herdr variables and fail-closed command stubs. Every outer command is bounded to at most 240 seconds. Each fixed batch retains all results and reports failure if any sample fails. No new sleep, retry-until-green loop, timeout increase or serialization was added.

| Validation | Logs | Result |
| --- | --- | --- |
| Exact managed-ambient regression before repair | `ownership-red.log` | Production control passed; all seven existing ownership tests failed |
| Same regression after repair | `ownership-green.log` | All eight children passed |
| Navigation, paths, App, TUI and runner suites | `navigation.log`, `paths.log`, `app.log`, `tui.log`, `runner.log` | 16, 4, 74, 48 and 10 passed; App retains 1 ignored control |
| Ten exact history-sentinel runs with default-parallel ordinary children | `fixed-sentinel-batch.log`, `fixed-sentinel-01.log` through `fixed-sentinel-10.log` | 10/10 passed; both history locations and production controls exercised each time |
| Three full default-parallel suites | `full-parallel-1.log` through `full-parallel-3.log` | Each 267 passed, 1 existing ignored control |
| Full serial suite | `full-serial.log` | 267 passed, 1 existing ignored control |
| Formatting, check, strict all-target Clippy and release build | `fmt.log`, `check.log`, `clippy.log`, `release.log` | Passed |
| Diff and scope checks | `diff.log`, `scope.log` | Passed |

The ten sentinel runtimes were 10.89 to 14.04 seconds, below the unchanged per-child bound. Full-parallel runtimes were 13.86, 15.09 and 12.08 seconds; serial was 26.31 seconds. No post-repair check failed or timed out. The red log retains every child failure. Isolation roots were removed and the runner reported no leftover TUI fixtures.

Scope verification compares every tracked and untracked file hash with fix-start. Only the five authorized files differ. Production code in the four Rust files is byte-identical after removing test-only items. `src/herdr.rs`, `src/navigation/server.rs`, the history sentinel body, prior deadline and Attach fixes, dependencies and all other files remain unchanged. No diagnostic logging or new repository file remains.

At the end of this repair, fresh independent rechecks were still pending. The prior five findings were not reopened by this separate record. No delegation, dependency download, private-config/history access, live Herdr/network/SSH operation, N10 implementation, commit, merge or push ran during the repair. The final interim review record below supersedes that pending status. N07/S01 and release gates remain blocked.

## Final interim review before the N10 checkpoint

Standards review `sa-3a77d876` and specification review `sa-6d27834f` both returned PASS with no issues. Both reviewed the full tracked and untracked snapshot in `/tmp/n09-final-review-Xr6NuF/changes.diff` and `status.txt`, against baseline `6b5ea9e6ce81a1771db7e535fb98b1b469118c81` on `feat/navigator-machine-panes`.

The independent reviews close all five original findings, the socket/deadline and Attach validation follow-ups, and the managed-ambient ownership fixture finding. The earlier FAIL results, controlled red tests and parallel failures remain above as historical evidence.

The latest validation logs remain under `/tmp/n09-ownership-zimuurk5`. Three default-parallel suites and one serial suite each passed 267 tests with one existing ignored control. All ten history-sentinel samples, formatting, offline check, strict all-target Clippy, release build, diff and scope checks passed. Commit preparation changed only this review record. All source, config, examples, design docs and fixtures match the reviewed and validated bytes.

The user authorized one local checkpoint commit after both review passes and before N10. This checkpoint contains N01 through N06, N08/N09 and partial N07/S01 work. It does not enable the merged UI or qualify a release. N10 has not started. N07, S01, client-contract, live terminal/platform and release gates remain unchecked. No merge or push is authorized.

Commit preparation staged exactly the 53 reviewed files. The offline path and credential-pattern checks found no artifact or credential matches. Only this review record differs from the validated hashes. No active local hooks were present. The full `git diff --cached --check` failed on preserved raw help captures under `tests/fixtures/navigation/ordinary-server/0.9.0/`: trailing whitespace at `pane-focus-help.txt:10` and blank lines at EOF at `pane-focus-help.txt:13`, `pane-list-help.txt:7`, `tab-focus-help.txt:7` and `workspace-focus-help.txt:7`. N01 requires byte-for-byte preservation of those captures. No fixture bytes or Git protections were changed. That attempt stopped before commit pending authorization for a whitespace exception.

The user has now authorized a whitespace-check exception only for those four byte-preserved Herdr 0.9.0 help captures. The full `git diff --cached --check` still exits 2 with exactly the five warnings listed above; it did not pass strict checking. The strict staged check passes for every other path, with four exact `:(top,literal,exclude)` path exclusions. A separate check restricted to those four paths passes with command-only `-c core.whitespace=-blank-at-eol,-blank-at-eof`. This exception preserves the approved raw evidence. It changes no fixture bytes, persistent Git configuration, attributes, hooks or other protections.

The indexed and worktree capture SHA-256 hashes match `/tmp/n09-ownership-zimuurk5/final-hashes.json`. All other recorded file hashes also match except this task record. Only `tasks/todo.md` changed during this authorization follow-up and is re-staged. Both review PASS results still apply to the unchanged source. The whitespace blocker is resolved for the authorized normal local commit `feat: add qualified navigation and picker foundations`, with exactly 53 reviewed files staged on `feat/navigator-machine-panes`. N10 has not started; N07/S01 and release gates remain blocked. No merge or push is authorized.
