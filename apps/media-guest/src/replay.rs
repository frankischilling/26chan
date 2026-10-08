//! Guest-only structural decoding of the production Tegaki 0.9.4 recorder format.
//!
//! Source: supplied 4chan-old snapshot at revision
//! 545b7812d1849f7958d914950c91fdbbe38f6b22,
//! `js/tegaki.min.js`: TegakiReplayRecorder, TegakiBinReader and TegakiEvents.
//! Multibyte fields are big-endian. The production recorder emits raw DEFLATE,
//! format 1, 21-byte metadata and 19-byte tool records. This profile rejects
//! other versions, extensions, trailing bytes, noncanonical booleans and NaNs.
//! Exactly eight unique tool IDs (1..=8) and outer-only prelude/conclusion
//! markers are required. These restrictions and the resource caps are our
//! security profile, not claims about what the original permissive PHP
//! validator or JS viewer accepts.
//!
//! With the guest's 96 MiB address-space / 5-second CPU limits, input (8 MiB),
//! inflated bytes (18 MiB + one overflow byte), and event storage (at most 4 MiB)
//! total about 30 MiB of bounded buffer sizes, plus eight tools. This estimate
//! excludes allocator capacity rounding and inflater/runtime overhead; it is
//! not a bound on total process memory. Parser-controlled buffer allocation
//! requests are checked and fallible. Dependency/runtime allocations, including
//! the inflater state, are not all fallible through this API. No rendering occurs.
//! The event cap also bounds decode work. These caps are not a timing proof:
//! an eventual caller must still run this inside the disposable guest limits.
//! Canvas dimensions match the existing 1024-square RGBA limit. No candidate
//! is written to the current fixed output protocol or accepted by image dispatch.
//!
//! A candidate is NON-authoritative. Structural success proves neither safe
//! playback nor valid layer/history state, bounded rendering work, or agreement
//! with a PNG. Finite floats are not checked for tool-dependent value ranges;
//! signed coordinates are not canvas-clamped. Layer references, tip values,
//! stroke pairing, chronology and history are not semantically verified. A
//! separate host contract and state machine remain necessary.

use flate2::{Decompress, FlushDecompress, Status};
use std::{fmt, mem::size_of};

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_INFLATED_BYTES: usize = 18 * 1024 * 1024;
pub const MAX_EVENTS: usize = 131_072;
pub const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOOLS: usize = 8;
pub const MAX_CANVAS_SIDE: u16 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayError {
    InputSize,
    UnsupportedHeader,
    InflatedSize,
    Deflate,
    Truncated,
    TrailingData,
    Metadata,
    Tool,
    EventCount,
    EventTag,
    Markers,
    Boolean,
    NonFiniteFloat,
    Allocation,
}
impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "replay rejected: {self:?}")
    }
}
impl std::error::Error for ReplayError {}

/// Untrusted decoded data, never an approval to store, serve, or play a replay.
#[derive(Debug)]
pub struct ReplayCandidate {
    pub header: Header,
    pub metadata: Metadata,
    pub tools: Vec<Tool>,
    pub events: Vec<Event>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Header {
    pub inflated_bytes: u32,
    pub tegaki_version: [u8; 3],
    pub format_version: u8,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Metadata {
    pub started_at_seconds: u32,
    pub ended_at_seconds: u32,
    pub width: u16,
    pub height: u16,
    pub background: [u8; 3],
    pub color: [u8; 3],
    pub tool_id: u8,
}
#[derive(Debug, PartialEq)]
pub struct Tool {
    pub id: u8,
    pub size: u8,
    pub alpha: f32,
    pub step: f32,
    pub size_dynamics: bool,
    pub alpha_dynamics: bool,
    // This is the source's usePreserveAlpha (capability), not its enabled state.
    pub use_preserve_alpha: bool,
    pub tip_id: i8,
    pub flow: f32,
    pub flow_dynamics: bool,
}
#[derive(Debug, PartialEq)]
pub struct Event {
    pub timestamp_ms: u32,
    pub kind: EventKind,
}
#[derive(Debug, PartialEq)]
pub enum EventKind {
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

/// Parse only the known compressed format. The caller must treat the result as
/// untrusted and keep this crate out of privileged services. No playback occurs.
pub fn parse_candidate(input: &[u8]) -> Result<ReplayCandidate, ReplayError> {
    if !(13..=MAX_INPUT_BYTES).contains(&input.len()) {
        return Err(ReplayError::InputSize);
    }
    let mut r = Reader::new(input);
    if r.array::<4>()? != *b"TGK\x01" {
        return Err(ReplayError::UnsupportedHeader);
    }
    let inflated_bytes = r.u32()?;
    let tegaki_version = r.array()?;
    let format_version = r.u8()?;
    if tegaki_version != [0, 9, 4] || format_version != 1 {
        return Err(ReplayError::UnsupportedHeader);
    }
    let declared = usize::try_from(inflated_bytes).map_err(|_| ReplayError::InflatedSize)?;
    if !(1..=MAX_INFLATED_BYTES).contains(&declared) {
        return Err(ReplayError::InflatedSize);
    }
    let capacity = declared.checked_add(1).ok_or(ReplayError::InflatedSize)?;
    let mut body = Vec::new();
    body.try_reserve_exact(capacity)
        .map_err(|_| ReplayError::Allocation)?;
    body.resize(capacity, 0);
    let compressed = &input[r.pos..];
    let mut inflater = Decompress::new(false);
    let status = inflater
        .decompress(compressed, &mut body, FlushDecompress::Finish)
        .map_err(|_| ReplayError::Deflate)?;
    if status != Status::StreamEnd || inflater.total_out() != u64::from(inflated_bytes) {
        return Err(ReplayError::InflatedSize);
    }
    if inflater.total_in() != compressed.len() as u64 {
        return Err(ReplayError::TrailingData);
    }
    body.truncate(declared);
    decode_body(
        Header {
            inflated_bytes,
            tegaki_version,
            format_version,
        },
        &body,
    )
}

fn decode_body(header: Header, body: &[u8]) -> Result<ReplayCandidate, ReplayError> {
    let mut r = Reader::new(body);
    if r.u16()? != 21 {
        return Err(ReplayError::Metadata);
    }
    let metadata = Metadata {
        started_at_seconds: r.u32()?,
        ended_at_seconds: r.u32()?,
        width: r.u16()?,
        height: r.u16()?,
        background: r.array()?,
        color: r.array()?,
        tool_id: r.u8()?,
    };
    if !(1..=MAX_CANVAS_SIDE).contains(&metadata.width)
        || !(1..=MAX_CANVAS_SIDE).contains(&metadata.height)
    {
        return Err(ReplayError::Metadata);
    }
    let count = usize::from(r.u8()?);
    if count != MAX_TOOLS || r.u8()? != 19 {
        return Err(ReplayError::Tool);
    }
    r.require(count.checked_mul(19).ok_or(ReplayError::Tool)?)?;
    let mut tools = Vec::new();
    tools
        .try_reserve_exact(count)
        .map_err(|_| ReplayError::Allocation)?;
    let mut seen = [false; MAX_TOOLS + 1];
    for _ in 0..count {
        let id = r.u8()?;
        if !(1..=8).contains(&id) || seen[usize::from(id)] {
            return Err(ReplayError::Tool);
        }
        seen[usize::from(id)] = true;
        tools.push(Tool {
            id,
            size: r.u8()?,
            alpha: r.float()?,
            step: r.float()?,
            size_dynamics: r.boolean()?,
            alpha_dynamics: r.boolean()?,
            use_preserve_alpha: r.boolean()?,
            tip_id: i8::from_be_bytes(r.array()?),
            flow: r.float()?,
            flow_dynamics: r.boolean()?,
        });
    }
    if !(1..=8).contains(&metadata.tool_id) {
        return Err(ReplayError::Tool);
    }
    let count = usize::try_from(r.u32()?).map_err(|_| ReplayError::EventCount)?;
    if !(2..=MAX_EVENTS).contains(&count)
        || count
            .checked_mul(size_of::<Event>())
            .filter(|n| *n <= MAX_EVENT_BYTES)
            .is_none()
    {
        return Err(ReplayError::EventCount);
    }
    r.require(count.checked_mul(5).ok_or(ReplayError::EventCount)?)?;
    let mut events = Vec::new();
    events
        .try_reserve_exact(count)
        .map_err(|_| ReplayError::Allocation)?;
    for index in 0..count {
        let tag = r.u8()?;
        let timestamp_ms = r.u32()?;
        let kind = match tag {
            0 => EventKind::Prelude,
            1 => EventKind::DrawStart {
                x: r.i16()?,
                y: r.i16()?,
                pressure: r.u16()?,
            },
            2 => EventKind::Draw {
                x: r.i16()?,
                y: r.i16()?,
                pressure: r.u16()?,
            },
            3 => EventKind::DrawCommit,
            4 => EventKind::Undo,
            5 => EventKind::Redo,
            6 => EventKind::SetColor(r.array()?),
            7 => EventKind::DrawStartNoPressure {
                x: r.i16()?,
                y: r.i16()?,
            },
            8 => EventKind::DrawNoPressure {
                x: r.i16()?,
                y: r.i16()?,
            },
            10 => {
                let id = r.u8()?;
                if !(1..=8).contains(&id) {
                    return Err(ReplayError::Tool);
                }
                EventKind::SetTool(id)
            }
            11 => EventKind::SetToolSize(r.u8()?),
            12 => EventKind::SetToolAlpha(r.float()?),
            13 => EventKind::SetToolSizeDynamics(r.boolean()?),
            14 => EventKind::SetToolAlphaDynamics(r.boolean()?),
            15 => EventKind::SetToolTip(r.u8()?),
            16 => EventKind::PreserveAlpha(r.boolean()?),
            17 => EventKind::SetToolFlowDynamics(r.boolean()?),
            18 => EventKind::SetToolFlow(r.float()?),
            20 => EventKind::AddLayer,
            21 => EventKind::DeleteLayers,
            22 => EventKind::MoveLayers(r.u8()?),
            23 => EventKind::MergeLayers,
            24 => EventKind::ToggleLayerVisibility(r.u8()?),
            25 => EventKind::SetActiveLayer(r.u8()?),
            26 => EventKind::ToggleLayerSelection(r.u8()?),
            27 => EventKind::SetSelectedLayersAlpha(r.float()?),
            254 => EventKind::HistoryDummy,
            255 => EventKind::Conclusion,
            _ => return Err(ReplayError::EventTag),
        };
        if (index == 0) != (tag == 0) || (index == count - 1) != (tag == 255) {
            return Err(ReplayError::Markers);
        }
        events.push(Event { timestamp_ms, kind });
    }
    if r.pos != body.len() {
        return Err(ReplayError::TrailingData);
    }
    Ok(ReplayCandidate {
        header,
        metadata,
        tools,
        events,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn require(&self, length: usize) -> Result<(), ReplayError> {
        self.pos
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .map(|_| ())
            .ok_or(ReplayError::Truncated)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ReplayError> {
        self.require(N)?;
        let end = self.pos.checked_add(N).ok_or(ReplayError::Truncated)?;
        let bytes = self.bytes[self.pos..end]
            .try_into()
            .map_err(|_| ReplayError::Truncated)?;
        self.pos = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, ReplayError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ReplayError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn i16(&mut self) -> Result<i16, ReplayError> {
        Ok(i16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, ReplayError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn boolean(&mut self) -> Result<bool, ReplayError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ReplayError::Boolean),
        }
    }
    fn float(&mut self) -> Result<f32, ReplayError> {
        let value = f32::from_bits(self.u32()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ReplayError::NonFiniteFloat)
        }
    }
}
