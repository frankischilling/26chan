//! Isolated, non-authoritative checks for a deliberately restricted core-v1.
//!
//! Success is only a logical state check. It establishes no rendering-work or
//! memory budget, runtime safety, source provenance, PNG agreement, or permission
//! to store, publish, or activate the existing viewer. There is no raster/Canvas
//! emulation here. A future cost check and consumer-equivalence review are
//! separate obligations; this candidate cannot replace either of them.
//!
//! Semantics are grounded in Tegaki 0.9.4 production source SHA-256
//! `daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744`,
//! revision `545b7812d1849f7958d914950c91fdbbe38f6b22`, and the independent
//! 42-case `26chan-replay-state-oracles` suite. In particular, history is not a
//! whole-state snapshot: alpha coalescing preserves redo, Draw restoration
//! activates its target, and the pending Draw reference survives stack changes.
//!
//! Profile restrictions, NOT source invariants: six brush tools, no dynamics,
//! in-canvas coordinates, strictly paired strokes with no intervening settings,
//! at most eight creation-only layers, no delete/merge/move or undo across Add,
//! and no empty history operations. Event times must be nondecreasing, with a
//! positive endpoint span of at most 24 hours. Epoch seconds must be ordered
//! with a span at most 24 hours but may be equal; the two clocks are not equated.
//! Repeated event times and source's SetActiveLayer(0) top-layer fallback remain
//! supported. Source no-pressure records mean `.5 / 65535`, not normalized .5.

use std::rc::Rc;

use crate::replay_wire::{
    CANDIDATE_PROFILE, MAX_CANVAS_SIDE, MAX_EVENTS, TOOL_COUNT, UntrustedReplay,
    UntrustedReplayEventKind as Event, UntrustedReplayTool,
};

pub const MAX_LAYERS: usize = 8;
pub const MAX_HISTORY: usize = 50;
pub const MAX_DURATION_MS: u32 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("replay state check failed at event {event_index:?}: {kind}")]
pub struct ReplayStateError {
    /// None identifies candidate metadata or tool-map validation.
    pub event_index: Option<usize>,
    pub kind: ReplayStateErrorKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReplayStateErrorKind {
    #[error("invalid candidate profile, dimensions, or event count")]
    Candidate,
    #[error("invalid prelude or conclusion placement")]
    Markers,
    #[error("timestamps exceed the stricter core-v1 chronology profile")]
    Chronology,
    #[error("invalid tool-map identity, range, capability, step bits, or tip")]
    ToolMap,
    #[error("tool is outside the supported brush subset")]
    UnsupportedTool,
    #[error("dynamics are outside the supported subset")]
    Dynamics,
    #[error("tool setting is outside its supported range or capability")]
    ToolSetting,
    #[error("nested stroke start")]
    NestedStroke,
    #[error("draw or commit without an open stroke")]
    NoStroke,
    #[error("setting, layer, history action, or conclusion inside a stroke")]
    DuringStroke,
    #[error("stroke coordinate is outside the canvas")]
    Coordinate,
    #[error("stroke starts on a hidden layer")]
    HiddenLayer,
    #[error("layer identity does not exist")]
    MissingLayer,
    #[error("selected-layer alpha requires a nonempty selection and unit alpha")]
    LayerAlpha,
    #[error("creation exceeds the eight-layer profile")]
    LayerLimit,
    #[error("delete, merge, and move are outside the supported subset")]
    UnsupportedLayerAction,
    #[error("undo or redo stack is empty")]
    EmptyHistory,
    #[error("undo across Add is outside the supported subset")]
    AddHistoryBoundary,
}

/// A borrowed candidate plus diagnostic logical state, never playback approval.
/// No unchecked constructor, deserialization, or execution conversion is exposed.
#[derive(Debug)]
pub struct StateCheckedCandidate<'a> {
    replay: &'a UntrustedReplay,
    final_state: ReplayState,
}

impl<'a> StateCheckedCandidate<'a> {
    /// Still untrusted for every purpose other than the documented state check.
    pub fn untrusted_replay(&self) -> &'a UntrustedReplay {
        self.replay
    }

    pub fn final_state(&self) -> &ReplayState {
        &self.final_state
    }
}

/// Map values are assigned directly in the source; only selected tools have
/// applied setter state. Enabled preserve-alpha starts false for every tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolState {
    pub map: UntrustedReplayTool,
    pub preserve_alpha: bool,
    pub applied: Option<AppliedToolState>,
    pub last_position: Option<(i16, i16)>,
}

/// Logical setter effects only. No kernel, tone cache, or Canvas is generated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppliedToolState {
    pub alpha: f32,
    /// Source input bits, preserved without a Rust floating-math approximation.
    pub flow: f32,
    pub flow_easing: FlowEasing,
    pub color: [u8; 3],
    pub size: u8,
    pub tip_id: i8,
}

/// Symbolic source formula only. A future consumer must separately qualify its
/// numeric semantics; Rust powi/powf and JS Math.pow need not be bit-identical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowEasing {
    /// flow
    Identity,
    /// 1 - Math.sqrt(1 - Math.pow(flow, 3))
    Pen,
    /// 1 - Math.sqrt(1 - flow)
    Airbrush,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PressureInput {
    Encoded(u16),
    NoPressure,
}

impl PressureInput {
    pub fn source_value(self) -> f64 {
        match self {
            Self::Encoded(value) => f64::from(value) / 65535.0,
            Self::NoPressure => 0.5 / 65535.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PressureState {
    pub previous: PressureInput,
    pub current: PressureInput,
}

/// Opaque logical content marker, not pixels, a digest, or proof of a change.
/// Equal markers mean a history restore points to the same symbolic content;
/// different markers do not imply different pixels (even zero-alpha draws get
/// a new marker). Initial blank content has marker zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentMarker(pub usize);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerState {
    pub id: u8,
    pub visible: bool,
    pub alpha: f32,
    pub content: ContentMarker,
}

/// Identifies a source snapshot-copy site; no payload allocation is simulated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SnapshotSite {
    pub event_index: usize,
    pub content: ContentMarker,
}

#[derive(Debug, PartialEq)]
pub struct HistoryAction {
    /// Creation event, also a stable identity across all retained references.
    pub event_index: usize,
    pub kind: HistoryActionKind,
}

#[derive(Debug, PartialEq)]
pub enum HistoryActionKind {
    Draw {
        layer_id: u8,
        before: SnapshotSite,
        after: Option<SnapshotSite>,
    },
    Alpha {
        /// In insertion order, not stack order or sorted layer IDs.
        before: Vec<(u8, f32)>,
        new_alpha: f32,
    },
    AddLayer {
        layer_id: u8,
    },
    Dummy,
}

#[derive(Clone, Copy, Debug)]
struct OpenStroke {
    layer_id: u8,
    tool_id: u8,
}

/// Read-only diagnostic state. Canvas context properties, pixels, rendering
/// buffers, allocations, and UI behavior are deliberately outside this model.
#[derive(Debug)]
pub struct ReplayState {
    tools: [ToolState; TOOL_COUNT],
    tool_id: u8,
    color: [u8; 3],
    layers: Vec<LayerState>,
    layer_counter: u8,
    active_layer: u8,
    selected_layers: Vec<u8>,
    undo: Vec<Rc<HistoryAction>>,
    redo: Vec<Rc<HistoryAction>>,
    pending: Option<Rc<HistoryAction>>,
    stroke: Option<OpenStroke>,
    pressure: PressureState,
}

impl ReplayState {
    pub fn tools(&self) -> &[ToolState; TOOL_COUNT] {
        &self.tools
    }
    pub fn tool_id(&self) -> u8 {
        self.tool_id
    }
    pub fn color(&self) -> [u8; 3] {
        self.color
    }
    /// Bottom-to-top order is separate from IDs.
    pub fn layers(&self) -> &[LayerState] {
        &self.layers
    }
    pub fn layer_counter(&self) -> u8 {
        self.layer_counter
    }
    pub fn active_layer(&self) -> u8 {
        self.active_layer
    }
    pub fn selected_layers(&self) -> &[u8] {
        &self.selected_layers
    }
    pub fn pressure(&self) -> PressureState {
        self.pressure
    }
    pub fn undo_history(&self) -> impl ExactSizeIterator<Item = &HistoryAction> {
        self.undo.iter().map(Rc::as_ref)
    }
    pub fn redo_history(&self) -> impl ExactSizeIterator<Item = &HistoryAction> {
        self.redo.iter().map(Rc::as_ref)
    }
    pub fn pending_action(&self) -> Option<&HistoryAction> {
        self.pending.as_deref()
    }

    fn new(replay: &UntrustedReplay) -> Self {
        let mut state = Self {
            tools: replay.tools.map(|map| ToolState {
                map,
                preserve_alpha: false,
                applied: None,
                last_position: None,
            }),
            tool_id: replay.metadata.tool_id,
            color: replay.metadata.color,
            layers: vec![LayerState {
                id: 1,
                visible: true,
                alpha: 1.0,
                content: ContentMarker(0),
            }],
            layer_counter: 1,
            active_layer: 1,
            selected_layers: vec![1],
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            stroke: None,
            pressure: PressureState {
                previous: PressureInput::Encoded(0),
                current: PressureInput::Encoded(0),
            },
        };
        state.select_tool(replay.metadata.tool_id);
        state
    }

    fn tool_mut(&mut self) -> &mut ToolState {
        &mut self.tools[usize::from(self.tool_id - 1)]
    }

    fn select_tool(&mut self, id: u8) {
        self.tool_id = id;
        let color = self.color;
        let tool = self.tool_mut();
        tool.applied = Some(AppliedToolState {
            alpha: tool.map.alpha,
            flow: tool.map.flow,
            flow_easing: flow_easing(id),
            color,
            size: tool.map.size,
            tip_id: tool.map.tip_id,
        });
    }

    fn layer_index(&self, id: u8) -> Result<usize, ReplayStateErrorKind> {
        self.layers
            .iter()
            .position(|layer| layer.id == id)
            .ok_or(ReplayStateErrorKind::MissingLayer)
    }

    fn activate(&mut self, id: u8) -> Result<(), ReplayStateErrorKind> {
        // Source setActiveLayer(0) resolves the existing top layer. Layers never
        // become empty in this creation-only profile.
        let id = if id == 0 {
            self.layers[self.layers.len() - 1].id
        } else {
            id
        };
        self.layer_index(id)?;
        self.active_layer = id;
        self.selected_layers.clear();
        self.selected_layers.push(id);
        Ok(())
    }

    fn push(&mut self, action: Rc<HistoryAction>) {
        self.undo.push(action);
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        // pending is intentionally independent of both stacks.
    }

    fn start(
        &mut self,
        index: usize,
        x: i16,
        y: i16,
        pressure: PressureInput,
    ) -> Result<(), ReplayStateErrorKind> {
        if self.stroke.is_some() {
            return Err(ReplayStateErrorKind::NestedStroke);
        }
        let layer = self.layers[self.layer_index(self.active_layer)?];
        if !layer.visible {
            return Err(ReplayStateErrorKind::HiddenLayer);
        }
        self.pending = Some(Rc::new(HistoryAction {
            event_index: index,
            kind: HistoryActionKind::Draw {
                layer_id: layer.id,
                before: SnapshotSite {
                    event_index: index,
                    content: layer.content,
                },
                after: None,
            },
        }));
        self.stroke = Some(OpenStroke {
            layer_id: layer.id,
            tool_id: self.tool_id,
        });
        self.pressure = PressureState {
            previous: pressure,
            current: pressure,
        };
        self.tool_mut().last_position = Some((x, y));
        Ok(())
    }

    fn draw(
        &mut self,
        x: i16,
        y: i16,
        pressure: PressureInput,
    ) -> Result<(), ReplayStateErrorKind> {
        let stroke = self.stroke.ok_or(ReplayStateErrorKind::NoStroke)?;
        // All state changes are rejected while open, including tool/layer edits.
        debug_assert_eq!(stroke.layer_id, self.active_layer);
        debug_assert_eq!(stroke.tool_id, self.tool_id);
        self.pressure.previous = self.pressure.current;
        self.pressure.current = pressure;
        self.tool_mut().last_position = Some((x, y));
        Ok(())
    }

    fn commit(&mut self, index: usize) -> Result<(), ReplayStateErrorKind> {
        let stroke = self.stroke.take().ok_or(ReplayStateErrorKind::NoStroke)?;
        let content = ContentMarker(index + 1);
        let layer = self.layer_index(stroke.layer_id)?;
        self.layers[layer].content = content;
        // The open action has not been pushed yet, so pending is its sole owner.
        // Paired-stroke checks prevent the source's malformed repeated-commit
        // aliasing; after this push both references really share the same Rc.
        let action = self
            .pending
            .as_mut()
            .expect("open stroke owns pending Draw");
        let HistoryActionKind::Draw { after, .. } = &mut Rc::get_mut(action)
            .expect("uncommitted Draw has one owner")
            .kind
        else {
            unreachable!("only start creates a pending action")
        };
        *after = Some(SnapshotSite {
            event_index: index,
            content,
        });
        let action = Rc::clone(action);
        self.push(action);
        Ok(())
    }

    fn alpha(&mut self, index: usize, alpha: f32) -> Result<(), ReplayStateErrorKind> {
        if !unit(alpha) || self.selected_layers.is_empty() {
            return Err(ReplayStateErrorKind::LayerAlpha);
        }
        let mut before = Vec::with_capacity(self.selected_layers.len());
        for &id in &self.selected_layers {
            let layer = self.layer_index(id)?;
            before.push((id, self.layers[layer].alpha));
            self.layers[layer].alpha = alpha;
        }
        if let Some(last) = self.undo.last_mut()
            && let HistoryActionKind::Alpha {
                before: previous, ..
            } = &last.kind
            && previous
                .iter()
                .map(|&(id, _)| id)
                .eq(before.iter().map(|&(id, _)| id))
        {
            // Alpha is never pending and valid actions cannot be on both stacks.
            let HistoryActionKind::Alpha { new_alpha, .. } =
                &mut Rc::get_mut(last).expect("Alpha has one stack owner").kind
            else {
                unreachable!()
            };
            *new_alpha = alpha;
            return Ok(()); // Source's early return DOES NOT clear redo.
        }
        self.push(Rc::new(HistoryAction {
            event_index: index,
            kind: HistoryActionKind::Alpha {
                before,
                new_alpha: alpha,
            },
        }));
        Ok(())
    }

    fn history(&mut self, redo: bool) -> Result<(), ReplayStateErrorKind> {
        let stack = if redo { &self.redo } else { &self.undo };
        let action = stack.last().ok_or(ReplayStateErrorKind::EmptyHistory)?;
        if matches!(action.kind, HistoryActionKind::AddLayer { .. }) {
            return Err(ReplayStateErrorKind::AddHistoryBoundary);
        }
        let action = Rc::clone(action);
        match &action.kind {
            HistoryActionKind::Draw {
                layer_id,
                before,
                after,
            } => {
                let snapshot = if redo {
                    after.as_ref().expect("stack Draw was committed")
                } else {
                    before
                };
                let layer = self.layer_index(*layer_id)?;
                self.layers[layer].content = snapshot.content;
                self.activate(*layer_id)?;
            }
            HistoryActionKind::Alpha { before, new_alpha } => {
                for &(id, old_alpha) in before {
                    let layer = self.layer_index(id)?;
                    self.layers[layer].alpha = if redo { *new_alpha } else { old_alpha };
                }
            }
            HistoryActionKind::Dummy => {}
            HistoryActionKind::AddLayer { .. } => unreachable!("Add checked before mutation"),
        }
        if redo {
            self.redo.pop();
            self.undo.push(action);
        } else {
            self.undo.pop();
            self.redo.push(action);
        }
        Ok(())
    }

    fn event(
        &mut self,
        replay: &UntrustedReplay,
        index: usize,
        event: Event,
    ) -> Result<(), ReplayStateErrorKind> {
        use ReplayStateErrorKind as Error;
        if self.stroke.is_some()
            && !matches!(
                event,
                Event::DrawStart { .. }
                    | Event::DrawStartNoPressure { .. }
                    | Event::Draw { .. }
                    | Event::DrawNoPressure { .. }
                    | Event::DrawCommit
            )
        {
            return Err(Error::DuringStroke);
        }
        match event {
            Event::Prelude | Event::Conclusion => {}
            Event::DrawStart { x, y, pressure } => {
                coordinates(replay, x, y)?;
                self.start(index, x, y, PressureInput::Encoded(pressure))?;
            }
            Event::DrawStartNoPressure { x, y } => {
                coordinates(replay, x, y)?;
                self.start(index, x, y, PressureInput::NoPressure)?;
            }
            Event::Draw { x, y, pressure } => {
                coordinates(replay, x, y)?;
                self.draw(x, y, PressureInput::Encoded(pressure))?;
            }
            Event::DrawNoPressure { x, y } => {
                coordinates(replay, x, y)?;
                self.draw(x, y, PressureInput::NoPressure)?;
            }
            Event::DrawCommit => self.commit(index)?,
            Event::Undo => self.history(false)?,
            Event::Redo => self.history(true)?,
            Event::SetTool(id) => {
                if !supported_tool(id) {
                    return Err(Error::UnsupportedTool);
                }
                self.select_tool(id);
            }
            Event::SetColor(color) => {
                self.color = color;
                self.tool_mut()
                    .applied
                    .as_mut()
                    .expect("selected tool is applied")
                    .color = color;
            }
            Event::SetToolSize(size) => {
                if !(1..=64).contains(&size) {
                    return Err(Error::ToolSetting);
                }
                let tool = self.tool_mut();
                tool.map.size = size;
                tool.applied
                    .as_mut()
                    .expect("selected tool is applied")
                    .size = size;
            }
            Event::SetToolAlpha(alpha) => {
                if !unit(alpha) {
                    return Err(Error::ToolSetting);
                }
                let tool = self.tool_mut();
                tool.map.alpha = alpha;
                tool.applied
                    .as_mut()
                    .expect("selected tool is applied")
                    .alpha = alpha;
            }
            Event::SetToolFlow(flow) => {
                if !unit(flow) {
                    return Err(Error::ToolSetting);
                }
                let tool = self.tool_mut();
                tool.map.flow = flow;
                tool.applied
                    .as_mut()
                    .expect("selected tool is applied")
                    .flow = flow;
            }
            Event::SetToolTip(tip) => {
                if self.tool_id != 8 || tip > 2 {
                    return Err(Error::ToolSetting);
                }
                let tool = self.tool_mut();
                tool.map.tip_id = tip as i8;
                tool.applied
                    .as_mut()
                    .expect("selected tool is applied")
                    .tip_id = tip as i8;
            }
            Event::PreserveAlpha(enabled) => {
                let tool = self.tool_mut();
                if !tool.map.use_preserve_alpha {
                    return Err(Error::ToolSetting);
                }
                tool.preserve_alpha = enabled;
            }
            Event::SetToolSizeDynamics(_)
            | Event::SetToolAlphaDynamics(_)
            | Event::SetToolFlowDynamics(_) => return Err(Error::Dynamics),
            Event::AddLayer => {
                if self.layers.len() >= MAX_LAYERS {
                    return Err(Error::LayerLimit);
                }
                let position = self.layer_index(self.active_layer)? + 1;
                self.layer_counter += 1; // At most eight; no deletion/counter reuse.
                let id = self.layer_counter;
                self.layers.insert(
                    position,
                    LayerState {
                        id,
                        visible: true,
                        alpha: 1.0,
                        content: ContentMarker(0),
                    },
                );
                self.push(Rc::new(HistoryAction {
                    event_index: index,
                    kind: HistoryActionKind::AddLayer { layer_id: id },
                }));
                self.activate(id)?;
            }
            Event::DeleteLayers | Event::MoveLayers(_) | Event::MergeLayers => {
                return Err(Error::UnsupportedLayerAction);
            }
            Event::ToggleLayerVisibility(id) => {
                let layer = self.layer_index(id)?;
                self.layers[layer].visible = !self.layers[layer].visible;
            }
            Event::SetActiveLayer(id) => self.activate(id)?,
            Event::ToggleLayerSelection(id) => {
                self.layer_index(id)?;
                if let Some(position) = self
                    .selected_layers
                    .iter()
                    .position(|&selected| selected == id)
                {
                    self.selected_layers.remove(position);
                } else {
                    self.selected_layers.push(id);
                }
            }
            Event::SetSelectedLayersAlpha(alpha) => self.alpha(index, alpha)?,
            Event::HistoryDummy => self.push(Rc::new(HistoryAction {
                event_index: index,
                kind: HistoryActionKind::Dummy,
            })),
        }
        Ok(())
    }
}

/// Check logical core-v1 only. Publicly constructible UntrustedReplay fields are
/// rechecked here rather than assuming the wire decoder was actually called.
/// On failure no partial candidate or externally mutated state is returned.
/// The borrowed events, float bits, and pressure variants are never rewritten.
pub fn check_core_v1(
    replay: &UntrustedReplay,
) -> Result<StateCheckedCandidate<'_>, ReplayStateError> {
    use ReplayStateErrorKind as Error;
    let fail = |kind| ReplayStateError {
        event_index: None,
        kind,
    };
    if replay.candidate_profile != CANDIDATE_PROFILE
        || !(1..=MAX_CANVAS_SIDE).contains(&replay.metadata.width)
        || !(1..=MAX_CANVAS_SIDE).contains(&replay.metadata.height)
        || !(2..=MAX_EVENTS).contains(&replay.events.len())
    {
        return Err(fail(Error::Candidate));
    }
    if !supported_tool(replay.metadata.tool_id) {
        return Err(fail(Error::UnsupportedTool));
    }
    let seconds = replay
        .metadata
        .ended_at_seconds
        .checked_sub(replay.metadata.started_at_seconds);
    if seconds.is_none_or(|duration| duration > MAX_DURATION_MS / 1000) {
        return Err(fail(Error::Chronology));
    }
    for (offset, tool) in replay.tools.iter().enumerate() {
        if tool.size_dynamics || tool.alpha_dynamics || tool.flow_dynamics {
            return Err(fail(Error::Dynamics));
        }
        if usize::from(tool.id) != offset + 1
            || !(1..=64).contains(&tool.size)
            || !unit(tool.alpha)
            || !unit(tool.flow)
            || tool.step.to_bits() != SOURCE_STEPS[offset].to_bits()
            || tool.use_preserve_alpha != matches!(tool.id, 1 | 2 | 3 | 5)
            || (tool.id == 8 && !(0..=2).contains(&tool.tip_id))
            || (tool.id != 8 && tool.tip_id != 0)
        {
            return Err(fail(Error::ToolMap));
        }
    }
    let duration = replay
        .events
        .last()
        .expect("bounded nonempty events")
        .timestamp_ms
        .checked_sub(replay.events[0].timestamp_ms);
    if !duration.is_some_and(|duration| (1..=MAX_DURATION_MS).contains(&duration)) {
        return Err(fail(Error::Chronology));
    }
    let mut state = ReplayState::new(replay);
    let mut previous_time = replay.events[0].timestamp_ms;
    for (index, event) in replay.events.iter().enumerate() {
        let fail = |kind| ReplayStateError {
            event_index: Some(index),
            kind,
        };
        if (index == 0) != matches!(event.kind, Event::Prelude)
            || (index == replay.events.len() - 1) != matches!(event.kind, Event::Conclusion)
        {
            return Err(fail(Error::Markers));
        }
        if event.timestamp_ms < previous_time {
            return Err(fail(Error::Chronology));
        }
        previous_time = event.timestamp_ms;
        state.event(replay, index, event.kind).map_err(fail)?;
    }
    Ok(StateCheckedCandidate {
        replay,
        final_state: state,
    })
}

const SOURCE_STEPS: [f32; TOOL_COUNT] = [0.01, 0.05, 0.1, 100.0, 0.01, 100.0, 0.25, 0.1];
fn supported_tool(id: u8) -> bool {
    matches!(id, 1 | 2 | 3 | 5 | 7 | 8)
}
fn unit(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
fn flow_easing(id: u8) -> FlowEasing {
    match id {
        2 => FlowEasing::Pen,
        3 => FlowEasing::Airbrush,
        _ => FlowEasing::Identity,
    }
}
fn coordinates(replay: &UntrustedReplay, x: i16, y: i16) -> Result<(), ReplayStateErrorKind> {
    if x < 0 || y < 0 || x as u16 >= replay.metadata.width || y as u16 >= replay.metadata.height {
        Err(ReplayStateErrorKind::Coordinate)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_wire::{UntrustedReplayEvent, decode};
    use Event as E;
    use ReplayStateErrorKind as Error;

    const EMPTY: &[u8] = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr");
    const COMMANDS: &[u8] =
        include_bytes!("../../../tests/media/fixtures/replay-wire/commands.ibr");

    fn replay(events: &[Event]) -> UntrustedReplay {
        let mut replay = decode(EMPTY).unwrap();
        replay.events = std::iter::once(E::Prelude)
            .chain(events.iter().copied())
            .chain(std::iter::once(E::Conclusion))
            .enumerate()
            .map(|(index, kind)| UntrustedReplayEvent {
                timestamp_ms: 100 + index as u32,
                kind,
            })
            .collect();
        replay
    }
    fn run(events: &[Event]) -> ReplayState {
        check_core_v1(&replay(events)).unwrap().final_state
    }
    fn error(replay: &UntrustedReplay, kind: Error) -> ReplayStateError {
        let error = check_core_v1(replay).unwrap_err();
        assert_eq!(error.kind, kind);
        error
    }
    fn reject(events: &[Event], kind: Error) {
        error(&replay(events), kind);
    }
    fn stroke() -> [Event; 3] {
        [
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::Draw {
                x: 1,
                y: 1,
                pressure: 32768,
            },
            E::DrawCommit,
        ]
    }
    fn orders(state: &ReplayState) -> Vec<u8> {
        state.layers().iter().map(|layer| layer.id).collect()
    }
    fn alphas(state: &ReplayState) -> Vec<f32> {
        state.layers().iter().map(|layer| layer.alpha).collect()
    }

    #[test]
    fn real_pinned_recorder_empty_fixture_is_supported_without_normalization() {
        // Recorder provenance and independent Python transcription pins are in
        // tests/media/fixtures/replay-wire/README.md. The fixture itself, rather
        // than a host-produced encoder round trip, is the acceptance control.
        let replay = decode(EMPTY).unwrap();
        let candidate = check_core_v1(&replay).unwrap();
        assert!(std::ptr::eq(candidate.untrusted_replay(), &replay));
        let state = candidate.final_state();
        assert_eq!(orders(state), [1]);
        assert_eq!(state.selected_layers(), [1]);
        assert_eq!(state.active_layer(), 1);
        assert_eq!(state.layer_counter(), 1);
        assert_eq!(state.undo_history().len(), 0);
        assert_eq!(state.redo_history().len(), 0);
        assert_eq!(state.pending_action(), None);
        assert_eq!(state.tool_id(), 1);
        assert_eq!(state.color(), [0; 3]);
        for (i, tool) in state.tools().iter().enumerate() {
            assert!(!tool.preserve_alpha);
            assert_eq!(tool.applied.is_some(), i == 0);
            assert_eq!(tool.last_position, None);
        }
    }

    #[test]
    fn all_tag_recorder_fixture_is_explicitly_outside_the_profile() {
        let replay = decode(COMMANDS).unwrap();
        assert_eq!(error(&replay, Error::Dynamics).event_index, Some(5));
    }

    #[test]
    fn public_candidate_does_not_bypass_structural_preconditions() {
        for profile in [0, 2, u16::MAX] {
            let mut r = replay(&[]);
            r.candidate_profile = profile;
            error(&r, Error::Candidate);
        }
        for side in [0, MAX_CANVAS_SIDE + 1, u16::MAX] {
            let mut r = replay(&[]);
            r.metadata.width = side;
            error(&r, Error::Candidate);
            let mut r = replay(&[]);
            r.metadata.height = side;
            error(&r, Error::Candidate);
        }
        for count in [0, 1, MAX_EVENTS + 1] {
            let mut r = replay(&[]);
            r.events.resize(
                count,
                UntrustedReplayEvent {
                    timestamp_ms: 100,
                    kind: E::HistoryDummy,
                },
            );
            error(&r, Error::Candidate);
        }
        for (width, height) in [(1, 1), (MAX_CANVAS_SIDE, MAX_CANVAS_SIDE)] {
            let mut r = replay(&[]);
            r.metadata.width = width;
            r.metadata.height = height;
            check_core_v1(&r).unwrap();
        }
        let r = replay(&vec![E::HistoryDummy; MAX_EVENTS - 2]);
        assert_eq!(
            check_core_v1(&r)
                .unwrap()
                .final_state()
                .undo_history()
                .len(),
            MAX_HISTORY
        );
    }

    #[test]
    fn public_candidate_markers_are_rechecked() {
        let mut r = replay(&[]);
        r.events[0].kind = E::HistoryDummy;
        error(&r, Error::Markers);
        let mut r = replay(&[]);
        r.events[1].kind = E::HistoryDummy;
        error(&r, Error::Markers);
        reject(&[E::Prelude], Error::Markers);
        reject(&[E::Conclusion], Error::Markers);
    }

    #[test]
    fn chronology_is_a_profile_choice_with_equal_epoch_and_event_times_allowed() {
        let mut r = replay(&[E::HistoryDummy, E::HistoryDummy]);
        r.metadata.ended_at_seconds = r.metadata.started_at_seconds;
        r.events[1].timestamp_ms = 100;
        r.events[2].timestamp_ms = 100;
        check_core_v1(&r).unwrap();
        r.events[2].timestamp_ms = 99;
        assert_eq!(error(&r, Error::Chronology).event_index, Some(2));
        let mut r = replay(&[]);
        r.metadata.ended_at_seconds = r.metadata.started_at_seconds - 1;
        error(&r, Error::Chronology);
        r.metadata.ended_at_seconds = r.metadata.started_at_seconds + MAX_DURATION_MS / 1000;
        check_core_v1(&r).unwrap();
        r.metadata.ended_at_seconds += 1;
        error(&r, Error::Chronology);
        for duration in [0, MAX_DURATION_MS + 1] {
            let mut r = replay(&[]);
            r.events[1].timestamp_ms = r.events[0].timestamp_ms + duration;
            error(&r, Error::Chronology);
        }
        let mut r = replay(&[]);
        r.events[0].timestamp_ms = u32::MAX - MAX_DURATION_MS;
        r.events[1].timestamp_ms = u32::MAX;
        check_core_v1(&r).unwrap();
        r.events[1].timestamp_ms = 0;
        error(&r, Error::Chronology);
    }

    #[test]
    fn every_tool_slot_checks_identity_size_step_tip_capability_and_unit_ranges() {
        for (slot, expected_step) in SOURCE_STEPS.iter().enumerate() {
            for id in [0, 9, 255] {
                let mut r = replay(&[]);
                r.tools[slot].id = id;
                error(&r, Error::ToolMap);
            }
            let mut r = replay(&[]);
            r.tools.swap(slot, (slot + 1) % TOOL_COUNT);
            error(&r, Error::ToolMap);
            for size in [0, 65, 255] {
                let mut r = replay(&[]);
                r.tools[slot].size = size;
                error(&r, Error::ToolMap);
            }
            for size in [1, 64] {
                let mut r = replay(&[]);
                r.tools[slot].size = size;
                check_core_v1(&r).unwrap();
            }
            for value in [
                -f32::MIN_POSITIVE,
                f32::from_bits(1.0_f32.to_bits() + 1),
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::MAX,
            ] {
                let mut r = replay(&[]);
                r.tools[slot].alpha = value;
                error(&r, Error::ToolMap);
                let mut r = replay(&[]);
                r.tools[slot].flow = value;
                error(&r, Error::ToolMap);
            }
            for value in [0.0, -0.0, 1.0] {
                let mut r = replay(&[]);
                r.tools[slot].alpha = value;
                r.tools[slot].flow = value;
                let state = check_core_v1(&r).unwrap().final_state;
                assert_eq!(state.tools()[slot].map.alpha.to_bits(), value.to_bits());
                assert_eq!(state.tools()[slot].map.flow.to_bits(), value.to_bits());
            }
            for step in [
                0.0,
                -0.0,
                -1.0,
                f32::NAN,
                f32::INFINITY,
                f32::from_bits(expected_step.to_bits() + 1),
                f32::from_bits(expected_step.to_bits() - 1),
            ] {
                let mut r = replay(&[]);
                r.tools[slot].step = step;
                error(&r, Error::ToolMap);
            }
            let mut r = replay(&[]);
            r.tools[slot].use_preserve_alpha = !r.tools[slot].use_preserve_alpha;
            error(&r, Error::ToolMap);
            for tip in [-128, -1, 3, 127] {
                let mut r = replay(&[]);
                r.tools[slot].tip_id = tip;
                error(&r, Error::ToolMap);
            }
            for tip in [1, 2] {
                let mut r = replay(&[]);
                r.tools[slot].tip_id = tip;
                if slot == 7 {
                    check_core_v1(&r).unwrap();
                } else {
                    error(&r, Error::ToolMap);
                }
            }
        }
    }

    #[test]
    fn dynamics_are_rejected_in_every_map_slot_and_even_false_event_changes() {
        for slot in 0..TOOL_COUNT {
            for flag in 0..3 {
                let mut r = replay(&[]);
                match flag {
                    0 => r.tools[slot].size_dynamics = true,
                    1 => r.tools[slot].alpha_dynamics = true,
                    _ => r.tools[slot].flow_dynamics = true,
                }
                error(&r, Error::Dynamics);
            }
        }
        for value in [false, true] {
            for event in [
                E::SetToolSizeDynamics(value),
                E::SetToolAlphaDynamics(value),
                E::SetToolFlowDynamics(value),
            ] {
                reject(&[event], Error::Dynamics);
            }
        }
    }

    #[test]
    fn every_initial_and_selected_tool_is_classified() {
        for id in 0..=u8::MAX {
            let mut r = replay(&[]);
            r.metadata.tool_id = id;
            if supported_tool(id) {
                check_core_v1(&r).unwrap();
                let state = run(&[E::SetTool(id)]);
                assert_eq!(state.tool_id(), id);
                assert!(state.tools()[usize::from(id - 1)].applied.is_some());
            } else {
                error(&r, Error::UnsupportedTool);
                reject(&[E::SetTool(id)], Error::UnsupportedTool);
            }
        }
    }

    #[test]
    fn direct_map_assignment_and_setter_reapplication_keep_per_tool_settings() {
        let mut r = replay(&[
            E::SetTool(2),
            E::SetToolFlow(0.5),
            E::SetToolSize(17),
            E::SetToolAlpha(0.25),
            E::PreserveAlpha(true),
            E::SetColor([12, 34, 56]),
            E::SetTool(3),
            E::SetColor([90, 80, 70]),
            E::SetTool(2),
            E::SetTool(2),
        ]);
        r.metadata.color = [3, 4, 5];
        r.tools[1].flow = 0.125;
        r.tools[1].alpha = 0.75;
        let state = check_core_v1(&r).unwrap().final_state;
        let pen = state.tools()[1];
        assert_eq!(pen.map.flow, 0.5);
        assert_eq!(pen.map.size, 17);
        assert_eq!(pen.map.alpha, 0.25);
        assert!(pen.preserve_alpha);
        let applied = pen.applied.unwrap();
        assert_eq!(applied.color, [90, 80, 70]);
        assert_eq!(applied.size, 17);
        assert_eq!(applied.alpha, 0.25);
        assert_eq!(applied.flow, 0.5);
        assert_eq!(applied.flow_easing, FlowEasing::Pen);
        assert_eq!(state.tools()[0].applied.unwrap().color, [3, 4, 5]);
        assert_eq!(state.tools()[4].applied, None); // unused tone map never applied
        assert_eq!(state.color(), [90, 80, 70]);
        let airbrush = run(&[E::SetTool(3), E::SetToolFlow(0.5)]);
        assert_eq!(airbrush.tools()[2].applied.unwrap().flow, 0.5);
        assert_eq!(
            airbrush.tools()[2].applied.unwrap().flow_easing,
            FlowEasing::Airbrush
        );
    }

    #[test]
    fn flow_is_symbolic_and_preserves_the_independent_numeric_mismatch_regression() {
        // Independent review ran the pinned production TegakiPen.setFlow at
        // these source f32 bits. Source Math.pow produced brushFlow bits
        // 4571478264740775808; Rust powi produced 4571478264740775936. Do not
        // expose either approximation as source output or acceptance evidence.
        let bits = 1_045_417_920;
        let flow = f32::from_bits(bits);
        for (id, easing) in [
            (1, FlowEasing::Identity),
            (2, FlowEasing::Pen),
            (3, FlowEasing::Airbrush),
        ] {
            let mut r = replay(&[E::SetTool(id), E::SetToolFlow(flow), E::SetTool(id)]);
            r.metadata.tool_id = id;
            r.tools[usize::from(id - 1)].flow = flow;
            let candidate = check_core_v1(&r).unwrap();
            let tool = candidate.final_state().tools()[usize::from(id - 1)];
            assert_eq!(tool.map.flow.to_bits(), bits);
            assert_eq!(tool.applied.unwrap().flow.to_bits(), bits);
            assert_eq!(tool.applied.unwrap().flow_easing, easing);
            assert_eq!(
                candidate.untrusted_replay().tools[usize::from(id - 1)]
                    .flow
                    .to_bits(),
                bits
            );
            let E::SetToolFlow(original_flow) = candidate.untrusted_replay().events[2].kind else {
                panic!("original event changed")
            };
            assert_eq!(original_flow.to_bits(), bits);
        }
    }

    #[test]
    fn source_accepted_tool_setters_keep_unit_boundaries_and_negative_zero_bits() {
        for id in [1, 2, 3, 5, 7, 8] {
            for value in [0.0, -0.0, 1.0] {
                let state = run(&[
                    E::SetTool(id),
                    E::SetToolAlpha(value),
                    E::SetToolFlow(value),
                ]);
                assert_eq!(
                    state.tools()[usize::from(id - 1)].map.alpha.to_bits(),
                    value.to_bits()
                );
                assert_eq!(
                    state.tools()[usize::from(id - 1)].map.flow.to_bits(),
                    value.to_bits()
                );
            }
            for size in [1, 64] {
                run(&[E::SetTool(id), E::SetToolSize(size)]);
            }
            for size in [0, 65, 255] {
                reject(&[E::SetTool(id), E::SetToolSize(size)], Error::ToolSetting);
            }
            for value in [-0.001, 1.001, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                reject(
                    &[E::SetTool(id), E::SetToolAlpha(value)],
                    Error::ToolSetting,
                );
                reject(&[E::SetTool(id), E::SetToolFlow(value)], Error::ToolSetting);
            }
            for flag in [false, true] {
                if matches!(id, 1 | 2 | 3 | 5) {
                    run(&[E::SetTool(id), E::PreserveAlpha(flag)]);
                } else {
                    reject(
                        &[E::SetTool(id), E::PreserveAlpha(flag)],
                        Error::ToolSetting,
                    );
                }
            }
            for tip in [0, 1, 2, 3, 255] {
                if id == 8 && tip <= 2 {
                    assert_eq!(
                        run(&[E::SetTool(id), E::SetToolTip(tip)]).tools()[7]
                            .map
                            .tip_id,
                        tip as i8
                    );
                } else {
                    reject(&[E::SetTool(id), E::SetToolTip(tip)], Error::ToolSetting);
                }
            }
        }
    }

    #[test]
    fn strict_stroke_lifecycle_rejects_every_unpaired_operation() {
        let starts = [
            E::DrawStart {
                x: 0,
                y: 0,
                pressure: 0,
            },
            E::DrawStartNoPressure { x: 0, y: 0 },
        ];
        for start in starts {
            for nested in starts {
                reject(&[start, nested, E::DrawCommit], Error::NestedStroke);
            }
            reject(&[start], Error::DuringStroke);
            reject(&[start, E::DrawCommit, E::DrawCommit], Error::NoStroke);
        }
        for event in [
            E::Draw {
                x: 0,
                y: 0,
                pressure: 1,
            },
            E::DrawNoPressure { x: 0, y: 0 },
            E::DrawCommit,
        ] {
            reject(&[event], Error::NoStroke);
            reject(&[starts[0], E::DrawCommit, event], Error::NoStroke);
        }
    }

    #[test]
    fn every_setting_layer_history_and_marker_event_is_blocked_midstroke() {
        for event in [
            E::Undo,
            E::Redo,
            E::SetColor([1, 2, 3]),
            E::SetTool(2),
            E::SetToolSize(1),
            E::SetToolAlpha(0.5),
            E::SetToolSizeDynamics(false),
            E::SetToolAlphaDynamics(false),
            E::SetToolTip(0),
            E::PreserveAlpha(false),
            E::SetToolFlowDynamics(false),
            E::SetToolFlow(1.0),
            E::AddLayer,
            E::DeleteLayers,
            E::MoveLayers(0),
            E::MergeLayers,
            E::ToggleLayerVisibility(1),
            E::SetActiveLayer(0),
            E::ToggleLayerSelection(1),
            E::SetSelectedLayersAlpha(1.0),
            E::HistoryDummy,
        ] {
            reject(
                &[E::DrawStartNoPressure { x: 0, y: 0 }, event, E::DrawCommit],
                Error::DuringStroke,
            );
        }
        reject(
            &[E::DrawStartNoPressure { x: 0, y: 0 }],
            Error::DuringStroke,
        );
    }

    #[test]
    fn coordinates_are_checked_for_all_variants_at_both_boundaries() {
        for (x, y) in [(-1, 0), (0, -1), (640, 0), (0, 480), (i16::MIN, i16::MAX)] {
            for start in [
                E::DrawStart { x, y, pressure: 0 },
                E::DrawStartNoPressure { x, y },
            ] {
                reject(&[start, E::DrawCommit], Error::Coordinate);
            }
            for draw in [
                E::Draw {
                    x,
                    y,
                    pressure: u16::MAX,
                },
                E::DrawNoPressure { x, y },
            ] {
                reject(
                    &[E::DrawStartNoPressure { x: 0, y: 0 }, draw, E::DrawCommit],
                    Error::Coordinate,
                );
            }
        }
        for (width, height) in [(1, 1), (1024, 1024)] {
            let mut r = replay(&[
                E::DrawStartNoPressure { x: 0, y: 0 },
                E::DrawNoPressure {
                    x: width - 1,
                    y: height - 1,
                },
                E::DrawCommit,
            ]);
            r.metadata.width = width as u16;
            r.metadata.height = height as u16;
            check_core_v1(&r).unwrap();
        }
    }

    #[test]
    fn mixed_pressure_variants_and_repeated_times_remain_exact() {
        let events = [
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::Draw {
                x: 0,
                y: 0,
                pressure: 32768,
            },
            E::DrawNoPressure { x: 1, y: 1 },
            E::DrawCommit,
        ];
        let mut r = replay(&events);
        for event in &mut r.events[1..=4] {
            event.timestamp_ms = 100;
        }
        let candidate = check_core_v1(&r).unwrap();
        let pressure = candidate.final_state().pressure();
        assert_eq!(pressure.previous, PressureInput::Encoded(32768));
        assert_eq!(pressure.current, PressureInput::NoPressure);
        assert_eq!(pressure.current.source_value(), 0.5 / 65535.0);
        assert_ne!(pressure.current.source_value(), 0.5);
        assert_eq!(candidate.untrusted_replay().events[1].kind, events[0]);
        assert_eq!(candidate.untrusted_replay().events[2].timestamp_ms, 100);
        assert_eq!(
            candidate.final_state().tools()[0].last_position,
            Some((1, 1))
        );
        for pressure in [0, u16::MAX] {
            let state = run(&[
                E::DrawStart {
                    x: 0,
                    y: 0,
                    pressure,
                },
                E::DrawCommit,
            ]);
            assert_eq!(state.pressure().previous, PressureInput::Encoded(pressure));
            assert_eq!(state.pressure().current, PressureInput::Encoded(pressure));
        }
        let state = run(&[
            E::DrawStart {
                x: 0,
                y: 0,
                pressure: u16::MAX,
            },
            E::DrawNoPressure { x: 0, y: 0 },
            E::DrawCommit,
            E::DrawStartNoPressure { x: 1, y: 1 },
            E::DrawCommit,
        ]);
        assert_eq!(
            state.pressure(),
            PressureState {
                previous: PressureInput::NoPressure,
                current: PressureInput::NoPressure
            }
        );
    }

    // The following expectations are independently observed production-source
    // oracle results, not generated using this Rust transition model. Case names
    // are cited beside the corresponding assertions. The oracle's mock Canvas
    // tests snapshot copying separately; these tests assert symbolic ownership
    // and state only, never source pixel or Canvas equivalence.

    #[test]
    fn source_add_order_and_zero_active_fallback_use_ids_not_positions() {
        // layer-add-above-active-undo-redo-recreates-object-with-counter-reuse;
        // active-selection-numeric-coercion-strict-lookup-and-invalid-id-noop.
        let state = run(&[
            E::AddLayer,
            E::AddLayer,
            E::SetActiveLayer(1),
            E::ToggleLayerSelection(3),
            E::AddLayer,
        ]);
        assert_eq!(orders(&state), [1, 4, 2, 3]);
        assert_eq!(state.layer_counter(), 4);
        assert_eq!(state.active_layer(), 4);
        assert_eq!(state.selected_layers(), [4]);
        let state = run(&[
            E::AddLayer,
            E::AddLayer,
            E::SetActiveLayer(1),
            E::AddLayer,
            E::SetActiveLayer(0),
        ]);
        assert_eq!(state.active_layer(), 3);
        assert_eq!(state.selected_layers(), [3]);
        assert_eq!(orders(&state), [1, 4, 2, 3]);
    }

    #[test]
    fn layer_id_visibility_selection_and_eight_layer_limits_fail_closed() {
        for id in [2, 8, 255] {
            reject(&[E::SetActiveLayer(id)], Error::MissingLayer);
            reject(&[E::ToggleLayerSelection(id)], Error::MissingLayer);
            reject(&[E::ToggleLayerVisibility(id)], Error::MissingLayer);
        }
        reject(&[E::ToggleLayerSelection(0)], Error::MissingLayer);
        reject(&[E::ToggleLayerVisibility(0)], Error::MissingLayer);
        reject(
            &[
                E::ToggleLayerVisibility(1),
                E::DrawStartNoPressure { x: 0, y: 0 },
                E::DrawCommit,
            ],
            Error::HiddenLayer,
        );
        run(&[
            E::ToggleLayerVisibility(1),
            E::ToggleLayerVisibility(1),
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::DrawCommit,
        ]);
        assert_eq!(
            run(&[E::AddLayer; MAX_LAYERS - 1]).layers().len(),
            MAX_LAYERS
        );
        reject(&[E::AddLayer; MAX_LAYERS], Error::LayerLimit);
        let state = run(&[E::ToggleLayerSelection(1)]);
        assert!(state.selected_layers().is_empty());
        assert_eq!(state.active_layer(), 1);
        run(&[
            E::ToggleLayerSelection(1),
            E::DrawStartNoPressure { x: 0, y: 0 },
            E::DrawCommit,
        ]);
        reject(
            &[E::ToggleLayerSelection(1), E::SetSelectedLayersAlpha(0.5)],
            Error::LayerAlpha,
        );
    }

    #[test]
    fn unsupported_layer_operations_and_add_boundary_never_produce_a_candidate() {
        for event in [
            E::DeleteLayers,
            E::MergeLayers,
            E::MoveLayers(0),
            E::MoveLayers(2),
            E::MoveLayers(255),
        ] {
            reject(&[event], Error::UnsupportedLayerAction);
            reject(&[E::AddLayer, event], Error::UnsupportedLayerAction);
        }
        reject(&[E::AddLayer, E::Undo], Error::AddHistoryBoundary);
        reject(
            &[E::AddLayer, E::HistoryDummy, E::Undo, E::Undo],
            Error::AddHistoryBoundary,
        );
        run(&[E::AddLayer, E::HistoryDummy, E::Undo, E::Redo]);
    }

    #[test]
    fn selected_layer_alpha_rejects_nonunit_values_but_retains_f32_bits() {
        for alpha in [-0.01, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            reject(&[E::SetSelectedLayersAlpha(alpha)], Error::LayerAlpha);
        }
        // alpha-input-zero / float32-rounding / one in independent oracle.
        for alpha in [0.0_f32, -0.0, 0.1, 1.0] {
            let state = run(&[E::SetSelectedLayersAlpha(alpha)]);
            assert_eq!(state.layers()[0].alpha.to_bits(), alpha.to_bits());
            assert_eq!(state.undo_history().len(), 1);
        }
        assert_eq!(
            f64::from(run(&[E::SetSelectedLayersAlpha(0.1)]).layers()[0].alpha),
            0.10000000149011612
        );
    }

    #[test]
    fn source_alpha_undo_redo_leaves_active_selection_visibility_and_tools_alone() {
        // alpha-undo-redo-retains-active-and-ordered-multiselection.
        let mut events = vec![
            E::AddLayer,
            E::AddLayer,
            E::SetActiveLayer(1),
            E::SetSelectedLayersAlpha(0.25),
            E::SetActiveLayer(2),
            E::SetSelectedLayersAlpha(0.5),
            E::SetActiveLayer(3),
            E::ToggleLayerSelection(3),
            E::ToggleLayerSelection(2),
            E::ToggleLayerSelection(1),
            E::SetSelectedLayersAlpha(0.75),
            E::ToggleLayerVisibility(1),
            E::SetTool(8),
            E::SetToolTip(2),
            E::Undo,
        ];
        let state = run(&events);
        assert_eq!(alphas(&state), [0.25, 0.5, 1.0]);
        assert_eq!(state.active_layer(), 3);
        assert_eq!(state.selected_layers(), [2, 1]);
        assert!(!state.layers()[0].visible);
        assert_eq!(state.tool_id(), 8);
        assert_eq!(state.tools()[7].map.tip_id, 2);
        events.push(E::Redo);
        let state = run(&events);
        assert_eq!(alphas(&state), [0.75, 0.75, 1.0]);
        assert_eq!(state.active_layer(), 3);
        assert_eq!(state.selected_layers(), [2, 1]);
        assert!(!state.layers()[0].visible);
    }

    #[test]
    fn source_ordered_alpha_coalescing_keeps_earliest_before_and_existing_redo() {
        // alpha-successful-coalescing-retains-redo-array-and-suppresses-history-notice.
        let mut events = vec![
            E::AddLayer,
            E::ToggleLayerSelection(1),
            E::SetSelectedLayersAlpha(0.25),
            E::HistoryDummy,
            E::Undo,
            E::SetSelectedLayersAlpha(0.75),
        ];
        let state = run(&events);
        assert_eq!(state.selected_layers(), [2, 1]);
        assert_eq!(state.undo_history().len(), 2); // initial Add remains on replay history
        assert_eq!(state.redo_history().len(), 1);
        assert_eq!(
            state.undo_history().last().unwrap(),
            &HistoryAction {
                event_index: 3,
                kind: HistoryActionKind::Alpha {
                    before: vec![(2, 1.0), (1, 1.0)],
                    new_alpha: 0.75
                },
            }
        );
        assert!(matches!(
            state.redo_history().last().unwrap().kind,
            HistoryActionKind::Dummy
        ));
        events.push(E::Redo);
        let state = run(&events);
        assert!(matches!(
            state.undo_history().last().unwrap().kind,
            HistoryActionKind::Dummy
        ));
        assert_eq!(alphas(&state), [0.75, 0.75]);
        events.extend([E::Undo, E::Undo]);
        let state = run(&events);
        assert_eq!(alphas(&state), [1.0, 1.0]);
    }

    #[test]
    fn source_alpha_target_order_or_length_change_prevents_coalescing_and_clears_redo() {
        // alpha-order-change-prevents-coalescing-and-clears-redo;
        // alpha-coalesces-only-identical-ordered-ids-keeps-earliest-alphas.
        let events = [
            E::AddLayer,
            E::ToggleLayerSelection(1),
            E::SetSelectedLayersAlpha(0.25),
            E::HistoryDummy,
            E::Undo,
            E::SetActiveLayer(1),
            E::ToggleLayerSelection(2),
            E::SetSelectedLayersAlpha(0.75),
        ];
        let state = run(&events);
        assert_eq!(state.selected_layers(), [1, 2]);
        assert_eq!(state.undo_history().len(), 3);
        assert_eq!(state.redo_history().len(), 0);
        assert_eq!(
            state.undo_history().last().unwrap().kind,
            HistoryActionKind::Alpha {
                before: vec![(1, 0.25), (2, 0.25)],
                new_alpha: 0.75,
            }
        );
        let mut events = events.to_vec();
        events.extend([E::ToggleLayerSelection(2), E::SetSelectedLayersAlpha(0.5)]);
        assert_eq!(run(&events).undo_history().len(), 4);
    }

    #[test]
    fn coalescing_can_retain_a_draw_redo_and_its_pending_alias() {
        let mut events = vec![E::SetSelectedLayersAlpha(0.25)];
        events.extend(stroke());
        events.extend([E::Undo, E::SetSelectedLayersAlpha(0.75)]);
        let state = run(&events);
        assert_eq!(state.undo_history().len(), 1);
        assert_eq!(state.redo_history().len(), 1);
        assert!(std::ptr::eq(
            state.pending_action().unwrap(),
            state.redo_history().last().unwrap()
        ));
        assert_eq!(state.layers()[0].content, ContentMarker(0));
        events.push(E::Redo);
        let state = run(&events);
        assert_eq!(state.layers()[0].alpha, 0.75);
        assert_eq!(state.layers()[0].content, ContentMarker(5));
        assert!(std::ptr::eq(
            state.pending_action().unwrap(),
            state.undo_history().last().unwrap()
        ));
    }

    #[test]
    fn source_draw_restore_activates_target_without_rolling_back_unrelated_state() {
        // draw-snapshot-copy-undo-redo-active-selection-and-context.
        let mut events = vec![E::AddLayer, E::SetActiveLayer(1)];
        events.extend(stroke());
        events.extend([
            E::SetActiveLayer(2),
            E::ToggleLayerSelection(1),
            E::ToggleLayerVisibility(1),
            E::SetTool(2),
            E::SetColor([6, 7, 8]),
            E::Undo,
        ]);
        let state = run(&events);
        assert_eq!(state.layers()[0].content, ContentMarker(0));
        assert_eq!(state.active_layer(), 1);
        assert_eq!(state.selected_layers(), [1]);
        assert!(!state.layers()[0].visible);
        assert_eq!(state.tool_id(), 2);
        assert_eq!(state.color(), [6, 7, 8]);
        assert!(std::ptr::eq(
            state.pending_action().unwrap(),
            state.redo_history().last().unwrap()
        ));
        events.extend([E::SetActiveLayer(2), E::Redo]);
        let state = run(&events);
        assert_eq!(state.active_layer(), 1);
        assert_eq!(state.selected_layers(), [1]);
        assert_eq!(state.layers()[0].content, ContentMarker(6));
        assert!(!state.layers()[0].visible);
        assert!(std::ptr::eq(
            state.pending_action().unwrap(),
            state.undo_history().last().unwrap()
        ));
    }

    #[test]
    fn draw_snapshot_sites_preserve_previous_content_and_pending_is_replaced_only_on_start() {
        // replay-draw-start-replaces-pending-without-clearing-history;
        // replay-draw-commit-keeps-pending-and-repeated-commit-aliases-synthetic
        // supplies the source evidence; repeated commits are rejected here.
        let mut events = stroke().to_vec();
        events.extend(stroke());
        let state = run(&events);
        assert_eq!(state.undo_history().len(), 2);
        let first = state.undo_history().next().unwrap();
        let second = state.pending_action().unwrap();
        assert!(!std::ptr::eq(first, second));
        assert!(std::ptr::eq(second, state.undo_history().last().unwrap()));
        assert_eq!(
            first.kind,
            HistoryActionKind::Draw {
                layer_id: 1,
                before: SnapshotSite {
                    event_index: 1,
                    content: ContentMarker(0)
                },
                after: Some(SnapshotSite {
                    event_index: 3,
                    content: ContentMarker(4)
                }),
            }
        );
        assert_eq!(
            second.kind,
            HistoryActionKind::Draw {
                layer_id: 1,
                before: SnapshotSite {
                    event_index: 4,
                    content: ContentMarker(4)
                },
                after: Some(SnapshotSite {
                    event_index: 6,
                    content: ContentMarker(7)
                }),
            }
        );
        events.push(E::Undo);
        assert_eq!(run(&events).layers()[0].content, ContentMarker(4));
        events.push(E::Undo);
        assert_eq!(run(&events).layers()[0].content, ContentMarker(0));
        events.push(E::Redo);
        assert_eq!(run(&events).layers()[0].content, ContentMarker(4));
    }

    #[test]
    fn source_history_fifty_action_eviction_and_empty_history_rejection() {
        // history-fifty-action-eviction-and-persistent-pending: 75 pushes retain
        // original indexes25..74, independently asserted by the source harness.
        let mut events = vec![E::HistoryDummy; 75];
        let state = run(&events);
        assert_eq!(
            state
                .undo_history()
                .map(|action| action.event_index)
                .collect::<Vec<_>>(),
            (26..=75).collect::<Vec<_>>()
        );
        events.extend([E::Undo; 50]);
        let state = run(&events);
        assert_eq!(state.undo_history().len(), 0);
        assert_eq!(state.redo_history().len(), 50);
        events.extend([E::Redo; 50]);
        let state = run(&events);
        assert_eq!(state.undo_history().len(), 50);
        assert_eq!(state.redo_history().len(), 0);
        reject(&[E::Undo], Error::EmptyHistory);
        reject(&[E::Redo], Error::EmptyHistory);
        let mut too_many = vec![E::HistoryDummy; 75];
        too_many.extend([E::Undo; 51]);
        reject(&too_many, Error::EmptyHistory);
        events.push(E::Redo);
        reject(&events, Error::EmptyHistory);
    }

    #[test]
    fn pending_draw_survives_its_eviction_and_redo_invalidation() {
        // history-eviction-does-not-clear-pending-reference-to-evicted-action.
        let mut events = stroke().to_vec();
        events.extend([E::HistoryDummy; MAX_HISTORY]);
        let state = run(&events);
        assert_eq!(state.undo_history().len(), MAX_HISTORY);
        assert_eq!(state.pending_action().unwrap().event_index, 1);
        assert!(
            state
                .undo_history()
                .all(|action| !std::ptr::eq(action, state.pending_action().unwrap()))
        );
        assert!(matches!(
            state.pending_action().unwrap().kind,
            HistoryActionKind::Draw { after: Some(_), .. }
        ));
        let mut events = stroke().to_vec();
        events.extend([E::Undo, E::HistoryDummy]);
        let state = run(&events);
        assert_eq!(state.redo_history().len(), 0);
        assert_eq!(state.pending_action().unwrap().event_index, 1);
        assert!(matches!(
            state.undo_history().last().unwrap().kind,
            HistoryActionKind::Dummy
        ));
        events.extend(stroke());
        let state = run(&events);
        assert_eq!(state.pending_action().unwrap().event_index, 6);
        assert!(std::ptr::eq(
            state.pending_action().unwrap(),
            state.undo_history().last().unwrap()
        ));
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(512))]
        #[test]
        fn arbitrary_public_candidate_values_never_panic(
            bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1024),
            float_bits in proptest::prelude::any::<u32>(),
            mutation in 0_u8..32,
        ) {
            let value = f32::from_bits(float_bits);
            let events: Vec<_> = bytes.chunks(4).map(|chunk| {
                let tag = chunk[0] % 28;
                let id = *chunk.get(1).unwrap_or(&0);
                let x = i16::from(id) - 16;
                let y = i16::from(*chunk.get(2).unwrap_or(&0)) - 16;
                match tag {
                    0 => E::DrawStart { x, y, pressure: u16::from(id) * 257 },
                    1 => E::Draw { x, y, pressure: u16::from(id) * 257 },
                    2 => E::DrawStartNoPressure { x, y },
                    3 => E::DrawNoPressure { x, y },
                    4 => E::DrawCommit,
                    5 => E::Undo,
                    6 => E::Redo,
                    7 => E::SetColor([id, id, id]),
                    8 => E::SetTool(id),
                    9 => E::SetToolSize(id),
                    10 => E::SetToolAlpha(value),
                    11 => E::SetToolFlow(value),
                    12 => E::SetToolTip(id),
                    13 => E::PreserveAlpha(id % 2 == 0),
                    14 => E::SetToolSizeDynamics(id % 2 == 0),
                    15 => E::SetToolAlphaDynamics(id % 2 == 0),
                    16 => E::SetToolFlowDynamics(id % 2 == 0),
                    17 => E::AddLayer,
                    18 => E::DeleteLayers,
                    19 => E::MergeLayers,
                    20 => E::MoveLayers(id),
                    21 => E::SetActiveLayer(id),
                    22 => E::ToggleLayerSelection(id),
                    23 => E::ToggleLayerVisibility(id),
                    24 => E::SetSelectedLayersAlpha(value),
                    25 => E::HistoryDummy,
                    26 => E::Prelude,
                    _ => E::Conclusion,
                }
            }).collect();
            let mut r = replay(&events);
            let slot = usize::from(mutation % 8);
            match mutation {
                0 => r.tools[slot].alpha = value,
                1 => r.tools[slot].flow = value,
                2 => r.tools[slot].step = value,
                3 => r.tools[slot].id = bytes.first().copied().unwrap_or(0),
                4 => r.metadata.tool_id = bytes.first().copied().unwrap_or(0),
                5 => r.metadata.width = float_bits as u16,
                6 => r.metadata.height = float_bits as u16,
                7 => r.metadata.ended_at_seconds = float_bits,
                8 => r.events[0].timestamp_ms = float_bits,
                _ => {},
            }
            if let Ok(candidate) = check_core_v1(&r) {
                let state = candidate.final_state();
                proptest::prop_assert!((1..=MAX_LAYERS).contains(&state.layers().len()));
                proptest::prop_assert!(state.undo_history().len() + state.redo_history().len() <= MAX_HISTORY);
                proptest::prop_assert!(state.layers().iter().all(|layer| unit(layer.alpha)));
                proptest::prop_assert!(state.layers().iter().any(|layer| layer.id == state.active_layer()));
                for (offset, id) in state.selected_layers().iter().enumerate() {
                    proptest::prop_assert!(!state.selected_layers()[..offset].contains(id));
                    proptest::prop_assert!(state.layers().iter().any(|layer| layer.id == *id));
                }
                proptest::prop_assert!(state.tools()[usize::from(state.tool_id() - 1)].applied.is_some());
                if let Some(action) = state.pending_action() {
                    proptest::prop_assert!(matches!(action.kind, HistoryActionKind::Draw { after: Some(_), .. }), "pending action must be a committed Draw");
                }
            }
        }
    }

    #[test]
    fn every_noncoalesced_push_invalidates_redo_without_resetting_other_state() {
        for suffix in [
            vec![E::HistoryDummy],
            vec![E::AddLayer],
            vec![E::SetSelectedLayersAlpha(0.5)],
            stroke().to_vec(),
        ] {
            let mut events = vec![E::HistoryDummy, E::Undo];
            events.extend(suffix);
            assert_eq!(run(&events).redo_history().len(), 0);
            events.push(E::Redo);
            reject(&events, Error::EmptyHistory);
        }
        let state = run(&[
            E::HistoryDummy,
            E::Undo,
            E::SetColor([1, 2, 3]),
            E::SetTool(2),
            E::ToggleLayerVisibility(1),
            E::ToggleLayerSelection(1),
        ]);
        assert_eq!(state.redo_history().len(), 1);
        assert_eq!(state.undo_history().len(), 0);
        assert_eq!(state.color(), [1, 2, 3]);
        assert!(!state.layers()[0].visible);
        assert!(state.selected_layers().is_empty());
    }
}
