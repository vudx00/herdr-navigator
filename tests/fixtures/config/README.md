# N02 config fixtures

N02 adds readable rollout requests and deterministic binding diagnostics. It does not enable merged rendering, pane collection, Refresh, focus cycling, or client capabilities. The renderer and collectors still use the legacy behavior. `herdr-plugin.toml` remains at version 0.4.2 with minimum Herdr 0.7.4. No new operation or minimum-version claim justifies changing it.

## Current contract after N05

| Setting | Default and current behavior |
| --- | --- |
| `picker.presentation` | `legacy`. Accepts `legacy` or `merged`. A merged request stays in config, emits a stderr diagnostic at load, and leaves the legacy renderer active. |
| `sources.panes` | `false`. N05 enables every valid pane when true, without a warning or a new Source variant. Agent-only pane rows remain the default. |
| `picker.focus_cycle_key` | `f6`. Reserved and inactive. It registers no command or help/footer hint. |
| `picker.refresh_key` | `f8`. Reserved and inactive. It registers no command or help/footer hint. |

These settings cannot grant capability or authorization. Future capability negotiation is independent of presentation and source preferences. U01 through U04 must supply the public client contract. N19 and separately authorized N20 verification still block combined production navigation and the native machine New workspace prompt.

Unsupported rollout requests and binding diagnostics use stderr only, not Herdr notifications or transport. Invalid TOML, including an unknown presentation enum value, keeps the existing whole-config fallback and error-notification policy. The pure `parse_or_default` test checks that fallback without invoking its notification wrapper. Missing fields receive the same values through serde and Rust defaults.

## Binding resolution

Custom filter names sort by their original strings, before source alias or key normalization. The first valid sorted alias owns each source. The first sorted custom source claiming a key wins that key. A losing custom key suppresses its own source's default rather than silently moving the filter elsewhere. A custom key also wins against another source's default. Invalid source names and invalid keys do not claim a binding.

After filter resolution, existing registry order decides dispatch. Back and movement precede filters. Filters precede Clear, Close workspace, Mark, Preview, and templates. Vim j/k precede plain source keys in Normal mode; Ctrl-J/K can still filter. Diagnostics name setting paths, normalized keys, precedence, and inactive bindings. They never silently repair an explicit custom binding. Dispatch, help, and collision checks use the same registry, with template/update availability supplied as values for pure checks.

Ctrl-P remains Projects, Ctrl-R remains Roots, Ctrl-S remains configured remotes, Tab cycles provenance filters, and Ctrl-O toggles Preview under defaults. Alt-Enter remains the conditional template action. F5 remains Update only when an update is available. Reserving any of those keys does not steal it. Reserved settings accept the existing Ctrl parser and Alt-Enter spellings plus F1 through F12. Filter and template grammar does not gain function keys.

The host-prefix fallback still tries Ctrl-G, Ctrl-E, Ctrl-D, Ctrl-F, then Ctrl-Y. It skips filter keys, a configured template key, and reserved keys. Ctrl-A therefore still moves the default Agent filter to Ctrl-G when nothing else claims it. An explicit custom conflict stays unchanged and emits an unresolved-host-interception warning. Exhausting fallback keys also leaves the binding unchanged and warns. Checks report reserved/template/legacy-command host conflicts without remapping them.

Diagnostics have a fixed order. Rollout warnings precede sorted custom-entry warnings, default-filter collisions in existing Source order, registry collisions, and reserved settings in focus/refresh order. Config load appends host-prefix diagnostics. No check loads a template, constructs an App, scans files, or contacts a host. Only the existing load wrapper reads config and the Herdr prefix.

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
- `binding-collisions.toml` covers duplicate aliases and keys, an unknown source, an invalid key, legacy command collisions, vim j, template collision, and mutual reserved conflicts. `collision_fixture_is_independent_of_hashmap_insertion_order` checks rotated and reversed insertion orders. Keymap tests verify winning commands against the shared registry and cover vim k, F5, Alt-Enter, reserved parser errors, and host conflicts.
- Both checked-in example configs parse in `rollout_defaults_and_checked_in_examples_remain_legacy`. Other focused config tests cover the unknown enum fallback, first valid alias, custom-over-default precedence, and host fallback success/exhaustion.

All new tests use in-memory config and registry inputs. Existing TUI tests remain the event-handler and rendering regressions. The fixture README does not claim later UI or transport acceptance.

N09's `App::picker_projection` uses a caller-owned snapshot/state and the actual App query/config. It adds no active filter handler. Provider Attach entries are Sessions; server-category Run entries are Actions. Both retain Server provenance and its shortcut. The config, App and picker tests cover these distinctions without changing fixture JSON or loading user config.

N05 adds typed inventory and exact server-scoped pane focus. Merged rendering, F6/F8, and machine/client native flows remain unavailable. The opening N02 description records the earlier task, not the current pane behavior.
