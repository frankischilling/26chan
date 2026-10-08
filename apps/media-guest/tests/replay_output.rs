use board_media_guest::{
    decode_image,
    replay::{Event, EventKind, ReplayCandidate, parse_candidate},
    replay_output::{
        MAX_WIRE_BYTES, MAX_WIRE_EVENTS, ReplayOutputError, encode_untrusted_candidate,
    },
};
use proptest::prelude::*;

const EMPTY: &[u8] = include_bytes!("fixtures/replay/empty.tgkr");
const COMMANDS: &[u8] = include_bytes!("fixtures/replay/commands.tgkr");
const EMPTY_WIRE: &[u8] = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr");
const COMMANDS_WIRE: &[u8] =
    include_bytes!("../../../tests/media/fixtures/replay-wire/commands.ibr");

fn empty() -> ReplayCandidate {
    parse_candidate(EMPTY).unwrap()
}

fn reject(candidate: &ReplayCandidate, error: ReplayOutputError) {
    assert_eq!(encode_untrusted_candidate(candidate).unwrap_err(), error);
}

fn with_inner(kind: EventKind, payload_size: u32) -> ReplayCandidate {
    let mut candidate = empty();
    candidate.events.insert(
        1,
        Event {
            timestamp_ms: 456,
            kind,
        },
    );
    candidate.header.inflated_bytes += 5 + payload_size;
    candidate
}

#[test]
fn pinned_recorder_fixtures_match_independent_frozen_wire_bytes() {
    // The fixtures are transcribed with Python's struct/zlib, not either Rust
    // codec. Host tests independently decode these same bytes to source values.
    for (source, expected) in [(EMPTY, EMPTY_WIRE), (COMMANDS, COMMANDS_WIRE)] {
        let candidate = parse_candidate(source).unwrap();
        let wire = encode_untrusted_candidate(&candidate).unwrap();
        assert_eq!(wire, expected);
        assert_eq!(wire.len(), 256 + 16 * candidate.events.len());
        assert!(decode_image(&wire).is_err());
        assert!(decode_image(source).is_err());
    }
}

#[test]
fn literal_header_and_default_tool_control() {
    let wire = encode_untrusted_candidate(&empty()).unwrap();
    assert_eq!(
        &wire[..36],
        &[
            b'I', b'B', b'R', b'P', b'L', b'Y', b'0', b'1', 0, 1, 0, 64, 0, 1, 0, 0, 0, 0, 1, 32,
            0, 0, 0, 2, 2, 128, 1, 224, 255, 255, 255, 0, 0, 0, 1, 0,
        ]
    );
    assert_eq!(
        &wire[44..64],
        &[0, 9, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        &wire[64..88],
        &[
            1, 1, 4, 0, 0x3f, 0x80, 0, 0, 0x3c, 0x23, 0xd7, 0x0a, 0x3f, 0x80, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ]
    );
    assert_eq!(
        &wire[256..272],
        &[0, 0, 0, 0, 0, 0, 0, 123, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        &wire[272..],
        &[255, 0, 0, 0, 0, 0, 4, 99, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn all_tags_have_literal_payload_and_padding_controls() {
    use EventKind::*;
    let cases: Vec<(EventKind, u8, Vec<u8>)> = vec![
        (
            DrawStart {
                x: i16::MIN,
                y: i16::MAX,
                pressure: u16::MAX,
            },
            1,
            vec![128, 0, 127, 255, 255, 255],
        ),
        (
            Draw {
                x: -1,
                y: 2,
                pressure: 0,
            },
            2,
            vec![255, 255, 0, 2, 0, 0],
        ),
        (DrawCommit, 3, vec![]),
        (Undo, 4, vec![]),
        (Redo, 5, vec![]),
        (SetColor([3, 2, 1]), 6, vec![3, 2, 1]),
        (DrawStartNoPressure { x: -1, y: 2 }, 7, vec![255, 255, 0, 2]),
        (DrawNoPressure { x: -1, y: 2 }, 8, vec![255, 255, 0, 2]),
        (SetTool(8), 10, vec![8]),
        (SetToolSize(255), 11, vec![255]),
        (SetToolAlpha(-0.0), 12, vec![128, 0, 0, 0]),
        (SetToolSizeDynamics(true), 13, vec![1]),
        (SetToolAlphaDynamics(false), 14, vec![0]),
        (SetToolTip(255), 15, vec![255]),
        (PreserveAlpha(false), 16, vec![0]),
        (SetToolFlowDynamics(true), 17, vec![1]),
        (SetToolFlow(-2.0), 18, vec![192, 0, 0, 0]),
        (AddLayer, 20, vec![]),
        (DeleteLayers, 21, vec![]),
        (MoveLayers(255), 22, vec![255]),
        (MergeLayers, 23, vec![]),
        (ToggleLayerVisibility(0), 24, vec![0]),
        (SetActiveLayer(255), 25, vec![255]),
        (ToggleLayerSelection(0), 26, vec![0]),
        (SetSelectedLayersAlpha(0.75), 27, vec![63, 64, 0, 0]),
        (HistoryDummy, 254, vec![]),
    ];
    for (kind, tag, payload) in cases {
        let candidate = with_inner(kind, payload.len() as u32);
        let wire = encode_untrusted_candidate(&candidate).unwrap();
        let mut expected = [0; 16];
        expected[0] = tag;
        expected[4..8].copy_from_slice(&[0, 0, 1, 200]);
        expected[8..8 + payload.len()].copy_from_slice(&payload);
        assert_eq!(&wire[272..288], &expected, "tag {tag}");
    }
}

#[test]
fn public_source_header_and_source_lengths_are_revalidated() {
    for version in [[1, 9, 4], [0, 8, 4], [0, 9, 5]] {
        let mut candidate = empty();
        candidate.header.tegaki_version = version;
        reject(&candidate, ReplayOutputError::SourceHeader);
    }
    for version in [0, 2, 255] {
        let mut candidate = empty();
        candidate.header.format_version = version;
        reject(&candidate, ReplayOutputError::SourceHeader);
    }
    for length in [0, 188, 190, u32::MAX] {
        let mut candidate = empty();
        candidate.header.inflated_bytes = length;
        reject(&candidate, ReplayOutputError::SourceLength);
    }
}

#[test]
fn dimensions_and_initial_tool_are_revalidated() {
    for side in [0, 1025, u16::MAX] {
        let mut candidate = empty();
        candidate.metadata.width = side;
        reject(&candidate, ReplayOutputError::Metadata);
        candidate.metadata.width = 1;
        candidate.metadata.height = side;
        reject(&candidate, ReplayOutputError::Metadata);
    }
    for side in [1, 1024] {
        let mut candidate = empty();
        candidate.metadata.width = side;
        candidate.metadata.height = side;
        assert!(encode_untrusted_candidate(&candidate).is_ok());
    }
    for id in [0, 9, 255] {
        let mut candidate = empty();
        candidate.metadata.tool_id = id;
        reject(&candidate, ReplayOutputError::Tool);
        reject(
            &with_inner(EventKind::SetTool(id), 1),
            ReplayOutputError::Tool,
        );
    }
}

#[test]
fn exact_tool_set_is_required_and_order_is_canonicalized() {
    for count in [0, 1, 7, 9] {
        let mut candidate = empty();
        if count == 9 {
            candidate.tools.push(empty().tools.remove(0));
        } else {
            candidate.tools.truncate(count);
        }
        reject(&candidate, ReplayOutputError::Tool);
    }
    for id in [0, 2, 9, 255] {
        let mut candidate = empty();
        candidate.tools[0].id = id;
        reject(&candidate, ReplayOutputError::Tool);
    }
    let mut candidate = empty();
    candidate.tools.reverse();
    assert_eq!(encode_untrusted_candidate(&candidate).unwrap(), EMPTY_WIRE);
    assert_eq!(candidate.tools[0].id, 8);
}

#[test]
fn all_tool_flags_are_preserved_without_enabling_preserve_alpha() {
    for flags in 0..16_u8 {
        let mut candidate = empty();
        let tool = &mut candidate.tools[0];
        tool.size_dynamics = flags & 1 != 0;
        tool.alpha_dynamics = flags & 2 != 0;
        tool.use_preserve_alpha = flags & 4 != 0;
        tool.flow_dynamics = flags & 8 != 0;
        let wire = encode_untrusted_candidate(&candidate).unwrap();
        assert_eq!(wire[66], flags);
    }
}

#[test]
fn all_float_positions_reject_nonfinite_values() {
    for bits in [
        0x7f80_0000,
        0xff80_0000,
        0x7fc0_0000,
        0x7f80_0001,
        0xffc0_0001,
    ] {
        let value = f32::from_bits(bits);
        for tool_index in 0..8 {
            for field in 0..3 {
                let mut candidate = empty();
                let tool = &mut candidate.tools[tool_index];
                match field {
                    0 => tool.alpha = value,
                    1 => tool.step = value,
                    _ => tool.flow = value,
                }
                reject(&candidate, ReplayOutputError::NonFiniteFloat);
            }
        }
        for kind in [
            EventKind::SetToolAlpha(value),
            EventKind::SetToolFlow(value),
            EventKind::SetSelectedLayersAlpha(value),
        ] {
            reject(&with_inner(kind, 4), ReplayOutputError::NonFiniteFloat);
        }
    }
}

#[test]
fn finite_bits_and_source_extremes_are_not_semantically_normalized() {
    for bits in [0, 0x8000_0000, 1, 0x8000_0001, 0x7f7f_ffff, 0xff7f_ffff] {
        let value = f32::from_bits(bits);
        let mut candidate = with_inner(EventKind::SetToolFlow(value), 4);
        candidate.metadata.started_at_seconds = u32::MAX;
        candidate.metadata.ended_at_seconds = 0;
        candidate.events[0].timestamp_ms = u32::MAX;
        candidate.events[2].timestamp_ms = 0;
        candidate.tools[0].alpha = value;
        candidate.tools[0].step = value;
        candidate.tools[0].flow = value;
        candidate.tools[0].size = 0;
        candidate.tools[0].tip_id = i8::MIN;
        let wire = encode_untrusted_candidate(&candidate).unwrap();
        assert_eq!(&wire[36..44], &[255, 255, 255, 255, 0, 0, 0, 0]);
        assert_eq!(&wire[65..68], &[0, 4, 128]);
        for offset in [68, 72, 76, 280] {
            assert_eq!(&wire[offset..offset + 4], &bits.to_be_bytes());
        }
        assert_eq!(&wire[260..264], &[255; 4]);
        assert_eq!(&wire[292..296], &[0; 4]);
    }
}

#[test]
fn markers_must_be_outer_only() {
    let mut candidate = empty();
    candidate.events[0].kind = EventKind::Undo;
    reject(&candidate, ReplayOutputError::Markers);
    let mut candidate = empty();
    candidate.events[1].kind = EventKind::Redo;
    reject(&candidate, ReplayOutputError::Markers);
    for kind in [EventKind::Prelude, EventKind::Conclusion] {
        reject(&with_inner(kind, 0), ReplayOutputError::Markers);
    }
}

#[test]
fn event_count_is_checked_before_other_fields_or_output_allocation() {
    for count in [0, 1, MAX_WIRE_EVENTS + 1, 131_072] {
        let mut candidate = empty();
        candidate.events = (0..count)
            .map(|_| Event {
                timestamp_ms: 0,
                kind: EventKind::Undo,
            })
            .collect();
        // Invalid source/header fields make the order of checks observable.
        candidate.header.tegaki_version = [255; 3];
        candidate.header.inflated_bytes = u32::MAX;
        candidate.tools.clear();
        reject(&candidate, ReplayOutputError::EventCount);
    }
    let mut candidate = empty();
    candidate.events = (0..MAX_WIRE_EVENTS)
        .map(|index| Event {
            timestamp_ms: 0,
            kind: if index == 0 {
                EventKind::Prelude
            } else if index == MAX_WIRE_EVENTS - 1 {
                EventKind::Conclusion
            } else {
                EventKind::Undo
            },
        })
        .collect();
    candidate.header.inflated_bytes = 179 + 5 * MAX_WIRE_EVENTS as u32;
    let wire = encode_untrusted_candidate(&candidate).unwrap();
    assert_eq!(wire.len(), MAX_WIRE_BYTES);
    assert_eq!(&wire[16..24], &[0, 4, 1, 0, 0, 0, 64, 0]);
}

proptest! {
    #[test]
    fn finite_tool_bits_roundtrip_exactly(bits in any::<u32>()) {
        let mut candidate = empty();
        let value = f32::from_bits(bits);
        candidate.tools[0].alpha = value;
        let result = encode_untrusted_candidate(&candidate);
        if value.is_finite() {
            let wire = result.unwrap();
            prop_assert_eq!(&wire[68..72], &bits.to_be_bytes());
        } else {
            prop_assert_eq!(result.unwrap_err(), ReplayOutputError::NonFiniteFloat);
        }
    }
}
