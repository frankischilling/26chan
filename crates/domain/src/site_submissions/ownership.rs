//! Random native ownership capabilities. The original controllers are absent.
//! Parsing a capability is not authentication: the private store must find its
//! digest and recheck the current campaign and any required email/challenge.
use crate::ValidationError;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Application,
    ContestEntry,
}
impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Application => "app1.",
            Self::ContestEntry => "entry1.",
        }
    }
}

/// Neither the bearer nor its storage digest implements Debug or Serialize.
/// Raw capabilities belong only in the owner's no-store success/edit response.
pub struct Capability {
    kind: Kind,
    token: String,
    storage_hash: [u8; 32],
}
impl Capability {
    pub fn generate(kind: Kind) -> Result<Self, ValidationError> {
        let mut random = [0u8; 32];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| ValidationError("Could not create submission ownership."))?;
        Ok(Self::from_bytes(kind, random))
    }
    pub fn parse(kind: Kind, raw: &str) -> Option<Self> {
        if raw.len() != kind.prefix().len() + 43 {
            return None;
        }
        let encoded = raw.strip_prefix(kind.prefix())?;
        let mut bytes = [0u8; 32];
        if URL_SAFE_NO_PAD.decode_slice(encoded, &mut bytes).ok()? != 32
            || URL_SAFE_NO_PAD.encode(bytes) != encoded
        {
            return None;
        }
        Some(Self::from_bytes(kind, bytes))
    }
    fn from_bytes(kind: Kind, bytes: [u8; 32]) -> Self {
        let token = format!("{}{}", kind.prefix(), URL_SAFE_NO_PAD.encode(bytes));
        let mut hash = digest::Context::new(&digest::SHA256);
        hash.update(b"26chan-site-submission-ownership-v1\0");
        hash.update(token.as_bytes());
        Self {
            kind,
            token,
            storage_hash: hash.finish().as_ref().try_into().expect("SHA256 length"),
        }
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn storage_hash(&self) -> &[u8; 32] {
        &self.storage_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_are_canonical_and_do_not_cross_workflows() {
        for kind in [Kind::Application, Kind::ContestEntry] {
            let cap = Capability::from_bytes(kind, [7; 32]);
            let parsed = Capability::parse(kind, cap.token()).unwrap();
            assert_eq!(parsed.kind(), kind);
            assert_eq!(parsed.storage_hash(), cap.storage_hash());
            for suffix in ["=", "\n", " ", "/"] {
                assert!(Capability::parse(kind, &format!("{}{suffix}", cap.token())).is_none());
            }
            assert!(Capability::parse(kind, &"x".repeat(100_000)).is_none());
        }
        let app = Capability::from_bytes(Kind::Application, [7; 32]);
        let entry = Capability::from_bytes(Kind::ContestEntry, [7; 32]);
        assert_ne!(app.storage_hash(), entry.storage_hash());
        assert!(Capability::parse(Kind::ContestEntry, app.token()).is_none());
        assert!(Capability::parse(Kind::Application, entry.token()).is_none());
        assert_ne!(
            Capability::generate(Kind::Application)
                .unwrap()
                .storage_hash(),
            Capability::generate(Kind::Application)
                .unwrap()
                .storage_hash()
        );
    }
}
