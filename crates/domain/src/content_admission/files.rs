//! Bounded file-filter decisions. Digests, prior-ban counts and warning history
//! must come from trusted server state, never submitted fields.
use super::{AdmissionError, network::ProviderValue};
use crate::BoardSlug;

pub mod history;

pub const MAX_FILE_RULES: usize = 128;
pub type Digest = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Reject,
    /// -1 is permanent; zero retains the source's zero-day ban request.
    Ban {
        days: i32,
    },
    Dmca,
}

pub struct Rule {
    pub id: i64,
    pub scope: String,
    pub digest: Digest,
    pub active: bool,
    pub quiet: bool,
    pub mode: Mode,
}

pub struct Request<'a> {
    pub board: &'a str,
    pub worksafe: bool,
    pub test_board: bool,
    pub badged: bool,
    /// Known absence matches the source's early return. An unreadable existing
    /// upload is Unavailable, not known absence.
    pub normalized: ProviderValue<Option<Digest>>,
    pub original: ProviderValue<Option<Digest>>,
    pub original_eligible: ProviderValue<bool>,
    pub prior_dmca: ProviderValue<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    FailedUpload,
    PrivateDiagnostic,
    Quiet,
    DmcaWarning,
    BanRedirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Ban { days: i32, dmca_repeat: bool },
    RecordDmcaWarning,
}

pub struct Block {
    pub rule: i64,
    pub digest: Digest,
    pub response: Response,
    pub effect: Effect,
}

/// A matching rule and digest, without a decision about mutable warning state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub rule: i64,
    pub digest: Digest,
}

pub struct Policy {
    rules: Vec<Rule>,
}

impl Policy {
    pub fn compile(rules: Vec<Rule>) -> Result<Self, AdmissionError> {
        let mut ids = std::collections::HashSet::new();
        if rules.len() > MAX_FILE_RULES
            || rules.iter().any(|rule| {
                rule.id < 1
                    || !ids.insert(rule.id)
                    || (!rule.scope.is_empty()
                        && rule.scope != "_ws_"
                        && BoardSlug::parse(&rule.scope).is_err())
                    || matches!(rule.mode, Mode::Ban { days } if !(-1..=3650).contains(&days))
            })
        {
            return Err(AdmissionError::InvalidPolicy);
        }
        Ok(Self { rules })
    }

    /// Exact blacklist only. The earlier normalized-digest prior-ban check is
    /// separate and has no badge exemption.
    pub fn evaluate(&self, request: Request<'_>) -> Result<Option<Block>, AdmissionError> {
        self.matching_rule(&request)?
            .map(|(rule, digest)| block(rule, digest, &request))
            .transpose()
    }

    /// Locate a candidate before an atomic effect transaction. This does not
    /// read or assume DMCA history; the finalizer must resolve that state under
    /// its own lock and revalidate the live rule and upload before committing.
    pub fn match_rule(&self, request: Request<'_>) -> Result<Option<Match>, AdmissionError> {
        Ok(self.matching_rule(&request)?.map(|(rule, digest)| Match {
            rule: rule.id,
            digest,
        }))
    }

    fn matching_rule<'a>(
        &'a self,
        request: &Request<'_>,
    ) -> Result<Option<(&'a Rule, Digest)>, AdmissionError> {
        if BoardSlug::parse(request.board).is_err() {
            return Err(AdmissionError::InvalidPolicy);
        }
        if request.badged {
            return Ok(None);
        }
        let candidates: Vec<_> = self
            .rules
            .iter()
            .filter(|rule| {
                rule.active
                    && (rule.scope.is_empty()
                        || rule.scope == request.board
                        || (rule.scope == "_ws_" && request.worksafe))
            })
            .collect();
        if candidates.is_empty() {
            return Ok(None);
        }
        let normalized = match request.normalized {
            ProviderValue::Available(Some(digest)) => digest,
            ProviderValue::Available(None) => return Ok(None),
            ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
        };
        if let Some(rule) = unique_match(&candidates, normalized)? {
            return Ok(Some((rule, normalized)));
        }
        if matches!(request.original_eligible, ProviderValue::Available(false)) {
            return Ok(None);
        }
        let original = match request.original {
            ProviderValue::Available(Some(digest)) => digest,
            ProviderValue::Available(None) => return Ok(None),
            ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
        };
        if original == normalized {
            return Ok(None);
        }
        let Some(rule) = unique_match(&candidates, original)? else {
            return Ok(None);
        };
        match request.original_eligible {
            ProviderValue::Available(true) => Ok(Some((rule, original))),
            ProviderValue::Available(false) => Ok(None),
            ProviderValue::Unavailable => Err(AdmissionError::Unavailable),
        }
    }
}

fn unique_match<'a>(
    rules: &[&'a Rule],
    digest: Digest,
) -> Result<Option<&'a Rule>, AdmissionError> {
    let mut matches = rules.iter().copied().filter(|rule| rule.digest == digest);
    let first = matches.next();
    // Legacy LIMIT 1 has no ORDER BY. Refuse ambiguous operator configuration
    // rather than invent a global/local scope priority or depend on row layout.
    if matches.next().is_some() {
        return Err(AdmissionError::InvalidPolicy);
    }
    Ok(first)
}

fn block(rule: &Rule, digest: Digest, request: &Request<'_>) -> Result<Block, AdmissionError> {
    let (response, effect) = match rule.mode {
        Mode::Dmca => match request.prior_dmca {
            ProviderValue::Available(true) => (
                Response::BanRedirect,
                Effect::Ban {
                    days: 3,
                    dmca_repeat: true,
                },
            ),
            ProviderValue::Available(false) => (Response::DmcaWarning, Effect::RecordDmcaWarning),
            ProviderValue::Unavailable => return Err(AdmissionError::Unavailable),
        },
        Mode::Reject if request.test_board => (Response::PrivateDiagnostic, Effect::None),
        mode => (
            if rule.quiet {
                Response::Quiet
            } else {
                Response::FailedUpload
            },
            match mode {
                Mode::Ban { days } => Effect::Ban {
                    days,
                    dmca_repeat: false,
                },
                _ => Effect::None,
            },
        ),
    };
    Ok(Block {
        rule: rule.id,
        digest,
        response,
        effect,
    })
}

#[derive(Clone, Copy)]
pub struct PriorBanCounts {
    pub digest: Digest,
    pub template_226: u64,
    pub template_6: u64,
}

/// Called before content filters. The query counts normalized-digest ban
/// history globally; template 6 contributes only on work-safe boards.
pub fn prior_ban_rejection(
    board: &str,
    worksafe: bool,
    normalized: ProviderValue<Option<Digest>>,
    counts: ProviderValue<PriorBanCounts>,
) -> Result<bool, AdmissionError> {
    if BoardSlug::parse(board).is_err() {
        return Err(AdmissionError::InvalidPolicy);
    }
    if board == "b" || matches!(normalized, ProviderValue::Available(None)) {
        return Ok(false);
    }
    let ProviderValue::Available(Some(digest)) = normalized else {
        return Err(AdmissionError::Unavailable);
    };
    let ProviderValue::Available(counts) = counts else {
        return Err(AdmissionError::Unavailable);
    };
    if counts.digest != digest {
        return Err(AdmissionError::InvalidPolicy);
    }
    Ok(counts
        .template_226
        .saturating_add(if worksafe { counts.template_6 } else { 0 })
        >= 3)
}

#[cfg(test)]
mod tests;
