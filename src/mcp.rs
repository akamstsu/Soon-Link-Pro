//! GUI-thread routing commands. No device enumeration, restart or audio IO here.
use crate::audio::Graph;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    expected_revision: String,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    routes: Vec<RoutePatch>,
    #[serde(default)]
    channels: Vec<ChannelPatch>,
    all_muted: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutePatch {
    input: String,
    output: String,
    on: Option<bool>,
    fader: Option<f32>,
    mute: Option<bool>,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Direction {
    Input,
    Output,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelPatch {
    direction: Direction,
    key: String,
    fader: Option<f32>,
    mute: Option<bool>,
}

fn revision(graph: &Graph, all_muted: bool) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    // Content revision covers edits from any source, including scenes, notes and
    // device reconciliation. It is opaque, not a persistent protocol identifier.
    serde_json::to_vec(graph)
        .unwrap_or_default()
        .hash(&mut hash);
    all_muted.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

pub fn state(
    graph: &Graph,
    all_muted: bool,
    running: bool,
    busy: bool,
    hardware_available: bool,
) -> Value {
    let channels = |input| {
        let nodes = if input { &graph.inputs } else { &graph.outputs };
        nodes.iter().enumerate().map(|(index,n)| {
            let group=Graph::card_group(&n.key);
            json!({"key":n.key,"name":graph.channel_name(input,index),"original_name":n.device_name,
                "group":group,"group_note":graph.card_name(input,&group),"visible":n.visible,
                "fader":n.fader,"mute":n.mute})
        }).collect::<Vec<_>>()
    };
    let routes=graph.routes.iter().enumerate().flat_map(|(i,row)|row.iter().enumerate().filter_map(move |(o,r)| {
        (r.on || r.mute || r.fader!=1.0).then(||json!({"input":graph.inputs[i].key,"output":graph.outputs[o].key,"on":r.on,"fader":r.fader,"mute":r.mute}))
    })).collect::<Vec<_>>();
    json!({"revision":revision(graph,all_muted),"engine_running":running,"engine_busy":busy,
        "hardware_available":hardware_available,"all_muted":all_muted,
        "inputs":channels(true),"outputs":channels(false),"routes":routes,
        "omitted_route_defaults":{"on":false,"fader":1.0,"mute":false},
        "fader_scale":"0 = silence; positions above 0 map to -40..0 dB. Hidden channels may carry audio."})
}

fn valid_fader(value: Option<f32>) -> Result<(), String> {
    if value.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
        Err("fader must be finite and between 0 and 1".into())
    } else {
        Ok(())
    }
}

/// Validate against a copy, then publish the whole graph. One bad target or value
/// leaves every existing route, fader, mute, note and visibility untouched.
pub fn patch(
    graph: &mut Graph,
    all_muted: &mut bool,
    arguments: Value,
) -> Result<(Value, bool), String> {
    let patch: Patch =
        serde_json::from_value(arguments).map_err(|e| format!("Invalid patch arguments: {e}"))?;
    let current = revision(graph, *all_muted);
    if patch.expected_revision != current {
        return Err(
            "Routing state changed. Call get_routing_state and review your patch again.".into(),
        );
    }
    if patch.routes.len() > 128 || patch.channels.len() > 128 {
        return Err("At most 128 route and 128 channel edits per call".into());
    }
    if patch.routes.is_empty() && patch.channels.is_empty() && patch.all_muted.is_none() {
        return Err("The patch contains no edits".into());
    }
    let mut next = graph.clone();
    let mut route_keys = HashSet::new();
    let mut channel_keys = HashSet::new();
    let mut route_changes = Vec::new();
    let mut channel_changes = Vec::new();
    for edit in patch.routes {
        if !route_keys.insert((edit.input.clone(), edit.output.clone())) {
            return Err("Duplicate route in patch".into());
        }
        if edit.on.is_none() && edit.fader.is_none() && edit.mute.is_none() {
            return Err("A route edit needs on, fader or mute".into());
        }
        valid_fader(edit.fader)?;
        let i = next
            .inputs
            .iter()
            .position(|n| n.key == edit.input)
            .ok_or_else(|| format!("Unknown input key: {}", edit.input))?;
        let o = next
            .outputs
            .iter()
            .position(|n| n.key == edit.output)
            .ok_or_else(|| format!("Unknown output key: {}", edit.output))?;
        let r = &mut next.routes[i][o];
        let before = serde_json::to_value(*r).map_err(|e| e.to_string())?;
        if let Some(v) = edit.on {
            r.on = v;
        }
        if let Some(v) = edit.fader {
            r.fader = v;
        }
        if let Some(v) = edit.mute {
            r.mute = v;
        }
        let after = serde_json::to_value(*r).map_err(|e| e.to_string())?;
        if before != after {
            route_changes.push(
                json!({"input":edit.input,"output":edit.output,"before":before,"after":after}),
            );
        }
    }
    for edit in patch.channels {
        let input = matches!(edit.direction, Direction::Input);
        if !channel_keys.insert((input, edit.key.clone())) {
            return Err("Duplicate channel in patch".into());
        }
        if edit.fader.is_none() && edit.mute.is_none() {
            return Err("A channel edit needs fader or mute".into());
        }
        valid_fader(edit.fader)?;
        let nodes = if input {
            &mut next.inputs
        } else {
            &mut next.outputs
        };
        let n = nodes
            .iter_mut()
            .find(|n| n.key == edit.key)
            .ok_or_else(|| format!("Unknown channel key in selected direction: {}", edit.key))?;
        let before = json!({"fader":n.fader,"mute":n.mute});
        if let Some(v) = edit.fader {
            n.fader = v;
        }
        if let Some(v) = edit.mute {
            n.mute = v;
        }
        let after = json!({"fader":n.fader,"mute":n.mute});
        if before != after {
            channel_changes.push(json!({"direction":if input {"input"} else {"output"},"key":edit.key,"before":before,"after":after}));
        }
    }
    let next_muted = patch.all_muted.unwrap_or(*all_muted);
    let proposed = revision(&next, next_muted);
    let changed = proposed != current;
    let result = json!({"dry_run":patch.dry_run,"applied":!patch.dry_run,"changed":changed,
        "revision":if patch.dry_run {&current} else {&proposed},"proposed_revision":proposed,
        "route_changes":route_changes,"channel_changes":channel_changes,
        "all_muted":{"before":*all_muted,"after":next_muted}});
    if !patch.dry_run && changed {
        *graph = next;
        *all_muted = next_muted;
    }
    Ok((result, changed && !patch.dry_run))
}

pub fn validate_empty(arguments: Value) -> Result<(), String> {
    serde_json::from_value::<Empty>(arguments)
        .map(|_| ())
        .map_err(|e| format!("Invalid state arguments: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(g: &Graph, routes: Value) -> Value {
        json!({"expected_revision":revision(g,false),"routes":routes})
    }
    #[test]
    fn atomic_patch_and_dry_run_preserve_unmentioned_state() {
        let mut g = Graph::new(2, 2);
        let mut muted = false;
        g.set_card_name(true, "hardware:0", "话筒");
        g.inputs[0].visible = false;
        let original = serde_json::to_value(&g).unwrap();
        let mut edit = args(
            &g,
            json!([{"input":"hardware:0","output":"hardware:1","on":true,"fader":0.75}]),
        );
        edit["dry_run"] = json!(true);
        let (preview, changed) = patch(&mut g, &mut muted, edit.clone()).unwrap();
        assert!(!changed);
        assert_eq!(preview["route_changes"].as_array().unwrap().len(), 1);
        assert_eq!(serde_json::to_value(&g).unwrap(), original);
        edit["dry_run"] = json!(false);
        assert!(patch(&mut g, &mut muted, edit.clone()).unwrap().1);
        assert!(g.routes[0][1].on);
        assert!(!g.routes[1][0].on);
        assert!(!g.inputs[0].visible);
        assert_eq!(g.card_name(true, "hardware:0"), Some("话筒"));
        assert!(patch(&mut g, &mut muted, edit).is_err()); // stale revision
        let before = serde_json::to_value(&g).unwrap();
        let invalid = args(
            &g,
            json!([{"input":"hardware:1","output":"hardware:1","on":true},{"input":"missing","output":"hardware:0","on":true}]),
        );
        assert!(patch(&mut g, &mut muted, invalid).is_err());
        assert_eq!(serde_json::to_value(&g).unwrap(), before);
    }
    #[test]
    fn keys_survive_reordering_and_disconnect_preserves_gain_and_mute() {
        let mut g = Graph::new(2, 2);
        let mut muted = false;
        g.inputs.swap(0, 1);
        g.routes.swap(0, 1);
        let edit = args(
            &g,
            json!([{"input":"hardware:0","output":"hardware:1","on":true,"mute":true,"fader":0.25}]),
        );
        patch(&mut g, &mut muted, edit).unwrap();
        assert!(g.routes[1][1].on);
        let edit = args(
            &g,
            json!([{"input":"hardware:0","output":"hardware:1","on":false}]),
        );
        patch(&mut g, &mut muted, edit).unwrap();
        assert!(!g.routes[1][1].on);
        assert!(g.routes[1][1].mute);
        assert_eq!(g.routes[1][1].fader, 0.25);
    }
    #[test]
    fn invalid_faders_unknown_fields_duplicates_and_directions_reject_whole_batch() {
        let mut g = Graph::new(2, 2);
        let mut muted = false;
        let original = serde_json::to_value(&g).unwrap();
        for entries in [
            json!([{"input":"hardware:0","output":"hardware:0","fader":1.01}]),
            json!([{"input":"hardware:0","output":"hardware:0","fader":-0.1}]),
            json!([{"input":"hardware:0","output":"hardware:0","on":true,"typo":true}]),
            json!([{"input":"hardware:0","output":"hardware:0","on":true},{"input":"hardware:0","output":"hardware:0","on":false}]),
            json!([{"input":"hardware:0","output":"hardware:0"}]),
        ] {
            let edit = args(&g, entries);
            assert!(patch(&mut g, &mut muted, edit).is_err());
        }
        let edit = json!({"expected_revision":revision(&g,false),"all_muted":true,"channels":[{"direction":"wrong","key":"hardware:0","mute":true}]});
        assert!(patch(&mut g, &mut muted, edit).is_err());
        assert!(!muted);
        assert_eq!(serde_json::to_value(&g).unwrap(), original);
        assert!(validate_empty(json!({"unexpected":1})).is_err());
    }
    #[test]
    fn explicit_channel_edits_master_mute_and_roundtrip() {
        let mut g = Graph::new(2, 2);
        let mut muted = false;
        let edit = json!({"expected_revision":revision(&g,false),"all_muted":true,"channels":[{"direction":"input","key":"hardware:0","mute":true,"fader":0.5}]});
        patch(&mut g, &mut muted, edit).unwrap();
        assert!(muted);
        assert!(g.inputs[0].mute);
        assert!(!g.inputs[1].mute);
        assert_eq!(g.inputs[1].fader, 1.0);
        let restored = Graph::from_value(serde_json::to_value(&g).unwrap()).unwrap();
        assert_eq!(restored.inputs[0].fader, 0.5);
        assert!(restored.inputs[0].mute);
        let snapshot = state(&g, muted, false, false, false);
        assert_eq!(snapshot["all_muted"], true);
        assert_eq!(snapshot["engine_running"], false);
    }
}
