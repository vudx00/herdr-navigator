# N01 navigation fixtures

These files freeze offline examples and intended behavior for later tasks. N01 does not implement navigation, capability negotiation, rendering, routing, or prompt handoff. The integrity tests check fixture consistency, not those runtime contracts.

## Provenance

`manifest.json` lists every data sample and separates three evidence classes.

| Directory | Evidence | Limits |
| --- | --- | --- |
| `ordinary-server/0.7.3` | The two JSON values inherited from `sources::tests::parses_herdr_workspace_and_agent_list_json` at baseline `6b5ea9e6ce81a1771db7e535fb98b1b469118c81`. Its comment attributes them to Herdr 0.7.3 workspace/agent lists. | Values and `id`/`result` envelopes are unchanged. Only whitespace changed. No fresh 0.7.3 capture or claim of cross-version compatibility. |
| `ordinary-server/0.9.0` | Installed `herdr 0.9.0` offline version, bundled schema, and CLI help. | Schema declarations and help, not observed live requests, acknowledgements, or session inventory. |
| `hypothetical-client` | Synthetic data based on the inventory counts and design decisions in `docs/navigator-experience.md`. | `navigation-n01-v1` is a fixture format, not a Herdr protocol. No fabricated `navigation.snapshot` response. All owner/session/incarnation keys, paths, labels beyond the workspace/machine names, agent metadata, revisions, and outcomes are hypothetical. |

The inherited parser test supplies `/tmp` cwd mappings for both workspaces, so it does not fall back to the user's HOME. It expects two workspace entries, `w41` and `w43`, and one agent linked to `w43:p1`. That sample has no machine/session/incarnation evidence. Its owner remains an unknown routed server, not client Local. Workspace actions retain their workspace IDs. N05 replaced the terminal-target assertion with exact qualified pane focus. The parser regression adds synthetic tab/pane inventory to the inherited samples and uses the production join. The App socket test records `pane.focus` with `w43:p1`, never `term_1`.

### Installed offline metadata capture

Each command below ran with a 20-second timeout, an empty temporary HOME/config directory, and no inherited `HERDR_*` variables. Each exited 0 with empty stderr. Only these metadata commands ran. No live snapshot/list, focus, create, attach, private config read, connection, SSH, or network call was used.

| Command | Stored stdout | Bytes before extraction |
| --- | --- | --- |
| `herdr --version` | `version.txt` | 12 |
| `herdr api schema --json` | `schema-excerpts.json` | 275129 |
| `herdr pane list --help` | `pane-list-help.txt` | 89 |
| `herdr pane focus --help` | `pane-focus-help.txt` | 223 |
| `herdr workspace focus --help` | `workspace-focus-help.txt` | 93 |
| `herdr tab focus --help` | `tab-focus-help.txt` | 69 |

Help/version files preserve stdout bytes. The schema sample stores selected JSON Pointer values from the full bundled schema, reserialized as JSON. Its `source_sha256` and `source_bytes` identify the original stdout. Each key in `excerpts` is the extraction pointer. This is not a complete schema. References may point to omitted definitions, and omissions do not prove that an operation is absent from the full schema.

The excerpt includes protocol 22/schema version 1, request/success/error envelope declarations, ordinary inventory and exact-focus variants, directional focus parameters, workspace/tab/pane/agent metadata, session snapshot, server capabilities, and invocation context. It contains schema field declarations, not terminal contents or credential values. No synthetic request/response framing was added.

The installed evidence agrees with the design's raw `pane.focus` target of `PaneTarget.pane_id`. The CLI `pane focus` instead requires `--direction`; `--pane` cannot be treated as an exact-destination command. Workspace/tab CLI help names exact IDs separately. The schema's `TabInfo` has a label but no explicit naming-state field. The synthetic named/unnamed/unknown field is a future design input, not an available 0.9.0 field. `SessionSnapshot` has no combined machine list or server-incarnation field. `endpoint_protocol_generation` describes protocol support, not proof of a particular server restart identity. Transport framing, acknowledgement semantics, durable identity, and client/native-flow exports still need the later adapter and upstream tasks.

## Synthetic format and cases

An owner is `[machine_key, session_key, server_incarnation]`. An expected identity is `[machine_key, session_key, server_incarnation, object_kind, object_id]`. Ancestor IDs are ordered workspace then tab and inherit that identity's owner. Machine identities in this test format include the session against which the native action is resolved. The eventual domain model must distinguish stable machine identity from that action context.

Input inventories nest workspaces, tabs, and panes under owners. Expected destinations explicitly list identities, ancestry, and intended actions. Foreign cwd/home values are strings only. The integrity helpers never inspect the filesystem or expand them.

| Case | Input and expected contract |
| --- | --- |
| `six-workspaces-nine-panes` | Local has gemini-cyber 2 panes/2 tabs, fedramp-rt 2/1, work 1/1, herdr-navigator 2/1. void.dev has seo-saas 1/1 and herdr-navigator 1/1. Total 6 workspaces, 7 tabs, 9 panes. Current is void.dev's navigator pane. Two machine destinations open the native naming prompt with target defaults, never switch-only or direct create. |
| Tab/title metadata | Named tabs include one explicitly named `1`; unnamed and unknown tabs also use `1`. Unknown cwd/home stays null. A manual pane label retains secondary agent metadata. Every terminal ID differs from its pane ID. |
| Agent links | Five updates describe two valid pane links, including a newer duplicate update, plus missing and nonexistent links. Expect two agent-bearing panes and zero extra Agent destinations. Invalid links cannot become focus targets. |
| `identity-collisions` | Four owners reuse `w1`, `w1:t1`, `w1:p1`, the same label, and `/fixture/shared/herdr-navigator`. Differences in machine, session, or incarnation preserve 12 distinct live destinations. |
| `capability-failure-cases` | 42 individually named cases cover server-only remote scope, snapshot-only, focus without native flow, full support, independently unsupported focus kinds, route failures, unsupported protocol/operation, cached/uncached connection states, empty/unknown inventory, independent agent failure, stale/wrong targets, transport failures, and native flow outcomes. |

Capability cases in `input.cases` inherit `input.defaults` and override only the fields shown in each case's `input`. Each has an explicit expected availability, scope, action, and reason. The default target is a pane. Workspace, tab, and native-flow cases explicitly supply the matching target kind; native flow targets the machine/session, not a pane. Route failures happen after combined collection and must not silently retry through the process server. By contrast, missing client support at initial collection permits explicitly labeled server scope. A cached count is a last-known count, not a live count. Connection failures on void.dev leave Local's pane action available. Prompt opened is a handoff, not creation; cancelled is zero creations; lost completion is unknown and must not be replayed.

## Regression matrix

Keep all existing tests in N01. The following names refer to the current inline tests. Later tasks must adapt or replace only the stated contract, not delete coverage wholesale.

| Tests | Decision and later task |
| --- | --- |
| `app::tests::search_prefers_exact_title_then_shorter_matching_paths`, `search_does_not_fuzzy_match_across_unrelated_fields` | Retain field isolation and match quality under nucleo, skim, simple. N09 adds owner/inherited-field ranking. |
| `app::tests::agent_token_filters_match_identity_parts`, `agent_shortcut_shows_all_agents_and_priority_is_configurable`, `agent_aliases_are_searchable_plain_text` | Retain query operators and aliases. N05/N09 adapt Agent filtering to pane metadata; ordering cannot defeat direct-match quality. |
| `app::tests::source_specific_reuse_distinguishes_same_path_workspaces`, `offers_directory_template_for_new_and_existing_directory_workspaces`, `directory_workspaces_reuse_unprefixed_labels`, `project_workspaces_are_not_reused_as_directory_workspaces`, `new_directory_workspaces_are_labelled_with_the_plain_directory_name` | Retain reuse/template distinctions. N06 adds known-owner constraints and resolved actions. |
| `app::tests::refuses_to_close_picker_owning_workspace`, `close_target_matches_entry_kind` | Retain qualified picker-workspace protection and project/directory close resolution in N06. N05 adds pane-close tests in both the Workspace and Agent buckets. N06 still owns full action resolution. |
| `app::tests::jump_back_records_only_real_workspace_transitions` | Retain transition semantics. N07/N13 require acknowledged qualified targets; prompt handoff is not a transition. |
| `app::tests::workspace_rows_are_not_deduped_by_path`, `refresh_selection_is_restored_by_stable_entry_key`, `jump_back_resolves_focused_and_previous_workspaces` | Retain distinct workspace rows and stable selection intent. Replace bare-ID/path history and selection assumptions with owner/session/incarnation identity in N03/N07/N15. |
| `app::tests::default_empty_picker_prioritizes_agent_status`, `previous_workspace_is_pinned_only_on_initial_unfiltered_view`, `pinned_entries_sort_first_and_persist`, `marking_keeps_the_cursor_on_the_marked_row`, `previous_workspace_sorts_before_marked_entries` | N07 through N09 replace status/pin-first merged ordering with stable machine browse order and direct-match search ranking. Preserve pin persistence and same-target cursor behavior. |
| `sources::tests::parses_herdr_workspace_and_agent_list_json` | Retain inherited envelopes/status/session metadata. N05 replaced the terminal target with exact qualified `pane_id` and one destination per pane. |
| `sources::tests::root_scan_excludes_generated_trees`, `a_shallow_root_does_not_truncate_a_deeper_overlapping_root`; `config::tests::root_scanning_is_cached_and_excludes_build_trees_by_default` | Retain process-owned root scanning and cache/exclusion contracts. |
| `integrations::command::tests::renders_shell_safe_open_command`, `collect_timeout_and_output_limit_are_enforced`, `failed_collect_is_optional`, `builds_entry_with_run_command_action` | Retain quoting, limits, optional failure, and Run semantics. N06 adds explicit execution owner. |
| `integrations::sessions::tests::local_session_entry_attaches_by_name`, `manual_remote_entry_opens_remote_target` | Retain explicit Attach semantics. A configured remote target is not client-discovered navigation. |
| `integrations::herdr_plus::tests::loads_reusable_template_by_safe_filename`, `parses_split_pane_project_tabs`, `bootstraps_split_panes_before_running_commands` | Retain safe filenames and tabs/panes-before-startup ordering. |
| `tui::tests::alt_j_and_alt_k_move_selection_while_searching`, `alt_enter_opens_selected_directory_with_configured_template`, `update_badge_renders_action_and_f5_triggers_it` | Retain Alt-J/K, Alt-Enter, and F5 Update. Refresh must not steal them. |
| `tests::main_picker_uses_full_size_modal_popup`, `side_pane_toggles_open_focus_close` in `src/main.rs` | Retain popup manifest and server-scoped side toggle behavior. N16 adds owner/authenticated invocation scope, not live host-delivery claims from a unit test. |
| `tui::tests::list_renders_source_groups_as_a_tree`, `detailed_rows_expand_directories_and_pane_breadcrumbs`, `open_and_pinned_workspaces_replace_tree_branches_with_diamonds`, `current_workspace_marker_wins_over_stale_pin`, `marked_entry_uses_a_yellow_diamond`, `mark_marker_wins_over_previous_workspace`, `mark_marker_wins_over_current_workspace` | N11 replaces source trees/density and marker precedence with machine ancestry and separate text markers for selection/current/pin/stale/unavailable. |
| `tui::tests::vim_mode_uses_normal_keys_then_searches_with_slash`, `vim_filter_search_starts_search_after_source_key`, `input_modes_transition_exclusively` | N10 adapts launch/focus assumptions while retaining vim Normal/Search commands and configurable filter-search behavior. |
| `tui::tests::question_mark_toggles_registry_help_overlay`, `registry_reports_active_toggle_state`, `registry_maps_ctrl_b_to_mark_without_stealing_enter`, `compact_footer_groups_movement_and_lists_filters`; config alias, update opt-in, notification, and key-conflict tests | Retain settings and key intent. N02/N10/N11 adapt registry/layout expectations only when new controls exist. |

Test names without a repeated module prefix belong to the first module in that row. Color/status tests may remain palette checks, but they cannot substitute for N11's plain-text marker tests.

## Offline verification

`src/main.rs` registers `src/navigation/test_support.rs` only under `cfg(test)`. Samples load with `include_str!` and `serde_json::Value`, using existing dependencies. N01 added no production module; N03 through N05 now have production navigation modules.

Run focused fixture and parser tests, then formatting, strict Clippy, serialized tests, and release build with `--offline` and bounded timeouts. Use temporary HOME/config/cache directories, preserve `CARGO_HOME` and `RUSTUP_HOME` for installed toolchains, clear inherited Herdr routing variables, and set `HERDR_BIN_PATH` to a fail-closed stub. Existing tests mutate process environment, so use `--test-threads=1` for the full suite. Do not fetch missing dependencies. See `tasks/todo.md` for the actual N01 check results.

## N04 transport tests

[Transport provenance and limits](ordinary-server/0.9.0/transport-contract.md) records the versioned public NDJSON and managed-plugin runtime evidence used by the adapter. [Synthetic transport cases](synthetic-transport/README.md) describes the inline Unix socket and command-runner tests. These Markdown supplements are not new captured responses or additions to the N01 enumerated fixture manifest.

## N05 production conversion tests

`join_inventory` consumes typed workspace/tab/pane records and independent agent reads. The six-workspace fixture test supplies each hypothetical owner to the production scope model, converts its nested input into ordinary record types, and calls that join. It asserts six workspaces, seven tabs, nine distinct qualified pane destinations, two enriched panes, and two invalid-link diagnostics. It emits no machine rows and does not test combined transport or client authority.

The fixture's explicit naming states, owner keys, agent aliases, and revisions remain hypothetical inputs. Protocol 22 provides no tab naming state, so the conversion retains `Unknown` and displays `1`. The synthetic `waiting` status is not a protocol-22 enum value and maps to unknown in this test adapter. No synthetic data was added to the raw 0.7.3 samples or offline 0.9.0 schema evidence.

Recording-runner tests exercise all four production list parsers, consumed optional field types, agent-session types, rejection, and timeout. App refresh tests exercise the production join and all source-toggle combinations, including hidden workspaces, metadata mutation, successful-empty agent reads, optional structured rejection, incomplete ancestry, and scope invalidation. Unix socket tests activate collected pane rows through App and record only exact `pane.focus`, with terminal IDs deliberately different from pane IDs. Renderer tests cover manual titles, secondary agents/status, numeric tab breadcrumbs, and errors without a selected row or preview.

## N08 data-only picker tests

`FixtureClient` deserializes only the hypothetical fixture's `input`. It builds production inventory records, calls `join_inventory`, retains exact tab destinations including empty tabs, and resolves actions without file preparation. Expected destinations are assertion data only. The adapter has no transport, focus, native-flow or authorization methods. Machine rows and explicit Named/Unnamed/Unknown metadata come only from synthetic input; they are not protocol-22 discoveries.

Eight `tui::tests::n08_*` tests exercise the pure projection and App's read-only input seam. Three `picker::tests` cases extend the compression matrix and check vanished selection and all four collision-fixture scopes. Two App tests check process/server ownership, command `kind=server` grouping as Actions, and explicit supported-empty groups. These tests do not implement merged rendering or keyboard handlers.

The fixture still has two machines, six workspaces, seven tabs, nine panes and two agent-bearing panes. Fully expanded browse compresses only its three proven single unnamed tabs; flat results retain all 24 destinations in caller order. Browse and flat selection/viewport anchors are separate. Hidden anchors retain identity and intra-row offset in saved state; projection returns no visible replacement. Current, pin and previous marks do not confer focus authority or reorder machine headings.

Recording-runner tests call actual App refresh before projection, then assert unchanged call count, history bytes and legacy view state. Ordinary ownership remains `Server scope: owner unknown`, and ordinary naming remains Unknown. Legacy defaults and the merged-unavailable diagnostic remain active. See [N08 verification](../../../tasks/todo.md#n08-verification) for bounded offline results.

## N09 search and filter tests

The same input-only `FixtureClient` adapter now feeds `picker::search_project` under nucleo, skim and simple. Tests assert qualified result identities after production joins, not a projection of expected fixture arrays. No new JSON sample or production client capability was added.

Nine added picker tests cover direct machine/workspace/pane relevance, each alias as one field, equal-direct workspace ties, delayed pins/source preferences, deterministic results across hash and alias order, named tabs under All, hidden qualified ancestry, absent cwd, explicit machine ownership, filter/operator intersections, and browse restoration. Structural ancestors retained by filtering have neither actions nor selection. Machine headings keep N08 browse order regardless of pins.

Direct fields outrank inherited owner/ancestry/path fields, which outrank supporting agent status/session and provider terms. Each candidate uses its best field by match quality and engine score. A pane does not lead an equally relevant workspace merely because it is a pane. Legacy search fields and status/pin ordering remain unchanged.

The added matcher, model and config tests cover operator field restrictions, merged provenance and overflow-safe source preference. App tests separate configured Attach from server-category Run using resolved actions. A TUI-module test calls actual App refresh through RecordingRunner, then uses `App::picker_projection` for repeated query/filter/selection changes. It asserts unchanged calls, history and legacy selection, and no displayed-action token commit. This is a data/state test, not new event handling or rendering.

All 241 serial offline tests pass, including the preceding 227. See [N09 verification](../../../tasks/todo.md#n09-verification). Legacy rendering and the merged-unavailable warning remain active; N07, S01, release and independent-review gates remain open.
