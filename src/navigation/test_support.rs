//! Fixture integrity and data-only production-join adapters. No transport lives here.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

pub(crate) const WORKSPACE_LIST: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.7.3/workspace-list.json");
pub(crate) const AGENT_LIST: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.7.3/agent-list.json");
pub(crate) const INVENTORY: &str = include_str!(
    "../../tests/fixtures/navigation/hypothetical-client/six-workspaces-nine-panes.json"
);
pub(crate) const COLLISIONS: &str =
    include_str!("../../tests/fixtures/navigation/hypothetical-client/identity-collisions.json");
const FAILURES: &str = include_str!(
    "../../tests/fixtures/navigation/hypothetical-client/capability-failure-cases.json"
);
const SCHEMA: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/schema-excerpts.json");
const VERSION: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/version.txt");
const PANE_LIST_HELP: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/pane-list-help.txt");
const PANE_FOCUS_HELP: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/pane-focus-help.txt");
const WORKSPACE_FOCUS_HELP: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/workspace-focus-help.txt");
const TAB_FOCUS_HELP: &str =
    include_str!("../../tests/fixtures/navigation/ordinary-server/0.9.0/tab-focus-help.txt");

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("valid fixture JSON")
}

fn array(value: &Value) -> &[Value] {
    value.as_array().expect("fixture array")
}

fn strings(value: &Value) -> Vec<&str> {
    array(value)
        .iter()
        .map(|v| v.as_str().expect("fixture string"))
        .collect()
}

fn qualified(owner: &Value, kind: &str, id: &str) -> Value {
    assert_eq!(array(owner).len(), 3);
    json!([owner[0], owner[1], owner[2], kind, id])
}

fn destinations(fixture: &Value) -> BTreeMap<String, &Value> {
    let rows = array(&fixture["expected"]["destinations"]);
    let mut result = BTreeMap::new();
    for row in rows {
        let identity = strings(&row["identity"]);
        assert_eq!(identity.len(), 5);
        assert!(identity.iter().all(|part| !part.is_empty()));
        assert!(
            result.insert(row["identity"].to_string(), row).is_none(),
            "duplicate identity"
        );
    }
    result
}

fn check_destination(
    remaining: &mut BTreeMap<String, &Value>,
    identity: &Value,
    ancestors: &[&str],
    action: &str,
) {
    let row = remaining
        .remove(&identity.to_string())
        .expect("input identity has exactly one expected destination");
    assert_eq!(strings(&row["ancestors"]), ancestors);
    assert_eq!(row["action"], action);
}

#[test]
fn manifest_separates_inherited_metadata_and_hypothetical_evidence() {
    let manifest = parse(include_str!(
        "../../tests/fixtures/navigation/manifest.json"
    ));
    assert_eq!(manifest["fixture_schema"], "navigation-n01-v1");
    let expected = [
        ("ordinary-server/0.7.3/workspace-list.json", WORKSPACE_LIST),
        ("ordinary-server/0.7.3/agent-list.json", AGENT_LIST),
        ("ordinary-server/0.9.0/version.txt", VERSION),
        ("ordinary-server/0.9.0/schema-excerpts.json", SCHEMA),
        ("ordinary-server/0.9.0/pane-list-help.txt", PANE_LIST_HELP),
        ("ordinary-server/0.9.0/pane-focus-help.txt", PANE_FOCUS_HELP),
        (
            "ordinary-server/0.9.0/workspace-focus-help.txt",
            WORKSPACE_FOCUS_HELP,
        ),
        ("ordinary-server/0.9.0/tab-focus-help.txt", TAB_FOCUS_HELP),
        (
            "hypothetical-client/six-workspaces-nine-panes.json",
            INVENTORY,
        ),
        ("hypothetical-client/identity-collisions.json", COLLISIONS),
        (
            "hypothetical-client/capability-failure-cases.json",
            FAILURES,
        ),
    ];
    let files = array(&manifest["files"]);
    assert_eq!(files.len(), expected.len());
    let mut paths = BTreeSet::new();
    for file in files {
        let path = file["path"].as_str().unwrap();
        assert!(paths.insert(path));
        let (_, contents) = expected.iter().find(|(p, _)| *p == path).unwrap();
        assert!(!contents.is_empty());
        assert!(contents.len() < 32_768, "keep individual samples bounded");
        let (evidence, version) = if path.contains("/0.7.3/") {
            ("inherited-parser-sample", json!("0.7.3"))
        } else if path.contains("/0.9.0/") {
            ("installed-offline-metadata", json!("0.9.0"))
        } else {
            let fixture = parse(contents);
            assert_eq!(fixture["fixture_schema"], "navigation-n01-v1");
            assert_eq!(fixture["evidence"], "synthetic-hypothetical");
            assert_eq!(
                fixture["case_id"],
                path.rsplit('/').next().unwrap().trim_end_matches(".json")
            );
            ("synthetic-hypothetical", Value::Null)
        };
        assert_eq!(file["evidence"], evidence);
        assert_eq!(file["herdr_version"], version);
    }
}

#[test]
fn inherited_samples_keep_their_original_envelopes_and_link_ids() {
    let ws = parse(WORKSPACE_LIST);
    let agents = parse(AGENT_LIST);
    assert_eq!(ws["id"], "cli:workspace:list");
    assert_eq!(ws["result"]["type"], "workspace_list");
    assert_eq!(array(&ws["result"]["workspaces"]).len(), 2);
    assert_eq!(ws["result"]["workspaces"][0]["workspace_id"], "w41");
    assert_eq!(ws["result"]["workspaces"][1]["workspace_id"], "w43");
    assert_eq!(agents["id"], "cli:agent:list");
    assert_eq!(agents["result"]["type"], "agent_list");
    assert_eq!(array(&agents["result"]["agents"]).len(), 1);
    let agent = &agents["result"]["agents"][0];
    assert_eq!(agent["agent"], "claude");
    assert_eq!(agent["pane_id"], "w43:p1");
    assert_eq!(agent["terminal_id"], "term_1");
    assert_eq!(agent["agent_session"]["value"], "58f4-session");
}

#[test]
fn installed_schema_distinguishes_exact_pane_target_from_directional_cli() {
    assert_eq!(VERSION.trim(), "herdr 0.9.0");
    let schema = parse(SCHEMA);
    assert_eq!(schema["evidence"], "installed-offline-metadata");
    assert_eq!(schema["herdr_version"], "0.9.0");
    assert_eq!(schema["command"], "herdr api schema --json");
    let excerpts = &schema["excerpts"];
    assert_eq!(excerpts["/protocol"], 22);
    assert_eq!(excerpts["/schema_version"], 1);
    assert_eq!(
        excerpts["/schemas/request/properties"],
        json!({"id":{"type":"string"}})
    );
    assert_eq!(excerpts["/schemas/request/required"], json!(["id"]));
    assert_eq!(
        excerpts["/schemas/success_response/required"],
        json!(["id", "result"])
    );
    assert_eq!(
        excerpts["/schemas/error_response/required"],
        json!(["id", "error"])
    );
    for (method, params) in [
        ("pane.focus", "PaneTarget"),
        ("pane.focus_direction", "PaneFocusDirectionParams"),
        ("workspace.focus", "WorkspaceTarget"),
        ("tab.focus", "TabTarget"),
    ] {
        let variants: Vec<_> = excerpts
            .as_object()
            .unwrap()
            .values()
            .filter(|v| v.pointer("/properties/method/const") == Some(&json!(method)))
            .collect();
        assert_eq!(variants.len(), 1);
        assert_eq!(
            variants[0]["properties"]["params"]["$ref"],
            format!("#/schemas/request/$defs/{params}")
        );
    }
    assert_eq!(
        excerpts["/schemas/request/$defs/PaneTarget"]["required"],
        json!(["pane_id"])
    );
    assert!(PANE_FOCUS_HELP.contains("--direction <DIRECTION>"));
    assert!(PANE_FOCUS_HELP.contains("--pane <ID>"));
    assert!(PANE_LIST_HELP.contains("--workspace <WORKSPACE_ID>"));
    assert!(WORKSPACE_FOCUS_HELP.contains("herdr workspace focus <workspace_id>"));
    assert!(TAB_FOCUS_HELP.contains("herdr tab focus <tab_id>"));
    let snapshot = &excerpts["/schemas/success_response/$defs/SessionSnapshot"]["properties"];
    assert!(snapshot.get("machines").is_none());
    assert!(snapshot.get("server_incarnation").is_none());
    let tab = &excerpts["/schemas/success_response/$defs/TabInfo"]["properties"];
    assert!(tab.get("label").is_some());
    assert!(
        tab.get("naming").is_none(),
        "synthetic naming metadata is not schema evidence"
    );
}

#[test]
fn inventory_has_all_destinations_and_qualified_ancestry() {
    let fixture = parse(INVENTORY);
    assert_eq!(
        fixture["expected"]["counts"],
        json!({
            "machines":2,"workspaces":6,"tabs":7,"panes":9,
            "agent_destinations":0,"linked_agent_panes":2
        })
    );
    let machines = array(&fixture["input"]["machines"]);
    assert_eq!(machines.len(), 2);
    assert_eq!(machines[0]["label"], "Local");
    assert_eq!(machines[1]["label"], "void.dev");
    assert_eq!(machines[0]["home"], "/fixture/local");
    assert!(machines[1]["home"].is_null());
    assert!(machines[0]["workspaces"][2]["cwd"].is_null());
    let labels: Vec<Vec<_>> = machines
        .iter()
        .map(|machine| {
            array(&machine["workspaces"])
                .iter()
                .map(|w| w["label"].as_str().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        labels,
        vec![
            vec!["gemini-cyber", "fedramp-rt", "work", "herdr-navigator"],
            vec!["seo-saas", "herdr-navigator"]
        ]
    );
    let mut remaining = destinations(&fixture);
    assert_eq!(remaining.len(), 24);
    let mut panes = BTreeSet::new();
    let mut naming_states = BTreeSet::new();
    let mut labels_of_one = BTreeSet::new();
    for (machine_index, machine) in machines.iter().enumerate() {
        let owner = &machine["owner"];
        let counts = &fixture["expected"]["owner_counts"][machine_index];
        assert_eq!(counts[0], *owner);
        check_destination(
            &mut remaining,
            &qualified(owner, "machine", owner[0].as_str().unwrap()),
            &[],
            "NativeNewWorkspace",
        );
        let workspaces = array(&machine["workspaces"]);
        assert_eq!(counts[1], workspaces.len());
        let (mut tab_count, mut pane_count) = (0, 0);
        for workspace in workspaces {
            let wid = workspace["id"].as_str().unwrap();
            check_destination(
                &mut remaining,
                &qualified(owner, "workspace", wid),
                &[],
                "FocusWorkspace",
            );
            for tab in array(&workspace["tabs"]) {
                tab_count += 1;
                let tid = tab["id"].as_str().unwrap();
                naming_states.insert(tab["naming"].as_str().unwrap());
                if tab["label"] == "1" {
                    labels_of_one.insert(tab["naming"].as_str().unwrap());
                }
                check_destination(
                    &mut remaining,
                    &qualified(owner, "tab", tid),
                    &[wid],
                    "FocusTab",
                );
                for pane in array(&tab["panes"]) {
                    pane_count += 1;
                    let id = qualified(owner, "pane", pane["id"].as_str().unwrap());
                    assert_ne!(pane["id"], pane["terminal_id"]);
                    assert!(panes.insert(id.to_string()));
                    check_destination(&mut remaining, &id, &[wid, tid], "FocusPane");
                }
            }
        }
        assert_eq!(counts[2], tab_count);
        assert_eq!(counts[3], pane_count);
    }
    assert!(remaining.is_empty());
    assert_eq!(panes.len(), 9);
    assert!(panes.contains(&fixture["input"]["current"].to_string()));
    assert_eq!(
        fixture["input"]["current"],
        json!([
            "void-dev",
            "session-remote",
            "epoch-remote-1",
            "pane",
            "w2:p1"
        ])
    );
    assert_eq!(
        naming_states,
        BTreeSet::from(["named", "unnamed", "unknown"])
    );
    assert_eq!(labels_of_one, naming_states);
    let titles = array(&fixture["expected"]["pane_titles"]);
    assert_eq!(titles.len(), 2);
    for title in titles {
        assert!(panes.contains(&title[0].to_string()));
    }
    assert_eq!(
        titles[0][1],
        machines[0]["workspaces"][3]["tabs"][0]["panes"][0]["label"]
    );
    assert_eq!(titles[0][2], "claude");
    assert_eq!(titles[1][1], "gemini");
    let updates = array(&fixture["input"]["agent_updates"]);
    let links = array(&fixture["expected"]["agent_links"]);
    assert_eq!(updates.len(), links.len());
    let mut linked_panes = BTreeSet::new();
    for (update, link) in updates.iter().zip(links) {
        assert_eq!(update["update_id"], link[0]);
        match link[1].as_str().unwrap() {
            "missing" => assert!(update["pane_id"].is_null()),
            validity @ ("valid" | "nonexistent") => {
                let pane = qualified(
                    &update["owner"],
                    "pane",
                    update["pane_id"].as_str().unwrap(),
                )
                .to_string();
                assert_eq!(panes.contains(&pane), validity == "valid");
                if validity == "valid" {
                    linked_panes.insert(pane);
                }
            }
            other => panic!("unexpected link state {other}"),
        }
    }
    assert_eq!(linked_panes.len(), 2);
    assert_eq!(updates[0]["pane_id"], updates[1]["pane_id"]);
    assert_eq!(updates[0]["owner"], updates[1]["owner"]);
    assert!(updates[1]["revision"].as_u64().unwrap() > updates[0]["revision"].as_u64().unwrap());
}

#[test]
fn collisions_differ_only_in_owner_session_or_incarnation() {
    let fixture = parse(COLLISIONS);
    let owners = array(&fixture["input"]["owners"]);
    let shared = &fixture["input"]["shared"];
    assert_eq!(owners.len(), 4);
    assert_eq!(
        fixture["expected"]["counts"],
        json!({"owners":4,"workspaces":4,"tabs":4,"panes":4})
    );
    assert_ne!(owners[0][0], owners[1][0]);
    assert_eq!(owners[1][0], owners[2][0]);
    assert_ne!(owners[1][1], owners[2][1]);
    assert_eq!(owners[1][1], owners[3][1]);
    assert_ne!(owners[1][2], owners[3][2]);
    assert_eq!(shared["cwd"], "/fixture/shared/herdr-navigator");
    assert!(shared["home"].is_null());
    let mut remaining = destinations(&fixture);
    assert_eq!(remaining.len(), 12);
    for owner in owners {
        for (kind, id, ancestors, action) in [
            ("workspace", "w1", vec![], "FocusWorkspace"),
            ("tab", "w1:t1", vec!["w1"], "FocusTab"),
            ("pane", "w1:p1", vec!["w1", "w1:t1"], "FocusPane"),
        ] {
            assert_eq!(shared[format!("{kind}_id")], id);
            check_destination(
                &mut remaining,
                &qualified(owner, kind, id),
                &ancestors,
                action,
            );
        }
    }
    assert!(remaining.is_empty());
}

#[test]
fn capability_cases_have_unique_ids_and_explicit_outcomes() {
    let fixture = parse(FAILURES);
    let required = "ordinary-remote-server snapshot-only focus-without-native-flow full-capabilities unsupported-focus-workspace unsupported-focus-tab unsupported-focus-pane route-missing route-expired route-unauthorized route-wrong-client unsupported-protocol unsupported-operation connecting-cached connecting-uncached disabled-cached disabled-uncached error-cached error-uncached offline-cached offline-uncached empty-inventory unknown-inventory agent-provider-failed target-deleted target-wrong-owner target-wrong-session target-wrong-ancestry target-wrong-incarnation target-wrong-revision transport-malformed-json transport-truncated-output transport-timeout transport-request-failed transport-ack-negative transport-ack-invalid transport-ack-lost native-opened native-cancelled native-created native-failed native-unknown";
    let mut ids = BTreeSet::new();
    assert_eq!(fixture["expected"]["case_count"], 42);
    assert!(fixture["expected"]["invariants"]
        .as_object()
        .unwrap()
        .values()
        .all(|v| v == true));
    for case in array(&fixture["input"]["cases"]) {
        let id = case["case_id"].as_str().unwrap();
        assert!(ids.insert(id), "duplicate scenario {id}");
        assert!(case["input"].is_object());
        let expected = &case["expected"];
        for field in ["availability", "scope", "action", "reason"] {
            assert!(
                !expected[field].as_str().unwrap().is_empty(),
                "{id}: {field}"
            );
        }
        assert!([
            "available",
            "disabled",
            "unavailable",
            "handed-off",
            "cancelled",
            "completed"
        ]
        .contains(&expected["availability"].as_str().unwrap()));
        assert!(["combined", "server:void.dev"].contains(&expected["scope"].as_str().unwrap()));
        assert!([
            "FocusWorkspace",
            "FocusTab",
            "FocusPane",
            "NativeNewWorkspace",
            "none"
        ]
        .contains(&expected["action"].as_str().unwrap()));
        for (action, kind) in [
            ("FocusWorkspace", "workspace"),
            ("FocusTab", "tab"),
            ("NativeNewWorkspace", "machine"),
        ] {
            if expected["action"] == action {
                assert_eq!(array(&case["input"]["target"]).len(), 5);
                assert_eq!(case["input"]["target"][3], kind);
            }
        }
        if case["input"].get("cached").is_some() {
            assert_eq!(expected["unaffected_owner"][0], "local");
            assert_eq!(expected["unaffected_action"], "FocusPane");
        }
        if id.ends_with("-uncached") || id == "unknown-inventory" {
            assert!(expected["pane_count"].is_null());
        }
        if id.ends_with("-cached") {
            assert_eq!(expected["inventory"], "stale");
            assert_eq!(expected["availability"], "disabled");
        }
        if id == "empty-inventory" {
            assert_eq!(expected["pane_count"], 0);
        }
        if id.starts_with("transport-") {
            assert_eq!(expected["history"], "unchanged");
            assert_eq!(expected["retry"], false);
        }
        if id == "native-opened" || id == "native-cancelled" {
            assert_eq!(expected["created_count"], 0);
            assert_eq!(expected["history"], "unchanged");
        }
    }
    assert_eq!(ids, required.split_whitespace().collect());
}

#[test]
fn hypothetical_data_contains_only_allowlisted_metadata() {
    // Schema excerpts describe upstream fields. Only synthetic data uses this allowlist.
    const KEYS: &str = "fixture_schema evidence case_id input expected machines owner label relationship home connection workspaces id cwd tabs naming panes terminal_id ordinal revision current agent_updates update_id pane_id agent aliases session status counts agent_destinations linked_agent_panes owner_counts destinations identity ancestors action agent_links pane_titles machine_action owners shared workspace_id tab_id path_policy defaults capabilities route phase cached target invariants case_count unaffected_owner unaffected_action no_ssh no_attach no_direct_create_fallback no_process_server_retry failures_keep_picker_open cases availability scope reason pane_action unsupported_focus_kind protocol operation inventory pane_count agent_provider agent_metadata target_validation selection transport outcome history retry native_flow created_count";
    fn check(value: &Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    assert!(
                        KEYS.split_whitespace().any(|allowed| allowed == key),
                        "unexpected metadata field {key}"
                    );
                    check(value);
                }
            }
            Value::Array(values) => values.iter().for_each(check),
            Value::String(text) => {
                assert!(!text.contains('\u{1b}'));
                assert!(text.len() < 256);
            }
            _ => {}
        }
    }
    for source in [INVENTORY, COLLISIONS, FAILURES] {
        check(&parse(source));
    }
}

/// Synthetic pane/tab reads extend the inherited samples. These are not captures.
pub(crate) fn ordinary_lists() -> [Value; 4] {
    let workspaces = parse(WORKSPACE_LIST);
    let agents = parse(AGENT_LIST);
    let mut panes = agents.clone();
    panes["result"] = json!({"type":"pane_list", "panes":agents["result"]["agents"]});
    panes["result"]["panes"][0]["label"] = "Review key handling".into();
    let tabs = json!({"id":"synthetic:tab", "result":{"type":"tab_list", "tabs":[{
        "tab_id":"w43:t1", "workspace_id":"w43", "number":1, "label":"1", "focused":true, "pane_count":1, "agent_status":"working"
    }]}});
    [workspaces, panes, tabs, agents]
}

/// Data-only hypothetical client input. It has no transport, focus or authorization methods.
#[derive(serde::Deserialize)]
pub(crate) struct FixtureClient {
    pub(crate) machines: Vec<FixtureMachine>,
    pub(crate) current: Option<[String; 5]>,
    agent_updates: Vec<FixtureAgent>,
}
#[derive(serde::Deserialize)]
pub(crate) struct FixtureMachine {
    pub(crate) owner: [String; 3],
    label: String,
    relationship: String,
    pub(crate) workspaces: Vec<FixtureWorkspace>,
}
#[derive(serde::Deserialize)]
pub(crate) struct FixtureWorkspace {
    pub(crate) id: String,
    label: String,
    pub(crate) tabs: Vec<FixtureTab>,
}
#[derive(serde::Deserialize)]
pub(crate) struct FixtureTab {
    pub(crate) id: String,
    label: String,
    naming: FixtureNaming,
    pub(crate) panes: Vec<FixturePane>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum FixtureNaming {
    Named,
    Unnamed,
    Unknown,
}
#[derive(serde::Deserialize)]
pub(crate) struct FixturePane {
    pub(crate) id: String,
    terminal_id: String,
    label: Option<String>,
    cwd: Option<String>,
    revision: u64,
}
#[derive(serde::Deserialize)]
struct FixtureAgent {
    owner: [String; 3],
    pane_id: Option<String>,
    agent: String,
    aliases: Vec<String>,
    session: Option<String>,
    status: String,
    revision: u64,
}
impl FixtureClient {
    pub(crate) fn sample() -> Self {
        // Deliberately deserialize only input. Expected output is assertion data.
        serde_json::from_value(parse(INVENTORY)["input"].take()).unwrap()
    }
    pub(crate) fn picker_input(&self) -> crate::picker::PickerInput {
        use crate::{
            actions::{self, Observations, Prepared, Slot},
            model::{Entry, EntryIdentity, LiveIdentity, MachineIdentity, ObjectKind, ServerScope},
            navigation::{inventory::*, NavigationContext},
            picker::{Destination, Heading, Owner, PickerInput, TabInventory},
            sources::{join_inventory, PaneOrdinals},
        };
        let mut input = PickerInput::default();
        for machine in &self.machines {
            let scope =
                ServerScope::reported(&machine.owner[0], &machine.owner[1], &machine.owner[2])
                    .unwrap();
            let context = NavigationContext::reported(scope.clone());
            let identity = MachineIdentity::new(
                scope.session().machine().clone(),
                Some(scope.session().clone()),
            )
            .unwrap();
            let owner = Owner::Machine(identity.key().clone());
            let machine_entry =
                Entry::machine(identity, machine.label.clone(), "Fixture only".into());
            input.headings.push(Heading {
                owner: owner.clone(),
                label: machine.label.clone(),
                local: machine.relationship == "local",
                machine: Some(machine_entry.key().clone()),
            });
            let mut workspaces = Vec::new();
            let mut tabs = Vec::new();
            let mut panes = Vec::new();
            for ws in &machine.workspaces {
                let mut pane_count = 0;
                for tab in &ws.tabs {
                    tabs.push(TabRecord {
                        tab_id: tab.id.clone(),
                        workspace_id: ws.id.clone(),
                        label: tab.label.clone(),
                    });
                    input.naming.insert(
                        LiveIdentity::new(&scope, ObjectKind::Tab, &tab.id).unwrap(),
                        match tab.naming {
                            FixtureNaming::Named => TabNaming::Named,
                            FixtureNaming::Unnamed => TabNaming::Unnamed,
                            FixtureNaming::Unknown => TabNaming::Unknown,
                        },
                    );
                    for pane in &tab.panes {
                        pane_count += 1;
                        panes.push(PaneRecord {
                            pane_id: pane.id.clone(),
                            workspace_id: ws.id.clone(),
                            tab_id: tab.id.clone(),
                            terminal_id: pane.terminal_id.clone(),
                            revision: pane.revision,
                            label: pane.label.clone(),
                            title: None,
                            terminal_title: None,
                            terminal_title_stripped: None,
                            cwd: pane.cwd.clone(),
                            foreground_cwd: None,
                        });
                    }
                }
                input.tabs.insert(
                    LiveIdentity::new(&scope, ObjectKind::Workspace, &ws.id).unwrap(),
                    TabInventory {
                        expected: ws.tabs.len() as u64,
                        complete: true,
                    },
                );
                workspaces.push(WorkspaceRecord {
                    workspace_id: ws.id.clone(),
                    label: ws.label.clone(),
                    focused: false,
                    tab_count: ws.tabs.len() as u64,
                    pane_count,
                    agent_status: AgentStatus::Unknown,
                });
            }
            let mut agents = Vec::new();
            let mut aliases = Vec::new();
            for agent in self
                .agent_updates
                .iter()
                .filter(|a| a.owner == machine.owner)
            {
                let pane_id = agent.pane_id.as_deref().unwrap_or("");
                let pane = panes.iter().find(|p| p.pane_id == pane_id);
                agents.push(AgentRecord {
                    pane_id: pane_id.into(),
                    workspace_id: pane.map(|p| p.workspace_id.clone()).unwrap_or_default(),
                    tab_id: pane.map(|p| p.tab_id.clone()).unwrap_or_default(),
                    terminal_id: pane.map(|p| p.terminal_id.clone()).unwrap_or_default(),
                    revision: agent.revision,
                    agent: Some(agent.agent.clone()),
                    name: None,
                    display_agent: None,
                    // Synthetic "waiting" is not protocol-22 evidence.
                    agent_status: serde_json::from_value(json!(agent.status))
                        .unwrap_or(AgentStatus::Unknown),
                    agent_session: agent.session.as_ref().map(|value| AgentSession {
                        agent: agent.agent.clone(),
                        source: "synthetic".into(),
                        kind: AgentSessionKind::Id,
                        value: value.clone(),
                    }),
                });
                aliases.extend(
                    agent
                        .aliases
                        .iter()
                        .map(|alias| crate::config::AgentAliasConfig {
                            alias: alias.clone(),
                            agent: Some(agent.agent.clone()),
                            workspace: None,
                            path: None,
                        }),
                );
            }
            let inventory = Inventory {
                workspaces: InventoryRead::new(&scope, Some(Ok(workspaces))),
                tabs: InventoryRead::new(&scope, Some(Ok(tabs))),
                panes: InventoryRead::new(&scope, Some(Ok(panes))),
                agents: InventoryRead::new(&scope, Some(Ok(agents))),
            };
            let joined =
                join_inventory(&inventory, &context, &mut PaneOrdinals::default(), &aliases);
            for entry in std::iter::once(&machine_entry)
                .chain(&joined.workspaces)
                .chain(&joined.tabs)
                .chain(&joined.panes)
            {
                if self.current.as_ref().is_some_and(|current| {
                    current[..3] == machine.owner
                        && entry.target().is_some_and(|t| {
                            t.identity().id() == current[4]
                                && current[3]
                                    == match t.identity().kind() {
                                        ObjectKind::Workspace => "workspace",
                                        ObjectKind::Tab => "tab",
                                        ObjectKind::Pane => "pane",
                                    }
                        })
                }) {
                    input.current = Some(entry.key().clone());
                }
                let action = actions::resolve(
                    entry,
                    Slot::Primary,
                    &Prepared::default(),
                    &crate::config::Config::default(),
                    Observations {
                        context: &context,
                        inventory: Some(&inventory),
                        joined: &joined,
                        reuse: &joined.reuse,
                    },
                );
                input.destinations.push(Destination {
                    entry: entry.clone(),
                    owner: owner.clone(),
                    eligible: true,
                    action,
                });
            }
            // This adapter never turns synthetic identity into a usable client route.
            assert!(input
                .destinations
                .iter()
                .filter(|d| matches!(d.entry.key(), EntryIdentity::Machine(_)))
                .all(|d| d.action.unavailable.is_some()));
        }
        input
    }
}
