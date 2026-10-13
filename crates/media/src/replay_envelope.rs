//! Numeric envelope for the full pinned Tegaki event vocabulary.
//!
//! This deliberately does not reuse core-v1's six-tool/no-dynamics subset.
//! Passing establishes finite input domains, not valid layer/history state,
//! raster agreement, a CPU/RSS bound, publication authority or permission to
//! execute on the board's main thread. The consumer must retain its separate
//! worker allocation limits, command deadlines and generation ownership.

use crate::replay_wire::{self, UntrustedReplay, UntrustedReplayEventKind as Event};

pub const MAX_DURATION_MS: u32 = 24 * 60 * 60 * 1000;
const SOURCE_STEPS: [f32; 8] = [0.01, 0.05, 0.1, 100.0, 0.01, 100.0, 0.25, 0.1];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("replay exceeds the pinned consumer's numeric envelope")]
pub struct ReplayEnvelopeError;

/// A borrowed numeric check, never a storage or rendering capability.
pub struct EnvelopeCheckedReplay<'a> {
    replay: &'a UntrustedReplay,
}

impl EnvelopeCheckedReplay<'_> {
    pub fn candidate(&self) -> &UntrustedReplay {
        self.replay
    }
}

fn unit(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

pub fn check(replay: &UntrustedReplay) -> Result<EnvelopeCheckedReplay<'_>, ReplayEnvelopeError> {
    let reject = || Err(ReplayEnvelopeError);
    let m = replay.metadata;
    if replay.candidate_profile != replay_wire::CANDIDATE_PROFILE
        || !(1..=replay_wire::MAX_CANVAS_SIDE).contains(&m.width)
        || !(1..=replay_wire::MAX_CANVAS_SIDE).contains(&m.height)
        || !(1..=8).contains(&m.tool_id)
        || !(2..=replay_wire::MAX_EVENTS).contains(&replay.events.len())
        || m.ended_at_seconds
            .checked_sub(m.started_at_seconds)
            .is_none_or(|seconds| seconds > MAX_DURATION_MS / 1000)
    {
        return reject();
    }
    for (index, tool) in replay.tools.iter().enumerate() {
        if usize::from(tool.id) != index + 1
            || !(1..=64).contains(&tool.size)
            || !unit(tool.alpha)
            || !unit(tool.flow)
            || tool.step.to_bits() != SOURCE_STEPS[index].to_bits()
            || tool.use_preserve_alpha != matches!(tool.id, 1 | 2 | 3 | 5)
            || (tool.id == 8 && !(0..=2).contains(&tool.tip_id))
            || (tool.id != 8 && tool.tip_id != 0)
        {
            return reject();
        }
    }
    let first = replay.events[0].timestamp_ms;
    let last = replay.events[replay.events.len() - 1].timestamp_ms;
    if last
        .checked_sub(first)
        .is_none_or(|span| span > MAX_DURATION_MS)
    {
        return reject();
    }
    let mut previous = first;
    let mut selected = m.tool_id;
    for (index, event) in replay.events.iter().enumerate() {
        if event.timestamp_ms < previous
            || (index == 0) != matches!(event.kind, Event::Prelude)
            || (index + 1 == replay.events.len()) != matches!(event.kind, Event::Conclusion)
        {
            return reject();
        }
        previous = event.timestamp_ms;
        match event.kind {
            Event::SetTool(id) => {
                if !(1..=8).contains(&id) {
                    return reject();
                }
                selected = id;
            }
            Event::SetToolSize(size) if !(1..=64).contains(&size) => return reject(),
            Event::SetToolAlpha(value)
            | Event::SetToolFlow(value)
            | Event::SetSelectedLayersAlpha(value)
                if !unit(value) =>
            {
                return reject();
            }
            Event::SetToolTip(tip) if (selected == 8 && tip > 2) || (selected != 8 && tip != 0) => {
                return reject();
            }
            Event::DrawStart { x, y, .. }
            | Event::Draw { x, y, .. }
            | Event::DrawStartNoPressure { x, y }
            | Event::DrawNoPressure { x, y } => {
                // Real pointer strokes may leave the canvas. Permit one full
                // canvas on either side, while refusing the i16 extremes that
                // would otherwise multiply interpolation work without pixels.
                let w = i32::from(m.width);
                let h = i32::from(m.height);
                if !(-w..=2 * w).contains(&i32::from(x)) || !(-h..=2 * h).contains(&i32::from(y)) {
                    return reject();
                }
            }
            Event::Prelude
            | Event::Conclusion
            | Event::DrawCommit
            | Event::Undo
            | Event::Redo
            | Event::SetColor(_)
            | Event::SetToolSize(_)
            | Event::SetToolAlpha(_)
            | Event::SetToolFlow(_)
            | Event::SetToolTip(_)
            | Event::SetToolSizeDynamics(_)
            | Event::SetToolAlphaDynamics(_)
            | Event::SetToolFlowDynamics(_)
            | Event::PreserveAlpha(_)
            | Event::AddLayer
            | Event::DeleteLayers
            | Event::MoveLayers(_)
            | Event::MergeLayers
            | Event::ToggleLayerVisibility(_)
            | Event::SetActiveLayer(_)
            | Event::ToggleLayerSelection(_)
            | Event::SetSelectedLayersAlpha(_)
            | Event::HistoryDummy => {}
        }
    }
    Ok(EnvelopeCheckedReplay { replay })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay_wire::{UntrustedReplayEvent, decode};

    fn candidate() -> UntrustedReplay {
        decode(include_bytes!(
            "../../../tests/media/fixtures/replay-wire/empty.ibr"
        ))
        .unwrap()
    }

    #[test]
    fn all_source_tools_and_dynamics_remain_in_the_numeric_domain() {
        for id in 1..=8 {
            let mut replay = candidate();
            replay.metadata.tool_id = id;
            for tool in &mut replay.tools {
                tool.size_dynamics = true;
                tool.alpha_dynamics = true;
                tool.flow_dynamics = true;
            }
            assert!(check(&replay).is_ok());
        }
    }

    #[test]
    fn values_are_not_clamped_rewritten_or_given_execution_authority() {
        let mut replay = candidate();
        let before = replay.tools;
        let checked = check(&replay).unwrap();
        assert!(std::ptr::eq(checked.candidate(), &replay));
        assert_eq!(checked.candidate().tools, before);
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            replay.tools[0].alpha = value;
            assert!(check(&replay).is_err());
        }
    }

    #[test]
    fn coordinates_may_leave_canvas_but_extreme_interpolation_is_refused() {
        let mut replay = candidate();
        let stamp = replay.events[0].timestamp_ms;
        replay.events.insert(
            1,
            UntrustedReplayEvent {
                timestamp_ms: stamp,
                kind: Event::DrawStartNoPressure { x: -1, y: 0 },
            },
        );
        assert!(check(&replay).is_ok());
        replay.events[1].kind = Event::DrawStartNoPressure { x: i16::MIN, y: 0 };
        assert!(check(&replay).is_err());
    }

    #[test]
    fn recorder_stroke_is_admitted_but_structural_extrema_remain_rejected() {
        let stroke = decode(include_bytes!(
            "../../../tests/media/fixtures/replay-wire/bounded-stroke.ibr"
        ))
        .unwrap();
        assert!(check(&stroke).is_ok());
        let commands = decode(include_bytes!(
            "../../../tests/media/fixtures/replay-wire/commands.ibr"
        ))
        .unwrap();
        assert!(commands.events.iter().any(|event| matches!(
            event.kind,
            Event::DrawStartNoPressure {
                x: i16::MIN,
                y: i16::MAX
            }
        )));
        assert!(check(&commands).is_err());
    }

    #[test]
    fn layer_and_history_events_are_not_silently_replaced_by_a_smaller_vocabulary() {
        let mut replay = candidate();
        let stamp = replay.events[0].timestamp_ms;
        for kind in [
            Event::DeleteLayers,
            Event::MergeLayers,
            Event::MoveLayers(1),
            Event::SetToolSizeDynamics(true),
            Event::SetToolAlphaDynamics(true),
            Event::SetToolFlowDynamics(true),
            Event::Undo,
            Event::Redo,
        ] {
            replay.events.insert(
                1,
                UntrustedReplayEvent {
                    timestamp_ms: stamp,
                    kind,
                },
            );
        }
        // Logical execution can still reject this intentionally invalid history.
        assert!(check(&replay).is_ok());
    }
}
