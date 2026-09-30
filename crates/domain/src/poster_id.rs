//! Thread-scoped public labels, never authentication or a raw network address.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::net::IpAddr;

pub struct PosterIdKey([u8; 32]);
impl PosterIdKey {
    pub fn parse(value: &str) -> Result<Self, crate::ValidationError> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(crate::ValidationError(
                "POSTER_ID_KEY must contain 64 hexadecimal digits.",
            ));
        }
        let mut key = [0; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| crate::ValidationError("Invalid POSTER_ID_KEY."))?;
        }
        if key.iter().all(|byte| *byte == 0) {
            return Err(crate::ValidationError(
                "POSTER_ID_KEY cannot be all zeroes.",
            ));
        }
        Ok(Self(key))
    }
    pub fn label(
        &self,
        board: &str,
        thread: i64,
        peer: IpAddr,
    ) -> Result<String, crate::ValidationError> {
        crate::BoardSlug::parse(board)?;
        if thread <= 0 {
            return Err(crate::ValidationError("Invalid poster ID thread."));
        }
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-poster-id-v1\0");
        context.update(board.as_bytes());
        context.update(&[0]);
        context.update(&thread.to_be_bytes());
        match peer.to_canonical() {
            IpAddr::V4(peer) => {
                context.update(&[4]);
                context.update(&peer.octets());
            }
            IpAddr::V6(peer) => {
                context.update(&[6]);
                context.update(&peer.octets());
            }
        }
        Ok(STANDARD.encode(&context.sign().as_ref()[..6]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_are_canonical_thread_scoped_and_keyed() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let address = "192.0.2.10".parse().unwrap();
        let first = key.label("test", 42, address).unwrap();
        // Independent Python stdlib HMAC-SHA256/base64 vector.
        assert_eq!(first, "0ww1zHbb");
        assert_eq!(first.len(), 8);
        assert!(
            first
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        );
        assert_eq!(
            first,
            key.label("test", 42, "::ffff:192.0.2.10".parse().unwrap())
                .unwrap()
        );
        for other in [
            key.label("other", 42, address).unwrap(),
            key.label("test", 43, address).unwrap(),
            key.label("test", 42, "192.0.2.11".parse().unwrap())
                .unwrap(),
            PosterIdKey::parse(&"2".repeat(64))
                .unwrap()
                .label("test", 42, address)
                .unwrap(),
        ] {
            assert_ne!(first, other);
        }
        assert!(key.label("test", 0, address).is_err());
        assert!(key.label("../test", 42, address).is_err());
        for invalid in [
            "0".repeat(64),
            "g".repeat(64),
            "1".repeat(63),
            "1".repeat(65),
        ] {
            assert!(PosterIdKey::parse(&invalid).is_err());
        }
    }
}
