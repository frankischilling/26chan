use board_media_guest::{
    decode_image,
    replay::{self, Event, EventKind, ReplayError, parse_candidate},
};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use proptest::prelude::*;
use std::io::{Read, Write};

const EMPTY: &[u8] = include_bytes!("fixtures/replay/empty.tgkr");
const COMMANDS: &[u8] = include_bytes!("fixtures/replay/commands.tgkr");
const TOOL_START: usize = 23;
const EVENT_COUNT: usize = 175;
const EVENTS: usize = 179;

fn body(file: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    DeflateDecoder::new(&file[12..])
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(
        bytes.len(),
        u32::from_be_bytes(file[4..8].try_into().unwrap()) as usize
    );
    bytes
}
fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).unwrap();
    let mut file = b"TGK\x01".to_vec();
    file.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    file.extend_from_slice(&[0, 9, 4, 1]);
    file.extend_from_slice(&encoder.finish().unwrap());
    file
}
fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn reject(bytes: &[u8], error: ReplayError) {
    assert_eq!(parse_candidate(bytes).unwrap_err(), error);
}
fn event_body(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = body(EMPTY);
    set_u32(&mut bytes, EVENT_COUNT, 3);
    let mut event = vec![tag];
    event.extend_from_slice(&200_u32.to_be_bytes());
    event.extend_from_slice(payload);
    bytes.splice(EVENTS + 5..EVENTS + 5, event);
    bytes
}

#[test]
fn original_recorder_metadata_tools_and_all_known_events() {
    let empty = parse_candidate(EMPTY).unwrap();
    assert_eq!(empty.header.tegaki_version, [0, 9, 4]);
    assert_eq!(empty.header.format_version, 1);
    assert_eq!(empty.metadata.started_at_seconds, 1_690_000_000);
    assert_eq!(empty.metadata.ended_at_seconds, 1_690_000_013);
    assert_eq!((empty.metadata.width, empty.metadata.height), (640, 480));
    assert_eq!(empty.metadata.background, [255; 3]);
    assert_eq!(empty.metadata.color, [0; 3]);
    assert_eq!(empty.metadata.tool_id, 1);
    assert_eq!(empty.tools.len(), 8);
    for (tool, id) in empty.tools.iter().zip(1..=8) {
        assert_eq!(tool.id, id);
        assert_eq!(tool.flow, 1.0);
        assert_eq!(tool.tip_id, 0);
        assert!(!tool.size_dynamics && !tool.alpha_dynamics && !tool.flow_dynamics);
        assert_eq!(tool.use_preserve_alpha, matches!(id, 1 | 2 | 3 | 5));
    }
    assert_eq!(empty.tools[0].step, 0.01_f32);
    assert_eq!(empty.tools[3].step, 100.0);
    assert_eq!(
        empty.events,
        [
            Event {
                timestamp_ms: 123,
                kind: EventKind::Prelude
            },
            Event {
                timestamp_ms: 1123,
                kind: EventKind::Conclusion
            },
        ]
    );
    let commands = parse_candidate(COMMANDS).unwrap();
    let expected = [
        EventKind::Prelude,
        EventKind::SetColor([12, 34, 56]),
        EventKind::SetTool(2),
        EventKind::SetToolSize(16),
        EventKind::SetToolAlpha(0.5),
        EventKind::SetToolSizeDynamics(true),
        EventKind::SetToolAlphaDynamics(true),
        EventKind::PreserveAlpha(true),
        EventKind::SetToolFlowDynamics(true),
        EventKind::SetToolFlow(0.25),
        EventKind::DrawStart {
            x: -3,
            y: 9,
            pressure: 0,
        },
        EventKind::Draw {
            x: 12,
            y: -2,
            pressure: 65535,
        },
        EventKind::DrawCommit,
        EventKind::Undo,
        EventKind::Redo,
        EventKind::SetTool(8),
        EventKind::SetToolTip(2),
        EventKind::DrawStartNoPressure {
            x: -32768,
            y: 32767,
        },
        EventKind::DrawNoPressure { x: 640, y: 480 },
        EventKind::Draw {
            x: 20,
            y: 30,
            pressure: 32768,
        },
        EventKind::DrawCommit,
        EventKind::AddLayer,
        EventKind::ToggleLayerVisibility(2),
        EventKind::SetActiveLayer(1),
        EventKind::ToggleLayerSelection(2),
        EventKind::SetSelectedLayersAlpha(0.75),
        EventKind::MoveLayers(3),
        EventKind::MergeLayers,
        EventKind::AddLayer,
        EventKind::DeleteLayers,
        EventKind::HistoryDummy,
        EventKind::Conclusion,
    ];
    assert_eq!(commands.events.len(), expected.len());
    for (event, expected) in commands.events.iter().zip(expected) {
        assert_eq!(event.kind, expected);
    }
    assert_eq!(commands.events[1].timestamp_ms, 1200);
    assert_eq!(commands.events[30].timestamp_ms, 1229);
    assert_eq!(commands.events[31].timestamp_ms, 2123);
    // Introducing a replay parser does not extend the existing image protocol.
    assert!(decode_image(EMPTY).is_err());
    assert!(decode_image(COMMANDS).is_err());
}

#[test]
fn rejects_every_file_and_inflated_body_truncation() {
    for fixture in [EMPTY, COMMANDS] {
        for length in 0..fixture.len() {
            assert!(
                parse_candidate(&fixture[..length]).is_err(),
                "file cut {length}"
            );
        }
        let decoded = body(fixture);
        for length in 0..decoded.len() {
            assert!(
                parse_candidate(&encode(&decoded[..length])).is_err(),
                "body cut {length}"
            );
        }
    }
}

#[test]
fn strict_header_version_size_and_exact_consumption() {
    for index in [0, 1, 2, 3, 8, 9, 10, 11] {
        let mut bytes = EMPTY.to_vec();
        bytes[index] ^= 0xff;
        reject(&bytes, ReplayError::UnsupportedHeader);
    }
    for size in [
        0,
        1,
        body(EMPTY).len() as u32 - 1,
        body(EMPTY).len() as u32 + 1,
        replay::MAX_INFLATED_BYTES as u32 + 1,
        u32::MAX,
    ] {
        let mut bytes = EMPTY.to_vec();
        set_u32(&mut bytes, 4, size);
        reject(&bytes, ReplayError::InflatedSize);
    }
    for suffix in [&[0][..], &EMPTY[12..]] {
        let mut bytes = EMPTY.to_vec();
        bytes.extend_from_slice(suffix);
        reject(&bytes, ReplayError::TrailingData);
    }
    let mut bytes = body(EMPTY);
    bytes.push(0);
    reject(&encode(&bytes), ReplayError::TrailingData);
    let mut bytes = EMPTY[..12].to_vec();
    bytes.extend_from_slice(&[0xff; 8]);
    reject(&bytes, ReplayError::Deflate);
    reject(
        &vec![0; replay::MAX_INPUT_BYTES + 1],
        ReplayError::InputSize,
    );
}

#[test]
fn rejects_metadata_and_tool_shape_mismatches() {
    let original = body(EMPTY);
    for size in [0_u16, 20, 22, u16::MAX] {
        let mut bytes = original.clone();
        bytes[..2].copy_from_slice(&size.to_be_bytes());
        reject(&encode(&bytes), ReplayError::Metadata);
    }
    for offset in [10, 12] {
        for size in [0_u16, 1025, u16::MAX] {
            let mut bytes = original.clone();
            bytes[offset..offset + 2].copy_from_slice(&size.to_be_bytes());
            reject(&encode(&bytes), ReplayError::Metadata);
        }
        for size in [1_u16, 1024] {
            let mut bytes = original.clone();
            bytes[offset..offset + 2].copy_from_slice(&size.to_be_bytes());
            assert!(parse_candidate(&encode(&bytes)).is_ok());
        }
    }
    for (offset, values) in [
        (21, &[0, 1, 7, 9, 255][..]),
        (22, &[0, 18, 20, 255][..]),
        (20, &[0, 9, 255][..]),
        (TOOL_START, &[0, 9, 255, 2][..]),
    ] {
        for value in values {
            let mut bytes = original.clone();
            bytes[offset] = *value;
            reject(&encode(&bytes), ReplayError::Tool);
        }
    }
}

#[test]
fn rejects_unknown_tags_bad_markers_counts_and_noncanonical_booleans() {
    let known = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26,
        27, 254, 255,
    ];
    for tag in 0..=255 {
        if !known.contains(&tag) {
            reject(&encode(&event_body(tag, &[])), ReplayError::EventTag);
        }
    }
    for count in [0, 1, replay::MAX_EVENTS as u32 + 1, u32::MAX] {
        let mut bytes = body(EMPTY);
        set_u32(&mut bytes, EVENT_COUNT, count);
        reject(&encode(&bytes), ReplayError::EventCount);
    }
    let mut bytes = body(EMPTY);
    set_u32(&mut bytes, EVENT_COUNT, 3);
    reject(&encode(&bytes), ReplayError::Truncated);
    for (offset, tag) in [(EVENTS, 3), (EVENTS + 5, 3)] {
        let mut bytes = body(EMPTY);
        bytes[offset] = tag;
        reject(&encode(&bytes), ReplayError::Markers);
    }
    for tag in [0, 255] {
        reject(&encode(&event_body(tag, &[])), ReplayError::Markers);
    }
    for tag in [13, 14, 16, 17] {
        reject(&encode(&event_body(tag, &[2])), ReplayError::Boolean);
        for flag in [0, 1] {
            assert!(parse_candidate(&encode(&event_body(tag, &[flag]))).is_ok());
        }
    }
    for tool in 0..8 {
        for field in [10, 11, 12, 18] {
            let mut bytes = body(EMPTY);
            bytes[TOOL_START + tool * 19 + field] = 2;
            reject(&encode(&bytes), ReplayError::Boolean);
        }
    }
    for id in [0, 9, 255] {
        reject(&encode(&event_body(10, &[id])), ReplayError::Tool);
    }
}

#[test]
fn rejects_every_nonfinite_float_field() {
    for bits in [
        f32::NAN.to_bits(),
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        0x7f80_0001,
    ] {
        for tool in 0..8 {
            for offset in [2, 6, 14] {
                let mut bytes = body(EMPTY);
                set_u32(&mut bytes, TOOL_START + tool * 19 + offset, bits);
                reject(&encode(&bytes), ReplayError::NonFiniteFloat);
            }
        }
        for tag in [12, 18, 27] {
            reject(
                &encode(&event_body(tag, &bits.to_be_bytes())),
                ReplayError::NonFiniteFloat,
            );
        }
    }
}

#[test]
fn bounded_inflation_rejects_bombs_and_oversized_declarations() {
    // A real stream expands well beyond both the claimed length and our cap.
    let bomb = vec![0; replay::MAX_INFLATED_BYTES + 1];
    let mut bytes = encode(&bomb);
    assert!(bytes.len() < 128 * 1024);
    reject(&bytes, ReplayError::InflatedSize);
    set_u32(&mut bytes, 4, 1);
    reject(&bytes, ReplayError::InflatedSize);
    set_u32(&mut bytes, 4, replay::MAX_INFLATED_BYTES as u32);
    reject(&bytes, ReplayError::InflatedSize);
}

#[test]
fn event_cap_and_actual_allocation_budget_are_enforced() {
    assert!(
        replay::MAX_EVENTS
            .checked_mul(std::mem::size_of::<Event>())
            .unwrap()
            <= replay::MAX_EVENT_BYTES
    );
    let mut bytes = body(EMPTY);
    bytes.truncate(EVENTS);
    set_u32(&mut bytes, EVENT_COUNT, replay::MAX_EVENTS as u32);
    for i in 0..replay::MAX_EVENTS {
        bytes.push(if i == 0 {
            0
        } else if i == replay::MAX_EVENTS - 1 {
            255
        } else {
            254
        });
        bytes.extend_from_slice(&(i as u32).to_be_bytes());
    }
    assert_eq!(
        parse_candidate(&encode(&bytes)).unwrap().events.len(),
        replay::MAX_EVENTS
    );
    set_u32(&mut bytes, EVENT_COUNT, replay::MAX_EVENTS as u32 + 1);
    reject(&encode(&bytes), ReplayError::EventCount);
}

#[test]
fn recorder_precision_and_signed_fields_are_preserved() {
    let mut bytes = body(EMPTY);
    let start = bytes[2..6].to_vec();
    bytes[6..10].copy_from_slice(&start);
    // Metadata seconds can be equal for a sub-second recording; event times
    // need not be strictly increasing. Neither is an approval for playback.
    set_u32(&mut bytes, EVENTS + 6, 123);
    let candidate = parse_candidate(&encode(&bytes)).unwrap();
    assert_eq!(
        candidate.metadata.started_at_seconds,
        candidate.metadata.ended_at_seconds
    );
    assert_eq!(
        candidate.events[0].timestamp_ms,
        candidate.events[1].timestamp_ms
    );
    for tip in [i8::MIN, -1, 0, i8::MAX] {
        bytes[TOOL_START + 13] = tip.to_be_bytes()[0];
        assert_eq!(
            parse_candidate(&encode(&bytes)).unwrap().tools[0].tip_id,
            tip
        );
    }
}

#[test]
fn structural_success_does_not_claim_playback_safety() {
    // These are deliberately not playback-safe guarantees. Layer/history state,
    // tool-dependent ranges, chronology and render work need a separate contract.
    for (tag, payload) in [
        (8, &[0, 1, 0, 2][..]),
        (24, &[255][..]),
        (12, &2_f32.to_be_bytes()[..]),
    ] {
        assert!(parse_candidate(&encode(&event_body(tag, payload))).is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_bounded_files_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        if let Ok(candidate) = parse_candidate(&bytes) {
            prop_assert!(candidate.events.len() <= replay::MAX_EVENTS);
            prop_assert!(candidate.events.len() * std::mem::size_of::<Event>() <= replay::MAX_EVENT_BYTES);
        }
    }
    #[test]
    fn mutations_of_valid_bodies_are_bounded(index in 0usize..512, value in any::<u8>()) {
        let mut bytes = body(COMMANDS);
        let index = index % bytes.len();
        bytes[index] = value;
        if let Ok(candidate) = parse_candidate(&encode(&bytes)) {
            prop_assert_eq!(candidate.tools.len(), 8);
            prop_assert!(candidate.events.len() <= replay::MAX_EVENTS);
        }
    }
    #[test]
    fn compressed_stream_mutations_are_bounded(index in 12usize..1024, value in any::<u8>()) {
        let mut bytes = COMMANDS.to_vec();
        let index = 12 + (index - 12) % (bytes.len() - 12);
        bytes[index] = value;
        if let Ok(candidate) = parse_candidate(&bytes) {
            prop_assert!(candidate.events.len() <= replay::MAX_EVENTS);
            prop_assert!(candidate.events.iter().all(|event| match event.kind {
                EventKind::SetToolAlpha(value) | EventKind::SetToolFlow(value)
                    | EventKind::SetSelectedLayersAlpha(value) => value.is_finite(),
                _ => true,
            }), "decoded float is nonfinite");
        }
    }
    #[test]
    fn float_decoding_preserves_all_finite_bit_patterns(bits in any::<u32>()) {
        let bytes = encode(&event_body(12, &bits.to_be_bytes()));
        let result = parse_candidate(&bytes);
        if f32::from_bits(bits).is_finite() {
            let candidate = result.unwrap();
            let EventKind::SetToolAlpha(value) = candidate.events[1].kind else { panic!("wrong event"); };
            prop_assert_eq!(value.to_bits(), bits);
        } else { prop_assert_eq!(result.unwrap_err(), ReplayError::NonFiniteFloat); }
    }
}
