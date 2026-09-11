# Herdr Navigator

<p align="center">
  <img src="docs/assets/herdr-navigator.svg" alt="Herdr Navigator — jump to anything in Herdr" width="100%" />
</p>

<p align="center">
  <strong>One fuzzy navigator for every workspace, agent, project, session, remote, directory, and action in Herdr.</strong>
</p>

<p align="center">
  <a href="https://github.com/vudx00/herdr-navigator/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/vudx00/herdr-navigator/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-2ea44f" /></a>
  <img alt="Herdr 0.7.4+" src="https://img.shields.io/badge/Herdr-0.7.4%2B-66b3ff" />
  <img alt="Linux and macOS" src="https://img.shields.io/badge/platform-Linux%20%7C%20macOS-c084fc" />
</p>

Type what you remember. Navigator decides whether to **focus, create, attach, hand off, invoke, or run**—without making you remember which Herdr surface owns the destination.

```text
prefix+t  →  type  →  Enter
```

> [!IMPORTANT]
> **Upgrading from v0.3.1 or earlier?** Sorry for the one-time breaking rename. Starting with v0.3.2, the plugin ID, binary, config directory, and action prefix are all `herdr-navigator`. See the migration steps below.

## Install

```bash
herdr plugin install vudx00/herdr-navigator --ref v0.4.2
herdr plugin action invoke herdr-navigator.open
```

If the overlay opens, add a shortcut to `~/.config/herdr/config.toml`. Invoking `open` again focuses the existing Navigator in the current workspace instead of opening a duplicate:

```toml
[[keys.command]]
key = "prefix+t"
type = "plugin_action"
command = "herdr-navigator.open"
description = "jump to anything"
```

### Upgrade from v0.3.1 or earlier

Sorry for the migration. This completes the rename while the project is still young:

```bash
herdr plugin uninstall herdr-picker-plus
herdr plugin install vudx00/herdr-navigator --ref v0.4.2
herdr server reload-config
```

Replace `herdr-picker-plus` with `herdr-navigator` in your Herdr keybindings. Navigator copies your old plugin config and Jump Back state into the new config directory on first run; the old files are left untouched. Local development links should use `herdr plugin unlink herdr-picker-plus` instead of `uninstall`.

Reload Herdr, then press `prefix+t`:

```bash
herdr server reload-config
```

## See it in action

<p align="center">
  <img src="docs/assets/herdr-navigator-demo.gif" alt="Herdr Navigator switching, filtering, and opening workspaces, agents, sessions, and directories" width="100%" />
</p>

*Source-aware rows keep live status, tabs, and panes aligned at a glance. [Watch the full 17-second demo.](https://github.com/thanhdat77/herdr-navigator/releases/download/v0.3.2/herdr-navigator-v0.3.2-demo.mp4)*

A single result list can move between live Herdr state and things that are not open yet:

- Type a repo name → focus its open workspace, or create one from a project, zoxide, or configured root.
- Type `@idle` or an agent alias → focus that agent pane.
- Filter remotes → hand off with Herdr's own `--remote TARGET --handoff` flow.
- Select an external integration → run its configured action.

## Why Navigator

| Capability | Why it matters |
| --- | --- |
| **One index across Herdr** | Search workspaces, agents, projects, sessions, remotes, directories, Quick Actions, and integrations together. |
| **Action-aware Enter** | Results do not just return paths; they focus, create, attach, hand off, invoke, or run. |
| **Reuse first** | Existing workspaces are focused before new ones are created. Project and directory workspaces sharing a cwd keep separate identities. |
| **Agents are first-class** | Search agent name, status, workspace, cwd, pane/tab/terminal IDs, session ID, and your own aliases. |
| **Extensible without Rust** | Add another tool with a command that returns JSON and a command that opens the selected item. |
| **No picker dependency** | The Rust/ratatui interface runs in a Herdr-managed modal popup; `fzf` and `tv` are not runtime requirements. |

Herdr's built-in navigation remains the simpler choice for a single entity type. Navigator is for the moment when “where next?” could mean a workspace, agent, path, session, remote, project, or action.

## Standout features

| Feature | What it does |
| --- | --- |
| **Last Workspace** | Blocked in this development branch. Ordinary focus acknowledgements cannot qualify invoking-client workspace history. |
| **Pin** | `Ctrl-B` saves provider preferences and keeps marked entries ahead of the normal source order. Live workspace/pane marks last only for this invocation and observation scope. |
| **Open with Template** | `Alt-Enter` by default applies one reusable Herdr Plus tabs/panes template to any zoxide/root directory, creating its workspace or appending fresh template tabs when already open. |

## What it can open

| Source | Data | Enter does |
| --- | --- | --- |
| `workspace` | `herdr workspace list` + pane cwd | Focus the exact workspace |
| `agent` | Agent metadata joined to workspace/tab/pane inventory | Focus the exact pane |
| `project` | Herdr Plus project TOML | Reuse or create a project workspace and apply tabs and split panes |
| `session` | Herdr sessions + configured local entries | Attach the local session |
| `server` | Configured remote targets | Hand off to the remote Herdr server |
| `zoxide` | `zoxide query -l` | Enter opens normally; `Alt-Enter` applies the shared template |
| `root` | Configured filesystem roots | Enter opens normally; `Alt-Enter` applies the shared template |
| `quick` | Herdr Plus Quick Actions | Open the Quick Actions picker |
| `plugin` | Command/JSON integrations | Run the configured open command |

Every source can be disabled. Missing optional tools degrade quietly.

### Server-scoped panes in the development branch

Set `sources.panes = true` to list every pane on the routed ordinary server. The default remains `false`, with `sources.agents = true` showing only panes linked to agent metadata. With both enabled, each pane appears once. Turning agents off removes enrichment; turning `open_workspaces` off hides workspace rows but keeps their ancestry and reuse index.

Ordinary panes use the existing `open` source bucket. Enriched panes use `agent`. Titles prefer a manual pane label, then the agent name, a useful reported title, or a Navigator-assigned `Pane N`. Workspace and tab breadcrumbs remain visible, including a tab labeled `1`. Agent operators read metadata even when the manual title differs. Ctrl-X never closes a pane or its parent workspace.

Enter sends exact `pane.focus` with `pane_id` through the frozen ordinary-server route. Terminal IDs are metadata only. This requires an explicit inherited Unix socket and the N04 adapter's verified Herdr 0.9.0/protocol 22 contract. An optional structured agent-list rejection leaves healthy panes usable with agent metadata marked unknown. Transport uncertainty still invalidates the scope and discards old reads. The query area reports inventory failures even when no row is selected.

The header shows `Server scope: owner unknown`, even with preview off. The process host does not establish the server's owner. Socket paths and route credentials are not part of this label.

Failed activation stays in the overlay or side picker with inline error text. There is no extra acknowledgement prompt, automatic retry, or failure-triggered refresh. `Unknown` means the server may have applied the mutation without an acknowledgement. Attach and Run still suspend the terminal and resume it on failure. Successful ordinary focus closes the overlay; the side picker refreshes and restores the selected identity. These paths have offline fake-terminal coverage, not live platform qualification.

Process-owned folder/project creation and templates require the managed-plugin co-location contract and fresh action revalidation. Unknown ownership blocks them. A silent server restart at the same socket and version remains undetectable.

This is not combined machine navigation or invoking-client focus. Machine rows and the native New workspace prompt remain unavailable. Merged rendering and F6/F8 remain inactive. Provider pins persist as installation-local preferences, but durable live-target marks and invoking-client jump-back remain incomplete. N07, S01 and the release gate stay open. See [the implementation plan](docs/implementation-plan.md) and [release hold](RELEASE.md#development-branch-release-hold).

## Keyboard workflow

| Key | Action |
| --- | --- |
| type | Fuzzy search |
| `Enter` | Open selected item normally |
| `Alt-Enter` | Apply `picker.directory_template` to the selected zoxide/root directory |
| `Up` / `Down` or `Alt-K` / `Alt-J` | Move selection; the main modal picker overrides Herdr's direct pane-navigation bindings |
| `Tab` | Cycle source filters |
| `Ctrl-W` | Workspaces |
| `Ctrl-A` / `@` | Agents; a conflicting Herdr prefix is remapped automatically |
| `Ctrl-P` | Herdr Plus projects |
| `Ctrl-Q` | Herdr Plus Quick Actions |
| `Ctrl-S` | Remotes |
| `Ctrl-L` | Sessions |
| `Ctrl-Z` | Zoxide |
| `Ctrl-R` | Roots |
| `Ctrl-X` | Confirm, then close the matching open workspace |
| `Ctrl-B` | Mark or unmark the selected item |
| `Ctrl-O` | Toggle preview |
| `Ctrl-U` | Clear query and filter |
| `?` | Show active keybindings |
| `Esc` / `Ctrl-C` | Back or close |

The main `Herdr Navigator` entrypoint is a modal plugin popup, so it receives
Alt-J and Alt-K before Herdr evaluates global `focus_pane_down` and
`focus_pane_up` bindings. The persistent `Navigator Side` entrypoint remains a
normal split pane and therefore follows Herdr's normal host-binding precedence.

Status glyphs follow Herdr's `prefix+g` visual language: `◉` blocked/attention, animated Braille spinner working, `●` idle, `✓` done, and `○` unknown. Diamond color priority is marked yellow, current accent/blue, then previous red. Selection uses `→`, and source trees use `▾`, `├─`, and `└─` markers.

On the initial unfiltered view, marked items precede the normal source order. No previous-workspace marker appears while qualified jump-back is unavailable.

Structured search narrows large result sets:

```text
!claude          # agent name
@idle            # agent workspace/status
@Dotfiles        # agent workspace label or id
/dotfiles        # cwd/path
```

Set `vim_mode = true` for normal-mode `j`/`k`, source keys, and `/` search. All source shortcuts can be remapped through `[picker.filter_keys]`.

## Power moves

### Close an open directory

Select an `open` or `agent` entry, or a `project`, `root`, or `zoxide` entry that matches an open workspace, then press `Ctrl-X`. Navigator asks for confirmation before closing the workspace and refreshing the list.

Navigator refuses to close the workspace that owns the picker; switch away first. Directories that are not open and server, session, quick-action, or plugin entries are left unchanged.

### Pins and history maintenance

Provider pins survive restart and provider absence. Navigator applies them only to fresh matching rows, without creating saved replacement rows or executing anything from disk. Keys include the provider namespace, item, configured action and template or attach identity. Labels, current workspace reuse and status do not identify a preference. A write must succeed before the visible mark changes.

Live workspace and pane marks remain in memory. Transport uncertainty invalidates their observation scope. Herdr 0.9.0/protocol 22 supplies no durable server incarnation, so Navigator does not persist replayable live IDs.

Navigator stores preferences in `navigation-history.json` beside a stable `navigation-history.lock` in the existing plugin config directory. `HERDR_PLUGIN_CONFIG_DIR` overrides the default `~/.config/herdr/plugins/config/herdr-navigator`. Each App freezes an absolute directory at construction. The generated provider owner identifies this installation's preference store, not a machine or client. Do not share it as machine identity. Copying the store copies preferences only.

Legacy `pinned-entries.json` values remain unresolved, even if one current row matches. The old bare `jump-back-workspace` value is retained as unresolved provenance, never replayed. Both legacy files stay byte-for-byte unchanged. Re-pin a fresh row deliberately, or remove unresolved records by their stable IDs:

```sh
herdr-navigator history list
herdr-navigator history remove RECORD_ID
```

These owner-only commands load no App or config and make no Herdr calls. Removal retains legacy source/value tombstones so unchanged input does not resurrect a record. Saved preferences remain when their provider is unavailable.

Storage accepts at most 1 MiB per document, 4096 records including tombstones and 16 KiB per string. It never evicts records. Malformed, future-version, oversized, unreadable or unsafe files disable writes rather than becoming empty state. Unix transactions use a 250 ms advisory-lock wait budget, reload under that lock, sync a private sibling temp file, rename it and sync the directory. The lock file is never unlinked. Symlinks, nonregular files, hard links and group/other-writable store files or directories are rejected. A failure after rename reports uncertain durability. Reopen to inspect the saved result before another change; Navigator does not retry the toggle.

### Jump-back is blocked

`herdr-navigator.jump-back` currently reports that qualified persistent history is unavailable. Existing `[jump_back]` settings and bindings do not restore replay. An exact ordinary focus acknowledgement is not an authorized invoking-client transition. Workspace lists, picker context and successful creation or provider completion cannot supply that evidence. No transition or prompt outcome records a previous workspace, and Navigator does not clear or replay the legacy file. N07 remains partial and S01 release qualification remains blocked.

### Persistent side pane

Keep Navigator beside your work:

```bash
herdr plugin action invoke herdr-navigator.open-side
```

The action opens the side pane, focuses it when it already exists, and closes it when invoked while focused. Unlike the overlay, the side pane stays open after `Enter`.

Optional binding:

```toml
[[keys.command]]
key = "prefix+shift+f"
type = "plugin_action"
command = "herdr-navigator.open-side"
description = "navigator side pane"
```

## Configuration

Navigator writes a fully commented config on first run:

```bash
herdr plugin config-dir herdr-navigator
```

See [`examples/default-config.toml`](examples/default-config.toml) for every option and its behavior. Common customizations:

```toml
[picker]
reuse_existing = true
create_missing = true
engine = "nucleo" # nucleo | skim | simple
source_order = ["workspace", "agent", "project", "session", "zoxide", "root", "server", "quick", "plugin"]
source_priority_boost = 5
agent_sort = "herdr" # herdr | priority | spaces
preview = true
detailed_rows = true # source-aware Herdr-style result rows
check_updates = false # enable only when you want GitHub release checks
confirm_close_workspace = true
root_cache_seconds = 60
# directory_template = "default.toml" # Herdr Plus project file
# directory_template_key = "alt-enter" # or ctrl-g / ctrl-t
vim_mode = false

[notifications]
enabled = true
audio = false # set true to enable sound
sound = "default" # default | custom
custom_sound = "" # Example: "~/sounds/navigator.wav"

[sources]
open_workspaces = true
agents = true
herdr_plus_projects = true
herdr_plus_quick_actions = true
sessions = true
servers = true
zoxide = true
roots = true

[[roots]]
path = "~/workspace"
max_depth = 3
```

Useful config surfaces:

- `picker.detailed_rows` enables source-aware rows: right-aligned metadata for most sources and a full-path second line only for zoxide/root.
- `picker.check_updates` optionally checks GitHub releases daily. F5 keeps Herdr's source/build review prompt before installation.
- `picker.directory_template = "default.toml"` reuses that Herdr Plus project file from its `projects/` config directory. `Enter` keeps normal reuse/create behavior. `picker.directory_template_key` defaults to `alt-enter` and also accepts Ctrl forms such as `ctrl-g`; the shortcut always applies all template tabs, panes, labels, and commands using the selected directory instead of the template's `working_dir`, creating the workspace or appending fresh template tabs.
- `[notifications]` can disable notifications entirely or use Herdr's default sounds, no sound, or a custom audio file.
- `[picker.filter_keys]` remaps source shortcuts.
- `[[agent_aliases]]` adds memorable search terms without renaming Herdr panes.
- `[sessions]` controls local sessions and manual remote targets.
- `[theme]` inherits supported Herdr themes and custom tokens.
- `[[integrations]]` adds external command/JSON sources.

## Add your own source

A tool only needs a list command and an open command:

```toml
[[integrations]]
id = "bookmarks"
label = "Bookmarks"
enabled = true
collect = "bookmarks list --json"
open = "bookmarks open {{id}}"
notify_success = true
notify_error = true
```

`collect` prints a JSON array:

```json
[{"id":"abc","title":"Item","subtitle":"Info","path":"/tmp","kind":"bookmark"}]
```

Navigator shell-quotes `{{id}}`, `{{title}}`, `{{subtitle}}`, `{{path}}`, and `{{kind}}` before running `open`. See [`docs/plugin-integrations.md`](docs/plugin-integrations.md) for the full contract.

## Requirements

- Herdr `0.7.4` or newer
- Linux or macOS
- Optional: `zoxide` for directory history
- Optional: Herdr Plus for project templates and Quick Actions
- Rust stable + Cargo only when building from source

Build and link locally:

```bash
git clone https://github.com/vudx00/herdr-navigator.git
cd herdr-navigator
cargo build --release
herdr plugin link "$PWD"
```

## Troubleshooting

Check that Herdr sees the plugin and its actions:

```bash
herdr plugin list
herdr plugin action list --plugin herdr-navigator
```

Inspect every collected candidate without opening the TUI:

```bash
./target/release/herdr-navigator list
```

If a keybinding does nothing, verify the action ID and reload config:

```bash
rg "herdr-navigator.open" ~/.config/herdr/config.toml
herdr server reload-config
```

Optional sources can be checked independently:

```bash
zoxide query -l
find ~/.config/herdr/plugins/config/cloudmanic.herdr-plus/projects -name '*.toml'
```

## Project docs

- [`docs/roadmap.md`](docs/roadmap.md) — roadmap and scope boundaries
- [`docs/architecture.md`](docs/architecture.md) — runtime flow and design
- [`docs/integrations.md`](docs/integrations.md) — Herdr/plugin integration patterns
- [`docs/plugin-integrations.md`](docs/plugin-integrations.md) — command/JSON contract
- [`CHANGELOG.md`](CHANGELOG.md) — released and unreleased changes
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — development workflow

Herdr Navigator is intentionally small: reuse Herdr primitives, keep optional integrations optional, and make the common path `prefix+t → type → Enter`.
