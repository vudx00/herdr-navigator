# Navigator experience

## Decision

Build one search-first picker for destinations and explicit actions. Borrow the native `prefix+g` hierarchy, not its row density. Keep the native picker available until Navigator has exact pane navigation across the invoking client's machines.

The supplied native "search panes" screenshot shows nine panes across Local and `void.dev`. Machine, workspace, tab, and pane ancestry makes ownership clear. Repeated `1` and `pane 1` labels, separate ancestor rows, and full paths at the far right make comparison slow. Navigator's separate open, agent, and session groups have the opposite problem. They organize providers rather than the user's destination, and can repeat the same target.

Use machine, workspace, and pane ancestry for an empty query. Use flat fuzzy results while typing. Do not add nine source tabs or a permanent Browse toggle that competes with the query. A visible Browse focus control gives the tree keyboard focus; it does not change query semantics. Clearing the query restores the previous tree expansion and scroll position.

This replaces the machine-then-source empty layout proposed in [multi-machine navigation](multi-machine-navigation.md). The capability and routing limits in that note still apply. Agent is a pane attribute and filter, not a second destination tree.

## What exists and what is proposed

- [`src/sources.rs`](../src/sources.rs) already calls `pane list`, but retains only workspace cwd information. It builds workspace and agent entries separately. [`src/model.rs`](../src/model.rs) has no Pane source, exact FocusPane action, or machine-qualified references.
- [`src/app.rs`](../src/app.rs) and [`src/matcher.rs`](../src/matcher.rs) provide fuzzy matching, query operators, source filters, and action dispatch. Existing projects, folders, sessions, and integrations remain useful; this note changes their presentation, not their ownership.
- [`src/tui.rs`](../src/tui.rs) gives the list 48% and preview 52% of the horizontal split. It exposes IDs and search terms in preview and handles keys, not mouse events. Clickable controls, query cursor editing, and the layouts below are proposals.
- Herdr v0.9.0's internal [`navigator_rows`][aggregate] combines client endpoint caches into machine/workspace/tab/pane rows and joins agent metadata by `pane_id`. Its query uses substring matching and status filters. Machine matches include descendants; matching workspaces expand during search.
- [`open_navigator_overlay`][overlay] expands all workspaces and selects the current pane. Acceptance dispatches machine, workspace, tab, or pane targets. [`focus_or_activate`][endpoint] rejects offline endpoints, focuses an exact target on the active machine, or activates another endpoint with that target. Herdr already has cross-machine pane navigation.
- Those shell methods are `pub(super)`, not a public plugin contract. The [keyboard documentation][keyboard] calls `prefix+g` the Goto picker. [Agents run inside panes][agents]; they do not need parallel destination rows.

Ship exact single-server pane support independently. Existing `pane.list` and raw `pane.focus` with `PaneTarget { pane_id }` provide the local mechanism. The installed 0.9.0 `herdr pane focus` CLI is directional; `--pane` specifies an origin, not an exact destination. Never pass a terminal ID as a pane ID. "Single-server" means the picker's routed server, which can be remote, not necessarily client Local.

Combined Local and remote support needs a thin client-scoped export of the native snapshot and focus machinery, with an authorized route back to the invoking client even when the plugin runs remotely. Negotiate both discovery and exact focus. No inspected public plugin interface provides that complete pair. Plugins run on the selected server, pane CLI routing stays with its socket, and IDs are server-scoped ([machines][machines], [socket API][socket], [plugins][plugins]). Do not replace this dependency with SSH enumeration, screen scraping, or synthetic prefix keys.

## Default display and filters

Start with an empty search field, Machines = All, and Type = All. Put Local first, then machines in client sidebar order. Expand machine headings and the current workspace only. Select the current pane when its identity is known. Other workspaces show pane counts without expanding every tab. Keep current destination and row selection separate, with `* current` and `>` text markers.

Under each machine, show live workspaces first. Put projects and folders under a collapsed "Projects and folders" group, sessions under "Sessions", and commands under "Actions". These are task groups, not one group per provider. Show groups only when the corresponding inventory is supported. Configured remote session attach targets must not masquerade as discovered live workspaces.

Compress one unnamed tab into the pane breadcrumb. Show named tabs and multiple tabs as tree rows. Preserve their ancestry even when compressed. Do not guess whether a tab is unnamed from its label `1`; use metadata, or keep the row if uncertain.

```text
Search [                                  ]  [Browse]
Machine [All v]  Type [All v]          2/2 connected
v Local                              7 panes
  > gemini-cyber                      2 panes
  > fedramp-rt                        2 panes
  > work                             1 pane
  > herdr-navigator                  2 panes
  > Projects and folders
  > Sessions
  > Actions
v void.dev                           2 panes
  > seo-saas                         1 pane
  v herdr-navigator                  1 pane
>     pane 1                         * current
Enter Focus pane   F6 Focus controls   ? Help
```

Counts and current machine follow the supplied screenshot. The wireframe assumes single default tabs can be identified and compressed. Titles and states in the next wireframe are illustrative.

```text
Search [navigator                         ]  [Browse]
Machine [All v]  Type [All v]
> herdr-navigator           workspace | Focus
  Local > herdr-navigator
  herdr-navigator           workspace | Focus
  void.dev > herdr-navigator
  Review key handling       pane | agent | waiting
  Local > herdr-navigator > main > pane 1
  herdr-navigator template  project | Create
  Local > ~/Projects/herdr-navigator
Enter Focus workspace   F6 Focus controls   ? Help
```

Every flat result has a human title, short type/status text, and an owner breadcrumb immediately below it. Do not force the reader to scan a far-right cwd column. Prefer a manually supplied pane label, then agent name, then a useful reported title, then a stable pane ordinal. Keep agent name as secondary text when a manual label wins. Status never becomes identity. Do not invent process or task metadata.

Machine and Type filters preserve the query and each other. Type offers All, Machines, Workspaces, Panes, Agents, Projects, Folders, Sessions, and Actions. Agents selects panes with agent metadata. Existing source-specific shortcuts remain provenance filters, including zoxide and roots, without adding permanent source tabs. Non-live type filters show a flat list even with no query.

Rank direct pane title and agent-name matches above inherited workspace matches. An exact workspace match leads a repository-name search unless a stronger explicit target match exists. Show descendant-only matches after direct matches; do not assign every pane the parent's top score. Panes and Agents filters retain matching descendants. Search includes owner names, ancestry, paths, and aliases. Existing `@`, `!`, `/`, and `#` query syntax keeps its meaning; machine selection gets no conflicting sigil.

### Machine search and new workspaces

Machine names are first-class fuzzy results. With Type = All or Machines, typing `void` can select `void.dev` itself, not only its descendants. A direct machine-name match ranks above descendants that match only through that machine's name. Explicit pane or workspace matches still follow normal match quality.

Show the machine result as `void.dev | Machine | New workspace`. Enter on that row opens Herdr's native new-workspace flow on `void.dev`, equivalent to selecting that machine and pressing `prefix+shift+n`. Apply the same behavior to Local and to selectable machine rows in browse view. Expansion remains a separate interaction.

Use the native naming prompt and target-machine defaults. Do not immediately create an unnamed workspace, infer a directory from another search result, or use the picker host's cwd. Cancelling the native prompt creates nothing. The footer and Details must say `Enter New workspace on void.dev`, not Focus or Switch.

This requires an explicit client-scoped new-workspace action in addition to exact focus. Herdr must activate the selected machine before opening its native flow in the invoking client. Disable the action when the machine is unavailable or the capability is missing; do not fall back to creating on the current server or sending synthetic shortcut keys.

## Identity and Enter

Use `(machine key, session key, server incarnation, object kind, object ID)` for live identity. Carry workspace, tab, and exact pane IDs for pane targets. Herdr must define these keys in the client contract. Use them for selection, pins, current markers, refresh, and jump-back. Labels, cwd, ordinals, and terminal IDs cannot substitute for object identity.

- Merge an agent and its pane only through the same qualified `pane_id`. Keep one pane row, with agent aliases and status available to search. Missing linkage is not proof that two rows are the same.
- Keep workspace and pane rows distinct. Workspace focus intentionally restores workspace focus; pane focus requests one exact pane. Keep distinct tabs, sessions, and machines even when labels and paths match.
- Merge project, zoxide, and root provenance only when the owner, target, template, and resolved action are identical. A same-path project template and plain-folder creation are different actions. Never resolve foreign paths on the picker host or use its home as an unknown cwd fallback.
- For non-live entries, retain an owner-qualified provider/action key. Session identity includes its attach target. Commands include their configured action identity, not just their display title.

Enter always executes the displayed verb, never expands a group as a surprise:

| Row | Enter |
| --- | --- |
| Machine | Open the native new-workspace flow on that machine, equivalent to `prefix+shift+n` there |
| Workspace or named tab | Focus that exact workspace or tab |
| Pane, including agent pane | Focus that exact pane; switch machine first if needed |
| Existing reusable project or folder | Focus the resolved workspace shown in preview |
| Unopened project | Create its specified workspace and template |
| Unopened folder | Open the folder by creating a directory workspace |
| Session | Attach the named session on the stated target |
| Command or plugin action | Run the stated action on its stated host |
| Organizational group | No action; show the expand control instead |

Revalidate the displayed action before dispatch. If a target vanished, or Focus would become Create after refresh, keep the picker open and require a fresh selection or acknowledgement. Never guess another same-named target. Close only after acknowledged navigation success or acknowledged handoff to the native new-workspace flow. Opening that flow is not proof that a workspace was created. A persistent picker restores its state after success.

Do not overload pane rows with close, kill, or restart. `Ctrl-X` remains workspace close, with its existing confirmation policy, and is unavailable on pane rows. Template opening remains an explicit alternate action. Session Attach and command Run must never look like Focus. Keep projects, folders, and commands scoped to their known provider host until remote execution is explicitly supported.

## Input, preview, and terminal size

Keep query focus on launch. Up/Down select results without moving query focus. Left/Right edit the query cursor; Space inserts a space. F6 cycles Query, Browse/list, filter controls, and visible Details, with a text focus marker. In Browse focus, Left collapses or moves to the parent and Right expands; typing returns to Query. Space stays text input except an explicitly documented vim browse binding. Enter on a focused filter opens its menu, not the selected destination.

Preserve existing bindings from [`src/keymap.rs`](../src/keymap.rs), including Tab filter cycling, `Ctrl-P` Projects, vim mode, preview, and template actions. New focus and expansion bindings must be remappable and checked for conflicts. Esc closes a filter or Details first, then follows existing mode/back behavior. Clearing by editing restores browse without resetting filters; keep the existing full-reset command explicit in Help. Show only navigation, the selected action, focus, and Help in the footer, not eight source shortcuts.

Mouse support requires capture, resize-aware hit testing, wheel scrolling, and cleanup on exit. Single-click selects a row; double-click or the labeled action executes it. A separate disclosure hit area expands groups. Filter clicks open real menus with keyboard equivalents. Until implemented, render plain labels rather than fake buttons. Status and focus need text, not color alone. Help lists every action and active binding.

At 110 columns or wider, give the list about 65% and Details 35%, with at least 36 columns for Details. Preview shows title, full owner breadcrumb, owner-qualified cwd, agent status and freshness, then a sentence such as "Enter switches to Local and focuses pane 1 in herdr-navigator." Projects show the template and reuse/create result; sessions show attach target; commands show command and execution host. Keep IDs, scores, and search terms behind an explicit technical-details view.

Below 110 columns, hide side-by-side Details. The existing preview toggle opens a full-width Details view and returns to the same selection. Keep machine and workspace visible on every search result, even at the cost of a second line and fewer rows. Shorten paths in the middle, not owner labels into indistinguishable prefixes. On short terminals, remove secondary hints before result rows. Never automatically stream pane output into preview; inactive-machine caches do not imply screen access, and terminal output can contain secrets.

## Refresh and failure states

The following timings are initial design targets to validate, not measured requirements. Use client cache events when available. Otherwise poll the combined cache at most every two seconds while visible. Allow one request in flight per provider, a two-second response deadline, and capped retry backoff to 30 seconds. Do not start a subprocess or network request on each keystroke. Slow optional providers cannot block cached pane results. Pause background refresh while hidden.

Update status in place, but freeze result order during selection movement and activation. Insert new matches and rerank only after a query/filter change or explicit Refresh; show "New results available" meanwhile. Restore by qualified identity, not index. If the selected row disappears, retain a disabled "Target closed" row until the user moves or refreshes, so Enter cannot hit its neighbor. A server incarnation change invalidates old targets and jump-back records.

Show connection counts, individual connecting/offline/error states, and last successful refresh time. Cached rows say "stale" and disable Focus while offline. An uncached offline machine says "Workspace list unavailable", not "0 workspaces". Stale activation must not trigger SSH or silently attach. Offer a separate supported Connect action or direct the user to Herdr's machine controls.

Distinguish "No matches" with visible active filters, "No panes on this server", "Provider unavailable", and "Client-wide navigation unsupported". Clearing filters must be explicit and must not erase the query. Keep successful machines visible if another fails. Without a client route, label the list "Server scope: <known owner>"; do not call it Local unless the client establishes that relationship.

## Migration and acceptance

1. Add qualified identities and local pane rows first. Preserve existing launch keys and source shortcuts. Retain native `prefix+g` as the fallback; do not rebind it automatically. Keep the current layout available during the first rollout.
2. Add the merged presentation with fixture tests. Migrate a pin only if its old identity maps unambiguously; otherwise mark it unresolved. Never guess the owner of an old jump-back record.
3. Enable combined navigation only after both client snapshot and exact focus capabilities pass. A remote-owned picker must address its originating client, not another client on the same server.
4. With the screenshot's six workspaces and nine panes, browse exposes all panes without separate agent duplicates. A repository query produces distinct Local and `void.dev` workspaces with visible owners. Panes filtering exposes their matching children.
5. Give two machines identical IDs, names, and cwd strings. Enter must focus the selected qualified pane. A terminal ID different from its pane ID must not change the target. No session or workspace is created.
6. Clear a query after browsing a named tab. Expansion, scroll, and filters return intact. `Ctrl-P` still filters Projects; spaces and arrow editing work in Query; keyboard and mouse filters produce the same results.
7. Disconnect `void.dev`, close the selected pane, and restart a server in separate fixtures. Keep other results, show stale/closed state, and reject old targets without moving focus to a neighbor or another client.
8. At 80 columns, every result still identifies its owner and action. Details returns to the same row. Same-path folder and project-template actions stay distinct unless their resolved action is identical.
9. Search `void` and select the `void.dev` machine row. Enter opens the same new-workspace flow as `prefix+shift+n` on that machine, with its native defaults. Cancelling creates nothing; submitting creates on `void.dev`, not Local. Selecting a workspace or pane still focuses it. Offline or unsupported machine actions stay disabled.

[aggregate]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/src/client/shell/aggregate_navigation.rs
[endpoint]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/src/client/shell/endpoint_navigation.rs
[overlay]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/src/client/shell/overlay_input.rs
[keyboard]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/keyboard.mdx
[agents]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/agents.mdx
[machines]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/connecting-machines.mdx
[socket]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/socket-api.mdx
[plugins]: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.0/docs/next/website/src/content/docs/plugins.mdx
