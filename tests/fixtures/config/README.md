# N02 config fixtures

N02 added readable rollout requests and deterministic binding diagnostics. N10 now uses the shared registry for legacy input and fixture-backed merged input, but merged rendering, Refresh, and client capabilities remain unavailable. The production renderer still uses legacy behavior. `herdr-plugin.toml` remains at version 0.4.2 with minimum Herdr 0.7.4. No new operation or minimum-version claim justifies changing it.

## Current contract after N10

| Setting | Default and current behavior |
| --- | --- |
| `picker.presentation` | `legacy`. Accepts `legacy` or `merged`. A merged request stays in config, emits a stderr diagnostic at load, and leaves the legacy renderer active. |
| `sources.panes` | `false`. N05 enables every valid pane when true, without a warning or a new Source variant. Agent-only pane rows remain the default. |
| `picker.focus_cycle_key` | `f6`. Active only for an actual merged input context. A retained merged config request does not activate it while the legacy renderer runs. |
| `picker.collapse_key`, `picker.expand_key` | `left`, `right`. Active only with Browse focus in an actual merged input context. They are remappable with the same fail-closed collision rules. |
| `picker.technical_details_key` | `f7`. Opens the technical Details state only in an actual merged input context. N11 owns its renderer. |
| `picker.refresh_key` | `f8`. Reserved for N15 and inactive. It registers no command or help/footer hint. |

These settings cannot grant capability or authorization. Future capability negotiation is independent of presentation and source preferences. U01 through U04 must supply the public client contract. N19 and separately authorized N20 verification still block combined production navigation and the native machine New workspace prompt.

Unsupported rollout requests and binding diagnostics use stderr only, not Herdr notifications or transport. Invalid TOML, including an unknown presentation enum value, keeps the existing whole-config fallback and error-notification policy. The pure `parse_or_default` test checks that fallback without invoking its notification wrapper. Missing fields receive the same values through serde and Rust defaults.

## Binding resolution

Custom filter names sort by their original strings, before source alias or key normalization. The first valid sorted alias owns each source. The first sorted custom source claiming a key wins that key. A losing custom key suppresses its own source's default rather than silently moving the filter elsewhere. A custom key also wins against another source's default. Invalid source names and invalid keys do not claim a binding.

After filter resolution, existing registry order decides dispatch. Back and movement precede filters. Filters precede full reset, Close workspace, Mark, Details, and templates. Vim j/k precede plain source keys in Normal mode; Ctrl-J/K can still filter. Diagnostics name setting paths, normalized keys, precedence, and inactive bindings. They never silently repair an explicit custom binding. Dispatch, help, and collision checks use the same registry, with template/update availability supplied as values for pure checks.

Ctrl-P remains Projects, Ctrl-R remains Roots, Ctrl-S remains configured remotes, Tab cycles provenance filters, and Ctrl-O toggles Preview under defaults. Alt-Enter remains the conditional template action. F5 remains Update only when an update is available. Reserving any of those keys does not steal it. Merged input settings accept arrows, the existing Ctrl and Alt-Enter spellings, and F1 through F12. Existing commands keep precedence over new merged bindings. A focus/refresh collision disables focus cycling and leaves Refresh inactive. Filter and template grammar does not gain function keys.

The host-prefix fallback still tries Ctrl-G, Ctrl-E, Ctrl-D, Ctrl-F, then Ctrl-Y. It skips filter keys, a configured template key, and every configured focus, refresh, collapse, expand, and technical-details key. Ctrl-A therefore still moves the default Agent filter to Ctrl-G when nothing else claims it. An explicit custom conflict stays unchanged and emits an unresolved-host-interception warning. Exhausting fallback keys also leaves the binding unchanged and warns. Checks report reserved, template, and legacy-command host conflicts without remapping them.

Diagnostics have a fixed order. Rollout warnings precede sorted custom-entry warnings, default-filter collisions in existing Source order, registry collisions, then configured settings in focus, refresh, collapse, expand, and technical-details order. Config load appends host-prefix diagnostics. No check loads a template, constructs an App, scans files, or contacts a host. Only the existing load wrapper reads config and the Herdr prefix.

## Translation and remaining tasks

N05 covers collection and N08/N09 cover pure merged state/search. N10/N11 input and rendering remain pending.

| Existing setting or concept | Future translation and assigned work |
| --- | --- |
| `sources.open_workspaces` | Controls selectable workspace destinations. Disabling workspace rows must still retain required workspace/tab ancestry when panes are enabled. N05 implements this distinction; N08 owns merged presentation. |
| `sources.panes`, `sources.agents` | Panes enables all pane destinations. Agents supplies linked pane metadata and preserves agent-only intent when ordinary panes are off. Agent filtering selects that metadata, not a second Agent destination. N05 implements collection using the legacy Workspace and Agent buckets. N09's pure merged Agents filter requires pane metadata. |
| `sources.servers`, Server aliases and Ctrl-S | Keep configured remote provenance and explicit Attach behavior. They do not become discovered Machines or machine Focus/New workspace actions. N06/N09 preserve this distinction. |
| Destination Type versus Source | Type identifies the destination; Source records provenance. N02 adds no Source variant and changes no Source COUNT, index, or order. N03/N09 add the separate model and pure intersecting filters without changing Source indices. |
| `source_order`, `source_priority_boost`, pins | Keep readable. Merged direct-match classes must outrank provenance boosts and pins. N09 tests merged field relevance before pins and source preferences under all three matchers. Its i128 preference calculation handles extreme boosts separately from the unchanged legacy bonus. |
| `jump_back.pin_previous`, marked entries | Keep readable and preserve history intent. Pins cannot reorder browse machines or force non-current branches open. N07/N08 replace legacy pin-first browse assumptions. |
| `vim_mode`, `vim_filter_search` | Preserve commands and filter-search intent. Merged launch starts with Query focus, even with vim enabled. N10 separates focus from Normal/Search mode. Legacy vim launch behavior is unchanged now. |
| `detailed_rows`, `preview` | Keep readable. Compact merged rows must still identify owners; Details must preserve selection. N11 replaces legacy density/rendering assertions. N02 does not change row rendering. |
| Reuse/create, templates, sessions, root cache, notifications, update opt-in | Remain readable with current defaults and behavior. Later action/owner work must preserve explicit template and Attach intent rather than infer a foreign action from a path. |

## Fixture and test coverage

- `legacy-partial.toml` omits rollout settings and retains old aliases, source toggles, template, vim, ranking, cache, notifications, and a configured remote Attach target. `config::tests::legacy_partial_fixture_retains_settings_and_aliases` parses it without load or refresh.
- `merged-request.toml` asks for merged panes while disabling workspace rows, keeping pins and vim, using compact rows, and raising the source boost. `unsupported_rollout_requests_are_retained_and_diagnosed_purely` asserts retained requests and the remaining merged warning. App tests cover the pane/agent/workspace toggle matrix; this config test does not claim merged ranking.
- `binding-collisions.toml` covers duplicate aliases and keys, an unknown source, an invalid key, legacy command collisions, vim j, template collision, a focus/refresh conflict, and disclosure/technical bindings shadowed by existing commands. `collision_fixture_is_independent_of_hashmap_insertion_order` checks rotated and reversed insertion orders. `collision_fixture_dispatch_matches_effective_bindings` runs the fixture through production resolution in actual legacy and merged contexts. It checks Back on Ctrl-C and filter winners on Ctrl-P/U/X/B/O/J, with no competing effective command. It also checks plain j in Vim Normal and Search.
- `configured_key_grammar_preserves_aliases_and_setting_limits` covers Ctrl aliases, Alt-Enter aliases, padded F1 through F12, arrows, invalid merged spellings, and invalid template/filter fallback. The existing precedence, reservation, Vim-scope, and host-prefix tests cover Normal and Search, conditional Enter/F5, inactive legacy F6/F8, Ctrl-K, Alt-J/K, normalized F5 diagnostics, and the ordered joint Ctrl-G host case.
- Both checked-in example configs parse in `rollout_defaults_and_checked_in_examples_remain_legacy`. Other focused config tests cover the unknown enum fallback, first valid alias, custom-over-default precedence, and host fallback success/exhaustion.

The config/keymap tests use in-memory config and pure registry inputs. `alt_j_and_alt_k_move_selection_while_searching` is a separate production TUI event-handler regression. It enters Search from Vim Normal, types a query that keeps two matches, and checks that Alt-J/K move selection without changing query focus, cursor, or Vim state. Rendering regressions remain separate. The fixture README does not claim later UI or transport acceptance.

N09's `App::picker_projection` uses a caller-owned snapshot/state and the actual App query/config. It adds no active filter handler. Provider Attach entries are Sessions; server-category Run entries are Actions. Both retain Server provenance and its shortcut. The config, App and picker tests cover these distinctions without changing fixture JSON or loading user config.

N05 adds typed inventory and exact server-scoped pane focus. N10 keyboard input is complete, but N11 visual work remains pending and the production renderer still supplies a legacy input context. F6 therefore remains inactive in production, F8 remains reserved, and machine/client native flows remain unavailable. The opening N02 description records the earlier task, not the current pane behavior.
