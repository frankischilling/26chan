//! Fixed-width transport for structurally decoded, still-untrusted replay data.
//!
//! `IBRPLY01` is a local candidate wire format, not the original TGKR format or
//! an approval token. Wire version 1 / candidate profile 1 use a 64-byte header,
//! eight 24-byte tool slots sorted by ID, then 16-byte event slots. All numbers
//! (including float bit patterns) are big-endian; all unused bytes are zero.
//! The exact packet length is `256 + 16 * event_count`, with 2..=16,384 events.
//! Profile 1 identifies this structural representation only. It establishes no
//! safe tool dynamics, valid state/history, playback cost, provenance, or PNG
//! correspondence. No production dispatcher or output-disk protocol uses it.
//!
//! Header offsets: magic 0, version u16 8, header size u16 10, profile u16 12,
//! flags u16 (zero) 14, total length u32 16, event count u32 20, width/height
//! u16 24/26, background RGB 28, initial RGB 31, initial tool u8 34, zero 35,
//! start/end epoch seconds u32 36/40, source version `[0,9,4,1]` 44, zeros 48.
//! Tool offsets: ID 0, size 1, flags 2, signed tip 3, alpha/step/flow f32 4/8/12,
//! zeros 16. Flag bits 0..3 mean size dynamics, alpha dynamics, preserve-alpha
//! capability (not enabled state), flow dynamics. Other bits are reserved.
//! Event offsets: original source tag 0, zeros 1..4, timestamp u32 4, payload 8.
//! Payload bytes have their source representation, left aligned with zero fill.
//! In particular, no-pressure tags 7/8 remain distinct from pressure tags 1/2.
//!
//! Candidate fields are public, so the encoder repeats structural checks rather
//! than assuming a parser produced them. The source inflated length is checked
//! against the candidate's canonical source record sizes but is not transmitted.
//! Tools may arrive in any order and are canonicalized by ID without changing
//! their values. Coordinates, timestamps and finite floats are not normalized.
//! Host decoding must be independent and must keep its result untrusted.

use crate::replay::{Event, EventKind, ReplayCandidate, Tool};
use std::fmt;

pub const MAX_WIRE_EVENTS: usize = 16_384;
pub const MAX_WIRE_BYTES: usize = 256 + 16 * MAX_WIRE_EVENTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayOutputError {
    EventCount,
    SourceHeader,
    SourceLength,
    Metadata,
    Tool,
    Markers,
    NonFiniteFloat,
    Allocation,
}

impl fmt::Display for ReplayOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "untrusted replay encoding rejected: {self:?}")
    }
}
impl std::error::Error for ReplayOutputError {}

/// Encode an untrusted candidate, not an approved replay. Every public input
/// field is checked for representable structural validity before allocation.
/// Further host state, cost and provenance validation remains necessary.
pub fn encode_untrusted_candidate(
    candidate: &ReplayCandidate,
) -> Result<Vec<u8>, ReplayOutputError> {
    let count = candidate.events.len();
    if !(2..=MAX_WIRE_EVENTS).contains(&count) {
        return Err(ReplayOutputError::EventCount);
    }
    let total = count
        .checked_mul(16)
        .and_then(|n| n.checked_add(256))
        .filter(|n| *n <= MAX_WIRE_BYTES)
        .ok_or(ReplayOutputError::EventCount)?;
    let total_u32 = u32::try_from(total).map_err(|_| ReplayOutputError::EventCount)?;
    let count_u32 = u32::try_from(count).map_err(|_| ReplayOutputError::EventCount)?;
    if candidate.header.tegaki_version != [0, 9, 4] || candidate.header.format_version != 1 {
        return Err(ReplayOutputError::SourceHeader);
    }
    let metadata = &candidate.metadata;
    if !(1..=1024).contains(&metadata.width) || !(1..=1024).contains(&metadata.height) {
        return Err(ReplayOutputError::Metadata);
    }
    if !(1..=8).contains(&metadata.tool_id) || candidate.tools.len() != 8 {
        return Err(ReplayOutputError::Tool);
    }
    let mut tools: [Option<&Tool>; 8] = [None; 8];
    for tool in &candidate.tools {
        if !(1..=8).contains(&tool.id) {
            return Err(ReplayOutputError::Tool);
        }
        let slot = &mut tools[usize::from(tool.id - 1)];
        if slot.is_some() {
            return Err(ReplayOutputError::Tool);
        }
        for value in [tool.alpha, tool.step, tool.flow] {
            finite(value)?;
        }
        *slot = Some(tool);
    }
    let mut source_length = 21_u32 + 2 + 8 * 19 + 4;
    for (index, event) in candidate.events.iter().enumerate() {
        let (slot, source_payload_size) = event_slot(event)?;
        if (index == 0) != (slot[0] == 0) || (index == count - 1) != (slot[0] == 255) {
            return Err(ReplayOutputError::Markers);
        }
        source_length = source_length
            .checked_add(5 + source_payload_size)
            .ok_or(ReplayOutputError::SourceLength)?;
    }
    if candidate.header.inflated_bytes != source_length {
        return Err(ReplayOutputError::SourceLength);
    }

    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(total)
        .map_err(|_| ReplayOutputError::Allocation)?;
    bytes.resize(total, 0);
    bytes[..8].copy_from_slice(b"IBRPLY01");
    bytes[8..10].copy_from_slice(&1_u16.to_be_bytes());
    bytes[10..12].copy_from_slice(&64_u16.to_be_bytes());
    bytes[12..14].copy_from_slice(&1_u16.to_be_bytes());
    bytes[16..20].copy_from_slice(&total_u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&count_u32.to_be_bytes());
    bytes[24..26].copy_from_slice(&metadata.width.to_be_bytes());
    bytes[26..28].copy_from_slice(&metadata.height.to_be_bytes());
    bytes[28..31].copy_from_slice(&metadata.background);
    bytes[31..34].copy_from_slice(&metadata.color);
    bytes[34] = metadata.tool_id;
    bytes[36..40].copy_from_slice(&metadata.started_at_seconds.to_be_bytes());
    bytes[40..44].copy_from_slice(&metadata.ended_at_seconds.to_be_bytes());
    bytes[44..48].copy_from_slice(&[0, 9, 4, 1]);
    for (slot, tool) in bytes[64..256].chunks_exact_mut(24).zip(tools) {
        let tool = tool.ok_or(ReplayOutputError::Tool)?;
        slot[0] = tool.id;
        slot[1] = tool.size;
        slot[2] = u8::from(tool.size_dynamics)
            | u8::from(tool.alpha_dynamics) << 1
            | u8::from(tool.use_preserve_alpha) << 2
            | u8::from(tool.flow_dynamics) << 3;
        slot[3] = tool.tip_id.to_be_bytes()[0];
        slot[4..8].copy_from_slice(&tool.alpha.to_bits().to_be_bytes());
        slot[8..12].copy_from_slice(&tool.step.to_bits().to_be_bytes());
        slot[12..16].copy_from_slice(&tool.flow.to_bits().to_be_bytes());
    }
    for (slot, event) in bytes[256..].chunks_exact_mut(16).zip(&candidate.events) {
        slot.copy_from_slice(&event_slot(event)?.0);
    }
    Ok(bytes)
}

fn finite(value: f32) -> Result<(), ReplayOutputError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ReplayOutputError::NonFiniteFloat)
    }
}

fn event_slot(event: &Event) -> Result<([u8; 16], u32), ReplayOutputError> {
    use EventKind::*;
    let mut bytes = [0; 16];
    bytes[4..8].copy_from_slice(&event.timestamp_ms.to_be_bytes());
    let payload = &mut bytes[8..];
    let (tag, size) = match &event.kind {
        Prelude => (0, 0),
        DrawStart { x, y, pressure } | Draw { x, y, pressure } => {
            payload[..2].copy_from_slice(&x.to_be_bytes());
            payload[2..4].copy_from_slice(&y.to_be_bytes());
            payload[4..6].copy_from_slice(&pressure.to_be_bytes());
            (
                if matches!(event.kind, DrawStart { .. }) {
                    1
                } else {
                    2
                },
                6,
            )
        }
        DrawCommit => (3, 0),
        Undo => (4, 0),
        Redo => (5, 0),
        SetColor(rgb) => {
            payload[..3].copy_from_slice(rgb);
            (6, 3)
        }
        DrawStartNoPressure { x, y } | DrawNoPressure { x, y } => {
            payload[..2].copy_from_slice(&x.to_be_bytes());
            payload[2..4].copy_from_slice(&y.to_be_bytes());
            (
                if matches!(event.kind, DrawStartNoPressure { .. }) {
                    7
                } else {
                    8
                },
                4,
            )
        }
        SetTool(id) => {
            if !(1..=8).contains(id) {
                return Err(ReplayOutputError::Tool);
            }
            payload[0] = *id;
            (10, 1)
        }
        SetToolSize(value)
        | SetToolTip(value)
        | MoveLayers(value)
        | ToggleLayerVisibility(value)
        | SetActiveLayer(value)
        | ToggleLayerSelection(value) => {
            payload[0] = *value;
            let tag = match event.kind {
                SetToolSize(_) => 11,
                SetToolTip(_) => 15,
                MoveLayers(_) => 22,
                ToggleLayerVisibility(_) => 24,
                SetActiveLayer(_) => 25,
                ToggleLayerSelection(_) => 26,
                _ => unreachable!(),
            };
            (tag, 1)
        }
        SetToolAlpha(value) | SetToolFlow(value) | SetSelectedLayersAlpha(value) => {
            finite(*value)?;
            payload[..4].copy_from_slice(&value.to_bits().to_be_bytes());
            let tag = match event.kind {
                SetToolAlpha(_) => 12,
                SetToolFlow(_) => 18,
                SetSelectedLayersAlpha(_) => 27,
                _ => unreachable!(),
            };
            (tag, 4)
        }
        SetToolSizeDynamics(value)
        | SetToolAlphaDynamics(value)
        | PreserveAlpha(value)
        | SetToolFlowDynamics(value) => {
            payload[0] = u8::from(*value);
            let tag = match event.kind {
                SetToolSizeDynamics(_) => 13,
                SetToolAlphaDynamics(_) => 14,
                PreserveAlpha(_) => 16,
                SetToolFlowDynamics(_) => 17,
                _ => unreachable!(),
            };
            (tag, 1)
        }
        AddLayer => (20, 0),
        DeleteLayers => (21, 0),
        MergeLayers => (23, 0),
        HistoryDummy => (254, 0),
        Conclusion => (255, 0),
    };
    bytes[0] = tag;
    Ok((bytes, size))
}
