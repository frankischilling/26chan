//! Bounded browser voting credentials for the reconstructed public poll routes.
//! The supplied poll controller is missing, so these are explicit rewrite
//! security rules. A cookie discourages repeat votes from one browser; clearing
//! it creates a new voter and cannot prove a unique human.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};

use crate::ValidationError;

pub const COOKIE_SECONDS: u64 = 31_536_000;
const COOKIE_LIFETIME: i64 = COOKIE_SECONDS as i64;
const FORM_LIFETIME: i64 = 1_800;
const COOKIE_PREFIX: &str = "pv1.";
const FORM_PREFIX: &str = "pf1.";
const COOKIE_PAYLOAD_LEN: usize = 48;
const FORM_PAYLOAD_LEN: usize = 24;
const COOKIE_BODY_LEN: usize = 64;
const FORM_BODY_LEN: usize = 32;
const TAG_LEN: usize = 32;
const TAG_TEXT_LEN: usize = 43;
const NONCE_LEN: usize = 32;

const INVALID_VOTER: &str = "Invalid poll voter.";
const INVALID_TIME: &str = "Invalid poll voting time.";
const INVALID_POLL: &str = "Invalid poll identifier.";

/// Three independently derived HMAC keys. No key bytes, signatures, or voter
/// credentials are exposed through Debug, Display, or serialization.
pub struct PollVotingKey {
    cookie: hmac::Key,
    form: hmac::Key,
    vote: hmac::Key,
}

/// The signed cookie and its verified, server-generated random identity.
/// Constructed only by a matching PollVotingKey; not a durable user identity.
pub struct PollVoter {
    credential: String,
    issued_at: i64,
    expires_at: i64,
    nonce: [u8; NONCE_LEN],
}

impl PollVoter {
    /// This is the cookie value; keep it out of logs and public page markup.
    pub fn credential(&self) -> &str {
        &self.credential
    }
}

impl PollVotingKey {
    pub(crate) fn derive(master: &[u8; 32]) -> Self {
        let master = hmac::Key::new(hmac::HMAC_SHA256, master);
        let purpose = |label: &[u8]| {
            let derived = hmac::sign(&master, label);
            hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref())
        };
        Self {
            cookie: purpose(b"26chan-poll-voter-cookie-key-v1\0"),
            form: purpose(b"26chan-poll-form-token-key-v1\0"),
            vote: purpose(b"26chan-poll-vote-hash-key-v1\0"),
        }
    }

    /// A new 256-bit random browser identity, signed with an exact one-year
    /// lifetime. The caller supplies the server clock and sets the HTTP cookie.
    pub fn generate_voter(&self, now: i64) -> Result<PollVoter, ValidationError> {
        valid_cookie_expiry(now).ok_or(ValidationError(INVALID_TIME))?;
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| ValidationError("Could not generate a poll voter."))?;
        self.issue_voter(now, nonce)
    }

    /// Only canonical, signed cookies whose issue time is not in the future
    /// and whose expiry has not arrived are accepted.
    pub fn parse_voter(&self, token: &str, now: i64) -> Option<PollVoter> {
        let (issued_at, expires_at, nonce) = self.decode_voter(token)?;
        if !active(issued_at, expires_at, now) {
            return None;
        }
        Some(PollVoter {
            credential: token.to_owned(),
            issued_at,
            expires_at,
            nonce,
        })
    }

    /// The form is bound to this signed voter cookie and one positive poll ID.
    /// It expires after 30 minutes, or sooner when the cookie expires.
    pub fn form_token(
        &self,
        voter: &PollVoter,
        poll_id: i64,
        now: i64,
    ) -> Result<String, ValidationError> {
        validate_poll(poll_id)?;
        if !self.valid_voter(voter, now) {
            return Err(ValidationError(INVALID_VOTER));
        }
        let form_expiry = now
            .checked_add(FORM_LIFETIME)
            .ok_or(ValidationError(INVALID_TIME))?
            .min(voter.expires_at);
        let mut payload = [0u8; FORM_PAYLOAD_LEN];
        payload[..8].copy_from_slice(&poll_id.to_be_bytes());
        payload[8..16].copy_from_slice(&now.to_be_bytes());
        payload[16..24].copy_from_slice(&form_expiry.to_be_bytes());
        let signed = form_input(voter, &payload);
        let tag = hmac::sign(&self.form, &signed);
        Ok(format!(
            "{FORM_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(tag.as_ref())
        ))
    }

    /// Reject changed cookies, polls, signatures, noncanonical encodings,
    /// future issue times, and expired forms without revealing which failed.
    pub fn verify_form_token(
        &self,
        voter: &PollVoter,
        poll_id: i64,
        token: &str,
        now: i64,
    ) -> bool {
        if validate_poll(poll_id).is_err() || !self.valid_voter(voter, now) {
            return false;
        }
        let Some((encoded, tag)) = encoded_parts(token, FORM_PREFIX, FORM_BODY_LEN) else {
            return false;
        };
        let (Some(payload), Some(tag)) = (
            decode_exact::<FORM_PAYLOAD_LEN>(encoded),
            decode_exact::<TAG_LEN>(tag),
        ) else {
            return false;
        };
        let expected_poll = i64::from_be_bytes(payload[..8].try_into().expect("8-byte poll"));
        let issued_at = i64::from_be_bytes(payload[8..16].try_into().expect("8-byte form issue"));
        let expires_at =
            i64::from_be_bytes(payload[16..24].try_into().expect("8-byte form expiry"));
        let Some(max_expiry) = issued_at.checked_add(FORM_LIFETIME) else {
            return false;
        };
        if expected_poll != poll_id
            || issued_at < voter.issued_at
            || issued_at >= voter.expires_at
            || now < issued_at
            || now >= expires_at
            || expires_at != max_expiry.min(voter.expires_at)
        {
            return false;
        }
        hmac::verify(&self.form, &form_input(voter, &payload), &tag).is_ok()
    }

    /// A private receipt digest that differs for every poll even with the same
    /// browser cookie. Never place it in a response, cookie, or public log.
    pub fn voter_hash(&self, voter: &PollVoter, poll_id: i64) -> Result<[u8; 32], ValidationError> {
        validate_poll(poll_id)?;
        if !self.matches_voter(voter) {
            return Err(ValidationError(INVALID_VOTER));
        }
        let mut input = [0u8; 8 + NONCE_LEN];
        input[..8].copy_from_slice(&poll_id.to_be_bytes());
        input[8..].copy_from_slice(&voter.nonce);
        Ok(hmac::sign(&self.vote, &input)
            .as_ref()
            .try_into()
            .expect("SHA-256 digest size"))
    }

    fn issue_voter(&self, now: i64, nonce: [u8; NONCE_LEN]) -> Result<PollVoter, ValidationError> {
        let expires_at = valid_cookie_expiry(now).ok_or(ValidationError(INVALID_TIME))?;
        let mut payload = [0u8; COOKIE_PAYLOAD_LEN];
        payload[..8].copy_from_slice(&now.to_be_bytes());
        payload[8..16].copy_from_slice(&expires_at.to_be_bytes());
        payload[16..].copy_from_slice(&nonce);
        let tag = hmac::sign(&self.cookie, &payload);
        let credential = format!(
            "{COOKIE_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(tag.as_ref())
        );
        Ok(PollVoter {
            credential,
            issued_at: now,
            expires_at,
            nonce,
        })
    }

    fn decode_voter(&self, token: &str) -> Option<(i64, i64, [u8; NONCE_LEN])> {
        let (encoded, tag) = encoded_parts(token, COOKIE_PREFIX, COOKIE_BODY_LEN)?;
        let payload = decode_exact::<COOKIE_PAYLOAD_LEN>(encoded)?;
        let tag = decode_exact::<TAG_LEN>(tag)?;
        hmac::verify(&self.cookie, &payload, &tag).ok()?;
        let issued_at = i64::from_be_bytes(payload[..8].try_into().ok()?);
        let expires_at = i64::from_be_bytes(payload[8..16].try_into().ok()?);
        if valid_cookie_expiry(issued_at)? != expires_at {
            return None;
        }
        let nonce = payload[16..].try_into().ok()?;
        Some((issued_at, expires_at, nonce))
    }

    fn matches_voter(&self, voter: &PollVoter) -> bool {
        self.decode_voter(voter.credential())
            .is_some_and(|(issued, expires, nonce)| {
                voter.issued_at == issued && voter.expires_at == expires && voter.nonce == nonce
            })
    }

    fn valid_voter(&self, voter: &PollVoter, now: i64) -> bool {
        self.matches_voter(voter) && active(voter.issued_at, voter.expires_at, now)
    }
}

fn valid_cookie_expiry(now: i64) -> Option<i64> {
    (now >= 0)
        .then(|| now.checked_add(COOKIE_LIFETIME))
        .flatten()
}

fn active(issued_at: i64, expires_at: i64, now: i64) -> bool {
    now >= issued_at && now < expires_at && issued_at >= 0
}

fn validate_poll(poll_id: i64) -> Result<(), ValidationError> {
    if poll_id <= 0 {
        return Err(ValidationError(INVALID_POLL));
    }
    Ok(())
}

fn encoded_parts<'a>(value: &'a str, prefix: &str, body_len: usize) -> Option<(&'a str, &'a str)> {
    let encoded = value.strip_prefix(prefix)?;
    if encoded.len() != body_len + 1 + TAG_TEXT_LEN {
        return None;
    }
    let (body, tag) = encoded.split_once('.')?;
    (body.len() == body_len && tag.len() == TAG_TEXT_LEN).then_some((body, tag))
}

fn decode_exact<const N: usize>(value: &str) -> Option<[u8; N]> {
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    if decoded.len() != N || URL_SAFE_NO_PAD.encode(&decoded) != value {
        return None;
    }
    decoded.try_into().ok()
}

fn form_input(voter: &PollVoter, payload: &[u8; FORM_PAYLOAD_LEN]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(voter.credential.len() + 1 + FORM_PAYLOAD_LEN);
    bytes.extend_from_slice(voter.credential.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(payload);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poster_id::PosterIdKey;

    const NOW: i64 = 1_700_000_000;
    const COOKIE_VECTOR: &str = "pv1.AAAAAGVT8QAAAAAAZzUkgAABAgMEBQYHCAkKCwwNDg8QERITFBUWFxgZGhscHR4f.DkCiYpm16o9eGaPfWXlOzI3lzewVyWA4Fe-TgJtKDss";
    const FORM_VECTOR: &str =
        "pf1.AAAAAAAAACoAAAAAZVPxAAAAAABlU_gI.NBODy-Z0Gr3kGgGHwIVnJ818MFPJckHViHKkqRbFGLo";
    const VOTE_HASH_VECTOR: &str =
        "6babfa4f9cc0fcbc248daa39934edf21e74be5e9ce4555841fd9252b03e669c0";

    fn key(value: &str) -> PollVotingKey {
        PosterIdKey::parse(&value.repeat(32))
            .unwrap()
            .poll_voting_key()
    }

    fn fixed_voter(key: &PollVotingKey) -> PollVoter {
        let mut nonce = [0u8; 32];
        for (index, byte) in nonce.iter_mut().enumerate() {
            *byte = index as u8;
        }
        key.issue_voter(NOW, nonce).unwrap()
    }

    fn forged_cookie(key: &PollVotingKey, payload: &[u8; COOKIE_PAYLOAD_LEN]) -> String {
        let tag = hmac::sign(&key.cookie, payload);
        format!(
            "{COOKIE_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(tag.as_ref())
        )
    }

    #[test]
    fn fixed_vectors_use_independently_derived_cookie_form_and_poll_keys() {
        // Reference values computed separately with Python stdlib HMAC-SHA256.
        let key = key("11");
        let voter = fixed_voter(&key);
        assert_eq!(voter.credential(), COOKIE_VECTOR);
        assert_eq!(voter.credential().len(), 112);
        assert_eq!(key.form_token(&voter, 42, NOW).unwrap(), FORM_VECTOR);
        assert_eq!(FORM_VECTOR.len(), 80);
        let hash = key.voter_hash(&voter, 42).unwrap();
        assert_eq!(
            hash.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            VOTE_HASH_VECTOR
        );
        assert!(key.parse_voter(COOKIE_VECTOR, NOW).is_some());
        assert!(key.verify_form_token(&voter, 42, FORM_VECTOR, NOW));
    }

    #[test]
    fn cookie_is_random_signed_canonical_and_valid_for_exactly_one_year() {
        let key = key("11");
        let voter = fixed_voter(&key);
        assert!(key.parse_voter(voter.credential(), NOW).is_some());
        assert!(
            key.parse_voter(voter.credential(), NOW + COOKIE_LIFETIME - 1)
                .is_some()
        );
        assert!(
            key.parse_voter(voter.credential(), NOW + COOKIE_LIFETIME)
                .is_none()
        );
        assert!(key.parse_voter(voter.credential(), NOW - 1).is_none());
        assert!(key.parse_voter(voter.credential(), -1).is_none());

        let generated = key.generate_voter(NOW).unwrap();
        assert_ne!(generated.credential(), voter.credential());
        assert!(key.parse_voter(generated.credential(), NOW).is_some());
        for forged in [
            "client-chosen".to_owned(),
            voter.credential().replace("pv1.", "pv2."),
            format!("{}=", voter.credential()),
            format!("{}.", voter.credential()),
            format!(" {} ", voter.credential()),
            voter.credential().replacen('A', "+", 1),
        ] {
            assert!(key.parse_voter(&forged, NOW).is_none());
        }
        for changed in [10, 60, 100, 111] {
            let mut corrupt = voter.credential().as_bytes().to_vec();
            corrupt[changed] = if corrupt[changed] == b'A' { b'B' } else { b'A' };
            let corrupt = String::from_utf8(corrupt).unwrap();
            assert!(key.parse_voter(&corrupt, NOW).is_none());
        }
    }

    #[test]
    fn signed_cookie_still_refuses_future_issue_invalid_lifetime_and_overflow() {
        let key = key("11");
        assert!(key.generate_voter(-1).is_err());
        assert!(key.generate_voter(i64::MAX).is_err());
        let future = key.issue_voter(NOW + 1, [4u8; NONCE_LEN]).unwrap();
        assert!(key.parse_voter(future.credential(), NOW).is_none());
        assert!(key.parse_voter(future.credential(), NOW + 1).is_some());

        let mut payload = [0u8; COOKIE_PAYLOAD_LEN];
        payload[16..].copy_from_slice(&[9u8; NONCE_LEN]);
        for (issued, expires) in [
            (NOW, NOW + COOKIE_LIFETIME + 1),
            (-1, COOKIE_LIFETIME - 1),
            (i64::MAX - 1, i64::MAX),
        ] {
            payload[..8].copy_from_slice(&issued.to_be_bytes());
            payload[8..16].copy_from_slice(&expires.to_be_bytes());
            assert!(
                key.parse_voter(&forged_cookie(&key, &payload), NOW)
                    .is_none()
            );
        }
    }

    #[test]
    fn form_tokens_are_cookie_and_poll_bound_and_expire_at_thirty_minutes() {
        let key = key("11");
        let voter = fixed_voter(&key);
        let second = key.issue_voter(NOW, [17u8; NONCE_LEN]).unwrap();
        let token = key.form_token(&voter, 42, NOW).unwrap();
        assert!(key.verify_form_token(&voter, 42, &token, NOW + FORM_LIFETIME - 1));
        assert!(!key.verify_form_token(&voter, 42, &token, NOW + FORM_LIFETIME));
        assert!(!key.verify_form_token(&voter, 43, &token, NOW));
        assert!(!key.verify_form_token(&second, 42, &token, NOW));
        assert!(!key.verify_form_token(&voter, 42, &token, NOW - 1));
        assert!(!key.verify_form_token(&voter, 42, &format!("{token}="), NOW));
        assert!(!key.verify_form_token(&voter, 42, &token.replace("pf1.", "pv1."), NOW));
        assert!(key.form_token(&voter, 0, NOW).is_err());
        assert!(key.form_token(&voter, -1, NOW).is_err());
        assert!(!key.verify_form_token(&voter, 0, &token, NOW));
        assert!(!key.verify_form_token(&voter, -1, &token, NOW));

        for changed in [8, 24, 55, 79] {
            let mut forged = token.as_bytes().to_vec();
            forged[changed] = if forged[changed] == b'A' { b'B' } else { b'A' };
            let forged = String::from_utf8(forged).unwrap();
            assert!(!key.verify_form_token(&voter, 42, &forged, NOW));
        }
    }

    #[test]
    fn form_expiry_is_capped_to_earlier_cookie_expiry() {
        let key = key("11");
        let voter = fixed_voter(&key);
        let closing = NOW + COOKIE_LIFETIME - 5;
        let token = key.form_token(&voter, i64::MAX, closing).unwrap();
        assert!(key.verify_form_token(&voter, i64::MAX, &token, closing + 4));
        assert!(!key.verify_form_token(&voter, i64::MAX, &token, closing + 5));
        assert!(key.form_token(&voter, 42, NOW + COOKIE_LIFETIME).is_err());
        assert!(key.form_token(&voter, 42, i64::MAX).is_err());
        assert!(!key.verify_form_token(&voter, 42, FORM_VECTOR, i64::MAX));
    }

    #[test]
    fn even_correctly_signed_forms_cannot_claim_future_time_or_longer_lifetime() {
        let key = key("11");
        let voter = fixed_voter(&key);
        let future = key.form_token(&voter, 42, NOW + 60).unwrap();
        assert!(!key.verify_form_token(&voter, 42, &future, NOW));
        for expires in [NOW + FORM_LIFETIME + 1, NOW + 1] {
            let mut payload = [0u8; FORM_PAYLOAD_LEN];
            payload[..8].copy_from_slice(&42i64.to_be_bytes());
            payload[8..16].copy_from_slice(&NOW.to_be_bytes());
            payload[16..].copy_from_slice(&expires.to_be_bytes());
            let signature = hmac::sign(&key.form, &form_input(&voter, &payload));
            let token = format!(
                "{FORM_PREFIX}{}.{}",
                URL_SAFE_NO_PAD.encode(payload),
                URL_SAFE_NO_PAD.encode(signature.as_ref())
            );
            assert!(!key.verify_form_token(&voter, 42, &token, NOW));
        }
    }

    #[test]
    fn vote_hashes_are_per_poll_and_key_rotation_invalidates_old_credentials() {
        let old = key("11");
        let new = key("22");
        let voter = fixed_voter(&old);
        let other = old.issue_voter(NOW, [99u8; NONCE_LEN]).unwrap();
        assert_eq!(
            old.voter_hash(&voter, 42).unwrap(),
            old.voter_hash(&voter, 42).unwrap()
        );
        assert_ne!(
            old.voter_hash(&voter, 42).unwrap(),
            old.voter_hash(&voter, 43).unwrap()
        );
        assert_ne!(
            old.voter_hash(&voter, 42).unwrap(),
            old.voter_hash(&other, 42).unwrap()
        );
        assert!(old.voter_hash(&voter, 0).is_err());
        assert!(old.voter_hash(&voter, -1).is_err());
        let form = old.form_token(&voter, 42, NOW).unwrap();
        assert!(new.parse_voter(voter.credential(), NOW).is_none());
        assert!(new.form_token(&voter, 42, NOW).is_err());
        assert!(!new.verify_form_token(&voter, 42, &form, NOW));
        assert!(new.voter_hash(&voter, 42).is_err());
    }
}
