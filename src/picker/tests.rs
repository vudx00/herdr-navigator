use super::*;
use crate::navigation_test_support::FixtureClient;

fn browse<'a>(input: &'a PickerInput, state: &PickerState) -> Projection<'a> {
    project(
        input,
        state,
        Request {
            query: "",
            non_live_type: false,
            eligible: &input.eligible_ids(),
        },
    )
}
fn expand_all(input: &PickerInput, state: &mut PickerState) {
    state.expanded.extend(
        input
            .destinations
            .iter()
            .map(|d| RowId::Destination(d.entry.key().clone())),
    );
}
fn row_id(d: &Destination) -> RowId {
    RowId::Destination(d.entry.key().clone())
}

fn live_id(input: &PickerInput, machine: usize, id: &str) -> EntryIdentity {
    input
        .destinations
        .iter()
        .find(|d| {
            d.owner == input.headings[machine].owner
                && d.entry.target().is_some_and(|t| t.identity().id() == id)
        })
        .unwrap()
        .entry
        .key()
        .clone()
}
fn search_ids(
    input: &PickerInput,
    state: &PickerState,
    query: &str,
    engine: &str,
) -> Vec<EntryIdentity> {
    let ranking = PickerConfig {
        engine: engine.into(),
        ..PickerConfig::default()
    };
    search_project(input, state, query, &ranking)
        .rows
        .into_iter()
        .filter_map(|row| row.destination.map(|d| d.entry.key().clone()))
        .collect()
}

#[test]
fn joined_direct_machine_and_workspace_matches_lead_inherited_pinned_children() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        input.pinned.extend(
            input
                .destinations
                .iter()
                .filter(|d| d.entry.destination_kind() == DestinationKind::Pane)
                .map(|d| d.entry.key().clone()),
        );
        let mut state = PickerState::new(&input);
        let ranking = PickerConfig {
            engine: engine.into(),
            source_priority_boost: i64::MAX,
            source_order: vec!["agents".into(), "workspaces".into()],
            ..PickerConfig::default()
        };
        let machine = input.headings[1].machine.clone().unwrap();
        let results = search_project(&input, &state, "void", &ranking);
        assert_eq!(
            results.rows[0].id,
            RowId::Destination(machine.clone()),
            "{engine}"
        );
        assert!(results
            .rows
            .iter()
            .skip(1)
            .all(|row| row.destination.unwrap().owner == input.headings[1].owner));
        state.set_type(TypeFilter::Machines);
        assert_eq!(search_ids(&input, &state, "void", engine), [machine]);
        state.set_type(TypeFilter::All);
        let results = search_project(&input, &state, "herdr-navigator", &ranking);
        let direct: HashSet<_> = results.rows.iter().take(2).map(|r| r.id.clone()).collect();
        assert_eq!(
            direct,
            [
                RowId::Destination(live_id(&input, 0, "w4")),
                RowId::Destination(live_id(&input, 1, "w2"))
            ]
            .into()
        );
        assert!(results
            .rows
            .iter()
            .skip(2)
            .any(|r| input.pinned.contains(r.destination.unwrap().entry.key())));
        state.set_type(TypeFilter::Panes);
        assert_eq!(
            search_ids(&input, &state, "herdr-navigator", engine).len(),
            3
        );
        state.set_type(TypeFilter::Agents);
        assert_eq!(
            search_ids(&input, &state, "herdr-navigator", engine),
            [live_id(&input, 0, "w4:p1")]
        );
    }
}

#[test]
fn discovered_machine_search_does_not_inherit_configured_remote_provenance() {
    for engine in ["nucleo", "skim", "simple"] {
        let input = FixtureClient::sample().picker_input();
        let machine = input.headings[1].machine.clone().unwrap();
        let expected: HashSet<_> = input
            .destinations
            .iter()
            .filter(|d| d.owner == input.headings[1].owner)
            .map(|d| d.entry.key().clone())
            .collect();
        assert!(expected.contains(&machine) && expected.len() > 1);
        for machine_filter in [
            MachineFilter::All,
            MachineFilter::Machine(MachineKey::new("void-dev").unwrap()),
        ] {
            let mut state = PickerState::new(&input);
            expand_all(&input, &mut state);
            let saved = state.browse.clone();
            assert!(saved.selection.is_some() && saved.viewport.is_some());
            let expanded = state.expanded.clone();
            let query = String::from("void");
            let query_bytes = query.as_bytes().to_vec();
            state.set_machine(machine_filter.clone());
            assert_eq!(state.filters.kind, TypeFilter::All);
            assert_eq!(state.filters.provenance, None);
            for (kind, provenance) in [
                (TypeFilter::All, None),
                (TypeFilter::Machines, None),
                (TypeFilter::All, Some(Source::Server)),
                (TypeFilter::Machines, Some(Source::Server)),
            ] {
                let previous_provenance = state.filters.provenance;
                state.set_type(kind);
                assert_eq!(state.filters.provenance, previous_provenance);
                assert_eq!(state.filters.machine, machine_filter);
                state.set_provenance(provenance);
                assert_eq!(state.filters.kind, kind);
                assert_eq!(state.filters.machine, machine_filter);
                let ids = search_ids(&input, &state, &query, engine);
                if provenance.is_some() {
                    assert!(ids.is_empty(), "{engine}: {kind:?}");
                } else if kind == TypeFilter::Machines {
                    assert_eq!(ids, std::slice::from_ref(&machine), "{engine}");
                } else {
                    assert_eq!(
                        ids.into_iter().collect::<HashSet<_>>(),
                        expected,
                        "{engine}"
                    );
                }
                assert_eq!(query.as_bytes(), query_bytes);
                assert_eq!(state.browse, saved);
                assert_eq!(state.expanded, expanded);
            }
            assert!(search_ids(&input, &state, "", engine).is_empty());
            state.set_machine(MachineFilter::Machine(MachineKey::new("local").unwrap()));
            assert_eq!(state.filters.kind, TypeFilter::Machines);
            assert_eq!(state.filters.provenance, Some(Source::Server));
            assert!(search_ids(&input, &state, &query, engine).is_empty());
            state.set_machine(machine_filter.clone());
            assert_eq!(state.filters.kind, TypeFilter::Machines);
            assert_eq!(state.filters.provenance, Some(Source::Server));
            // Clearing provenance restores the same machine without replacing picker state.
            state.set_provenance(None);
            assert_eq!(state.filters.kind, TypeFilter::Machines);
            assert_eq!(state.filters.machine, machine_filter);
            assert_eq!(
                search_ids(&input, &state, &query, engine),
                std::slice::from_ref(&machine)
            );
            state.set_machine(MachineFilter::Machine(MachineKey::new("local").unwrap()));
            assert_eq!(state.filters.kind, TypeFilter::Machines);
            assert_eq!(state.filters.provenance, None);
            assert!(search_ids(&input, &state, &query, engine).is_empty());
            state.set_type(TypeFilter::All);
            assert!(search_ids(&input, &state, &query, engine).is_empty());
            assert_eq!(query.as_bytes(), query_bytes);
            assert_eq!(state.browse, saved);
            assert_eq!(state.expanded, expanded);
        }
    }
}

#[test]
fn direct_best_field_quality_not_pane_kind_or_field_order_decides_rank() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        let workspace = live_id(&input, 0, "w4");
        let pane = live_id(&input, 0, "w4:p1");
        let other = live_id(&input, 0, "w4:p2");
        for d in &mut input.destinations {
            if d.entry.key() == &workspace {
                d.entry.title = "target repository".into();
            }
            if d.entry.key() == &pane {
                d.entry.title = "tiny assorted rungs get extra time".into();
                d.entry.agent.as_mut().unwrap().aliases = vec!["target".into()];
            }
            if d.entry.key() == &other {
                d.entry.title = "target pane".into();
            }
        }
        input.pinned.insert(workspace.clone());
        let state = PickerState::new(&input);
        assert_eq!(
            search_ids(&input, &state, "target", engine)[0],
            pane,
            "best alias wins for {engine}"
        );
        for d in &mut input.destinations {
            if d.entry.key() == &workspace {
                d.entry.title = "target".into();
            }
        }
        assert_eq!(
            search_ids(&input, &state, "target", engine)[0],
            workspace,
            "equal exact favors workspace for {engine}"
        );
        for d in &mut input.destinations {
            if d.entry.key() == &pane {
                d.entry.agent.as_mut().unwrap().aliases = vec!["targeting".into()];
            }
        }
        let results = search_ids(&input, &state, "target", engine);
        assert_eq!(results[0], workspace);
        assert!(results.iter().position(|id| id == &other).unwrap() < results.len() - 1);
    }
}

#[test]
fn joined_filters_and_operators_intersect_without_touching_unicode_query_or_browse() {
    for engine in ["nucleo", "skim", "simple"] {
        let input = FixtureClient::sample().picker_input();
        let mut state = PickerState::new(&input);
        let saved = state.browse.clone();
        let expanded = state.expanded.clone();
        let query = "  !gemini @seo @idle /seo #idle  ".to_owned();
        let original = query.clone();
        state.set_machine(MachineFilter::Machine(MachineKey::new("void-dev").unwrap()));
        state.set_type(TypeFilter::Agents);
        state.set_provenance(Some(Source::Workspace));
        assert_eq!(
            search_ids(&input, &state, &query, engine),
            [live_id(&input, 1, "w1:p1")]
        );
        state.set_provenance(Some(Source::Agent));
        assert_eq!(
            search_ids(&input, &state, &query, engine),
            [live_id(&input, 1, "w1:p1")]
        );
        state.set_provenance(Some(Source::Server));
        assert!(search_ids(&input, &state, &query, engine).is_empty());
        state.set_provenance(None);
        state.set_machine(MachineFilter::Machine(MachineKey::new("local").unwrap()));
        assert!(search_ids(&input, &state, &query, engine).is_empty());
        state.reset_filters();
        assert_eq!(search_ids(&input, &state, "@", engine).len(), 2);
        assert_eq!(query, original);
        let unicode = "  !RéVIEWER  東京 α  ";
        state.set_type(TypeFilter::Panes);
        assert!(search_ids(&input, &state, unicode, engine).is_empty());
        state.reset_filters();
        assert_eq!(unicode, "  !RéVIEWER  東京 α  ");
        assert_eq!(state.browse, saved);
        assert_eq!(state.expanded, expanded);
    }
}

#[test]
fn plain_fields_do_not_cross_titles_owners_aliases_or_legacy_breadcrumbs() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        let pane = live_id(&input, 0, "w4:p1");
        let destination = input
            .destinations
            .iter_mut()
            .find(|d| d.entry.key() == &pane)
            .unwrap();
        destination.entry.title = "violet".into();
        destination.entry.agent.as_mut().unwrap().aliases = vec![
            "copper".into(),
            "tulip".into(),
            "multi word alias".into(),
            "東京 α".into(),
        ];
        destination.entry.search_terms = vec!["forbidden copied breadcrumb".into()];
        let state = PickerState::new(&input);
        for query in [
            "violet local",
            "copper tulip",
            "forbidden copied breadcrumb",
        ] {
            assert!(
                !search_ids(&input, &state, query, engine).contains(&pane),
                "{engine}: {query}"
            );
        }
        for query in ["multi word alias", "東京 α"] {
            assert_eq!(search_ids(&input, &state, query, engine)[0], pane);
        }
    }
}

#[test]
fn hidden_qualified_ancestry_matches_without_same_id_cross_machine_leaks_or_cwd_fallback() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        let local = live_id(&input, 0, "w1");
        input
            .destinations
            .iter_mut()
            .find(|d| d.entry.key() == &local)
            .unwrap()
            .eligible = false;
        let mut state = PickerState::new(&input);
        state.set_type(TypeFilter::Panes);
        let results = search_ids(&input, &state, "gemini-cyber", engine);
        assert_eq!(
            results.into_iter().collect::<HashSet<_>>(),
            [live_id(&input, 0, "w1:p1"), live_id(&input, 0, "w1:p2")].into()
        );
        let missing = live_id(&input, 0, "w3:p1");
        assert!(input.destination(&missing).unwrap().entry.path.is_none());
        assert!(!search_ids(&input, &state, "/fixture", engine).contains(&missing));
        let mut unknown = input.clone();
        for d in &mut unknown.destinations {
            d.owner = Owner::Server(
                ServerScope::reported("local", "session-local", "epoch-local-1").unwrap(),
            );
        }
        state.set_machine(MachineFilter::Machine(MachineKey::new("local").unwrap()));
        assert!(search_ids(&unknown, &state, "@", engine).is_empty());
    }
}

#[test]
fn filtered_browse_keeps_only_required_unselectable_ancestors_and_saved_anchors() {
    let mut input = FixtureClient::sample().picker_input();
    for heading in &input.headings {
        input
            .groups
            .push((heading.owner.clone(), TaskKind::Sessions));
    }
    let mut state = PickerState::new(&input);
    expand_all(&input, &mut state);
    state.set_machine(MachineFilter::Machine(MachineKey::new("local").unwrap()));
    state.set_type(TypeFilter::Agents);
    state.set_provenance(Some(Source::Agent));
    let pane = live_id(&input, 0, "w4:p1");
    state.browse = Position {
        selection: Some(RowId::Destination(pane.clone())),
        viewport: Some(ViewportAnchor {
            row: RowId::Destination(pane.clone()),
            intra_row_offset: 2,
        }),
    };
    let saved = state.clone();
    let ranking = PickerConfig::default();
    let tree = search_project(&input, &state, "", &ranking);
    assert!(!tree.flat);
    assert_eq!(tree.rows.len(), 4);
    assert!(tree
        .rows
        .iter()
        .take(3)
        .all(|row| row.destination.is_none() && row.action().is_none()));
    assert_eq!(tree.rows[3].id, RowId::Destination(pane));
    for query in ["@", "  ", "reviewer"] {
        let flat = search_project(&input, &state, query, &ranking);
        assert!(flat.flat);
        assert!(flat
            .rows
            .iter()
            .all(|row| row.depth == 0 && row.destination.is_some()));
    }
    let restored = search_project(&input, &state, "", &ranking);
    assert_eq!(restored.selection, saved.browse.selection);
    assert_eq!(restored.viewport, saved.browse.viewport);
    assert_eq!(state.filters, saved.filters);
    assert_eq!(state.expanded, saved.expanded);
    state.browse.selection = Some(RowId::Destination(live_id(&input, 0, "w4")));
    assert!(search_project(&input, &state, "", &ranking)
        .selection
        .is_none());
    assert!(state.browse.selection.is_some());
    state.set_provenance(Some(Source::Zoxide));
    assert!(search_project(&input, &state, "", &ranking).rows.is_empty());
}

#[test]
fn named_tabs_remain_targets_only_under_all_and_pins_do_not_reorder_browse_machines() {
    let mut input = FixtureClient::sample().picker_input();
    input
        .pinned
        .insert(input.headings[1].machine.clone().unwrap());
    let mut state = PickerState::new(&input);
    expand_all(&input, &mut state);
    for engine in ["nucleo", "skim", "simple"] {
        assert!(search_ids(&input, &state, "main", engine).contains(&live_id(&input, 0, "w1:t1")));
    }
    let ranking = PickerConfig {
        source_priority_boost: i64::MIN,
        ..PickerConfig::default()
    };
    assert_eq!(
        search_project(&input, &state, "", &ranking).rows[0].id,
        RowId::Destination(input.headings[0].machine.clone().unwrap())
    );
    for kind in [
        TypeFilter::Machines,
        TypeFilter::Workspaces,
        TypeFilter::Panes,
        TypeFilter::Agents,
        TypeFilter::Projects,
        TypeFilter::Folders,
        TypeFilter::Sessions,
        TypeFilter::Actions,
    ] {
        state.set_type(kind);
        assert!(
            !search_ids(&input, &state, "main", "nucleo").contains(&live_id(&input, 0, "w1:t1"))
        );
    }
}

#[test]
fn pins_and_source_preferences_break_only_equal_relevance_ties() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        let agent = live_id(&input, 0, "w4:p1");
        let pane = live_id(&input, 0, "w4:p2");
        for d in &mut input.destinations {
            if [agent.clone(), pane.clone()].contains(d.entry.key()) {
                d.entry.title = "needle".into();
            }
        }
        let state = PickerState::new(&input);
        let ranking = PickerConfig {
            engine: engine.into(),
            source_order: vec!["agents".into()],
            source_priority_boost: i64::MAX,
            ..PickerConfig::default()
        };
        assert_eq!(
            search_project(&input, &state, "needle", &ranking).rows[0].id,
            RowId::Destination(agent.clone())
        );
        input.pinned.insert(pane.clone());
        assert_eq!(
            search_project(&input, &state, "needle", &ranking).rows[0].id,
            RowId::Destination(pane.clone())
        );
        input
            .destinations
            .iter_mut()
            .find(|d| d.entry.key() == &pane)
            .unwrap()
            .entry
            .title = "needle suffix".into();
        assert_eq!(
            search_project(&input, &state, "needle", &ranking).rows[0].id,
            RowId::Destination(agent)
        );
    }
}

#[test]
fn hash_and_alias_enumeration_order_never_choose_a_weaker_field_or_change_ties() {
    for engine in ["nucleo", "skim", "simple"] {
        let mut input = FixtureClient::sample().picker_input();
        for d in &mut input.destinations {
            if let Some(agent) = &mut d.entry.agent {
                agent.aliases = vec!["query suffix".into(), "query".into()];
                d.entry.provenance = ["query supporting", "query", "agents"]
                    .into_iter()
                    .map(|id| crate::model::ProviderId::new(id).unwrap())
                    .collect();
            }
        }
        let state = PickerState::new(&input);
        let expected = search_ids(&input, &state, "query", engine);
        for _ in 0..12 {
            for d in &mut input.destinations {
                if let Some(agent) = &mut d.entry.agent {
                    agent.aliases.reverse();
                }
                d.entry.provenance = d.entry.provenance.iter().cloned().collect();
            }
            assert_eq!(search_ids(&input, &state, "query", engine), expected);
        }
    }
}

#[test]
fn vanished_selection_never_chooses_a_neighbor_in_either_view() {
    let mut input = FixtureClient::sample().picker_input();
    let mut state = PickerState::new(&input);
    let id = state.browse.selection.clone().unwrap();
    state.flat = state.browse.clone();
    input.destinations.retain(|d| row_id(d) != id);
    assert!(browse(&input, &state).selection.is_none());
    let flat = project(
        &input,
        &state,
        Request {
            query: "gone",
            non_live_type: false,
            eligible: &input.eligible_ids(),
        },
    );
    assert!(flat.selection.is_none() && flat.viewport.is_none());
    assert_eq!(state.browse.selection, Some(id));
    // An unresolvable current observation cannot expand or select a guessed target.
    let new_state = PickerState::new(&input);
    assert!(new_state.browse.selection.is_none());
    assert_eq!(
        new_state.expanded,
        input.headings.iter().map(Heading::id).collect()
    );
}

#[test]
fn compression_matrix_rejects_missing_counts_and_duplicate_observations() {
    for naming in [TabNaming::Named, TabNaming::Unnamed, TabNaming::Unknown] {
        for complete in [false, true] {
            for expected in [0, 1, 2] {
                let mut input = FixtureClient::sample().picker_input();
                let tab = input
                    .destinations
                    .iter()
                    .find(|d| input.compressed(d))
                    .unwrap()
                    .clone();
                let target = tab.entry.target().unwrap();
                input.naming.insert(target.identity().clone(), naming);
                input.tabs.insert(
                    target.workspace().unwrap().clone(),
                    TabInventory { expected, complete },
                );
                let mut state = PickerState::new(&input);
                expand_all(&input, &mut state);
                let compressed = naming == TabNaming::Unnamed && complete && expected == 1;
                let tree = browse(&input, &state);
                assert_eq!(tree.rows.iter().any(|r| r.id == row_id(&tab)), !compressed);
                for pane in tree
                    .rows
                    .iter()
                    .filter(|r| r.ancestors.last() == Some(&row_id(&tab)))
                {
                    assert_eq!(pane.depth, if compressed { 2 } else { 3 });
                }
                let flat = project(
                    &input,
                    &state,
                    Request {
                        query: "1",
                        non_live_type: false,
                        eligible: &input.eligible_ids(),
                    },
                );
                assert!(flat.rows.iter().any(|r| r.id == row_id(&tab)));
                input.tabs.clear();
                assert!(!input.compressed(&tab));
            }
        }
    }
    let mut input = FixtureClient::sample().picker_input();
    let tab = input
        .destinations
        .iter()
        .find(|d| input.compressed(d))
        .unwrap()
        .clone();
    input.destinations.push(tab.clone());
    assert!(
        !input.compressed(&tab),
        "duplicate observations cannot prove single-tab inventory"
    );
}

#[test]
fn collision_fixture_input_keeps_machine_session_and_incarnation_in_live_row_ids() {
    let collision: serde_json::Value =
        serde_json::from_str(crate::navigation_test_support::COLLISIONS).unwrap();
    let mut ids = HashSet::new();
    let mut previous_state = None;
    for owner in collision["input"]["owners"].as_array().unwrap() {
        let mut fixture = FixtureClient::sample();
        fixture.current = None;
        fixture.machines.truncate(1);
        let machine = &mut fixture.machines[0];
        machine.owner = serde_json::from_value(owner.clone()).unwrap();
        machine.workspaces.truncate(1);
        let ws = &mut machine.workspaces[0];
        ws.id = collision["input"]["shared"]["workspace_id"]
            .as_str()
            .unwrap()
            .into();
        ws.tabs.truncate(1);
        ws.tabs[0].id = collision["input"]["shared"]["tab_id"]
            .as_str()
            .unwrap()
            .into();
        ws.tabs[0].panes.truncate(1);
        ws.tabs[0].panes[0].id = collision["input"]["shared"]["pane_id"]
            .as_str()
            .unwrap()
            .into();
        let input = fixture.picker_input();
        let mut state = PickerState::new(&input);
        expand_all(&input, &mut state);
        for row in browse(&input, &state)
            .rows
            .iter()
            .filter(|r| r.destination.is_some_and(|d| d.entry.target().is_some()))
        {
            assert!(ids.insert(row.id.clone()));
        }
        if let Some(old) = &previous_state {
            assert!(browse(&input, old).selection.is_none());
        }
        state.browse.selection = Some(row_id(
            input
                .destinations
                .iter()
                .find(|d| d.entry.destination_kind() == DestinationKind::Pane)
                .unwrap(),
        ));
        previous_state = Some(state);
    }
    assert_eq!(ids.len(), 12);
}
