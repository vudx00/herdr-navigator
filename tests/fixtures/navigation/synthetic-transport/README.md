# Synthetic transport cases

N04 transport cases live in inline Rust tests, not in captured official responses. `src/navigation/server.rs::tests::Socket` binds a temporary Unix socket, records requests, supplies explicit replies, and removes the socket after joining its bounded worker. It never contacts Herdr. Empty snapshot values and rejection-code examples are synthetic inputs using the versioned schema, not evidence of a running server or a capability export.

The tests cover:

- Exact workspace/tab/pane requests, pane IDs different from terminal IDs, unique ID correlation, and no create/attach/directional/keyboard fallback.
- Read-only version/protocol negotiation before each mutation, version changes, unknown support, per-kind explicit method rejection, and target rejection without scope rotation.
- Missing route, foreign scope, malformed and duplicate JSON keys, conflicting success/error, missing or mistyped fields, trailing JSON, truncated NDJSON, wrong IDs, hangs, and byte caps.
- Unknown focus outcomes clearing scope/capabilities and rejecting retained targets without retries.
- CLI-owned IDs, strict existing list envelopes, required inventory field types, and raw requests never becoming CLI stdin.

`src/herdr.rs::runner_tests` exercises frozen executable/env/cwd and absent routing variables, the recording runner, concurrent stdout/stderr draining, stdin, nonzero exit, spawn failure, request/output limits, hung children, inherited pipes, and descendant group cleanup. Managed-plugin tests check the explicit runtime markers, no client-authority inference, and rejection of local access to reported paths.

App tests exercise the production refresh and focus paths with injected adapters. A pane-read timeout discards the same batch's successful workspace result before conversion, clears co-location, and rotates scope. A later healthy refresh cannot restore old identities or the startup co-location observation. Workspace-row failure sends only snapshot plus exact workspace focus and leaves history unchanged. Existing Quick Actions stubs still check success and failure without filesystem permission.

See [transport provenance and limits](../ordinary-server/0.9.0/transport-contract.md). These inline synthetic cases and explanatory Markdown are not added to N01's enumerated evidence-fixture manifest.
