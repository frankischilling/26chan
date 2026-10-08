//! Bounded, independent host decoding of the fixed `IBRPLY01` candidate wire.
//!
//! Every returned value is still untrusted. Structural decoding does not approve
//! replay safety, layer/history state, playback cost, provenance, storage,
//! publication, or agreement with an image. Profile 1 identifies a candidate
//! representation, not an approved playback profile. This module does not parse
//! Tegaki uploads or import the guest's parser, types, or decoder dependencies.
//!
//! This API accepts an already acquired byte slice. A future stream/disk caller
//! must independently cap acquisition bytes and impose a deadline, including
//! while waiting for EOF. The limits here bound this decoder's own work and
//! allocation requests, not prior buffering, wall-clock time, or rendering.

pub const WIRE_VERSION: u16 = 1;
/// A structural candidate profile, never a safety or playback approval.
pub const CANDIDATE_PROFILE: u16 = 1;
pub const HEADER_BYTES: usize = 64;
pub const TOOL_BYTES: usize = 24;
pub const TOOL_COUNT: usize = 8;
pub const EVENT_BYTES: usize = 16;
pub const EVENTS_OFFSET: usize = HEADER_BYTES + TOOL_COUNT * TOOL_BYTES;
pub const MAX_EVENTS: usize = 16_384;
pub const MAX_WIRE_BYTES: usize = EVENTS_OFFSET + MAX_EVENTS * EVENT_BYTES;
pub const MAX_CANVAS_SIDE: u16 = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReplayWireError {
    #[error("replay wire is outside the byte limit")]
    InputSize,
    #[error("unsupported replay wire header")]
    Header,
    #[error("replay wire event count is outside the limit")]
    EventCount,
    #[error("replay wire length does not exactly match its count and declaration")]
    Length,
    #[error("invalid replay wire metadata")]
    Metadata,
    #[error("invalid replay wire tool record")]
    Tool,
    #[error("nonzero replay wire reserved or unused byte")]
    Reserved,
    #[error("unknown replay wire event tag")]
    EventTag,
    #[error("invalid replay wire prelude or conclusion placement")]
    Markers,
    #[error("noncanonical replay wire boolean")]
    Boolean,
    #[error("nonfinite replay wire float")]
    NonFiniteFloat,
    #[error("replay wire allocation failed")]
    Allocation,
}

/// Structurally decoded candidate data; no authority to store, serve, or play it.
#[derive(Debug, PartialEq)]
pub struct UntrustedReplay {
    /// Identifies this representation only, not an approved safety profile.
    pub candidate_profile: u16,
    pub metadata: UntrustedReplayMetadata,
    pub tools: [UntrustedReplayTool; TOOL_COUNT],
    pub events: Vec<UntrustedReplayEvent>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UntrustedReplayMetadata {
    pub started_at_seconds: u32,
    pub ended_at_seconds: u32,
    pub width: u16,
    pub height: u16,
    pub background: [u8; 3],
    pub color: [u8; 3],
    pub tool_id: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UntrustedReplayTool {
    pub id: u8,
    pub size: u8,
    pub alpha: f32,
    pub step: f32,
    pub size_dynamics: bool,
    pub alpha_dynamics: bool,
    /// Source `usePreserveAlpha` capability, not its enabled state.
    pub use_preserve_alpha: bool,
    pub tip_id: i8,
    pub flow: f32,
    pub flow_dynamics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UntrustedReplayEvent {
    pub timestamp_ms: u32,
    pub kind: UntrustedReplayEventKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UntrustedReplayEventKind {
    Prelude,
    DrawStart { x: i16, y: i16, pressure: u16 },
    Draw { x: i16, y: i16, pressure: u16 },
    DrawCommit,
    Undo,
    Redo,
    SetColor([u8; 3]),
    DrawStartNoPressure { x: i16, y: i16 },
    DrawNoPressure { x: i16, y: i16 },
    SetTool(u8),
    SetToolSize(u8),
    SetToolAlpha(f32),
    SetToolSizeDynamics(bool),
    SetToolAlphaDynamics(bool),
    SetToolTip(u8),
    PreserveAlpha(bool),
    SetToolFlowDynamics(bool),
    SetToolFlow(f32),
    AddLayer,
    DeleteLayers,
    MoveLayers(u8),
    MergeLayers,
    ToggleLayerVisibility(u8),
    SetActiveLayer(u8),
    ToggleLayerSelection(u8),
    SetSelectedLayersAlpha(f32),
    HistoryDummy,
    Conclusion,
}

/// Decode exactly one bounded wire value, with no trailing bytes or padding.
/// Success is structural only. Signed coordinates, finite float values, tool
/// settings, layer references, timestamps, and history remain semantically
/// unchecked. In particular, this is not a rendering or publication boundary.
pub fn decode(input: &[u8]) -> Result<UntrustedReplay, ReplayWireError> {
    if !(EVENTS_OFFSET + 2 * EVENT_BYTES..=MAX_WIRE_BYTES).contains(&input.len()) {
        return Err(ReplayWireError::InputSize);
    }
    // The minimum input length established above covers every fixed header read.
    if &input[..8] != b"IBRPLY01"
        || u16_at(input, 8) != WIRE_VERSION
        || usize::from(u16_at(input, 10)) != HEADER_BYTES
        || u16_at(input, 12) != CANDIDATE_PROFILE
        || u16_at(input, 14) != 0
        || input[44..48] != [0, 9, 4, 1]
    {
        return Err(ReplayWireError::Header);
    }
    let count = usize::try_from(u32_at(input, 20)).map_err(|_| ReplayWireError::EventCount)?;
    let expected = wire_len(count)?;
    let declared = usize::try_from(u32_at(input, 16)).map_err(|_| ReplayWireError::Length)?;
    if declared != expected || input.len() != expected {
        return Err(ReplayWireError::Length);
    }
    require_zero(&input[35..36])?;
    require_zero(&input[48..HEADER_BYTES])?;
    let metadata = UntrustedReplayMetadata {
        started_at_seconds: u32_at(input, 36),
        ended_at_seconds: u32_at(input, 40),
        width: u16_at(input, 24),
        height: u16_at(input, 26),
        background: [input[28], input[29], input[30]],
        color: [input[31], input[32], input[33]],
        tool_id: input[34],
    };
    if !(1..=MAX_CANVAS_SIDE).contains(&metadata.width)
        || !(1..=MAX_CANVAS_SIDE).contains(&metadata.height)
        || !(1..=8).contains(&metadata.tool_id)
    {
        return Err(ReplayWireError::Metadata);
    }
    // Eight fixed-size stack entries; no attacker-dependent tool allocation.
    let mut tools = [UntrustedReplayTool {
        id: 0,
        size: 0,
        alpha: 0.0,
        step: 0.0,
        size_dynamics: false,
        alpha_dynamics: false,
        use_preserve_alpha: false,
        tip_id: 0,
        flow: 0.0,
        flow_dynamics: false,
    }; TOOL_COUNT];
    for ((tool, record), expected_id) in tools
        .iter_mut()
        .zip(input[HEADER_BYTES..EVENTS_OFFSET].chunks_exact(TOOL_BYTES))
        .zip(1..=8)
    {
        if record[0] != expected_id || record[2] & 0xf0 != 0 {
            return Err(ReplayWireError::Tool);
        }
        require_zero(&record[16..])?;
        *tool = UntrustedReplayTool {
            id: record[0],
            size: record[1],
            alpha: finite_f32(record, 4)?,
            step: finite_f32(record, 8)?,
            size_dynamics: record[2] & 1 != 0,
            alpha_dynamics: record[2] & 2 != 0,
            use_preserve_alpha: record[2] & 4 != 0,
            tip_id: record[3] as i8,
            flow: finite_f32(record, 12)?,
            flow_dynamics: record[2] & 8 != 0,
        };
    }
    // Exact byte length and bounded count are validated before this sole
    // attacker-sized allocation. try_reserve_exact also checks layout overflow.
    let mut events = Vec::new();
    events
        .try_reserve_exact(count)
        .map_err(|_| ReplayWireError::Allocation)?;
    for (index, record) in input[EVENTS_OFFSET..].chunks_exact(EVENT_BYTES).enumerate() {
        let event = decode_event(record)?;
        if (index == 0 && event.kind != UntrustedReplayEventKind::Prelude)
            || (index == count - 1 && event.kind != UntrustedReplayEventKind::Conclusion)
            || (index != 0 && event.kind == UntrustedReplayEventKind::Prelude)
            || (index != count - 1 && event.kind == UntrustedReplayEventKind::Conclusion)
        {
            return Err(ReplayWireError::Markers);
        }
        events.push(event);
    }
    Ok(UntrustedReplay {
        candidate_profile: CANDIDATE_PROFILE,
        metadata,
        tools,
        events,
    })
}

fn wire_len(count: usize) -> Result<usize, ReplayWireError> {
    if !(2..=MAX_EVENTS).contains(&count) {
        return Err(ReplayWireError::EventCount);
    }
    count
        .checked_mul(EVENT_BYTES)
        .and_then(|length| EVENTS_OFFSET.checked_add(length))
        .filter(|&length| length <= MAX_WIRE_BYTES)
        .ok_or(ReplayWireError::Length)
}

fn require_zero(bytes: &[u8]) -> Result<(), ReplayWireError> {
    if bytes.iter().any(|&byte| byte != 0) {
        Err(ReplayWireError::Reserved)
    } else {
        Ok(())
    }
}

// All callers first establish the complete fixed-size header or record slice.
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}
fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    i16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
fn finite_f32(bytes: &[u8], offset: usize) -> Result<f32, ReplayWireError> {
    let value = f32::from_bits(u32_at(bytes, offset));
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ReplayWireError::NonFiniteFloat)
    }
}
fn boolean(value: u8) -> Result<bool, ReplayWireError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ReplayWireError::Boolean),
    }
}

fn decode_event(record: &[u8]) -> Result<UntrustedReplayEvent, ReplayWireError> {
    use UntrustedReplayEventKind as Kind;
    require_zero(&record[1..4])?;
    let payload = &record[8..];
    let (kind, used) = match record[0] {
        0 => (Kind::Prelude, 0),
        1 => (
            Kind::DrawStart {
                x: i16_at(payload, 0),
                y: i16_at(payload, 2),
                pressure: u16_at(payload, 4),
            },
            6,
        ),
        2 => (
            Kind::Draw {
                x: i16_at(payload, 0),
                y: i16_at(payload, 2),
                pressure: u16_at(payload, 4),
            },
            6,
        ),
        3 => (Kind::DrawCommit, 0),
        4 => (Kind::Undo, 0),
        5 => (Kind::Redo, 0),
        6 => (Kind::SetColor([payload[0], payload[1], payload[2]]), 3),
        7 => (
            Kind::DrawStartNoPressure {
                x: i16_at(payload, 0),
                y: i16_at(payload, 2),
            },
            4,
        ),
        8 => (
            Kind::DrawNoPressure {
                x: i16_at(payload, 0),
                y: i16_at(payload, 2),
            },
            4,
        ),
        10 => {
            if !(1..=8).contains(&payload[0]) {
                return Err(ReplayWireError::Tool);
            }
            (Kind::SetTool(payload[0]), 1)
        }
        11 => (Kind::SetToolSize(payload[0]), 1),
        12 => (Kind::SetToolAlpha(finite_f32(payload, 0)?), 4),
        13 => (Kind::SetToolSizeDynamics(boolean(payload[0])?), 1),
        14 => (Kind::SetToolAlphaDynamics(boolean(payload[0])?), 1),
        15 => (Kind::SetToolTip(payload[0]), 1),
        16 => (Kind::PreserveAlpha(boolean(payload[0])?), 1),
        17 => (Kind::SetToolFlowDynamics(boolean(payload[0])?), 1),
        18 => (Kind::SetToolFlow(finite_f32(payload, 0)?), 4),
        20 => (Kind::AddLayer, 0),
        21 => (Kind::DeleteLayers, 0),
        22 => (Kind::MoveLayers(payload[0]), 1),
        23 => (Kind::MergeLayers, 0),
        24 => (Kind::ToggleLayerVisibility(payload[0]), 1),
        25 => (Kind::SetActiveLayer(payload[0]), 1),
        26 => (Kind::ToggleLayerSelection(payload[0]), 1),
        27 => (Kind::SetSelectedLayersAlpha(finite_f32(payload, 0)?), 4),
        254 => (Kind::HistoryDummy, 0),
        255 => (Kind::Conclusion, 0),
        _ => return Err(ReplayWireError::EventTag),
    };
    require_zero(&payload[used..])?;
    Ok(UntrustedReplayEvent {
        timestamp_ms: u32_at(record, 4),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Literal wire records, independently specified here, not encoder output.
    // Source times are intentionally reversed and event times need not increase.
    const HEADER: [u8; 64] = [
        b'I', b'B', b'R', b'P', b'L', b'Y', b'0', b'1', 0, 1, 0, 64, 0, 1, 0, 0, 0, 0, 1, 32, 0, 0,
        0, 2, 2, 128, 1, 224, 255, 128, 0, 12, 34, 56, 8, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 9,
        4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const TOOL: [u8; 24] = [
        1, 255, 15, 128, 63, 0, 0, 0, 194, 200, 0, 0, 63, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const PRELUDE: [u8; 16] = [0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0];
    const CONCLUSION: [u8; 16] = [255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const KNOWN_TAGS: [u8; 28] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22, 23, 24, 25, 26,
        27, 254, 255,
    ];

    fn literal_empty() -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        for id in 1..=8 {
            let mut tool = TOOL;
            tool[0] = id;
            bytes.extend_from_slice(&tool);
        }
        bytes.extend_from_slice(&PRELUDE);
        bytes.extend_from_slice(&CONCLUSION);
        bytes
    }

    fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    fn with_event(tag: u8, payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() <= 8);
        let mut bytes = literal_empty();
        let mut record = [0; 16];
        record[0] = tag;
        record[4..8].copy_from_slice(&[18, 52, 86, 120]);
        record[8..8 + payload.len()].copy_from_slice(payload);
        bytes.splice(272..272, record);
        set_u32(&mut bytes, 16, 304);
        set_u32(&mut bytes, 20, 3);
        bytes
    }
    fn reject(bytes: &[u8], error: ReplayWireError) {
        assert_eq!(decode(bytes).unwrap_err(), error);
    }

    #[test]
    fn frozen_recorder_cross_fixtures_decode_without_a_guest_dependency() {
        // Python raw-DEFLATE/struct transcription of pinned production-recorder
        // TGKR fixtures. Neither Rust codec generates these expectation bytes.
        const EMPTY: &[u8] = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr");
        const COMMANDS: &[u8] =
            include_bytes!("../../../tests/media/fixtures/replay-wire/commands.ibr");
        use UntrustedReplayEventKind as K;
        let empty = decode(EMPTY).unwrap();
        assert_eq!(EMPTY.len(), 288);
        assert_eq!(
            empty.metadata,
            UntrustedReplayMetadata {
                started_at_seconds: 1_690_000_000,
                ended_at_seconds: 1_690_000_013,
                width: 640,
                height: 480,
                background: [255; 3],
                color: [0; 3],
                tool_id: 1,
            }
        );
        let settings = [
            (1, 1.0, 0.01, true),
            (8, 1.0, 0.05, true),
            (32, 1.0, 0.1, true),
            (1, 1.0, 100.0, false),
            (8, 0.5, 0.01, true),
            (1, 1.0, 100.0, false),
            (32, 0.5, 0.25, false),
            (8, 1.0, 0.1, false),
        ];
        for ((tool, id), (size, alpha, step, capability)) in
            empty.tools.iter().zip(1..=8).zip(settings)
        {
            assert_eq!(
                *tool,
                UntrustedReplayTool {
                    id,
                    size,
                    alpha,
                    step,
                    size_dynamics: false,
                    alpha_dynamics: false,
                    use_preserve_alpha: capability,
                    tip_id: 0,
                    flow: 1.0,
                    flow_dynamics: false,
                }
            );
        }
        assert_eq!(
            empty.events,
            [
                UntrustedReplayEvent {
                    timestamp_ms: 123,
                    kind: K::Prelude
                },
                UntrustedReplayEvent {
                    timestamp_ms: 1123,
                    kind: K::Conclusion
                },
            ]
        );
        let commands = decode(COMMANDS).unwrap();
        assert_eq!(COMMANDS.len(), 768);
        assert_eq!(
            commands.metadata,
            UntrustedReplayMetadata {
                started_at_seconds: 1_690_000_013,
                ended_at_seconds: 1_690_000_025,
                ..empty.metadata
            }
        );
        assert_eq!(commands.tools, empty.tools);
        let expected = [
            K::Prelude,
            K::SetColor([12, 34, 56]),
            K::SetTool(2),
            K::SetToolSize(16),
            K::SetToolAlpha(0.5),
            K::SetToolSizeDynamics(true),
            K::SetToolAlphaDynamics(true),
            K::PreserveAlpha(true),
            K::SetToolFlowDynamics(true),
            K::SetToolFlow(0.25),
            K::DrawStart {
                x: -3,
                y: 9,
                pressure: 0,
            },
            K::Draw {
                x: 12,
                y: -2,
                pressure: 65535,
            },
            K::DrawCommit,
            K::Undo,
            K::Redo,
            K::SetTool(8),
            K::SetToolTip(2),
            K::DrawStartNoPressure {
                x: -32768,
                y: 32767,
            },
            K::DrawNoPressure { x: 640, y: 480 },
            K::Draw {
                x: 20,
                y: 30,
                pressure: 32768,
            },
            K::DrawCommit,
            K::AddLayer,
            K::ToggleLayerVisibility(2),
            K::SetActiveLayer(1),
            K::ToggleLayerSelection(2),
            K::SetSelectedLayersAlpha(0.75),
            K::MoveLayers(3),
            K::MergeLayers,
            K::AddLayer,
            K::DeleteLayers,
            K::HistoryDummy,
            K::Conclusion,
        ];
        assert_eq!(commands.events.len(), expected.len());
        for (index, (event, expected)) in commands.events.iter().zip(expected).enumerate() {
            assert_eq!(event.kind, expected);
            let timestamp = match index {
                0 => 1123,
                31 => 2123,
                _ => 1199 + index as u32,
            };
            assert_eq!(event.timestamp_ms, timestamp);
        }
        for fixture in [EMPTY, COMMANDS] {
            for length in 0..fixture.len() {
                assert!(
                    decode(&fixture[..length]).is_err(),
                    "frozen fixture cut {length}"
                );
            }
        }
    }

    #[test]
    fn literal_metadata_and_tools_are_preserved_without_semantic_approval() {
        let replay = decode(&literal_empty()).unwrap();
        assert_eq!(replay.candidate_profile, 1);
        assert_eq!(
            replay.metadata,
            UntrustedReplayMetadata {
                started_at_seconds: u32::MAX,
                ended_at_seconds: 0,
                width: 640,
                height: 480,
                background: [255, 128, 0],
                color: [12, 34, 56],
                tool_id: 8,
            }
        );
        for (tool, id) in replay.tools.iter().zip(1..=8) {
            assert_eq!(
                *tool,
                UntrustedReplayTool {
                    id,
                    size: 255,
                    alpha: 0.5,
                    step: -100.0,
                    size_dynamics: true,
                    alpha_dynamics: true,
                    use_preserve_alpha: true,
                    tip_id: -128,
                    flow: 1.0,
                    flow_dynamics: true,
                }
            );
        }
        assert_eq!(
            replay.events,
            [
                UntrustedReplayEvent {
                    timestamp_ms: u32::MAX,
                    kind: UntrustedReplayEventKind::Prelude,
                },
                UntrustedReplayEvent {
                    timestamp_ms: 0,
                    kind: UntrustedReplayEventKind::Conclusion,
                },
            ]
        );
    }

    #[test]
    fn repeated_timestamps_are_preserved_without_chronology_or_state_claims() {
        for timestamp in [0, 123, u32::MAX] {
            // An undo with no preceding action deliberately receives no state
            // validation here. Equal timestamps are also structural data.
            let mut bytes = with_event(4, &[]);
            for offset in [260, 276, 292] {
                set_u32(&mut bytes, offset, timestamp);
            }
            let replay = decode(&bytes).unwrap();
            assert!(
                replay
                    .events
                    .iter()
                    .all(|event| event.timestamp_ms == timestamp)
            );
            assert_eq!(replay.events[1].kind, UntrustedReplayEventKind::Undo);
        }
    }

    #[test]
    fn literal_payloads_cover_every_tag_and_preserve_signed_values() {
        use UntrustedReplayEventKind as K;
        let cases: &[(u8, &[u8], K)] = &[
            (
                1,
                &[128, 0, 127, 255, 255, 255],
                K::DrawStart {
                    x: i16::MIN,
                    y: i16::MAX,
                    pressure: u16::MAX,
                },
            ),
            (
                2,
                &[255, 255, 128, 0, 0, 0],
                K::Draw {
                    x: -1,
                    y: i16::MIN,
                    pressure: 0,
                },
            ),
            (3, &[], K::DrawCommit),
            (4, &[], K::Undo),
            (5, &[], K::Redo),
            (6, &[12, 34, 56], K::SetColor([12, 34, 56])),
            (
                7,
                &[128, 0, 127, 255],
                K::DrawStartNoPressure {
                    x: i16::MIN,
                    y: i16::MAX,
                },
            ),
            (
                8,
                &[255, 255, 128, 0],
                K::DrawNoPressure { x: -1, y: i16::MIN },
            ),
            (10, &[8], K::SetTool(8)),
            (11, &[0], K::SetToolSize(0)),
            (12, &[191, 128, 0, 0], K::SetToolAlpha(-1.0)),
            (13, &[1], K::SetToolSizeDynamics(true)),
            (14, &[0], K::SetToolAlphaDynamics(false)),
            (15, &[255], K::SetToolTip(255)),
            (16, &[1], K::PreserveAlpha(true)),
            (17, &[0], K::SetToolFlowDynamics(false)),
            (18, &[127, 127, 255, 255], K::SetToolFlow(f32::MAX)),
            (20, &[], K::AddLayer),
            (21, &[], K::DeleteLayers),
            (22, &[255], K::MoveLayers(255)),
            (23, &[], K::MergeLayers),
            (24, &[0], K::ToggleLayerVisibility(0)),
            (25, &[255], K::SetActiveLayer(255)),
            (26, &[0], K::ToggleLayerSelection(0)),
            (
                27,
                &[0, 0, 0, 1],
                K::SetSelectedLayersAlpha(f32::from_bits(1)),
            ),
            (254, &[], K::HistoryDummy),
        ];
        assert_eq!(cases.len() + 2, KNOWN_TAGS.len());
        for &(tag, payload, expected) in cases {
            let replay = decode(&with_event(tag, payload)).unwrap();
            assert_eq!(replay.events[1].timestamp_ms, 0x12345678);
            assert_eq!(replay.events[1].kind, expected, "tag {tag}");
        }
    }

    #[test]
    fn rejects_every_truncation_and_any_extra_bytes() {
        for bytes in [
            literal_empty(),
            with_event(1, &[128, 0, 127, 255, 255, 255]),
        ] {
            for length in 0..bytes.len() {
                assert!(decode(&bytes[..length]).is_err(), "cut {length}");
            }
            for suffix in [&[0][..], &CONCLUSION[..]] {
                let mut extra = bytes.clone();
                extra.extend_from_slice(suffix);
                reject(&extra, ReplayWireError::Length);
            }
        }
        reject(&vec![0; MAX_WIRE_BYTES + 1], ReplayWireError::InputSize);
    }

    #[test]
    fn rejects_header_version_profile_flags_and_source_variants() {
        for offset in (0..16).chain(44..48) {
            let mut bytes = literal_empty();
            bytes[offset] ^= 0xff;
            reject(&bytes, ReplayWireError::Header);
        }
        for offset in [8, 10, 12] {
            for value in [0, 2, 63, 65, u16::MAX] {
                let mut bytes = literal_empty();
                set_u16(&mut bytes, offset, value);
                reject(&bytes, ReplayWireError::Header);
            }
        }
    }

    #[test]
    fn rejects_count_length_mismatches_and_overflow_sized_declarations() {
        for count in [0, 1, MAX_EVENTS as u32 + 1, u32::MAX] {
            let mut bytes = literal_empty();
            set_u32(&mut bytes, 20, count);
            reject(&bytes, ReplayWireError::EventCount);
        }
        for count in [3, MAX_EVENTS as u32] {
            let mut bytes = literal_empty();
            set_u32(&mut bytes, 20, count);
            reject(&bytes, ReplayWireError::Length);
        }
        for length in [0, 1, 287, 289, 304, MAX_WIRE_BYTES as u32, u32::MAX] {
            let mut bytes = literal_empty();
            set_u32(&mut bytes, 16, length);
            reject(&bytes, ReplayWireError::Length);
        }
        assert_eq!(wire_len(usize::MAX), Err(ReplayWireError::EventCount));
        assert_eq!(
            wire_len(usize::MAX / EVENT_BYTES + 1),
            Err(ReplayWireError::EventCount)
        );
        assert_eq!(wire_len(MAX_EVENTS), Ok(262_400));
    }

    #[test]
    fn accepts_exact_maximum_event_and_byte_count() {
        let mut bytes = literal_empty();
        bytes.truncate(272);
        let dummy = [254, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        for _ in 0..MAX_EVENTS - 2 {
            bytes.extend_from_slice(&dummy);
        }
        bytes.extend_from_slice(&CONCLUSION);
        set_u32(&mut bytes, 16, 262_400);
        set_u32(&mut bytes, 20, 16_384);
        assert_eq!(bytes.len(), 262_400);
        assert_eq!(decode(&bytes).unwrap().events.len(), 16_384);
        bytes.extend_from_slice(&dummy);
        set_u32(&mut bytes, 16, 262_416);
        set_u32(&mut bytes, 20, 16_385);
        reject(&bytes, ReplayWireError::InputSize);
    }

    #[test]
    fn rejects_dimensions_and_initial_tool_outside_structural_domain() {
        for offset in [24, 26] {
            for value in [0, 1025, u16::MAX] {
                let mut bytes = literal_empty();
                set_u16(&mut bytes, offset, value);
                reject(&bytes, ReplayWireError::Metadata);
            }
            for value in [1, 1024] {
                let mut bytes = literal_empty();
                set_u16(&mut bytes, offset, value);
                assert!(decode(&bytes).is_ok());
            }
        }
        for id in [0, 9, 255] {
            let mut bytes = literal_empty();
            bytes[34] = id;
            reject(&bytes, ReplayWireError::Metadata);
            reject(&with_event(10, &[id]), ReplayWireError::Tool);
        }
        for id in 1..=8 {
            let mut bytes = literal_empty();
            bytes[34] = id;
            assert!(decode(&bytes).is_ok());
            assert!(decode(&with_event(10, &[id])).is_ok());
        }
    }

    #[test]
    fn requires_exact_sorted_tools_and_checks_each_flag_bit() {
        for index in 0..8 {
            for id in 0..=255 {
                if id == index as u8 + 1 {
                    continue;
                }
                let mut bytes = literal_empty();
                bytes[64 + index * 24] = id;
                reject(&bytes, ReplayWireError::Tool);
            }
            for flags in 16..=255 {
                let mut bytes = literal_empty();
                bytes[66 + index * 24] = flags;
                reject(&bytes, ReplayWireError::Tool);
            }
        }
        for flags in 0..=15 {
            let mut bytes = literal_empty();
            bytes[66] = flags;
            let tool = decode(&bytes).unwrap().tools[0];
            assert_eq!(tool.size_dynamics, flags & 1 != 0);
            assert_eq!(tool.alpha_dynamics, flags & 2 != 0);
            assert_eq!(tool.use_preserve_alpha, flags & 4 != 0);
            assert_eq!(tool.flow_dynamics, flags & 8 != 0);
        }
    }

    #[test]
    fn rejects_every_reserved_header_tool_and_event_byte() {
        let original = literal_empty();
        let offsets = [35]
            .into_iter()
            .chain(48..64)
            .chain((0..8).flat_map(|index| 80 + index * 24..88 + index * 24));
        for offset in offsets {
            let mut bytes = original.clone();
            bytes[offset] = 1;
            reject(&bytes, ReplayWireError::Reserved);
        }
        for start in [256, 272] {
            for offset in (start + 1..start + 4).chain(start + 8..start + 16) {
                let mut bytes = original.clone();
                bytes[offset] = 255;
                reject(&bytes, ReplayWireError::Reserved);
            }
        }
        for tag in KNOWN_TAGS.into_iter().filter(|tag| ![0, 255].contains(tag)) {
            let mut payload = [0; 8];
            if tag == 10 {
                payload[0] = 1;
            }
            let used = match tag {
                1 | 2 => 6,
                7 | 8 | 12 | 18 | 27 => 4,
                6 => 3,
                10 | 11 | 13 | 14 | 15 | 16 | 17 | 22 | 24 | 25 | 26 => 1,
                _ => 0,
            };
            for offset in (273..276).chain(280 + used..288) {
                let mut bytes = with_event(tag, &payload);
                bytes[offset] = 1;
                reject(&bytes, ReplayWireError::Reserved);
            }
        }
    }

    #[test]
    fn rejects_unknown_tags_and_misplaced_markers() {
        for tag in 0..=255 {
            if !KNOWN_TAGS.contains(&tag) {
                reject(&with_event(tag, &[]), ReplayWireError::EventTag);
            }
        }
        for tag in [0, 255] {
            reject(&with_event(tag, &[]), ReplayWireError::Markers);
        }
        for (offset, tag) in [(256, 3), (256, 255), (272, 0), (272, 254)] {
            let mut bytes = literal_empty();
            bytes[offset] = tag;
            reject(&bytes, ReplayWireError::Markers);
        }
    }

    #[test]
    fn booleans_are_canonical_but_other_one_byte_values_are_preserved() {
        for tag in [13, 14, 16, 17] {
            for value in 0..=1 {
                assert!(decode(&with_event(tag, &[value])).is_ok());
            }
            for value in 2..=255 {
                reject(&with_event(tag, &[value]), ReplayWireError::Boolean);
            }
        }
        for tag in [11, 15, 22, 24, 25, 26] {
            for value in 0..=255 {
                assert!(decode(&with_event(tag, &[value])).is_ok());
            }
        }
        for value in 0..=255 {
            let mut bytes = literal_empty();
            bytes[65] = value;
            bytes[67] = value;
            let tool = decode(&bytes).unwrap().tools[0];
            assert_eq!(tool.size, value);
            assert_eq!(tool.tip_id, value as i8);
        }
    }

    #[test]
    fn rejects_nan_and_infinities_in_all_float_positions() {
        for bits in [0x7f800000, 0xff800000, 0x7fc00000, 0x7f800001, 0xffc12345] {
            for index in 0..8 {
                for field in [4, 8, 12] {
                    let mut bytes = literal_empty();
                    set_u32(&mut bytes, 64 + index * 24 + field, bits);
                    reject(&bytes, ReplayWireError::NonFiniteFloat);
                }
            }
            for tag in [12, 18, 27] {
                reject(
                    &with_event(tag, &bits.to_be_bytes()),
                    ReplayWireError::NonFiniteFloat,
                );
            }
        }
    }

    #[test]
    fn finite_floats_preserve_bits_including_negative_zero_and_extremes() {
        for bits in [0_u32, 0x80000000, 1, 0x807fffff, 0x7f7fffff, 0xff7fffff] {
            for index in 0..8 {
                let mut bytes = literal_empty();
                for field in [4, 8, 12] {
                    set_u32(&mut bytes, 64 + index * 24 + field, bits);
                }
                let tool = decode(&bytes).unwrap().tools[index];
                assert_eq!(tool.alpha.to_bits(), bits);
                assert_eq!(tool.step.to_bits(), bits);
                assert_eq!(tool.flow.to_bits(), bits);
            }
            for tag in [12, 18, 27] {
                let replay = decode(&with_event(tag, &bits.to_be_bytes())).unwrap();
                let value = match replay.events[1].kind {
                    UntrustedReplayEventKind::SetToolAlpha(value)
                    | UntrustedReplayEventKind::SetToolFlow(value)
                    | UntrustedReplayEventKind::SetSelectedLayersAlpha(value) => value,
                    _ => panic!("expected float event"),
                };
                assert_eq!(value.to_bits(), bits);
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn arbitrary_bounded_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..=4096)) {
            let _ = decode(&bytes);
        }
        #[test]
        fn arbitrary_mutations_preserve_decoder_bounds(
            edits in prop::collection::vec((0_usize..304, any::<u8>()), 0..=48)
        ) {
            let mut bytes = with_event(1, &[128, 0, 127, 255, 255, 255]);
            for (offset, value) in edits {
                bytes[offset] = value;
            }
            if let Ok(replay) = decode(&bytes) {
                prop_assert_eq!(replay.events.len(), 3);
                prop_assert_eq!(replay.tools.len(), 8);
                prop_assert_eq!(replay.events.first().unwrap().kind, UntrustedReplayEventKind::Prelude);
                prop_assert_eq!(replay.events.last().unwrap().kind, UntrustedReplayEventKind::Conclusion);
            }
        }
    }
}
