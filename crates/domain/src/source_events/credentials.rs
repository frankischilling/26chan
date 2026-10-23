//! Rewrite-defined event credentials. Source bootstrap secrets are not reused.
//! Browser credentials and private storage identities have separate derivations.
use crate::ValidationError;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::hmac;
use std::net::IpAddr;

pub const COOKIE_SECONDS: i64 = 172_800;
const PAYLOAD: usize = 57;
const PREFIX: &str = "ev1.";
const ENCODED_PAYLOAD: usize = 76;
const ENCODED_TAG: usize = 43;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Market = 1,
    Emotes = 2,
}

/// One immutable, operator-defined run. Closing a run must be checked against
/// current stored policy before issuing or accepting any account operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    kind: Kind,
    id: i64,
    starts_at: i64,
    ends_at: i64,
}
impl Run {
    pub fn new(kind: Kind, id: i64, starts_at: i64, ends_at: i64) -> Result<Self, ValidationError> {
        if id <= 0 || starts_at < 0 || ends_at <= starts_at {
            return Err(ValidationError("Invalid event run"));
        }
        Ok(Self {
            kind,
            id,
            starts_at,
            ends_at,
        })
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn id(&self) -> i64 {
        self.id
    }
    pub fn starts_at(&self) -> i64 {
        self.starts_at
    }
    pub fn ends_at(&self) -> i64 {
        self.ends_at
    }
    pub fn active(&self, now: i64) -> bool {
        now >= self.starts_at && now < self.ends_at
    }
}

/// No Debug, Display or Serialize implementation: credentials and actor hashes
/// must not enter logs, HTML, public JSON or analytics.
pub struct Key {
    cookie: hmac::Key,
    peer: hmac::Key,
    storage: hmac::Key,
}
pub struct Session {
    credential: String,
    storage_hash: [u8; 32],
    expires_at: i64,
}
impl Session {
    /// Secret bearer credential for the HttpOnly cookie only.
    pub fn credential(&self) -> &str {
        &self.credential
    }
    /// Private database capability, not a public account identifier.
    pub fn storage_hash(&self) -> &[u8; 32] {
        &self.storage_hash
    }
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
}
impl Key {
    pub(crate) fn derive(master: &[u8; 32]) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, master);
        let derive =
            |purpose: &[u8]| hmac::Key::new(hmac::HMAC_SHA256, hmac::sign(&key, purpose).as_ref());
        Self {
            cookie: derive(b"26chan-event-cookie-v1\0"),
            peer: derive(b"26chan-event-peer-v1\0"),
            storage: derive(b"26chan-event-storage-v1\0"),
        }
    }
    /// Preserve source same-address recovery without publishing a raw address
    /// or using its historical hardcoded key. The caller supplies the verified
    /// transport peer, never a submitted address or forwarding header.
    pub fn issue_for_peer(
        &self,
        run: Run,
        peer: IpAddr,
        now: i64,
    ) -> Result<Session, ValidationError> {
        if !run.active(now) {
            return Err(ValidationError("Event is closed"));
        }
        let expiry = expiry(run, now).ok_or(ValidationError("Invalid event time"))?;
        let mut context = hmac::Context::with_key(&self.peer);
        context.update(&[run.kind as u8]);
        context.update(&run.id.to_be_bytes());
        match peer.to_canonical() {
            IpAddr::V4(ip) => {
                context.update(&[4]);
                context.update(&ip.octets());
            }
            IpAddr::V6(ip) => {
                context.update(&[6]);
                context.update(&ip.octets());
            }
        }
        let nonce = context.sign();
        let mut payload = [0u8; PAYLOAD];
        payload[0] = run.kind as u8;
        payload[1..9].copy_from_slice(&run.id.to_be_bytes());
        payload[9..17].copy_from_slice(&now.to_be_bytes());
        payload[17..25].copy_from_slice(&expiry.to_be_bytes());
        payload[25..].copy_from_slice(nonce.as_ref());
        let tag = hmac::sign(&self.cookie, &payload);
        let credential = format!(
            "{PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(payload),
            URL_SAFE_NO_PAD.encode(tag.as_ref())
        );
        Ok(Session {
            credential,
            storage_hash: self.storage_hash(&payload),
            expires_at: expiry,
        })
    }
    pub fn parse(&self, run: Run, token: &str, now: i64) -> Option<Session> {
        if !run.active(now) || token.len() != PREFIX.len() + ENCODED_PAYLOAD + 1 + ENCODED_TAG {
            return None;
        }
        let (body, tag) = token.strip_prefix(PREFIX)?.split_once('.')?;
        if body.len() != ENCODED_PAYLOAD || tag.len() != ENCODED_TAG {
            return None;
        }
        let mut payload = [0u8; PAYLOAD];
        let mut signature = [0u8; 32];
        if URL_SAFE_NO_PAD.decode_slice(body, &mut payload).ok()? != PAYLOAD
            || URL_SAFE_NO_PAD.decode_slice(tag, &mut signature).ok()? != 32
        {
            return None;
        }
        if URL_SAFE_NO_PAD.encode(payload) != body || URL_SAFE_NO_PAD.encode(signature) != tag {
            return None;
        }
        hmac::verify(&self.cookie, &payload, &signature).ok()?;
        let id = i64::from_be_bytes(payload[1..9].try_into().ok()?);
        let issued_at = i64::from_be_bytes(payload[9..17].try_into().ok()?);
        let expires_at = i64::from_be_bytes(payload[17..25].try_into().ok()?);
        if payload[0] != run.kind as u8
            || id != run.id
            || !run.active(issued_at)
            || now < issued_at
            || now >= expires_at
            || expires_at != expiry(run, issued_at)?
        {
            return None;
        }
        Some(Session {
            credential: token.to_owned(),
            storage_hash: self.storage_hash(&payload),
            expires_at,
        })
    }
    fn storage_hash(&self, payload: &[u8; PAYLOAD]) -> [u8; 32] {
        let mut context = hmac::Context::with_key(&self.storage);
        context.update(&payload[..9]);
        context.update(&payload[25..]);
        context.sign().as_ref().try_into().expect("SHA-256 length")
    }
}
fn expiry(run: Run, issued_at: i64) -> Option<i64> {
    Some(issued_at.checked_add(COOKIE_SECONDS)?.min(run.ends_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> Key {
        Key::derive(&[7; 32])
    }
    fn run() -> Run {
        Run::new(Kind::Emotes, 12, 100, 1_000_000).unwrap()
    }
    fn peer() -> IpAddr {
        "192.0.2.1".parse().unwrap()
    }
    #[test]
    fn credentials_expire_and_private_identity_survives_cookie_renewal() {
        let key = key();
        let run = run();
        let session = key.issue_for_peer(run, peer(), 100).unwrap();
        assert_eq!(session.expires_at(), 100 + COOKIE_SECONDS);
        assert!(key.parse(run, session.credential(), 99).is_none());
        assert!(key.parse(run, session.credential(), 100).is_some());
        assert!(
            key.parse(run, session.credential(), 100 + COOKIE_SECONDS - 1)
                .is_some()
        );
        assert!(
            key.parse(run, session.credential(), 100 + COOKIE_SECONDS)
                .is_none()
        );
        let renewed = key.issue_for_peer(run, peer(), 101).unwrap();
        assert_ne!(renewed.credential(), session.credential());
        assert_eq!(renewed.storage_hash(), session.storage_hash());
        assert!(!session.credential().contains("192.0.2.1"));
        assert_ne!(
            &URL_SAFE_NO_PAD
                .decode(session.credential().split('.').nth(1).unwrap())
                .unwrap()[25..],
            session.storage_hash()
        );
    }
    #[test]
    fn run_kind_peer_key_and_window_boundaries_are_not_interchangeable() {
        let key = key();
        let current = run();
        let session = key.issue_for_peer(current, peer(), 100).unwrap();
        for other in [
            Run::new(Kind::Market, 12, 100, 1_000_000).unwrap(),
            Run::new(Kind::Emotes, 13, 100, 1_000_000).unwrap(),
        ] {
            assert!(key.parse(other, session.credential(), 100).is_none());
            assert_ne!(
                key.issue_for_peer(other, peer(), 100)
                    .unwrap()
                    .storage_hash(),
                session.storage_hash()
            );
        }
        assert!(
            Key::derive(&[8; 32])
                .parse(current, session.credential(), 100)
                .is_none()
        );
        assert_ne!(
            key.issue_for_peer(current, "192.0.2.2".parse().unwrap(), 100)
                .unwrap()
                .storage_hash(),
            session.storage_hash()
        );
        assert_eq!(
            key.issue_for_peer(current, "::ffff:192.0.2.1".parse().unwrap(), 100)
                .unwrap()
                .storage_hash(),
            session.storage_hash()
        );
        let short = Run::new(Kind::Market, 14, 100, 110).unwrap();
        let short_session = key.issue_for_peer(short, peer(), 100).unwrap();
        assert_eq!(short_session.expires_at(), 110);
        assert!(key.parse(short, short_session.credential(), 110).is_none());
        assert!(key.issue_for_peer(short, peer(), 99).is_err());
        assert!(key.issue_for_peer(short, peer(), 110).is_err());
        assert!(Run::new(Kind::Market, 0, 0, 100).is_err());
        assert!(Run::new(Kind::Market, 1, 100, 100).is_err());
    }
    #[test]
    fn every_changed_byte_and_noncanonical_encoding_is_rejected() {
        let key = key();
        let run = run();
        let token = key
            .issue_for_peer(run, peer(), 100)
            .unwrap()
            .credential()
            .to_owned();
        for index in 0..token.len() {
            let mut changed = token.as_bytes().to_vec();
            changed[index] = if changed[index] == b'A' { b'B' } else { b'A' };
            let changed = String::from_utf8(changed).unwrap();
            assert!(key.parse(run, &changed, 100).is_none(), "byte {index}");
        }
        for bad in [
            String::new(),
            token.clone() + "=",
            token.clone() + ".extra",
            "x".repeat(10000),
        ] {
            assert!(key.parse(run, &bad, 100).is_none());
        }
    }
}
