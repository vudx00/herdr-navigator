# Multi-machine navigation

## Decision

Navigator should show the invoking client's Local workspaces and agents alongside those of its saved remote machines, regardless of where the picker process runs. This needs upstream client-level discovery and navigation support. The inspected Herdr 0.9.0 CLI and socket schema do not expose that complete contract. Adding another Navigator source or looping over SSH targets does not solve client selection.

Scope the first release to workspaces and agents in the sessions represented by the client's machine sidebar. A saved machine is not a promise to enumerate every session on that host. Include disconnected saved machines as unavailable groups, with explicitly stale rows only when the client has cached data.

## Evidence and feasibility

- [`collect_workspaces` and `fetch_agents`](../src/sources.rs) call bare `workspace list`, `pane list`, and `agent list`. [`herdr_json` and `run_herdr`](../src/herdr.rs) inherit the process environment. They do not ask the invoking client for its combined sidebar.
- Herdr's [connecting machines documentation][machines] says plugins run on the selected server, a pane's CLI keeps its socket routing when UI selection changes, and object IDs belong to one server. The client owns the combined sidebar and retains cached remote state while disconnected. These facts explain why a picker on `void.dev` can list only `void.dev` even when the client also displays Local.
- `herdr machine list --help` documents saved-machine listing with `--json`. This is supported profile discovery, not a documented query for the invoking client's live Local and remote snapshots. A remote process's saved profiles cannot be assumed to be the client's profiles.
- `herdr api --help` describes `snapshot` as the live session snapshot. The bundled `herdr api schema --json` defines `session.snapshot` and `SessionSnapshot` with workspaces, tabs, panes, and agents, but no combined machine inventory. See the official [socket API documentation][socket].
- The schema exposes `workspace.focus`, `agent.focus`, and `pane.focus`, but no machine-qualified, invoking-client navigation request. Their CLI help has bare workspace or target arguments. `client_shell.surface.set` accepts only `active: boolean`; its schema describes pane presentation interest, not machine navigation. `command.invoke` takes an opaque endpoint-issued command ID, not a documented combined inventory or exact cross-machine target contract.
- The [plugin documentation][plugins] describes injected runtime context. The inspected schema's `PluginInvocationContext` has workspace, tab, pane, and invocation metadata, but no invoking-client route. A correlation ID must not be treated as a client address without an upstream contract.
- [`Entry`, `WorkspaceRef`, and `EntryAction`](../src/model.rs) have no machine/session identity. [`App`](../src/app.rs) focuses bare IDs, keys workspace pins and deduplication by bare IDs, stores bare jump-back workspace IDs, and indexes cwd matches by locally canonicalized paths.
- [`sessions`](../src/integrations/sessions.rs) gets remote entries from Navigator's manual configuration and opens them with `--remote TARGET --handoff`. Its local-session collector runs on the process host. These are attach actions, not selection of an existing machine in the invoking client's sidebar.

Verification used repository reads, installed Herdr 0.9.0 CLI help and bundled schema, and the official versioned documentation linked below. No live snapshot, private configuration, SSH connection, focus action, or build was run.

## Minimal upstream contract, proposed only

The names below are illustrative, not supported Herdr methods or CLI flags.

1. A client-owned `navigation.snapshot` operation returns the invoking client's Local entry and saved remote entries, selected machine/session, connection state, last successful refresh, and available workspace/agent records. Each record includes an opaque machine key, session identity, server incarnation, and exact object IDs. Return errors separately from empty results. Exclude terminal contents and credentials.
2. A client-owned `navigation.focus` operation accepts that qualified target and an invocation-scoped client route. It validates the target, selects the machine/session in that client, and focuses the exact workspace or agent pane. It acknowledges success or a structured failure. It must not silently attach another session, create a workspace, guess by cwd, or redirect to another machine.
3. Herdr supplies a short-lived, authorized route to those operations when it launches the plugin, including plugins on remote servers. Use the existing client connection for the relay. Do not require SSH back to the user's laptop or expose a general client control socket. Scope access to navigation and the invoking client.

If two clients share a server, one invocation must address exactly its originating client. Reject missing or expired routes rather than choosing the first client. Define the route lifetime for a reused picker pane. Rebind it only through a new authenticated invocation.

Capability negotiation must cover both discovery and focus. A display-only combined list is not complete support. Snapshot polling is enough initially; a subscription can follow. The client should perform discovery from its existing connections and cache, not let a remote plugin open connections to every saved profile.

## Navigator identity and cwd rules

- Separate machine location from `Source`. Local is a client-relative machine label, not a new source type. Preserve Workspace and Agent as source types.
- Use `(machine_key, session_key, server_incarnation, object_kind, object_id)` for live identity. Labels, SSH aliases, workspace numbers, and cwd are display/search data, not identity. Let Herdr define session and incarnation semantics so reused IDs after restart cannot select a different object.
- Carry qualified identity through entries, workspace references, actions, selection restoration, deduplication, pins, current-workspace checks, and jump-back. Keep jump-back scoped to the invoking client, and record only acknowledged transitions. Invalidate old unqualified records rather than guessing their owner.
- Keep cwd as an optional owner-qualified path. Match only within the same machine and session for workspace reuse. Never canonicalize a foreign path on the picker host. Use an owner-provided canonical path when available; otherwise exact path matching is safer than resolving symlinks or `..` locally.
- Do not substitute the picker's home directory for an unknown foreign cwd. [`display_path`](../src/tui.rs) currently strips the process home; use the owning machine's supplied home or show the full path. Never deduplicate two machines' workspaces because both use `/home/void/Projects/herdr-navigator`.
- Keep projects, zoxide, roots, and command integrations on their current process host in the first release, with an explicit host label. Do not imply that the client snapshot discovers these sources across machines. Disable unsupported cross-machine create, close, and command actions rather than sending bare IDs through the old path.

## UI and failure behavior

At an empty query, group by machine, then source. Show Local first and saved machines in client order. [`tui.rs`](../src/tui.rs) currently groups by source; retaining that layout with permanent machine labels is an acceptable smaller first change.

Search should be flat and ranked across machines. Keep the machine label visible on every result, including narrow layouts. Add machine names to search fields and offer a machine filter separate from existing source filters. Distinguish the selected client machine from the machine that owns the picker.

Show connecting, disconnected, disabled, and error states separately. Cached workspaces and agents must say stale and show their last refresh time; stale agent status is not live status. An uncached disconnected machine has no known workspace list, not zero workspaces.

Refresh each machine independently with bounded waits. Keep successful results if another machine fails. Preserve selection by qualified identity across refreshes. Revalidate on activation; a vanished target or changed server incarnation returns an error and keeps the picker open. Do not choose another same-named workspace. Leave automatic reconnection to Herdr. Navigator must not initiate SSH connections as a side effect of search or stale-row activation.

Without the upstream capability or client route, retain single-server operation and show its scope. Never label the process host Local unless the client establishes that relationship.

## Phases and acceptance tests

1. **Model and fallback.** Use fixtures only. Two machines with `w1`, matching agent IDs, identical labels, and identical cwd strings retain distinct rows, pins, and actions. Unknown cwd causes no home-directory match. Foreign paths cause no local filesystem lookup. Old jump-back state cannot route cross-machine. An older Herdr remains usable with an explicit single-server scope.
2. **Upstream contract.** Test Local and remote plugin invocations against the same combined snapshot. With two clients attached to one server, each route selects only its own client. Missing, expired, unauthorized, and unsupported routes fail without focus or attach side effects. A server restart rejects an old incarnation even if IDs repeat.
3. **Read-only combined UI.** Fixture the observed sidebar with Local `gemini-cyber`, `fedramp-rt`, `work`, and `herdr-navigator`; `void.dev` has `seo-saas` and `herdr-navigator`. Either picker host shows all six workspaces plus both machines' agents. Test machine/source filters, narrow labels, partial failures, cached disconnects, and uncached saved machines.
4. **Exact navigation.** In a separately authorized integration environment, select Local from a remote-owned picker and remote from a Local-owned picker. Assert exact machine/session/workspace/pane selection, success acknowledgement, and jump-back. Verify no new session or workspace, no handoff substitute, and no focus change in another client. Deletion or disconnection between snapshot and Enter must fail visibly without fallback.

[machines]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/connecting-machines.mdx
[socket]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/socket-api.mdx
[plugins]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/plugins.mdx
