use super::{RequestFacts, diagnostics};
use crate::{PostingContext, StoreError, anonymous_session::PostingSession};
use ProviderValue::{Available as A, Unavailable as U};
use board_domain::{
    anonymous_session::State,
    content_admission::{
        classifiers::posting,
        network::{ProviderValue, hints::DerivedHints},
        threat::Score,
    },
};
use sqlx::{Postgres, Transaction};

pub(crate) struct Context<'a> {
    pub board: &'a str,
    pub parent: i64,
    pub image: bool,
    pub actor: Option<&'a State>,
    pub activity_at: u64,
    pub session: Option<&'a PostingSession>,
    pub posting: PostingContext,
    pub hints: Option<&'a DerivedHints>,
    pub thread: Option<&'a (String, String)>,
    pub facts: Option<&'a RequestFacts>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Continue,
    Quiet,
    ImageRange,
    CountryRange,
}
pub(crate) struct Plan {
    pub outcome: Outcome,
    pub events: Vec<diagnostics::Event>,
    pub country: Option<String>,
    pub score: Option<Score>,
}
fn unavailable(_: impl Sized) -> StoreError {
    StoreError::Database(sqlx::Error::Protocol(
        "Posting classifier evidence is unavailable.".into(),
    ))
}

pub(crate) async fn evaluate(
    tx: &mut Transaction<'_, Postgres>,
    context: Context<'_>,
) -> Result<Plan, StoreError> {
    let facts = context
        .facts
        .filter(|facts| facts.matches(context.posting, context.board, context.parent));
    let score = facts.map(|facts| facts.score).unwrap_or(U);
    let country = facts.map(|facts| facts.country).unwrap_or(U);
    let country_bytes = match &country {
        A(value) => A(value.as_slice()),
        U => U,
    };
    let timing_cookie = match facts.map(|facts| &facts.timing_cookie) {
        Some(A(value)) => A(value.as_deref()),
        _ => U,
    };
    let mut random = posting::NativeRandom::default();
    let prefix = posting::evaluate(
        posting::Request {
            board: context.board,
            reply: context.parent != 0,
            image: context.image,
            // The caller already selected the locked enabled public-board path.
            captcha_bypass: false,
            actor: A(context.actor),
            activity_at: context.activity_at,
            score: score.into(),
            thread: context
                .thread
                .map(|(subject, comment)| {
                    A(posting::Thread {
                        subject: subject.as_bytes(),
                        comment: comment.as_bytes(),
                    })
                })
                .unwrap_or(U),
            browser_id: context
                .hints
                .map(|hints| hints.signatures().browser_id)
                .unwrap_or(U),
            country: country_bytes,
            timing_cookie,
        },
        &mut random,
    )
    .map_err(unavailable)?;
    let mut events: Vec<_> = prefix
        .events
        .into_iter()
        .flatten()
        .map(diagnostics::Event::Prefix)
        .collect();
    let mut outcome = match prefix.outcome {
        posting::Outcome::Continue => Outcome::Continue,
        posting::Outcome::Quiet => Outcome::Quiet,
        posting::Outcome::ImageRangeRejected => Outcome::ImageRange,
    };
    if outcome == Outcome::Continue {
        let needs_country = context
            .actor
            .is_some_and(|actor| actor.verified_level == 0 && actor.post_count() < 20);
        let country_history = if needs_country {
            let session = context.session.ok_or_else(|| unavailable(()))?;
            A(super::history(tx, context.board, session)
                .await?
                .country_blocked)
        } else {
            U
        };
        let suffix = posting::history::evaluate(posting::history::Request {
            captcha_bypass: false,
            actor: A(context.actor),
            activity_at: context.activity_at,
            // This native deployment has no trusted ingress score adapter.
            // Direct request headers never provide that authority. This is
            // confirmed absence on the trusted channel, not a failed lookup.
            edge_present: A(false),
            edge_history: U,
            automated: U,
            country_history,
        })
        .map_err(unavailable)?;
        events.extend(
            suffix
                .events
                .into_iter()
                .flatten()
                .map(diagnostics::Event::History),
        );
        if suffix.rejected {
            outcome = Outcome::CountryRange;
        }
    }
    let country = match country {
        A(bytes) if bytes.iter().all(u8::is_ascii_uppercase) => {
            Some(String::from_utf8(bytes.to_vec()).map_err(unavailable)?)
        }
        _ => None,
    };
    // Known actors use the source's explicit zero; do not mislabel their raw
    // transport score as the effective score when optional logging omits it.
    let score = if context
        .actor
        .is_some_and(|actor| actor.is_known_or_verified(context.activity_at, 360, 0))
    {
        None
    } else {
        match score {
            A(score) => Some(score),
            U => None,
        }
    };
    Ok(Plan {
        outcome,
        events,
        country,
        score,
    })
}

pub(crate) async fn record(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    parent: i64,
    plan: &Plan,
) -> Result<(), StoreError> {
    if !plan.events.is_empty() {
        match diagnostics::record(
            tx,
            board,
            parent,
            &plan.events,
            plan.country.as_deref(),
            plan.score,
        )
        .await?
        {
            diagnostics::Written::Recorded => {}
            diagnostics::Written::CapacityDropped => tracing::warn!(
                events = plan.events.len(),
                "Posting diagnostic capacity exhausted"
            ),
        }
    }
    Ok(())
}
