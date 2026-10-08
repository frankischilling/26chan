//! Isolated drawing annotation rules from `lib/oekaki.php:102–160` and
//! `imgboard.php:6052–6097` at reference revision
//! `545b7812d1849f7958d914950c91fdbbe38f6b22`.
//!
//! Inputs are already parsed integers and supplied facts, not raw PHP values or
//! HTTP form fields. This module does not emulate PHP casts/truthiness, execute
//! SQL, admit uploads, verify replay provenance, store metadata, or grant image
//! or replay access. No current schema or route is implied. A future caller must
//! obtain genuine source records from the selected board, handle lookup failure,
//! and independently authorize all storage and reads.
//!
//! Annotation time is submitted wall-clock `oe_time`, independent of replay
//! event duration and header timestamps. Replay is optional. A source reference
//! hides the replay link intent but never deletes or rejects a stored replay.
//! Outputs are typed display data: escaped markup and safe, server-selected
//! links belong to a future renderer. No legacy `javascript:` link is emitted.

use std::fmt;
use std::num::NonZeroU64;

/// The source accepts at most 60 days of submitted wall-clock time.
pub const MAX_DRAWING_SECONDS: i64 = 5_184_000;

/// Validated annotation seconds, not a measured or authenticated duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawingTime(u32);

impl DrawingTime {
    /// Accept the source's inclusive integer range. Parsing is out of scope.
    pub fn from_seconds(seconds: i64) -> Option<Self> {
        if (1..=MAX_DRAWING_SECONDS).contains(&seconds) {
            Some(Self(seconds as u32))
        } else {
            None
        }
    }

    pub fn seconds(self) -> u32 {
        self.0
    }
}

impl fmt::Display for DrawingTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let seconds = self.0;
        if seconds < 60 {
            write!(f, "{seconds}s")
        } else if seconds < 3600 {
            // Positive integer arithmetic reproduces PHP round(... / 60).
            // Do not normalize the independently rounded minutes to hours.
            write!(f, "{}m", (seconds + 30) / 60)
        } else {
            write!(f, "{}h {}m", seconds / 3600, (seconds % 3600 + 30) / 60)
        }
    }
}

/// Supplied branch facts, not evidence that an image was authorized or stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationGate {
    pub accepted_image: bool,
    pub painter_enabled: bool,
    pub replays_enabled: bool,
}

impl AnnotationGate {
    /// Mirrors entry to the metadata branch, including `isset(oe_time)`.
    pub fn metadata_requested(self, time_is_set: bool) -> bool {
        self.accepted_image && self.painter_enabled && self.replays_enabled && time_is_set
    }

    /// Whether the source validation function would be called, not whether it
    /// would query a database. A negative target is truthy here but is rejected
    /// by source-ID eligibility. Invalid time still reaches source validation;
    /// do not replace `time_is_set` with successful `DrawingTime` validation.
    pub fn source_resolution_requested(
        self,
        time_is_set: bool,
        source_is_set: bool,
        target_thread_id: i64,
    ) -> bool {
        self.metadata_requested(time_is_set) && source_is_set && target_thread_id != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceTarget<'a> {
    pub board: &'a str,
    pub thread_id: i64,
}

/// Supplied fields of one source post. A future store must resolve a real,
/// unambiguous record; constructing this value does not establish provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePost<'a> {
    pub board: &'a str,
    pub post_id: i64,
    /// Zero denotes an OP, including an OP of a different thread.
    pub parent_thread_id: i64,
    /// Exact source `tim != 0` predicate, including synthetic negative values.
    /// It is not an assertion that the file is readable or even still exists.
    pub image_tim: i64,
}

/// Pure source-reference predicate. No SQL, upload authority or fetch authority.
/// `filedeleted` is intentionally absent: the original does not check it.
/// Neither this helper nor the original proves that the source image was used
/// to create the submitted drawing, or that the target thread exists.
pub fn eligible_source_post_id(
    target: SourceTarget<'_>,
    requested_source_id: i64,
    source: Option<SourcePost<'_>>,
) -> Option<NonZeroU64> {
    let source = source?;
    if requested_source_id < 1
        || target.thread_id < 1
        || source.board != target.board
        || source.post_id != requested_source_id
        || source.image_tim == 0
        || (source.parent_thread_id != 0 && source.parent_thread_id != target.thread_id)
    {
        return None;
    }
    NonZeroU64::new(requested_source_id as u64)
}

/// An exclusive display intent, never a URL or a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationLink {
    /// Refer to this post's independently stored/authorized replay. No arbitrary
    /// replay identifier or destination is accepted by this projection.
    Replay,
    /// Historical source reference, not permission to read its image.
    SourcePost(NonZeroU64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationDisplay {
    pub time: DrawingTime,
    pub link: Option<AnnotationLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnnotationInput {
    pub gate: AnnotationGate,
    /// `None` means not set; `Some` retains invalid parsed values so presence
    /// and range checking are not confused. No PHP coercion is performed.
    pub wall_clock_seconds: Option<i64>,
    /// Supplied result of a separate replay storage path, not an admission rule.
    /// Upload errors must be handled before calling this projection.
    pub has_stored_replay: bool,
    /// Result of resolving the requested source when the presence gate permits
    /// it, using genuine selected-board records and the predicate above.
    pub resolved_source_post_id: Option<NonZeroU64>,
}

/// Project annotation data after independent upload/error handling and source
/// resolution. Missing/invalid time hides everything; it does not affect replay
/// storage. The image basename and replay filename are not annotation gates.
pub fn project_annotation(input: AnnotationInput) -> Option<AnnotationDisplay> {
    if !input
        .gate
        .metadata_requested(input.wall_clock_seconds.is_some())
    {
        return None;
    }
    let time = DrawingTime::from_seconds(input.wall_clock_seconds?)?;
    let link = if let Some(source) = input.resolved_source_post_id {
        Some(AnnotationLink::SourcePost(source))
    } else if input.has_stored_replay {
        Some(AnnotationLink::Replay)
    } else {
        None
    };
    Some(AnnotationDisplay { time, link })
}
