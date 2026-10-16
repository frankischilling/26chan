//! Internal service protocol. Public handlers construct these values from
//! verified transport identity and server-owned sessions, never browser JSON.
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprints {
    pub token: [u8; 32],
    pub network: [u8; 32],
    pub address: [u8; 32],
    pub environment: [u8; 32],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub fingerprints: Fingerprints,
    pub minted: bool,
    pub peer: IpAddr,
    pub country: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Thread,
    Reply,
    Report,
    EmailRequest,
    EmailVerify,
    Ticket,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeContext {
    pub session: Session,
    pub purpose: Purpose,
    pub board: Option<String>,
    pub target: i64,
    /// Random per-form nonce bound to the browser's CSRF state. Email-request
    /// challenges may load before the recipient has been typed.
    pub form_nonce: String,
    pub email_token: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Challenge {
    Ready {
        token: String,
        image: String,
        background: Option<String>,
        ttl_seconds: u16,
        cooldown_seconds: u16,
        slider: bool,
        offset_max: u16,
    },
    Wait {
        seconds: u16,
        ticket: Option<String>,
    },
    Credits {
        remaining: u8,
        ttl_seconds: u16,
    },
    Disabled {},
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub context: ChallengeContext,
    pub challenge: String,
    pub answer: String,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnswerResult {
    Verified {
        receipt: String,
        peer_hash: [u8; 32],
        context_hash: [u8; 32],
    },
    Rejected {},
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailRequest {
    pub context: ChallengeContext,
    pub email: String,
    pub challenge: String,
    pub answer: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailVerification {
    pub context: ChallengeContext,
    pub replacement: Fingerprints,
    pub challenge: String,
    pub answer: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailToken {
    pub token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailState {
    BadEmail,
    DisallowedProvider,
    Subaddressing,
    Pending,
    Queued,
    AlreadyQueued,
    Verified,
    AlreadyUsed,
    Expired,
    Invalid,
    ChallengeRequired,
    AuthorizationChanged,
    Cooldown,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailResult {
    pub state: EmailState,
    pub cooldown_seconds: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionResult {
    pub changed: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub ready: bool,
}

/// Ticket escalation is distinct from the challenge that authorizes a write.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketAnswer {
    pub challenge: String,
    pub answer: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingChallengeRequest {
    pub context: ChallengeContext,
    pub ticket: Option<String>,
    pub ticket_answer: Option<TicketAnswer>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostingChallengeResult {
    pub challenge: Challenge,
    pub ticket_challenge: bool,
}
