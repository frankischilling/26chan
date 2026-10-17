//! Ordered late posting checks before the optional edge/history classifiers.
//! Facts are server-qualified; this module neither authenticates nor persists.
pub mod history;
use crate::content_admission::{AdmissionError, network::ProviderValue, threat::Score};
use crate::{BoardSlug, anonymous_session::State};
use ProviderValue::{Available as A, Unavailable as U};

/// Only the qualified scorer can construct production numeric evidence.
#[derive(Clone, Copy)]
pub struct NumericScore(ProviderValue<f64>);
impl From<ProviderValue<Score>> for NumericScore {
    fn from(score: ProviderValue<Score>) -> Self {
        Self(match score {
            A(value) => A(value.value()),
            U => U,
        })
    }
}
#[derive(Clone, Copy)]
pub struct Thread<'a> {
    pub subject: &'a [u8],
    pub comment: &'a [u8],
}
pub struct Request<'a> {
    /// Must name an admitted board; invalid slugs fail before any bypass.
    pub board: &'a str,
    pub reply: bool,
    pub image: bool,
    /// A trusted source CAPTCHA bypass, never a form field or cosmetic badge.
    pub captcha_bypass: bool,
    /// The server-decoded pre-post snapshot, resumed using activity_at.
    pub actor: ProviderValue<Option<&'a State>>,
    /// Frozen UserPwd-equivalent snapshot time, never a later wall-clock read.
    pub activity_at: u64,
    pub score: NumericScore,
    pub thread: ProviderValue<Thread<'a>>,
    pub browser_id: ProviderValue<Option<&'a str>>,
    pub country: ProviderValue<&'a [u8]>,
    pub timing_cookie: ProviderValue<Option<&'a [u8]>>,
}

/// Supply bounded uniform draws only when the source reaches a random branch.
pub trait Random {
    fn draw(&mut self, upper_exclusive: u8) -> Option<u8>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    Quiet,
    ImageRangeRejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    MaskChanged,
    TextThreat,
    Aicg,
    Lolg,
    Overwatch,
    Bag,
    Granblue,
    Adgq,
    Zzz,
    Funkg,
    PublicProxy,
    Bogota,
}
impl Event {
    pub fn source_name(self) -> &'static str {
        match self {
            Self::MaskChanged => "log_mask_changed",
            Self::TextThreat => "block_txt_threat",
            Self::Aicg => "block_aicg",
            Self::Lolg => "block_lolg",
            Self::Overwatch => "block_owg",
            Self::Bag => "block_bag",
            Self::Granblue => "block_gbfg",
            Self::Adgq => "block_adgq",
            Self::Zzz => "block_zzz",
            Self::Funkg => "block_funkg",
            Self::PublicProxy => "block_pub_prox",
            Self::Bogota => "block_bag_scat",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub outcome: Outcome,
    pub events: [Option<Event>; 2],
}
fn finish(mask: bool, event: Option<Event>, outcome: Outcome) -> Decision {
    Decision {
        outcome,
        events: if mask {
            [Some(Event::MaskChanged), event]
        } else {
            [event, None]
        },
    }
}
fn contains(value: &[u8], needle: &[u8]) -> bool {
    value.windows(needle.len()).any(|part| part == needle)
}
fn draw(random: &mut impl Random, limit: u8) -> Result<u8, AdmissionError> {
    random
        .draw(limit)
        .filter(|value| *value < limit)
        .ok_or(AdmissionError::Unavailable)
}
fn thread<'a>(request: &Request<'a>) -> Result<Thread<'a>, AdmissionError> {
    match request.thread {
        A(value) if value.subject.len() <= 16384 && value.comment.len() <= 1048576 => Ok(value),
        A(_) => Err(AdmissionError::InvalidPolicy),
        U => Err(AdmissionError::Unavailable),
    }
}

pub fn evaluate(
    request: Request<'_>,
    random: &mut impl Random,
) -> Result<Decision, AdmissionError> {
    if BoardSlug::parse(request.board).is_err() {
        return Err(AdmissionError::InvalidPolicy);
    }
    if request.captcha_bypass {
        return Ok(finish(false, None, Outcome::Continue));
    }
    let actor = match request.actor {
        A(value) => value,
        U => return Err(AdmissionError::Unavailable),
    };
    let known = actor.is_some_and(|state| state.is_known_or_verified(request.activity_at, 360, 0));
    let verified = actor.is_some_and(|state| state.verified_level > 0);
    let trusted = verified || actor.is_some_and(|state| state.post_count() > 10);
    // This is the source's explicit known-user default, not a missing score.
    let score = if known {
        0.0
    } else {
        match request.score.0 {
            A(value) if value.is_finite() && value >= 0.0 => value,
            A(_) => return Err(AdmissionError::InvalidPolicy),
            U => return Err(AdmissionError::Unavailable),
        }
    };
    let mask = actor.is_some_and(|state| {
        !verified
            && state.post_count() > 0
            && state.password_age(request.activity_at) < 86400
            && state.created_at != request.activity_at
            && state.network_at == request.activity_at
    });
    let end = |event, outcome| Ok(finish(mask, Some(event), outcome));
    if !known && score > 0.31 {
        return end(Event::TextThreat, Outcome::Quiet);
    }
    if request.reply && request.image {
        if !trusted && request.board == "g" && contains(thread(&request)?.subject, b"/aicg/") {
            return end(Event::Aicg, Outcome::ImageRangeRejected);
        }
        if !trusted && request.board == "vg" {
            let target = thread(&request)?;
            if contains(target.comment, b"/lolg/") {
                return end(Event::Lolg, Outcome::ImageRangeRejected);
            }
            if contains(target.comment, b"/overwatch") {
                return end(Event::Overwatch, Outcome::ImageRangeRejected);
            }
        }
        if !known && request.board == "vg" && contains(thread(&request)?.subject, b"/bag/") {
            let id = match request.browser_id {
                A(value) => value,
                U => return Err(AdmissionError::Unavailable),
            };
            if id.is_some_and(|value| value.len() > 64) {
                return Err(AdmissionError::InvalidPolicy);
            }
            if id == Some("04d2237a2") {
                return end(Event::Bag, Outcome::ImageRangeRejected);
            }
        }
        if !trusted && request.board == "vg" && contains(thread(&request)?.subject, b"granblue") {
            return end(Event::Granblue, Outcome::ImageRangeRejected);
        }
        if !known && score >= 0.09 {
            if request.board == "v"
                && contains(thread(&request)?.subject, b"gamesdonequick")
                && draw(random, 10)? >= 1
            {
                return end(Event::Adgq, Outcome::Quiet);
            }
            if request.board == "vg"
                && contains(thread(&request)?.subject, b"/zzz/")
                && draw(random, 10)? >= 1
            {
                return end(Event::Zzz, Outcome::Quiet);
            }
        }
        if !verified
            && request.board == "vg"
            && score >= 0.09
            && contains(thread(&request)?.subject, b"/funkg/")
        {
            return end(Event::Funkg, Outcome::Quiet);
        }
    }
    // Keep the draw before the /f/ exception, as in the original condition.
    if !verified && score >= 0.09 {
        if !request.image {
            if draw(random, 10)? >= 5 && request.board != "f" {
                return end(Event::PublicProxy, Outcome::Quiet);
            }
        } else if draw(random, 10)? >= if request.reply { 4 } else { 3 } && request.board != "f" {
            return end(
                Event::PublicProxy,
                if draw(random, 2)? == 1 {
                    Outcome::ImageRangeRejected
                } else {
                    Outcome::Quiet
                },
            );
        }
    }
    if !known && request.reply && request.image && request.board == "vg" {
        let country = match request.country {
            A(value) => value,
            U => return Err(AdmissionError::Unavailable),
        };
        if country.len() != 2 || !country.iter().all(u8::is_ascii_uppercase) {
            return Err(AdmissionError::InvalidPolicy);
        }
        if country == b"US" {
            let cookie = match request.timing_cookie {
                A(value) => value,
                U => return Err(AdmissionError::Unavailable),
            };
            if cookie.is_some_and(|value| value.len() > 4096) {
                return Err(AdmissionError::InvalidPolicy);
            }
            if cookie.is_some_and(|value| contains(value, b".America/Bogota.")) {
                return end(Event::Bogota, Outcome::Quiet);
            }
        }
    }
    Ok(finish(mask, None, Outcome::Continue))
}

#[cfg(test)]
mod tests;
