//! Domain-separated peer identities; only thread-scoped labels are public.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::net::IpAddr;

pub struct PosterIdKey([u8; 32]);

/// Private, cross-board deletion throttle identity. Never project to clients or
/// logs; deliberately has no Debug, Display, or serialization implementation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicDeletionRateIdentity([u8; 32]);

impl PublicDeletionRateIdentity {
    /// Full digest for private rate-limit storage, never a public identifier.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Private, cross-board posting throttle identity. Never project to clients or
/// logs; deliberately has no Debug, Display, or serialization implementation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicPostingRateIdentity([u8; 32]);

impl PublicPostingRateIdentity {
    /// Full digest for private rate-limit storage, never a public identifier.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Private, cross-board report admission identity from a trusted transport peer.
/// Never project to clients or logs; no Debug, Display, or serialization.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PublicReportRateIdentity {
    report: [u8; 32],
    history_epoch: [u8; 32],
    source_ipv4: bool,
    restriction: PublicPostingRateIdentity,
}

impl PublicReportRateIdentity {
    /// Full digest for private admission storage, never a public identifier.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.report
    }

    /// Private fingerprint of the key and both admission identity derivations.
    /// It is shared across peers and must never be exposed as a public label.
    pub fn report_history_epoch(&self) -> &[u8; 32] {
        &self.history_epoch
    }

    /// Whether PHP's source ip2long predicate accepts this canonical peer.
    pub fn permits_source_report_exemption(&self) -> bool {
        self.source_ipv4
    }

    /// Independently derived posting identity for source restriction matching.
    /// The pair always comes from the same verified peer and secret key. Report
    /// quotas continue to use their own digest; neither digest is public.
    pub fn restriction_identity(&self) -> &PublicPostingRateIdentity {
        &self.restriction
    }
}

/// Private comparison token for an IPv4 /16 automatic-range event.
/// It is domain-separated from every actor identity and has no public formatter.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AutomaticRangePrefix([u8; 32]);

impl AutomaticRangePrefix {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

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
    /// Poll credentials use their own derived keys. Neither poster labels nor
    /// transport-peer rate identities can be reused as a voting credential.
    pub fn poll_voting_key(&self) -> crate::poll_voting::PollVotingKey {
        crate::poll_voting::PollVotingKey::derive(&self.0)
    }

    /// Optional event cookies and storage capabilities use separate purposes;
    /// existing poster labels and throttle hashes cannot authorize an event.
    pub fn event_key(&self) -> crate::source_events::credentials::Key {
        crate::source_events::credentials::Key::derive(&self.0)
    }

    /// The caller must supply the verified current transport peer, never a
    /// request field. Cookies, boards and environment cannot change this key.
    /// Missing transport identity must be handled by the caller, not fabricated.
    pub fn public_deletion_rate_identity(&self, peer: IpAddr) -> PublicDeletionRateIdentity {
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-public-deletion-rate-v1\0");
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
        PublicDeletionRateIdentity(context.sign().as_ref().try_into().expect("SHA-256 length"))
    }

    /// The caller must supply the verified current transport peer, never a
    /// request field. Cookies, boards and environment cannot change this key.
    /// Missing transport identity must be handled by the caller, not fabricated.
    pub fn public_posting_rate_identity(&self, peer: IpAddr) -> PublicPostingRateIdentity {
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-public-posting-rate-v1\0");
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
        PublicPostingRateIdentity(context.sign().as_ref().try_into().expect("SHA-256 length"))
    }

    /// The caller must supply the verified current transport peer, never a
    /// request field. This IP identity does not represent a password or Pass.
    /// Missing transport identity must be handled by the caller, not fabricated.
    pub fn public_report_rate_identity(&self, peer: IpAddr) -> PublicReportRateIdentity {
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-public-report-rate-v1\0");
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
        let epoch = ring::hmac::sign(
            &signing,
            b"26chan:report-history-epoch:v1\0report-rate-v1\0posting-rate-v1",
        );
        PublicReportRateIdentity {
            report: context.sign().as_ref().try_into().expect("SHA-256 length"),
            history_epoch: epoch.as_ref().try_into().expect("SHA-256 length"),
            source_ipv4: matches!(peer.to_canonical(), IpAddr::V4(ip) if !ip.is_unspecified()),
            restriction: self.public_posting_rate_identity(peer),
        }
    }

    /// Derive only from the canonical trusted peer. Native IPv6 has no source
    /// IPv4 /16 equivalent; never manufacture one from its displayed text.
    pub fn automatic_range_prefix(&self, peer: IpAddr) -> Option<AutomaticRangePrefix> {
        let IpAddr::V4(peer) = peer.to_canonical() else {
            return None;
        };
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0);
        let mut context = ring::hmac::Context::with_key(&signing);
        context.update(b"26chan-automatic-range-prefix-v1\0");
        context.update(&peer.octets()[..2]);
        Some(AutomaticRangePrefix(
            context.sign().as_ref().try_into().expect("SHA-256 length"),
        ))
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
    fn automatic_range_prefix_is_private_keyed_canonical_and_domain_separated() {
        let key = PosterIdKey::parse(&"11".repeat(32)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let token = key.automatic_range_prefix(peer).unwrap();
        // Independent Python stdlib HMAC-SHA256 vector with a synthetic key.
        assert_eq!(
            token
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "ae7908dc5a0ece7502cc78977e7160331e3c94d2c8a3970f89f0b9ea206e48c2"
        );
        for same in ["192.0.0.0", "192.0.255.255", "::ffff:192.0.2.10"] {
            assert!(key.automatic_range_prefix(same.parse().unwrap()).unwrap() == token);
        }
        assert!(
            key.automatic_range_prefix("192.1.2.10".parse().unwrap())
                .unwrap()
                != token
        );
        assert!(
            key.automatic_range_prefix("2001:db8::10".parse().unwrap())
                .is_none()
        );
        assert!(
            PosterIdKey::parse(&"22".repeat(32))
                .unwrap()
                .automatic_range_prefix(peer)
                .unwrap()
                != token
        );
        assert_ne!(
            token.as_bytes(),
            key.public_posting_rate_identity(peer).as_bytes()
        );
        assert_ne!(
            token.as_bytes(),
            key.public_report_rate_identity(peer).as_bytes()
        );
        assert_ne!(
            token.as_bytes(),
            key.public_deletion_rate_identity(peer).as_bytes()
        );
    }

    #[test]
    fn report_rate_identity_is_canonical_full_address_and_keyed() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        for (peer, expected) in [
            (
                "192.0.2.10",
                "ea8668e9b0e5ce786fed77239dadb83365f7ddd58561ce72ee31fe656f56e2cc",
            ),
            (
                "2001:db8::10",
                "cbaf7aee818565aa34196f7a397a5ecb4ec96eb695c488760f5edc5e4c65dc87",
            ),
        ] {
            let peer = peer.parse().unwrap();
            let identity = key.public_report_rate_identity(peer);
            // Independent Python stdlib HMAC-SHA256 vectors.
            assert_eq!(
                identity
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                expected
            );
            assert!(identity == key.public_report_rate_identity(peer));
            assert!(
                identity
                    != PosterIdKey::parse(&"2".repeat(64))
                        .unwrap()
                        .public_report_rate_identity(peer)
            );
            assert_ne!(
                identity.as_bytes(),
                key.public_deletion_rate_identity(peer).as_bytes()
            );
            assert_ne!(
                identity.as_bytes(),
                key.public_posting_rate_identity(peer).as_bytes()
            );
            assert!(*identity.restriction_identity() == key.public_posting_rate_identity(peer));
        }
        let ipv4 = key.public_report_rate_identity("192.0.2.10".parse().unwrap());
        assert!(ipv4 == key.public_report_rate_identity("::ffff:192.0.2.10".parse().unwrap()));
        assert!(ipv4 != key.public_report_rate_identity("192.0.2.11".parse().unwrap()));
        assert!(ipv4 != key.public_report_rate_identity("::192.0.2.10".parse().unwrap()));
        assert!(
            key.public_report_rate_identity("2001:db8::10".parse().unwrap())
                != key.public_report_rate_identity("2001:db8::11".parse().unwrap())
        );
    }

    #[test]
    fn report_rate_identity_is_independent_of_boards_sessions_and_environment() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let identity = key.public_report_rate_identity(peer);
        for board in ["test", "other"] {
            for cookie in ["1", "2"] {
                for country in [*b"US", *b"JP"] {
                    let session = crate::anonymous_session::Capability::parse(&format!(
                        "a1_{}",
                        cookie.repeat(64)
                    ))
                    .unwrap()
                    .fingerprints(Some(peer), country);
                    assert!(identity == key.public_report_rate_identity(peer));
                    assert_ne!(identity.as_bytes(), &session.address);
                    assert_ne!(
                        identity.as_bytes(),
                        &key.robot9000_fingerprint(board, peer).unwrap()
                    );
                    for domain in [
                        b"26chan-poster-id-v1\0".as_slice(),
                        b"26chan-poster-count-v1\0".as_slice(),
                    ] {
                        assert_ne!(
                            identity.as_bytes(),
                            &key.digest(domain, board, 42, peer).unwrap()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn posting_rate_identity_is_canonical_full_address_and_keyed() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        for (peer, expected) in [
            (
                "192.0.2.10",
                "4d84d3b135f830cd10a11a43f1eb35a220ecdb9970628b04fca56e5df034ad37",
            ),
            (
                "2001:db8::10",
                "9b33a4a0ac34fefa9e4ff252dd8106cf4ae869e96ed20a888075f373b25b420a",
            ),
        ] {
            let peer = peer.parse().unwrap();
            let identity = key.public_posting_rate_identity(peer);
            // Independent Python stdlib HMAC-SHA256 vectors.
            assert_eq!(
                identity
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                expected
            );
            assert!(identity == key.public_posting_rate_identity(peer));
            assert!(
                identity
                    != PosterIdKey::parse(&"2".repeat(64))
                        .unwrap()
                        .public_posting_rate_identity(peer)
            );
            assert_ne!(
                identity.as_bytes(),
                key.public_deletion_rate_identity(peer).as_bytes()
            );
        }
        let ipv4 = key.public_posting_rate_identity("192.0.2.10".parse().unwrap());
        assert!(ipv4 == key.public_posting_rate_identity("::ffff:192.0.2.10".parse().unwrap()));
        assert!(ipv4 != key.public_posting_rate_identity("192.0.2.11".parse().unwrap()));
        assert!(ipv4 != key.public_posting_rate_identity("::192.0.2.10".parse().unwrap()));
        // Keep the full address, not the anonymous network's IPv6 /64.
        assert!(
            key.public_posting_rate_identity("2001:db8::10".parse().unwrap())
                != key.public_posting_rate_identity("2001:db8::11".parse().unwrap())
        );
    }

    #[test]
    fn posting_rate_identity_is_independent_of_public_thread_ids_and_sessions() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let identity = key.public_posting_rate_identity(peer);
        let label = key.label("test", 42, peer).unwrap();
        for board in ["test", "other"] {
            for thread in [42, 43] {
                let other_label = key.label(board, thread, peer).unwrap();
                if board != "test" || thread != 42 {
                    assert_ne!(label, other_label);
                }
                assert!(identity == key.public_posting_rate_identity(peer));
                for domain in [
                    b"26chan-poster-id-v1\0".as_slice(),
                    b"26chan-poster-count-v1\0".as_slice(),
                ] {
                    assert_ne!(
                        identity.as_bytes(),
                        &key.digest(domain, board, thread, peer).unwrap()
                    );
                }
            }
        }
        for cookie in ["1", "2"] {
            for country in [*b"US", *b"JP"] {
                let session = crate::anonymous_session::Capability::parse(&format!(
                    "a1_{}",
                    cookie.repeat(64)
                ))
                .unwrap()
                .fingerprints(Some(peer), country);
                assert!(identity == key.public_posting_rate_identity(peer));
                assert_ne!(identity.as_bytes(), &session.address);
            }
        }
    }

    #[test]
    fn deletion_rate_identity_is_canonical_full_address_and_keyed() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        for (peer, expected) in [
            (
                "192.0.2.10",
                "42003ef63786626b3e6656c329529a6fbd331f2967bac9b69a605ec7ad86ab18",
            ),
            (
                "2001:db8::10",
                "7b36e8d645393e34e130b6ef0aa38c546f3ccb4ab52d09dd1eb90f9f182fc60f",
            ),
        ] {
            let identity = key.public_deletion_rate_identity(peer.parse().unwrap());
            // Independent Python stdlib HMAC-SHA256 vectors.
            assert_eq!(
                identity
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                expected
            );
            assert!(
                identity
                    != PosterIdKey::parse(&"2".repeat(64))
                        .unwrap()
                        .public_deletion_rate_identity(peer.parse().unwrap())
            );
        }
        let ipv4 = key.public_deletion_rate_identity("192.0.2.10".parse().unwrap());
        assert!(ipv4 == key.public_deletion_rate_identity("::ffff:192.0.2.10".parse().unwrap()));
        assert!(ipv4 != key.public_deletion_rate_identity("192.0.2.11".parse().unwrap()));
        // Keep the full address, not the anonymous network's IPv6 /64.
        assert!(
            key.public_deletion_rate_identity("2001:db8::10".parse().unwrap())
                != key.public_deletion_rate_identity("2001:db8::11".parse().unwrap())
        );
    }

    #[test]
    fn deletion_rate_identity_is_independent_of_boards_cookies_and_environment() {
        let key = PosterIdKey::parse(&"1".repeat(64)).unwrap();
        let peer = "192.0.2.10".parse().unwrap();
        let expected = key.public_deletion_rate_identity(peer);
        let initial =
            crate::anonymous_session::Capability::parse(&format!("a1_{}", "1".repeat(64)))
                .unwrap()
                .fingerprints(Some(peer), *b"US");
        for board in ["test", "other"] {
            for cookie in ["1", "2"] {
                for country in [*b"US", *b"JP"] {
                    let capability = crate::anonymous_session::Capability::parse(&format!(
                        "a1_{}",
                        cookie.repeat(64)
                    ))
                    .unwrap();
                    let session = capability.fingerprints(Some(peer), country);
                    if cookie != "1" {
                        assert_ne!(initial.address, session.address);
                    }
                    if country != *b"US" {
                        assert_ne!(initial.environment, session.environment);
                    }
                    assert!(expected == key.public_deletion_rate_identity(peer));
                    // Sharing the server key never shares another purpose's identity.
                    assert_ne!(expected.as_bytes(), &session.address);
                    assert_ne!(
                        expected.as_bytes(),
                        &key.robot9000_fingerprint(board, peer).unwrap()
                    );
                    for domain in [
                        b"26chan-poster-id-v1\0".as_slice(),
                        b"26chan-poster-count-v1\0".as_slice(),
                    ] {
                        assert_ne!(
                            expected.as_bytes(),
                            &key.digest(domain, board, 42, peer).unwrap()
                        );
                    }
                }
            }
        }
        // Isolate domain separation from changes in the payload layout.
        let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key.0);
        let other_domain = ring::hmac::sign(&signing, b"26chan-r9k-v1\0\x04\xc0\x00\x02\x0a");
        assert_ne!(expected.as_bytes().as_slice(), other_domain.as_ref());
    }

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

#[cfg(test)]
mod report_history_epoch_tests {
    use super::*;
    #[test]
    fn epoch_tracks_keys_and_derivations_without_becoming_a_peer_identity() {
        let key = PosterIdKey::parse(&"31".repeat(32)).unwrap();
        let first = key.public_report_rate_identity("192.0.2.1".parse().unwrap());
        let second = key.public_report_rate_identity("2001:db8::1".parse().unwrap());
        assert_eq!(first.report_history_epoch(), second.report_history_epoch());
        assert_ne!(first.as_bytes(), second.as_bytes());
        assert_ne!(first.report_history_epoch(), first.as_bytes());
        assert_ne!(
            first.report_history_epoch(),
            first.restriction_identity().as_bytes()
        );
        let other = PosterIdKey::parse(&"32".repeat(32))
            .unwrap()
            .public_report_rate_identity("192.0.2.1".parse().unwrap());
        assert_ne!(first.report_history_epoch(), other.report_history_epoch());
    }
}

#[cfg(test)]
mod report_source_address_tests {
    use super::*;
    #[test]
    fn optional_report_exemption_keeps_the_source_ipv4_predicate() {
        let key = PosterIdKey::parse(&"ab".repeat(32)).unwrap();
        for (address, expected) in [
            ("192.0.2.1", true),
            ("255.255.255.255", true),
            ("::ffff:192.0.2.1", true),
            ("0.0.0.0", false),
            ("::ffff:0.0.0.0", false),
            ("::192.0.2.1", false),
            ("2001:db8::1", false),
            ("::", false),
        ] {
            assert_eq!(
                key.public_report_rate_identity(address.parse().unwrap())
                    .permits_source_report_exemption(),
                expected
            );
        }
    }
}
