//! Non-authoritative resource accounting for the restricted state-checked core.
//!
//! These are cumulative, checked-u64 source-work and request envelopes, not
//! measured time, live memory, an admission policy, or permission to play,
//! publish, store, or promote a replay. No production thresholds are supplied.
//! The input remains untrusted. A future consumer needs an independent
//! equivalence review, browser/native restrictions, and caller-chosen policy.
//!
//! Authority: Tegaki 0.9.4 production SHA-256
//! `daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744`.
//! Analytic envelopes and independently instrumented source counters are pinned
//! in `tests/media/fixtures/replay-cost`. The counters distinguish defined shape
//! work, brush leaf/selected-loop work, tone work, flood-stack scalar pushes,
//! typed backing requests, copies, clears, and Canvas API request volume. Units
//! from different counters are never silently summed into a single score.
//!
//! All allocation sites are charged, without refunds for history eviction,
//! replacement, alias removal, or GC. Every accepted Undo/Redo is charged as a
//! Draw restore. This deliberately overcounts Alpha/Dummy restores rather than
//! duplicating history classification. Canvas surfaces are nominal pixel areas
//! only, not a native-memory multiplier. Preview requests use the conservative
//! 24*24 destination envelope, even for a smaller source aspect ratio.
//!
//! Excluded: JS objects/arrays and their capacity, uninstrumented JS work,
//! cursor loops/storage, viewport-dependent canvas allocation, real UI/DOM/CSS,
//! native/GPU work/storage, GC/JIT, acquisition/decoding, concurrency, external
//! pointer/zoom activity, replay rewind/seek, and actual latency/RSS. Counting
//! dispatches records event overhead frequency, not its CPU or memory cost.

use crate::replay_state::StateCheckedCandidate;
use crate::replay_wire::{UntrustedReplayEventKind as Event, UntrustedReplayTool};

macro_rules! cost_metrics {
    ($( $variant:ident => $field:ident: $description:literal ),+ $(,)?) => {
        /// Independent model units. None is a time or total-memory metric.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CostMetric { $( #[doc = $description] $variant, )+ }

        impl CostMetric {
            pub const ALL: &'static [Self] = &[$( Self::$variant, )+];
        }

        /// A zero-initialized counter set is not a set of policy limits.
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct CostCounters { $( #[doc = $description] pub $field: u64, )+ }

        impl CostCounters {
            pub fn get(&self, metric: CostMetric) -> u64 {
                match metric { $( CostMetric::$variant => self.$field, )+ }
            }

            fn checked_accumulate(&mut self, other: &Self) -> Result<(), ReplayCostError> {
                // Build first so an overflow never leaves a partial update.
                let next = Self {
                    $( $field: add(CostMetric::$variant, self.$field, other.$field)?, )+
                };
                *self = next;
                Ok(())
            }

            fn component_max(&mut self, other: &Self) {
                $( self.$field = self.$field.max(other.$field); )+
            }
        }
    };
}

cost_metrics! {
    Events => events: "Parsed dispatch count, including markers and metadata-only/no-op events; not instruction work.",
    ShapeRegenerations => shape_regenerations: "Initial selected shape and every repeated selection, size, or accepted tip setter.",
    ShapeLoopUnits => shape_loop_units: "64H^2 defined instrumented shape-work units, H=4(size+2); not all JS operations.",
    ShapeFloodScalarPushes => shape_flood_scalar_pushes: "4H^2+2 scalar pushes per regeneration; not JS array backing bytes.",
    BrushLeafUnits => brush_leaf_units: "Traversal points, brush cells, blur neighbor cells and rectangle-copy cells.",
    BrushLoopUnits => brush_loop_units: "Selected brush/copy/neighbor loop bodies/tests and traversal points, using the corrected blur envelope.",
    ToneMapCells => tone_map_cells: "16A map-cell bodies on first tone use at fixed dimensions.",
    ToneLoopUnits => tone_loop_units: "32A+48*height+49 selected map/row/cell loop bodies/tests on first tone use.",
    TypedBackingBytes => typed_backing_bytes: "Cumulative new typed backing requests, including conservative shape allocations; never live heap or RSS.",
    TypedCopyBytes => typed_copy_bytes: "Explicit typed-array snapshot/restore copy bytes, separate from allocation.",
    TypedClearBytes => typed_clear_bytes: "Explicit ghost/blend fill bytes; not allocation zero-initialization.",
    CanvasSurfacePixels => canvas_surface_pixels: "Cumulative nominal background/layer/optional-flatten surface pixels; excludes previews and viewport/cursor.",
    PreviewSurfacePixels => preview_surface_pixels: "Cumulative nominal preview surface requests at at most 576 pixels each; not native bytes.",
    CanvasGetCalls => canvas_get_calls: "Layer getImageData API calls.",
    CanvasGetPixels => canvas_get_pixels: "Layer getImageData requested pixels.",
    CanvasFillCalls => canvas_fill_calls: "Background fillRect API calls.",
    CanvasFillPixels => canvas_fill_pixels: "Background fillRect requested pixels.",
    CanvasPutCalls => canvas_put_calls: "Conservative layer putImageData API call envelope.",
    CanvasPutInputPixels => canvas_put_input_pixels: "Full ImageData input area supplied per layer put, independent of dirty rectangle.",
    CanvasPutDirtyPixels => canvas_put_dirty_pixels: "Requested dirty rectangle area, never clipped to visible canvas pixels.",
    PreviewDrawCalls => preview_draw_calls: "Preview drawImage API calls, including conservative history envelopes.",
    PreviewSourcePixels => preview_source_pixels: "Full source-layer area requested by preview drawImage.",
    PreviewDestinationPixels => preview_destination_pixels: "Preview drawImage destination envelope at most 576 pixels per call.",
    PreviewClearCalls => preview_clear_calls: "Preview clearRect API calls; excludes cursor calls.",
    PreviewClearPixels => preview_clear_pixels: "Preview clearRect requested-pixel envelope at most 576 per call.",
    FlattenDrawCalls => flatten_draw_calls: "Only an explicitly requested final flatten: background plus visible layers.",
    FlattenSourcePixels => flatten_source_pixels: "Full-area source requests for an explicitly requested final flatten.",
    FlattenDestinationPixels => flatten_destination_pixels: "Full-area destination requests for an explicitly requested final flatten.",
    HistoryRestoreEnvelopes => history_restore_envelopes: "Every Undo/Redo charged as Draw; overcounts Alpha/Dummy restores.",
    CommittedStrokes => committed_strokes: "Completed strokes, regardless of whether pixels changed.",
}

/// Initialization and optional flatten are deliberately separate from per-event
/// peaks. A draw-event budget must not accidentally become a setup threshold:
/// one size-64 shape alone has 4,460,544 defined shape-work units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CostScope {
    /// Initialization + every event + the optional one-time final flatten.
    Total,
    /// Component-wise maximum across dispatched events only. Different maxima
    /// can come from different events; setup and flatten are not events.
    PeakEvent,
    Initialization,
    FinalFlatten,
}

/// The caller must say whether it actually needs one final flatten. Conclusion
/// does not flatten, and this model never implicitly flattens more than once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostOptions {
    pub include_final_flatten: bool,
}

/// A diagnostic report with no conversion to a playback or publication type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayCost {
    total: CostCounters,
    peak_event: CostCounters,
    initialization: CostCounters,
    final_flatten: CostCounters,
}

impl ReplayCost {
    pub fn counters(&self, scope: CostScope) -> &CostCounters {
        match scope {
            CostScope::Total => &self.total,
            CostScope::PeakEvent => &self.peak_event,
            CostScope::Initialization => &self.initialization,
            CostScope::FinalFlatten => &self.final_flatten,
        }
    }

    /// Compare only explicitly supplied metric/scope limits, inclusively.
    /// An empty slice checks nothing; omitted metrics are unconstrained. Passing
    /// even a complete list establishes only this model's numeric comparisons,
    /// never browser safety, consumer equivalence, or any authority.
    pub fn check_limits(&self, limits: &[CostLimit]) -> Result<(), CostLimitExceeded> {
        for limit in limits {
            let value = self.counters(limit.scope).get(limit.metric);
            if value > limit.maximum {
                return Err(CostLimitExceeded {
                    scope: limit.scope,
                    metric: limit.metric,
                    value,
                    maximum: limit.maximum,
                });
            }
        }
        Ok(())
    }
}

/// An explicit caller policy choice; there is intentionally no Default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CostLimit {
    pub scope: CostScope,
    pub metric: CostMetric,
    pub maximum: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("model cost {metric:?} in {scope:?} is {value}, exceeding caller limit {maximum}")]
pub struct CostLimitExceeded {
    pub scope: CostScope,
    pub metric: CostMetric,
    pub value: u64,
    pub maximum: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ReplayCostError {
    #[error("resource-model arithmetic overflow for {metric:?}")]
    Overflow { metric: CostMetric },
    /// Fail closed if the state checker's domain grows without a cost review.
    #[error("event {event_index} is outside this resource model")]
    UnsupportedStateEvent { event_index: usize },
}

/// Estimate only the immutable, state-checked restricted candidate. No drawing,
/// browser call, input mutation, or attacker-sized allocation is performed here.
pub fn estimate(
    candidate: &StateCheckedCandidate<'_>,
    options: CostOptions,
) -> Result<ReplayCost, ReplayCostError> {
    let replay = candidate.untrusted_replay();
    let area = mul(
        CostMetric::CanvasSurfacePixels,
        u64::from(replay.metadata.width),
        u64::from(replay.metadata.height),
    )?;
    let mut model = Model {
        area,
        height: u64::from(replay.metadata.height),
        tools: replay.tools,
        selected: usize::from(replay.metadata.tool_id - 1),
        tone_maps_built: false,
        position: (0, 0),
    };
    let mut initialization = CostCounters {
        typed_backing_bytes: mul(CostMetric::TypedBackingBytes, 12, area)?,
        canvas_surface_pixels: mul(CostMetric::CanvasSurfacePixels, 2, area)?,
        preview_surface_pixels: PREVIEW_AREA,
        canvas_get_calls: 1,
        canvas_get_pixels: area,
        canvas_fill_calls: 1,
        canvas_fill_pixels: area,
        ..CostCounters::default()
    };
    // Header assignment initializes all slots, but only the selected tool's
    // setters run. A first tone selection builds maps even at alpha zero.
    model.tone_maps(&mut initialization)?;
    model.shape(&mut initialization)?;
    let mut result = ReplayCost {
        total: initialization,
        initialization,
        peak_event: CostCounters::default(),
        final_flatten: CostCounters::default(),
    };
    for (event_index, event) in replay.events.iter().enumerate() {
        let mut delta = CostCounters {
            events: 1,
            ..CostCounters::default()
        };
        match event.kind {
            Event::DrawStart { x, y, .. } | Event::DrawStartNoPressure { x, y } => {
                model.snapshot(&mut delta)?;
                model.tone_maps(&mut delta)?;
                model.paint(&mut delta, 0, 0, true)?;
                model.position = (x, y);
            }
            Event::Draw { x, y, .. } | Event::DrawNoPressure { x, y } => {
                let dx = u64::from(model.position.0.abs_diff(x));
                let dy = u64::from(model.position.1.abs_diff(y));
                model.paint(&mut delta, dx, dy, false)?;
                model.position = (x, y);
            }
            Event::DrawCommit => {
                model.snapshot(&mut delta)?;
                delta.typed_clear_bytes = mul(CostMetric::TypedClearBytes, 8, area)?;
                delta.committed_strokes = 1;
                model.preview(&mut delta);
            }
            Event::Undo | Event::Redo => {
                // No retained-memory credit and no classification based on a
                // guessed history top. This safely overcounts non-Draw actions.
                model.snapshot(&mut delta)?;
                model.put(&mut delta, area);
                model.preview(&mut delta);
                delta.history_restore_envelopes = 1;
            }
            Event::AddLayer => {
                delta.typed_backing_bytes = mul(CostMetric::TypedBackingBytes, 4, area)?;
                delta.canvas_surface_pixels = area;
                delta.preview_surface_pixels = PREVIEW_AREA;
                delta.canvas_get_calls = 1;
                delta.canvas_get_pixels = area;
            }
            Event::SetTool(id) => {
                model.selected = usize::from(id - 1);
                model.tone_maps(&mut delta)?;
                model.shape(&mut delta)?;
            }
            Event::SetToolSize(size) => {
                model.tools[model.selected].size = size;
                model.shape(&mut delta)?;
            }
            Event::SetToolTip(tip) => {
                model.tools[model.selected].tip_id = tip as i8;
                model.shape(&mut delta)?;
            }
            Event::SetToolAlpha(_) => model.tone_maps(&mut delta)?,
            Event::Prelude
            | Event::Conclusion
            | Event::SetColor(_)
            | Event::SetToolFlow(_)
            | Event::PreserveAlpha(_)
            | Event::ToggleLayerVisibility(_)
            | Event::SetActiveLayer(_)
            | Event::ToggleLayerSelection(_)
            | Event::SetSelectedLayersAlpha(_)
            | Event::HistoryDummy => {}
            Event::SetToolSizeDynamics(_)
            | Event::SetToolAlphaDynamics(_)
            | Event::SetToolFlowDynamics(_)
            | Event::DeleteLayers
            | Event::MoveLayers(_)
            | Event::MergeLayers => {
                return Err(ReplayCostError::UnsupportedStateEvent { event_index });
            }
        }
        result.total.checked_accumulate(&delta)?;
        result.peak_event.component_max(&delta);
    }
    if options.include_final_flatten {
        let mut calls = 1; // The background, even with no visible layers.
        for layer in candidate.final_state().layers() {
            if layer.visible {
                calls = add(CostMetric::FlattenDrawCalls, calls, 1)?;
            }
        }
        result.final_flatten = CostCounters {
            canvas_surface_pixels: area,
            flatten_draw_calls: calls,
            flatten_source_pixels: mul(CostMetric::FlattenSourcePixels, calls, area)?,
            flatten_destination_pixels: mul(CostMetric::FlattenDestinationPixels, calls, area)?,
            ..CostCounters::default()
        };
        result.total.checked_accumulate(&result.final_flatten)?;
    }
    Ok(result)
}

const PREVIEW_AREA: u64 = 24 * 24;

struct Model {
    area: u64,
    height: u64,
    tools: [UntrustedReplayTool; crate::replay_wire::TOOL_COUNT],
    selected: usize,
    tone_maps_built: bool,
    position: (i16, i16),
}

impl Model {
    fn shape(&self, counters: &mut CostCounters) -> Result<(), ReplayCostError> {
        let metric = CostMetric::ShapeLoopUnits;
        let h = mul(
            metric,
            4,
            add(metric, u64::from(self.tools[self.selected].size), 2)?,
        )?;
        let square = mul(metric, h, h)?;
        counters.checked_accumulate(&CostCounters {
            shape_regenerations: 1,
            shape_loop_units: mul(metric, 64, square)?,
            shape_flood_scalar_pushes: add(
                CostMetric::ShapeFloodScalarPushes,
                mul(CostMetric::ShapeFloodScalarPushes, 4, square)?,
                2,
            )?,
            typed_backing_bytes: mul(CostMetric::TypedBackingBytes, 12, square)?,
            ..CostCounters::default()
        })
    }

    fn tone_maps(&mut self, counters: &mut CostCounters) -> Result<(), ReplayCostError> {
        if self.tools[self.selected].id == 5 && !self.tone_maps_built {
            let metric = CostMetric::ToneLoopUnits;
            counters.checked_accumulate(&CostCounters {
                typed_backing_bytes: mul(CostMetric::TypedBackingBytes, 16, self.area)?,
                tone_map_cells: mul(CostMetric::ToneMapCells, 16, self.area)?,
                tone_loop_units: sum(
                    metric,
                    &[
                        mul(metric, 32, self.area)?,
                        mul(metric, 48, self.height)?,
                        49,
                    ],
                )?,
                ..CostCounters::default()
            })?;
            self.tone_maps_built = true;
        }
        Ok(())
    }

    fn snapshot(&self, counters: &mut CostCounters) -> Result<(), ReplayCostError> {
        counters.checked_accumulate(&CostCounters {
            typed_backing_bytes: mul(CostMetric::TypedBackingBytes, 4, self.area)?,
            typed_copy_bytes: mul(CostMetric::TypedCopyBytes, 4, self.area)?,
            ..CostCounters::default()
        })
    }

    fn preview(&self, counters: &mut CostCounters) {
        // Called once per event; each event has a fresh delta.
        counters.preview_draw_calls = 1;
        counters.preview_source_pixels = self.area;
        counters.preview_destination_pixels = PREVIEW_AREA;
        counters.preview_clear_calls = 1;
        counters.preview_clear_pixels = PREVIEW_AREA;
    }

    fn put(&self, counters: &mut CostCounters, dirty_area: u64) {
        // Each source dispatch requests at most one layer putImageData.
        counters.canvas_put_calls = 1;
        counters.canvas_put_input_pixels = self.area;
        counters.canvas_put_dirty_pixels = dirty_area;
    }

    fn paint(
        &self,
        counters: &mut CostCounters,
        dx: u64,
        dy: u64,
        start: bool,
    ) -> Result<(), ReplayCostError> {
        let tool = &self.tools[self.selected];
        let metric = CostMetric::BrushLoopUnits;
        let size = u64::from(tool.size);
        let b = match (tool.id, tool.tip_id) {
            (2, _) | (8, 1) => add(metric, size, 2)?,
            (3, _) | (8, 2) => mul(metric, 2, size)?,
            _ => size,
        };
        let square = mul(metric, b, b)?;
        let i = if start {
            1
        } else {
            add(metric, dx.max(dy), 1)?
        };
        let q = add(metric, dx, b)?;
        let r = add(metric, dy, b)?;
        let rectangle = mul(CostMetric::CanvasPutDirtyPixels, q, r)?;
        let traversal = if start { 0 } else { i };
        let blur = tool.id == 7;
        let leaf_metric = CostMetric::BrushLeafUnits;
        counters.brush_leaf_units = sum(
            leaf_metric,
            &[
                traversal,
                mul(
                    leaf_metric,
                    mul(leaf_metric, if blur { 10 } else { 1 }, i)?,
                    square,
                )?,
                if blur {
                    mul(leaf_metric, 2, rectangle)?
                } else {
                    0
                },
            ],
        )?;
        // Selected loop metric, NOT the smaller 10IB^2 leaf expression.
        let stamp = sum(
            metric,
            &[
                mul(metric, if blur { 30 } else { 2 }, square)?,
                mul(metric, 3, b)?,
                1,
            ],
        )?;
        let copy_loops = if blur {
            sum(metric, &[mul(metric, 4, rectangle)?, mul(metric, 6, q)?, 2])?
        } else {
            0
        };
        counters.brush_loop_units = sum(metric, &[traversal, mul(metric, i, stamp)?, copy_loops])?;
        // Never clip Q*R to the canvas: blur copies the requested rectangle
        // even off-canvas or when spacing prevents every visible stamp.
        self.put(counters, rectangle);
        Ok(())
    }
}

fn add(metric: CostMetric, a: u64, b: u64) -> Result<u64, ReplayCostError> {
    a.checked_add(b).ok_or(ReplayCostError::Overflow { metric })
}

fn mul(metric: CostMetric, a: u64, b: u64) -> Result<u64, ReplayCostError> {
    a.checked_mul(b).ok_or(ReplayCostError::Overflow { metric })
}

fn sum(metric: CostMetric, values: &[u64]) -> Result<u64, ReplayCostError> {
    values
        .iter()
        .try_fold(0, |total, &value| add(metric, total, value))
}

#[cfg(test)]
#[path = "replay_cost_tests.rs"]
mod tests;
