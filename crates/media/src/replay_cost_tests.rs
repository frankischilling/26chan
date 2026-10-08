use super::*;
use crate::replay_state::check_core_v1;
use crate::replay_wire::{UntrustedReplay, UntrustedReplayEvent, decode};
use CostMetric as M;
use CostScope as S;
use Event as E;
use sha2::{Digest, Sha256};

macro_rules! fixture {
    ($name:literal) => {
        include_str!(concat!("../../../tests/media/fixtures/replay-cost/", $name))
    };
}

fn replay(events: &[Event]) -> UntrustedReplay {
    let mut replay = decode(include_bytes!(
        "../../../tests/media/fixtures/replay-wire/empty.ibr"
    ))
    .unwrap();
    replay.metadata.width = 32;
    replay.metadata.height = 24;
    replay.metadata.tool_id = 1;
    // Frozen actual source-constructor/header slots, not estimator defaults.
    for line in fixture!("tools.tsv").lines() {
        let v: Vec<_> = line.split('\t').collect();
        let id: u8 = v[0].parse().unwrap();
        replay.tools[usize::from(id - 1)] = UntrustedReplayTool {
            id,
            size: v[1].parse().unwrap(),
            alpha: v[2].parse().unwrap(),
            flow: v[3].parse().unwrap(),
            step: v[4].parse().unwrap(),
            tip_id: v[5].parse().unwrap(),
            use_preserve_alpha: v[6] == "1",
            size_dynamics: false,
            alpha_dynamics: false,
            flow_dynamics: false,
        };
    }
    replay.events = std::iter::once(E::Prelude)
        .chain(events.iter().copied())
        .chain(std::iter::once(E::Conclusion))
        .enumerate()
        .map(|(index, kind)| UntrustedReplayEvent {
            timestamp_ms: index as u32,
            kind,
        })
        .collect();
    replay
}

fn cost(replay: &UntrustedReplay) -> ReplayCost {
    estimate(
        &check_core_v1(replay).unwrap(),
        CostOptions {
            include_final_flatten: false,
        },
    )
    .unwrap()
}

fn numbers(line: &str) -> Vec<u64> {
    line.split('\t').map(|v| v.parse().unwrap()).collect()
}

fn events(plan: &str) -> Vec<Event> {
    plan.trim()
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| {
            let p: Vec<_> = e.split(',').collect();
            let int = |i: usize| p[i].parse::<i16>().unwrap();
            let byte = |i: usize| p[i].parse::<u8>().unwrap();
            let float = || p[1].parse::<f32>().unwrap();
            match p[0] {
                "DrawStartNoP" => E::DrawStartNoPressure {
                    x: int(1),
                    y: int(2),
                },
                "DrawNoP" => E::DrawNoPressure {
                    x: int(1),
                    y: int(2),
                },
                "DrawStart" => E::DrawStart {
                    x: int(1),
                    y: int(2),
                    pressure: p[3].parse().unwrap(),
                },
                "Draw" => E::Draw {
                    x: int(1),
                    y: int(2),
                    pressure: p[3].parse().unwrap(),
                },
                "DrawCommit" => E::DrawCommit,
                "SetTool" => E::SetTool(byte(1)),
                "SetToolSize" => E::SetToolSize(byte(1)),
                "SetToolAlpha" => E::SetToolAlpha(float()),
                "SetToolTip" => E::SetToolTip(byte(1)),
                "SetSelectedLayersAlpha" => E::SetSelectedLayersAlpha(float()),
                "AddLayer" => E::AddLayer,
                "SetActiveLayer" => E::SetActiveLayer(byte(1)),
                "ToggleLayerVisibility" => E::ToggleLayerVisibility(byte(1)),
                "ToggleLayerSelection" => E::ToggleLayerSelection(byte(1)),
                "HistoryDummy" => E::HistoryDummy,
                "Undo" => E::Undo,
                "Redo" => E::Redo,
                other => panic!("unrecognized frozen plan event {other}"),
            }
        })
        .collect()
}

fn source_metric(name: &str) -> CostMetric {
    match name {
        "typed_backing_bytes" | "typedAllocatedBytes" => M::TypedBackingBytes,
        "typed_copy_bytes" | "typedCopyBytes" => M::TypedCopyBytes,
        "typed_clear_bytes" | "typedClearBytes" => M::TypedClearBytes,
        "shape_loop_units" | "shapeWorkUnits" => M::ShapeLoopUnits,
        "shape_flood_scalar_pushes" | "shapeStackScalarPushes" => M::ShapeFloodScalarPushes,
        "brush_leaf_units" | "rasterLeafUnits" => M::BrushLeafUnits,
        "brush_loop_units" | "rasterAllLoopUnits" => M::BrushLoopUnits,
        "tone_map_cells" | "toneLeafUnits" => M::ToneMapCells,
        "tone_loop_units" => M::ToneLoopUnits,
        "canvas_surface_pixels" => M::CanvasSurfacePixels,
        "preview_surface_pixels" => M::PreviewSurfacePixels,
        "canvas_get_calls" => M::CanvasGetCalls,
        "canvas_get_pixels" => M::CanvasGetPixels,
        "canvas_fill_calls" => M::CanvasFillCalls,
        "canvas_fill_pixels" => M::CanvasFillPixels,
        "canvas_put_calls" | "canvasPutImageDataCalls" => M::CanvasPutCalls,
        "canvas_put_input_pixels" => M::CanvasPutInputPixels,
        "canvas_put_dirty_pixels" => M::CanvasPutDirtyPixels,
        "preview_draw_calls" | "previewDrawCalls" => M::PreviewDrawCalls,
        "preview_source_pixels" => M::PreviewSourcePixels,
        "preview_destination_pixels" => M::PreviewDestinationPixels,
        "preview_clear_calls" => M::PreviewClearCalls,
        "preview_clear_pixels" => M::PreviewClearPixels,
        "flatten_draw_calls" => M::FlattenDrawCalls,
        "flatten_source_pixels" => M::FlattenSourcePixels,
        "flatten_destination_pixels" => M::FlattenDestinationPixels,
        _ => panic!("unknown frozen source metric {name}"),
    }
}

#[test]
fn frozen_source_evidence_and_extractor_are_pinned() {
    macro_rules! pin {
        ($name:literal, $sha:literal) => {
            assert_eq!(
                format!(
                    "{:x}",
                    Sha256::digest(include_bytes!(concat!(
                        "../../../tests/media/fixtures/replay-cost/",
                        $name
                    )))
                ),
                $sha,
                "fixture or generator changed: {}",
                $name,
            );
        };
    }
    pin!(
        "source/shapes.json",
        "dbc102099b5e6315e87939739997a9657b5be66057bda656db076d485705d123"
    );
    pin!(
        "source/segments.json",
        "e3ac631fb0700c962683a6eeca5e7e102ba7945fea7674fdb4da3e9aac772035"
    );
    pin!(
        "source/state.json",
        "f34b673385bb9b5e4d7c30ff17c7a749af836d9c297dfe109d7db747c69f3fee"
    );
    pin!(
        "source/state-cost.json",
        "3dd282cf2afaf0961389ffa8fdc211d55713330ef0c6c8315b6aa3758a9b814f"
    );
    pin!(
        "source/ledger.json",
        "5fc240231dd799de7302117afbd521ee907231115403499841a2135b9f1a54b7"
    );
    pin!(
        "generate.py",
        "108b1a8e648e114b823fadc90bef9689a67b039524094f6e4cec6132b952b7c5"
    );
    pin!(
        "shapes.tsv",
        "e4eae7f839d3cfa9f149a4764506fd0c17ecbeb5045788aeef4c07d98f523f53"
    );
    pin!(
        "segments.tsv",
        "518a377d0553f12f322b7feb6cf69a1be1c3e96adec9dffe57971982ca6e37d6"
    );
    pin!(
        "tools.tsv",
        "bf129d8c24ff5df5dc4182b387ee05cd40e9a378213df55caaf505c6eab82d4e"
    );
    pin!(
        "ledger-events.txt",
        "a740eb38f99dd43a699325e088262c5a6b1e3ec3d24446d7742a7734047f096e"
    );
    pin!(
        "ledger-counters.tsv",
        "80533346a4e2ba594479117a830246120e5260f2ee4f58a8b4b99a482a592e2a"
    );
    pin!(
        "state-scenarios.tsv",
        "9a0dcdaa56184d5138e8c8b4cbbdb0a579f199d341d2b26dd29ed4ada77f9c67"
    );
    pin!(
        "state-counters.tsv",
        "dc456a57fb24b63a83f29272714faf2f95c1430901a6b9ca52a907a09abde4e0"
    );
    pin!(
        "tone.tsv",
        "915f4ee99b4f8357918dbac7dc3f72299be792b7b8aa26de84f5f446bb9bac94"
    );
}

#[test]
fn encloses_all_512_source_shapes_without_generating_them() {
    let mut count = 0;
    for line in fixture!("shapes.tsv").lines() {
        let v = numbers(line);
        let mut r = replay(&[E::DrawStartNoPressure { x: 0, y: 0 }, E::DrawCommit]);
        r.metadata.tool_id = v[0] as u8;
        r.tools[v[0] as usize - 1].tip_id = v[1] as i8;
        r.tools[v[0] as usize - 1].size = v[2] as u8;
        let c = cost(&r);
        let setup = c.counters(S::Initialization);
        let shape_bytes =
            setup.typed_backing_bytes - 12 * 32 * 24 - if v[0] == 5 { 16 * 32 * 24 } else { 0 };
        assert!(shape_bytes >= v[4], "{line}");
        assert!(setup.shape_loop_units >= v[5], "{line}");
        assert!(setup.shape_flood_scalar_pushes >= v[6], "{line}");
        assert_eq!(setup.shape_regenerations, 1);
        // Actual generated kernel B, not the supersampling envelope H.
        assert_eq!(c.counters(S::Total).canvas_put_dirty_pixels, v[3] * v[3]);
        count += 1;
    }
    assert_eq!(count, 512);
}

#[test]
fn encloses_all_672_independent_source_segments() {
    let mut count = 0;
    for line in fixture!("segments.tsv").lines() {
        let v = numbers(line);
        let start = E::DrawStartNoPressure {
            x: v[3] as i16,
            y: v[4] as i16,
        };
        let draw = E::Draw {
            x: v[5] as i16,
            y: v[6] as i16,
            pressure: 65535,
        };
        let mut a = replay(&[start, E::DrawCommit]);
        a.metadata.width = 32;
        a.metadata.height = 32;
        a.metadata.tool_id = v[0] as u8;
        a.tools[v[0] as usize - 1].tip_id = v[1] as i8;
        a.tools[v[0] as usize - 1].size = v[2] as u8;
        let before = cost(&a);
        let mut b = replay(&[start, draw, E::DrawCommit]);
        b.metadata = a.metadata;
        b.tools = a.tools;
        let after = cost(&b);
        let old = before.counters(S::Total);
        let new = after.counters(S::Total);
        assert!(
            new.brush_leaf_units - old.brush_leaf_units >= v[9],
            "{line}"
        );
        assert!(
            new.brush_loop_units - old.brush_loop_units >= v[10],
            "{line}"
        );
        assert_eq!(new.typed_backing_bytes, old.typed_backing_bytes);
        assert_eq!(new.canvas_put_calls - old.canvas_put_calls, 1);
        count += 1;
    }
    assert_eq!(count, 672);
}

#[test]
fn sixty_stroke_plan_uses_source_counters_not_oracle_estimates() {
    let plan = events(fixture!("ledger-events.txt"));
    assert_eq!(plan.len(), 190);
    let r = replay(&plan);
    let result = cost(&r);
    let total = result.counters(S::Total);
    for line in fixture!("ledger-counters.tsv").lines() {
        let (name, measured) = line.split_once('\t').unwrap();
        let observed = measured.parse::<u64>().unwrap();
        let metric = source_metric(name);
        assert!(total.get(metric) >= observed, "{name}: {observed}");
        if matches!(
            metric,
            M::TypedCopyBytes
                | M::TypedClearBytes
                | M::ToneMapCells
                | M::CanvasPutCalls
                | M::PreviewDrawCalls
        ) {
            assert_eq!(total.get(metric), observed, "{name}");
        }
    }
    assert_eq!(total.events, 192); // Source plan plus both wire markers.
    assert_eq!(total.committed_strokes, 60);
    assert_eq!(
        check_core_v1(&r)
            .unwrap()
            .final_state()
            .undo_history()
            .len(),
        50
    );
}

#[test]
fn encloses_all_six_source_layer_history_scenarios() {
    let mut scenarios = 0;
    for line in fixture!("state-scenarios.tsv").lines() {
        let row: Vec<_> = line.split('\t').collect();
        let flatten = row[1] == "1";
        let r = replay(&events(row[2]));
        let checked = check_core_v1(&r).unwrap();
        let result = estimate(
            &checked,
            CostOptions {
                include_final_flatten: flatten,
            },
        )
        .unwrap();
        let scope = if flatten { S::FinalFlatten } else { S::Total };
        for counter in fixture!("state-counters.tsv").lines() {
            let c: Vec<_> = counter.split('\t').collect();
            if c[0] != row[0] {
                continue;
            }
            let metric = source_metric(c[1]);
            let observed: u64 = c[2].parse().unwrap();
            let modeled = result.counters(scope).get(metric);
            assert!(
                modeled >= observed,
                "{} {:?}: {modeled} < {observed}",
                row[0],
                metric
            );
            // Exact counters except intentional shape/preview/brush envelopes
            // and charging every Alpha/Dummy restore as Draw.
            if flatten
                || matches!(
                    metric,
                    M::TypedClearBytes
                        | M::CanvasGetCalls
                        | M::CanvasGetPixels
                        | M::CanvasFillCalls
                        | M::CanvasFillPixels
                        | M::CanvasSurfacePixels
                )
            {
                assert_eq!(modeled, observed, "{} {:?}", row[0], metric);
            }
        }
        scenarios += 1;
    }
    assert_eq!(scenarios, 6);
}

#[test]
fn source_tone_maps_are_charged_once_even_at_zero_alpha() {
    let measured = fixture!("tone.tsv")
        .lines()
        .find(|line| line.starts_with("select-tone-first\t"))
        .unwrap();
    let values = numbers(measured.split_once('\t').unwrap().1);
    let mut r = replay(&[
        E::SetTool(5),
        E::SetToolAlpha(0.0),
        E::SetTool(1),
        E::SetTool(5),
        E::SetToolSize(8),
        E::SetToolAlpha(1.0),
        E::DrawStartNoPressure { x: 0, y: 0 },
        E::DrawCommit,
    ]);
    r.metadata.width = 64;
    r.metadata.height = 48;
    r.tools[4].alpha = 0.0;
    let c = cost(&r);
    assert_eq!(c.counters(S::Total).tone_map_cells, values[0]);
    assert_eq!(c.counters(S::Total).tone_loop_units, values[1]);
    assert_eq!(c.counters(S::Initialization).tone_map_cells, 0);
    r.metadata.tool_id = 5;
    let c = cost(&r);
    assert_eq!(c.counters(S::Initialization).tone_map_cells, values[0]);
    assert_eq!(c.counters(S::Total).tone_map_cells, values[0]);
    assert_eq!(c.counters(S::Total).tone_loop_units, values[1]);
}

#[test]
fn repeated_settings_regenerate_and_retain_each_tools_size_and_tip() {
    let r = replay(&[
        E::SetTool(2),
        E::SetToolSize(64),
        E::SetTool(2),
        E::SetToolSize(64),
        E::SetTool(8),
        E::SetToolSize(64),
        E::SetToolTip(1),
        E::SetToolTip(1),
        E::SetTool(1),
        E::SetToolSize(2),
        E::SetTool(8),
        E::DrawStartNoPressure { x: 0, y: 0 },
        E::DrawCommit,
        E::SetTool(2),
        E::DrawStartNoPressure { x: 0, y: 0 },
        E::DrawCommit,
        E::SetTool(1),
        E::DrawStartNoPressure { x: 0, y: 0 },
        E::DrawCommit,
    ]);
    let c = cost(&r);
    let total = c.counters(S::Total);
    assert_eq!(total.shape_regenerations, 14);
    assert_eq!(total.canvas_put_dirty_pixels, 66 * 66 + 66 * 66 + 2 * 2);
    assert_eq!(c.counters(S::PeakEvent).shape_loop_units, 4_460_544);
    assert_eq!(total.typed_copy_bytes, 3 * 8 * 32 * 24);
}

#[test]
fn repeated_cheap_looking_events_accumulate_without_a_hidden_threshold() {
    let events = vec![E::SetTool(2); 16_382];
    let mut r = replay(&events);
    r.tools[1].size = 64;
    let c = cost(&r);
    assert_eq!(c.counters(S::Total).events, 16_384);
    assert_eq!(c.counters(S::Total).shape_regenerations, 16_383);
    assert_eq!(
        c.counters(S::Total).shape_loop_units,
        9_216 + 16_382 * 4_460_544
    );
    assert!(c.counters(S::Total).shape_loop_units > u64::from(u32::MAX));
    assert_eq!(c.counters(S::PeakEvent).shape_loop_units, 4_460_544);
    assert!(c.check_limits(&[]).is_ok());
}

#[test]
fn a_hundred_history_restores_each_allocate_copy_and_preview() {
    let mut plan = vec![E::DrawStartNoPressure { x: 0, y: 0 }, E::DrawCommit];
    for _ in 0..50 {
        plan.extend([E::Undo, E::Redo]);
    }
    plan.extend(std::iter::repeat_n(E::HistoryDummy, 50));
    let r = replay(&plan);
    let c = cost(&r);
    let total = c.counters(S::Total);
    assert_eq!(total.history_restore_envelopes, 100);
    assert_eq!(total.typed_copy_bytes, 102 * 4 * 32 * 24);
    assert_eq!(total.typed_clear_bytes, 8 * 32 * 24);
    assert_eq!(
        total.typed_backing_bytes - c.counters(S::Initialization).typed_backing_bytes,
        102 * 4 * 32 * 24
    );
    assert_eq!(total.canvas_put_calls, 101);
    assert_eq!(total.preview_draw_calls, 101);
    assert_eq!(total.canvas_get_calls, 1);
    let checked = check_core_v1(&r).unwrap();
    assert_eq!(checked.final_state().undo_history().len(), 50);
    assert!(checked.final_state().pending_action().is_some());
}

#[test]
fn alpha_dummy_restores_are_explicit_conservative_overcounts() {
    let r = replay(&[
        E::SetSelectedLayersAlpha(0.25),
        E::HistoryDummy,
        E::Undo,
        E::SetSelectedLayersAlpha(0.75),
        E::Undo,
        E::Redo,
        E::Redo,
    ]);
    let c = cost(&r);
    let total = c.counters(S::Total);
    assert_eq!(total.history_restore_envelopes, 4);
    assert_eq!(total.typed_copy_bytes, 4 * 4 * 32 * 24);
    assert_eq!(total.typed_clear_bytes, 0);
    assert_eq!(total.brush_loop_units, 0);
    assert_eq!(total.preview_draw_calls, 4);
}

#[test]
fn metadata_only_events_still_count_but_do_not_regenerate_shapes() {
    let r = replay(&[
        E::SetColor([1, 2, 3]),
        E::SetToolAlpha(0.0),
        E::SetToolFlow(0.0),
        E::PreserveAlpha(true),
        E::SetActiveLayer(0),
        E::ToggleLayerSelection(1),
        E::ToggleLayerSelection(1),
        E::ToggleLayerVisibility(1),
        E::SetSelectedLayersAlpha(0.0),
        E::HistoryDummy,
    ]);
    let c = cost(&r);
    let total = c.counters(S::Total);
    assert_eq!(total.events, 12);
    assert_eq!(total.shape_regenerations, 1);
    assert_eq!(
        total.typed_backing_bytes,
        c.counters(S::Initialization).typed_backing_bytes
    );
    assert_eq!(total.canvas_put_calls, 0);
    assert_eq!(
        c.counters(S::PeakEvent),
        &CostCounters {
            events: 1,
            ..CostCounters::default()
        }
    );
}

#[test]
fn pressure_variants_and_zero_alpha_never_discount_work() {
    for (id, tip) in [
        (1, 0),
        (2, 0),
        (3, 0),
        (5, 0),
        (7, 0),
        (8, 0),
        (8, 1),
        (8, 2),
    ] {
        let make = |start, draw, alpha| {
            let mut r = replay(&[start, draw, E::DrawCommit]);
            r.metadata.tool_id = id;
            r.tools[usize::from(id - 1)].tip_id = tip;
            r.tools[usize::from(id - 1)].alpha = alpha;
            cost(&r)
        };
        let no_pressure = make(
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::DrawNoPressure { x: 31, y: 23 },
            1.0,
        );
        let pressure = make(
            E::DrawStart {
                x: 0,
                y: 0,
                pressure: 0,
            },
            E::Draw {
                x: 31,
                y: 23,
                pressure: 65535,
            },
            1.0,
        );
        let zero_alpha = make(
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::Draw {
                x: 31,
                y: 23,
                pressure: 0,
            },
            0.0,
        );
        assert_eq!(no_pressure, pressure);
        assert_eq!(no_pressure, zero_alpha);
    }
}

#[test]
fn corrected_blur_metric_and_unclipped_rectangle_are_separate() {
    let mut r = replay(&[
        E::DrawStartNoPressure { x: 8, y: 8 },
        E::DrawNoPressure { x: 16, y: 8 },
        E::DrawCommit,
    ]);
    r.metadata.tool_id = 7;
    r.metadata.height = 32;
    r.tools[6].size = 1;
    let c = cost(&r);
    // Source counterexample: segment leaf=107, selected loops=373. The
    // independently derived envelopes are 117 and 407, plus start=12/46.
    assert_eq!(c.counters(S::Total).brush_leaf_units, 129);
    assert_eq!(c.counters(S::Total).brush_loop_units, 453);
    assert_eq!(c.counters(S::PeakEvent).brush_loop_units, 407);
    let mut r = replay(&[
        E::DrawStartNoPressure { x: 0, y: 0 },
        E::DrawNoPressure { x: 7, y: 7 },
        E::DrawCommit,
    ]);
    r.metadata.width = 8;
    r.metadata.height = 8;
    r.metadata.tool_id = 7;
    r.tools[6].size = 64;
    let c = cost(&r);
    assert_eq!(c.counters(S::Total).canvas_put_dirty_pixels, 4096 + 5041);
    assert_eq!(c.counters(S::Total).canvas_put_input_pixels, 128);
    assert!(c.counters(S::PeakEvent).brush_loop_units > 4 * 5041);
}

#[test]
fn flatten_is_optional_once_and_observes_final_visibility() {
    let r = replay(&[E::AddLayer, E::AddLayer, E::ToggleLayerVisibility(2)]);
    let checked = check_core_v1(&r).unwrap();
    let no = cost(&r);
    let yes = estimate(
        &checked,
        CostOptions {
            include_final_flatten: true,
        },
    )
    .unwrap();
    assert_eq!(no.counters(S::FinalFlatten), &CostCounters::default());
    assert_eq!(no.counters(S::Total).flatten_draw_calls, 0);
    assert_eq!(yes.counters(S::FinalFlatten).flatten_draw_calls, 3);
    assert_eq!(yes.counters(S::FinalFlatten).canvas_surface_pixels, 768);
    assert_eq!(yes.counters(S::FinalFlatten).flatten_source_pixels, 2304);
    assert_eq!(
        yes.counters(S::Total).typed_backing_bytes,
        no.counters(S::Total).typed_backing_bytes
    );
    assert_eq!(yes.counters(S::PeakEvent), no.counters(S::PeakEvent));
    assert_eq!(yes.counters(S::Total).events, no.counters(S::Total).events);
    assert_eq!(
        yes,
        estimate(
            &checked,
            CostOptions {
                include_final_flatten: true
            }
        )
        .unwrap()
    );
}

#[test]
fn initialization_is_not_a_draw_event_peak_or_implicit_million_unit_limit() {
    let mut r = replay(&[]);
    r.metadata.width = 1024;
    r.metadata.height = 1024;
    r.metadata.tool_id = 5;
    for tool in &mut r.tools {
        tool.size = 64;
    }
    let c = cost(&r);
    assert_eq!(c.counters(S::Initialization).shape_regenerations, 1);
    assert_eq!(c.counters(S::Initialization).shape_loop_units, 4_460_544);
    assert_eq!(
        c.counters(S::Initialization).typed_backing_bytes,
        28 * 1_048_576 + 836_352
    );
    assert_eq!(c.counters(S::PeakEvent).shape_loop_units, 0);
    let event_limit = CostLimit {
        scope: S::PeakEvent,
        metric: M::ShapeLoopUnits,
        maximum: 1_000_000,
    };
    assert_eq!(c.check_limits(&[event_limit]), Ok(()));
    let setup_limit = CostLimit {
        scope: S::Initialization,
        ..event_limit
    };
    let failure = c.check_limits(&[setup_limit]).unwrap_err();
    assert_eq!(failure.value, 4_460_544);
    assert_eq!(failure.metric, M::ShapeLoopUnits);
    assert_eq!(failure.scope, S::Initialization);
}

#[test]
fn explicit_limits_check_each_unit_at_exact_inclusive_boundaries() {
    let r = replay(&events(fixture!("ledger-events.txt")));
    let c = estimate(
        &check_core_v1(&r).unwrap(),
        CostOptions {
            include_final_flatten: true,
        },
    )
    .unwrap();
    for scope in [S::Total, S::PeakEvent, S::Initialization, S::FinalFlatten] {
        for &metric in CostMetric::ALL {
            let value = c.counters(scope).get(metric);
            let limit = CostLimit {
                scope,
                metric,
                maximum: value,
            };
            assert_eq!(c.check_limits(&[limit]), Ok(()));
            assert_eq!(
                c.check_limits(&[CostLimit {
                    maximum: value + 1,
                    ..limit
                }]),
                Ok(())
            );
            if value > 0 {
                let error = c
                    .check_limits(&[CostLimit {
                        maximum: value - 1,
                        ..limit
                    }])
                    .unwrap_err();
                assert_eq!(
                    error,
                    CostLimitExceeded {
                        scope,
                        metric,
                        value,
                        maximum: value - 1
                    }
                );
                assert!(error.to_string().contains(&value.to_string()));
                assert!(error.to_string().contains(&format!("{metric:?}")));
            }
        }
    }
}

#[test]
fn every_metric_arithmetic_overflow_fails_closed_without_partial_update() {
    for &metric in CostMetric::ALL {
        let error = ReplayCostError::Overflow { metric };
        assert_eq!(add(metric, u64::MAX, 1), Err(error));
        assert_eq!(mul(metric, u64::MAX, 2), Err(error));
        assert_eq!(sum(metric, &[u64::MAX, 1]), Err(error));
        assert_eq!(add(metric, u64::MAX, 0), Ok(u64::MAX));
        assert_eq!(mul(metric, u64::MAX, 1), Ok(u64::MAX));
    }
    let mut counters = CostCounters {
        events: 1,
        typed_copy_bytes: u64::MAX,
        ..CostCounters::default()
    };
    let before = counters;
    let extra = CostCounters {
        events: 1,
        typed_copy_bytes: 1,
        ..CostCounters::default()
    };
    assert_eq!(
        counters.checked_accumulate(&extra),
        Err(ReplayCostError::Overflow {
            metric: M::TypedCopyBytes
        })
    );
    assert_eq!(counters, before);
    // Current bounded candidates cannot reach u64 overflow; directly exercise
    // future-size failures in the same arithmetic paths without forging the
    // StateCheckedCandidate or pretending it accepted impossible dimensions.
    let r = replay(&[]);
    let mut model = Model {
        area: u64::MAX,
        height: u64::MAX,
        tools: r.tools,
        selected: 4,
        tone_maps_built: false,
        position: (0, 0),
    };
    assert!(matches!(
        model.tone_maps(&mut CostCounters::default()),
        Err(ReplayCostError::Overflow { .. })
    ));
    assert!(matches!(
        model.snapshot(&mut CostCounters::default()),
        Err(ReplayCostError::Overflow { .. })
    ));
    model.selected = 6;
    assert!(matches!(
        model.paint(&mut CostCounters::default(), u64::MAX, 1, false),
        Err(ReplayCostError::Overflow { .. })
    ));
}

#[test]
fn costing_borrows_without_changing_candidate_or_state() {
    let r = replay(&[
        E::AddLayer,
        E::DrawStartNoPressure { x: 2, y: 2 },
        E::DrawCommit,
        E::Undo,
    ]);
    let before = format!("{r:?}");
    let checked = check_core_v1(&r).unwrap();
    let state_before = format!("{:?}", checked.final_state());
    let one = estimate(
        &checked,
        CostOptions {
            include_final_flatten: false,
        },
    )
    .unwrap();
    let two = estimate(
        &checked,
        CostOptions {
            include_final_flatten: false,
        },
    )
    .unwrap();
    assert_eq!(one, two);
    assert_eq!(format!("{:?}", checked.final_state()), state_before);
    assert_eq!(format!("{r:?}"), before);
}
