//! Thread-scoped public labels, never authentication or a raw network address.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::net::IpAddr;

pub struct PosterIdKey([u8; 32]);
pub struct PosterCountContext {
    pub fingerprint: String,
    pub epoch: String,
}
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
        let digest = self.digest(b"26chan-poster-id-v1\0", board, thread, peer)?;
        Ok(STANDARD.encode(&digest[..6]))
    }
    pub fn count_context(
        &self,
        board: &str,
        thread: i64,
        peer: IpAddr,
    ) -> Result<PosterCountContext, crate::ValidationError> {
        let fingerprint = self.digest(b"26chan-poster-count-v1\0", board, thread, peer)?;
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let epoch = ring::hmac::sign(&signing, b"26chan-poster-count-epoch-v1\0");
        let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(PosterCountContext {
            fingerprint: hex(&fingerprint),
            epoch: hex(epoch.as_ref()),
        })
    }
    pub fn robot9000_fingerprint(
        &self,
        board: &str,
        peer: IpAddr,
    ) -> Result<[u8; 32], crate::ValidationError> {
        crate::BoardSlug::parse(board)?;
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-r9k-v1\0");
        context.update(board.as_bytes());
        context.update(&[0]);
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
        Ok(context.sign().as_ref().try_into().expect("SHA-256 length"))
    }
    fn digest(
        &self,
        prefix: &[u8],
        board: &str,
        thread: i64,
        peer: IpAddr,
    ) -> Result<[u8; 32], crate::ValidationError> {
        crate::BoardSlug::parse(board)?;
        if thread <= 0 {
            return Err(crate::ValidationError("Invalid poster ID thread."));
        }
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(prefix);
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
        Ok(context.sign().as_ref().try_into().expect("SHA-256 length"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn count_contexts_keep_full_digests_and_detect_key_epochs() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let context = key.count_context("test", 42, peer).unwrap();
        // Independent Python stdlib HMAC-SHA256 vectors.
        assert_eq!(
            context.fingerprint,
            "6f6608a2a7d49a95f44671ad22606c41df1638486a5f29e962f2205af6f42ae2"
        );
        assert_eq!(
            context.epoch,
            "69ff6a15e216180257a02b59d8684ab380ef8a652d54ce385be9ddf055ba1fb5"
        );
        let mapped = key
            .count_context("test", 42, "::ffff:192.0.2.10".parse().unwrap())
            .unwrap();
        assert_eq!(mapped.fingerprint, context.fingerprint);
        for changed in [
            key.count_context("other", 42, peer).unwrap(),
            key.count_context("test", 43, peer).unwrap(),
            key.count_context("test", 42, "192.0.2.11".parse().unwrap())
                .unwrap(),
        ] {
            assert_ne!(changed.fingerprint, context.fingerprint);
            assert_eq!(changed.epoch, context.epoch);
        }
        let rotated = PosterIdKey::parse(&"2".repeat(64))
            .unwrap()
            .count_context("test", 42, peer)
            .unwrap();
        assert_ne!(rotated.epoch, context.epoch);
        assert_ne!(rotated.fingerprint, context.fingerprint);
    }
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
    #[test]
    fn robot9000_fingerprints_are_private_board_scoped_and_canonical() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let first = key.robot9000_fingerprint("r9k", peer).unwrap();
        assert_eq!(
            first
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "cd02dccd1cdd175f5f3fb02a00ba57eff8398e6aea76d0733d62a8acb28ae666"
        );
        assert_eq!(
            first,
            key.robot9000_fingerprint("r9k", "::ffff:192.0.2.10".parse().unwrap())
                .unwrap()
        );
        assert_ne!(first, key.robot9000_fingerprint("test", peer).unwrap());
        assert!(key.robot9000_fingerprint("../r9k", peer).is_err());
    }
}
