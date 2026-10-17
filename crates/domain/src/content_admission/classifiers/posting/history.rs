//! Ordered optional-edge and country-history checks after the numeric prefix.
//! Private history and edge inputs must be qualified by the transactional caller.
use crate::anonymous_session::State;
use crate::content_admission::{AdmissionError, network::ProviderValue};
use ProviderValue::{Available as A, Unavailable as U};

pub struct Request<'a> {
    pub captcha_bypass: bool,
    /// Server-decoded snapshot resumed at exactly activity_at.
    pub actor: ProviderValue<Option<&'a State>>,
    pub activity_at: u64,
    /// Qualified presence of the trusted edge field, independent of its value.
    /// An empty or non-numeric value is still present. A public header cannot
    /// establish this fact; provider failure is Unavailable.
    pub edge_present: ProviderValue<bool>,
    /// Same-session block_bm_bot event strictly newer than the 24-hour cutoff.
    pub edge_history: ProviderValue<bool>,
    /// Qualified result of the existing likely_automated predicate.
    pub automated: ProviderValue<bool>,
    /// Same-session country_changed event strictly newer than that cutoff.
    pub country_history: ProviderValue<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    LogEdgeCooldown,
    RecordEdgeBlock,
    RecordCountryChange,
    LogCountryBlock,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub rejected: bool,
    pub events: [Option<Event>; 2],
}
fn fact(value: ProviderValue<bool>) -> Result<bool, AdmissionError> {
    match value {
        A(value) => Ok(value),
        U => Err(AdmissionError::Unavailable),
    }
}
fn reject(first: Event, second: Option<Event>) -> Decision {
    Decision {
        rejected: true,
        events: [Some(first), second],
    }
}
/// No event is emitted here. Apply the entire returned plan atomically only
/// after revalidating authority. Needed unavailable facts return no partial plan.
pub fn evaluate(request: Request<'_>) -> Result<Decision, AdmissionError> {
    let allow = Decision {
        rejected: false,
        events: [None, None],
    };
    if request.captcha_bypass {
        return Ok(allow);
    }
    let actor = match request.actor {
        A(Some(actor)) => actor,
        A(None) => return Ok(allow),
        U => return Err(AdmissionError::Unavailable),
    };
    // The source's preceding flood call is disabled and assigns zero.
    if !actor.is_known_or_verified(request.activity_at, 1440, 0) {
        let present = fact(request.edge_present)?;
        if present {
            if fact(request.edge_history)? {
                return Ok(reject(Event::LogEdgeCooldown, None));
            }
            if fact(request.automated)? {
                return Ok(reject(Event::RecordEdgeBlock, None));
            }
        }
    }
    if actor.verified_level == 0 && actor.post_count() < 20 {
        if fact(request.country_history)? {
            return Ok(reject(Event::LogCountryBlock, None));
        }
        if actor.created_at != request.activity_at && actor.environment_at == request.activity_at {
            return Ok(reject(
                Event::RecordCountryChange,
                Some(Event::LogCountryBlock),
            ));
        }
    }
    Ok(allow)
}

#[cfg(test)]
mod tests;
