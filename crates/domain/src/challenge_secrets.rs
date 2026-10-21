//! Domain-separated private verification material. No type containing a key,
//! address or bearer token implements Debug or automatic serialization.
use ring::{
    digest::{Context, SHA256},
    hmac,
    rand::{SecureRandom, SystemRandom},
};

pub struct VerificationKey(hmac::Key);

/// Render only inside the verification issuer. Never serialize into its public
/// response; the browser receives raster images and an opaque challenge ID.
pub struct GeneratedAnswer {
    bytes: [u8; 8],
    len: usize,
}
impl GeneratedAnswer {
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).expect("fixed ASCII challenge alphabet")
    }
}

impl VerificationKey {
    pub fn parse(value: &str) -> Option<Self> {
        Some(Self(hmac::Key::new(hmac::HMAC_SHA256, &decode(value)?)))
    }

    fn digest(&self, purpose: &[u8], parts: &[&[u8]]) -> [u8; 32] {
        let mut context = hmac::Context::with_key(&self.0);
        context.update(b"26chan:public-verification:v1\0");
        context.update(purpose);
        for part in parts {
            context.update(&(part.len() as u64).to_be_bytes());
            context.update(part);
        }
        context
            .sign()
            .as_ref()
            .try_into()
            .expect("HMAC-SHA256 size")
    }

    pub fn pass_identifier(&self, value: &str) -> Option<[u8; 32]> {
        if value.len() != 10 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return None;
        }
        Some(self.digest(b"pass-identifier", &[value.to_ascii_uppercase().as_bytes()]))
    }

    /// Deployment-owned VIP identifiers use exact UTF-8 spelling. The supplied
    /// reconstructed schema does not establish the original SQL collation.
    pub fn vip_identifier(&self, value: &str) -> Option<[u8; 32]> {
        if matches!(value, "" | "0")
            || value.len() > crate::MAX_PUBLIC_FIELD_BYTES
            || value.contains('!')
        {
            return None;
        }
        Some(self.digest(b"vip-identifier", &[value.as_bytes()]))
    }

    pub fn pass_recipient(&self, value: &str) -> Option<[u8; 32]> {
        let recipient = crate::pass_mail::Recipient::parse(value)?;
        Some(self.digest(b"pass-recipient", &[recipient.identity().as_bytes()]))
    }

    pub fn pass_renewal(&self, value: &str) -> Option<[u8; 32]> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return None;
        }
        Some(self.digest(b"pass-renewal", &[value.as_bytes()]))
    }

    pub fn checkout_view(&self, nonce: &str, reference: &[u8; 32]) -> Option<CheckoutToken> {
        let nonce = decode(nonce)?;
        Some(CheckoutToken(
            self.digest(b"checkout-idempotent-view", &[&nonce, reference]),
        ))
    }

    pub fn checkout_purchase_view(
        &self,
        nonce: &str,
        recipient: &[u8; 32],
    ) -> Option<CheckoutToken> {
        let nonce = decode(nonce)?;
        Some(CheckoutToken(
            self.digest(b"checkout-purchase-view", &[&nonce, recipient]),
        ))
    }

    pub fn pass_networks(&self, address: std::net::IpAddr) -> Option<PassNetworks> {
        use std::net::IpAddr;
        let address = match address {
            IpAddr::V6(value) => value.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(address),
            _ => address,
        };
        if address.is_unspecified() || address.is_multicast() {
            return None;
        }
        let (login, posting) = match address {
            IpAddr::V4(value) => {
                let raw = u32::from(value);
                let login = raw & 0xffff0000;
                let posting = raw & 0xffffff00;
                (
                    (login != 0).then(|| self.digest(b"pass-login-v4", &[&login.to_be_bytes()])),
                    (posting != 0)
                        .then(|| self.digest(b"pass-posting-v4", &[&posting.to_be_bytes()])),
                )
            }
            IpAddr::V6(value) => {
                // Native IPv6 uses /64 scopes; the source's IPv4-only masking
                // otherwise collapses IPv6 peers to an unset network.
                let bytes = value.octets();
                (
                    Some(self.digest(b"pass-login-v6", &[&bytes[..8]])),
                    Some(self.digest(b"pass-posting-v6", &[&bytes[..8]])),
                )
            }
        };
        Some(PassNetworks {
            peer: self.network_identity(address),
            login,
            posting,
        })
    }

    pub fn email_identity(&self, email: &crate::email_verification::PreparedEmail) -> [u8; 32] {
        self.digest(b"email-identity", &[email.identity().as_bytes()])
    }

    pub fn email_recipient(&self, email: &crate::email_verification::PreparedEmail) -> [u8; 32] {
        self.digest(b"email-recipient", &[email.recipient().as_bytes()])
    }

    pub fn form_context(
        &self,
        purpose: &str,
        board: Option<&str>,
        target: i64,
        nonce: &str,
    ) -> Option<[u8; 32]> {
        let nonce = decode(nonce)?;
        if target < 0
            || board.is_some_and(|board| {
                board.is_empty()
                    || board.len() > 10
                    || !board
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
        {
            return None;
        }
        let valid = match purpose {
            "email_request" => board.is_none() && target == 0,
            "thread" | "restriction_status" => board.is_some() && target == 0,
            "reply" | "report" | "warning_view" => board.is_some() && target > 0,
            "ticket" => board.is_some(),
            _ => false,
        };
        valid.then(|| {
            self.digest(
                b"form-context",
                &[
                    purpose.as_bytes(),
                    board.unwrap_or("").as_bytes(),
                    &target.to_be_bytes(),
                    &nonce,
                ],
            )
        })
    }

    /// The trusted transport address is canonicalized by the caller. This key
    /// is separate from session-local fingerprints so request quotas cannot be
    /// reset by discarding an anonymous cookie.
    pub fn network_identity(&self, address: std::net::IpAddr) -> [u8; 32] {
        match address {
            std::net::IpAddr::V4(address) => self.digest(b"peer-ipv4", &[&address.octets()]),
            std::net::IpAddr::V6(address) => match address.to_ipv4_mapped() {
                Some(address) => self.digest(b"peer-ipv4", &[&address.octets()]),
                None => self.digest(b"peer-ipv6", &[&address.octets()]),
            },
        }
    }

    pub fn token_hash(&self, token: &VerificationToken) -> [u8; 32] {
        self.digest(token.kind.purpose(), &[&token.bytes])
    }

    pub fn render_seed(&self, token: &VerificationToken) -> Option<[u8; 32]> {
        (token.kind == TokenKind::Challenge)
            .then(|| self.digest(b"challenge-render", &[&token.bytes]))
    }

    pub fn challenge_answer(
        &self,
        token: &VerificationToken,
        characters: u8,
    ) -> Option<GeneratedAnswer> {
        const ALPHABET: &[u8; 20] = b"ADGHJKMNPRSTVWXY0248";
        if token.kind != TokenKind::Challenge || !(4..=8).contains(&characters) {
            return None;
        }
        let entropy = self.digest(b"challenge-solution", &[&token.bytes]);
        let mut bytes = [0; 8];
        let mut len = 0;
        for byte in entropy {
            // Rejection sampling avoids biased glyph probabilities. Extremely
            // unlikely exhaustion fails closed instead of using a fallback.
            if byte >= 240 {
                continue;
            }
            bytes[len] = ALPHABET[usize::from(byte % 20)];
            len += 1;
            if len == usize::from(characters) {
                return Some(GeneratedAnswer { bytes, len });
            }
        }
        None
    }

    /// Answer digests are private database values, never challenge JSON fields.
    /// Purpose and target context must also be matched during consumption.
    pub fn answer_hash(&self, challenge: &VerificationToken, answer: &str) -> Option<[u8; 32]> {
        if challenge.kind != TokenKind::Challenge || answer.len() > 64 {
            return None;
        }
        // Primary Twister normalizeReponseStr: retain ASCII alphanumerics,
        // uppercase, then fold its eight confusable glyphs. Bound raw work and
        // normalized storage before hashing either issued or submitted text.
        let mut normalized = [0; 8];
        let mut len = 0;
        for byte in answer.bytes().filter(u8::is_ascii_alphanumeric) {
            if len == normalized.len() {
                return None;
            }
            normalized[len] = match byte.to_ascii_uppercase() {
                b'F' => b'P',
                b'B' => b'8',
                b'O' | b'Q' => b'0',
                b'Z' => b'2',
                b'U' => b'V',
                b'5' => b'S',
                b'6' => b'G',
                byte => byte,
            };
            len += 1;
        }
        if len < 4 {
            return None;
        }
        Some(self.digest(b"challenge-answer", &[&challenge.bytes, &normalized[..len]]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Challenge,
    Email,
    Ticket,
}

impl TokenKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Challenge => "c1_",
            Self::Email => "e1_",
            Self::Ticket => "t1_",
        }
    }
    fn purpose(self) -> &'static [u8] {
        match self {
            Self::Challenge => b"challenge-token",
            Self::Email => b"email-token",
            Self::Ticket => b"pre-cooldown-ticket",
        }
    }
}

pub struct VerificationToken {
    kind: TokenKind,
    bytes: [u8; 32],
}

/// A random receipt candidate is not authority until the isolated issuer has
/// recorded a successful, context-bound verification. The public writer can
/// hash and consume a receipt without inheriting the issuer's signing key.
pub struct PassNetworks {
    pub peer: [u8; 32],
    pub login: Option<[u8; 32]>,
    pub posting: Option<[u8; 32]>,
}

pub struct PassSessionToken([u8; 32]);
impl PassSessionToken {
    pub fn generate() -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self(bytes))
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(Self(decode(value.strip_prefix("p1_")?)?))
    }
    pub fn encode(&self) -> String {
        encode("p1_", &self.0)
    }
    pub fn storage_hash(&self) -> [u8; 32] {
        let mut context = Context::new(&SHA256);
        context.update(b"26chan:pass-session:v1\0");
        context.update(&self.0);
        context.finish().as_ref().try_into().expect("SHA-256 size")
    }
}

/// Single-use PIN recovery material, isolated from membership sessions and receipts.
pub struct PassRecoveryToken([u8; 32]);
impl PassRecoveryToken {
    pub fn generate() -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self(bytes))
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(Self(decode(value.strip_prefix("pr1_")?)?))
    }
    pub fn encode(&self) -> String {
        encode("pr1_", &self.0)
    }
    pub fn storage_hash(&self) -> [u8; 32] {
        let mut context = Context::new(&SHA256);
        context.update(b"26chan:pass-recovery:v1\0");
        context.update(&self.0);
        context.finish().as_ref().try_into().expect("SHA-256 size")
    }
}

/// Browser capability for one checkout result. It never authenticates a Pass.
pub struct CheckoutToken([u8; 32]);
impl CheckoutToken {
    pub fn generate() -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self(bytes))
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(Self(decode(value.strip_prefix("po1_")?)?))
    }
    pub fn encode(&self) -> String {
        encode("po1_", &self.0)
    }
    pub fn storage_hash(&self) -> [u8; 32] {
        let mut context = Context::new(&SHA256);
        context.update(b"26chan:checkout-view:v1\0");
        context.update(&self.0);
        context.finish().as_ref().try_into().expect("SHA-256 size")
    }
}

pub struct ReceiptToken([u8; 32]);
impl ReceiptToken {
    pub fn generate() -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self(bytes))
    }
    pub fn parse(value: &str) -> Option<Self> {
        Some(Self(decode(value.strip_prefix("r1_")?)?))
    }
    pub fn encode(&self) -> String {
        encode("r1_", &self.0)
    }
    pub fn storage_hash(&self) -> [u8; 32] {
        let mut context = Context::new(&SHA256);
        context.update(b"26chan:verification-receipt:v1\0");
        context.update(&self.0);
        context.finish().as_ref().try_into().expect("SHA-256 size")
    }
}

impl VerificationToken {
    pub fn generate(kind: TokenKind) -> Result<Self, ring::error::Unspecified> {
        let mut bytes = [0; 32];
        SystemRandom::new().fill(&mut bytes)?;
        Ok(Self { kind, bytes })
    }
    pub fn parse(kind: TokenKind, value: &str) -> Option<Self> {
        Some(Self {
            kind,
            bytes: decode(value.strip_prefix(kind.prefix())?)?,
        })
    }
    /// Expose only through the matching challenge/ticket response or mail queue.
    pub fn encode(&self) -> String {
        encode(self.kind.prefix(), &self.bytes)
    }
}

fn encode(prefix: &str, bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(67);
    output.push_str(prefix);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}

pub(crate) fn decode(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        }
    }
    let mut bytes = [0; 32];
    for (output, input) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
        *output = nibble(input[0])? * 16 + nibble(input[1])?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_encodings_and_domains_are_disjoint() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let challenge =
            VerificationToken::parse(TokenKind::Challenge, &format!("c1_{}", "02".repeat(32)))
                .unwrap();
        let email =
            VerificationToken::parse(TokenKind::Email, &format!("e1_{}", "02".repeat(32))).unwrap();
        assert_ne!(key.token_hash(&challenge), key.token_hash(&email));
        assert!(VerificationToken::parse(TokenKind::Email, &challenge.encode()).is_none());
        assert!(VerificationToken::parse(TokenKind::Challenge, &email.encode()).is_none());
        assert!(key.answer_hash(&email, "ADGH").is_none());
        assert_eq!(
            key.answer_hash(&challenge, "adgh"),
            key.answer_hash(&challenge, "ADGH")
        );
        assert_eq!(
            key.answer_hash(&challenge, " f-b.o:z_u5 "),
            key.answer_hash(&challenge, "P802VS")
        );
        assert_eq!(
            key.answer_hash(&challenge, "6qéad"),
            key.answer_hash(&challenge, "G0AD")
        );
        assert!(
            key.answer_hash(&challenge, &format!("{}ADGH", " ".repeat(61)))
                .is_none()
        );
        assert_ne!(
            key.answer_hash(&challenge, "ADGH"),
            key.answer_hash(&challenge, "ADGJ")
        );
        for invalid in ["abc", "ABCDEFGHI", "A GJ", "éé"] {
            assert!(key.answer_hash(&challenge, invalid).is_none());
        }
    }
    #[test]
    fn keys_addresses_and_normalized_accounts_have_separate_hashes() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let other = VerificationKey::parse(&"02".repeat(32)).unwrap();
        let a = crate::email_verification::PreparedEmail::prepare("a.b@gmail.com").unwrap();
        let b = crate::email_verification::PreparedEmail::prepare("ab@gmail.com").unwrap();
        assert_eq!(key.email_identity(&a), key.email_identity(&b));
        assert_ne!(key.email_recipient(&a), key.email_recipient(&b));
        assert_ne!(key.email_recipient(&a), key.email_identity(&a));
        assert_ne!(key.email_identity(&a), other.email_identity(&a));
        assert_eq!(
            key.network_identity("192.0.2.1".parse().unwrap()),
            key.network_identity("::ffff:192.0.2.1".parse().unwrap())
        );
        assert_ne!(
            key.network_identity("192.0.2.1".parse().unwrap()),
            key.network_identity("192.0.2.2".parse().unwrap())
        );
        for invalid in ["", "00", &"GG".repeat(32), &"AA".repeat(32)] {
            assert!(VerificationKey::parse(invalid).is_none());
        }
    }
    #[test]
    fn form_contexts_bind_action_board_target_and_nonce_without_an_email_address() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let nonce = "ab".repeat(32);
        let expected = key.form_context("reply", Some("g"), 42, &nonce).unwrap();
        assert_ne!(
            Some(expected),
            key.form_context("report", Some("g"), 42, &nonce)
        );
        assert_ne!(
            Some(expected),
            key.form_context("reply", Some("a"), 42, &nonce)
        );
        assert_ne!(
            Some(expected),
            key.form_context("reply", Some("g"), 43, &nonce)
        );
        assert_ne!(
            Some(expected),
            key.form_context("reply", Some("g"), 42, &"cd".repeat(32))
        );
        assert!(key.form_context("email_request", None, 0, &nonce).is_some());
        assert!(
            key.form_context("email_request", Some("g"), 0, &nonce)
                .is_none()
        );
        assert!(key.form_context("reply", Some("g"), 0, &nonce).is_none());
        assert!(
            key.form_context("thread", Some("../g"), 0, &nonce)
                .is_none()
        );
        assert!(key.form_context("email_verify", None, 0, &nonce).is_none());
    }
    #[test]
    fn pass_warning_contexts_are_separate_from_posting_and_bound_to_exact_targets() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let nonce = "ab".repeat(32);
        let status = key
            .form_context("restriction_status", Some("g"), 0, &nonce)
            .unwrap();
        let warning = key
            .form_context("warning_view", Some("g"), 42, &nonce)
            .unwrap();
        assert_ne!(
            Some(status),
            key.form_context("thread", Some("g"), 0, &nonce)
        );
        for purpose in ["reply", "report"] {
            assert_ne!(
                Some(warning),
                key.form_context(purpose, Some("g"), 42, &nonce)
            );
        }
        assert_ne!(
            Some(warning),
            key.form_context("warning_view", Some("g"), 43, &nonce)
        );
        assert_ne!(
            Some(warning),
            key.form_context("warning_view", Some("a"), 42, &nonce)
        );
        assert_ne!(
            Some(warning),
            key.form_context("warning_view", Some("g"), 42, &"cd".repeat(32))
        );
        assert!(
            key.form_context("warning_view", Some("g"), 0, &nonce)
                .is_none()
        );
        assert!(
            key.form_context("restriction_status", Some("g"), 42, &nonce)
                .is_none()
        );
        assert!(
            key.form_context("restriction_status", None, 0, &nonce)
                .is_none()
        );
    }
    #[test]
    fn generated_tokens_roundtrip_without_using_display_or_debug() {
        for kind in [TokenKind::Challenge, TokenKind::Email, TokenKind::Ticket] {
            let token = VerificationToken::generate(kind).unwrap();
            assert_eq!(
                VerificationToken::parse(kind, &token.encode())
                    .unwrap()
                    .encode(),
                token.encode()
            );
        }
    }

    #[test]
    fn tickets_cannot_be_used_as_answers_emails_or_receipts() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let ticket =
            VerificationToken::parse(TokenKind::Ticket, &format!("t1_{}", "02".repeat(32)))
                .unwrap();
        for kind in [TokenKind::Challenge, TokenKind::Email] {
            assert!(VerificationToken::parse(kind, &ticket.encode()).is_none());
            let other =
                VerificationToken::parse(kind, &format!("{}{}", kind.prefix(), "02".repeat(32)))
                    .unwrap();
            assert_ne!(key.token_hash(&ticket), key.token_hash(&other));
            assert!(VerificationToken::parse(TokenKind::Ticket, &other.encode()).is_none());
        }
        assert!(ReceiptToken::parse(&ticket.encode()).is_none());
        assert!(key.answer_hash(&ticket, "ADGH").is_none());
        assert!(key.challenge_answer(&ticket, 6).is_none());
        assert!(key.render_seed(&ticket).is_none());
    }

    #[test]
    fn receipt_hashing_needs_no_issuer_key_but_keeps_its_own_namespace() {
        let receipt = ReceiptToken::generate().unwrap();
        assert_eq!(
            ReceiptToken::parse(&receipt.encode())
                .unwrap()
                .storage_hash(),
            receipt.storage_hash()
        );
        let challenge = VerificationToken::parse(
            TokenKind::Challenge,
            &receipt.encode().replacen("r1_", "c1_", 1),
        )
        .unwrap();
        assert!(ReceiptToken::parse(&challenge.encode()).is_none());
        assert!(VerificationToken::parse(TokenKind::Email, &receipt.encode()).is_none());
        assert!(VerificationToken::parse(TokenKind::Challenge, &receipt.encode()).is_none());
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        assert_ne!(receipt.storage_hash(), key.token_hash(&challenge));
    }

    #[test]
    fn generated_answers_are_private_canonical_and_bound_to_the_nonce() {
        let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
        let first =
            VerificationToken::parse(TokenKind::Challenge, &format!("c1_{}", "02".repeat(32)))
                .unwrap();
        let second =
            VerificationToken::parse(TokenKind::Challenge, &format!("c1_{}", "03".repeat(32)))
                .unwrap();
        for length in 4..=8 {
            let answer = key.challenge_answer(&first, length).unwrap();
            assert_eq!(answer.as_str().len(), usize::from(length));
            assert!(
                answer
                    .as_str()
                    .bytes()
                    .all(|b| b"ADGHJKMNPRSTVWXY0248".contains(&b))
            );
            assert_eq!(
                answer.as_str(),
                key.challenge_answer(&first, length).unwrap().as_str()
            );
            assert_ne!(
                key.answer_hash(&first, answer.as_str()),
                key.answer_hash(&second, answer.as_str())
            );
        }
        assert!(key.challenge_answer(&first, 3).is_none());
        assert!(key.challenge_answer(&first, 9).is_none());
    }
}

#[cfg(test)]
mod pass_secret_tests {
    use super::*;
    #[test]
    fn vip_identity_is_separate_and_preserves_exact_owned_spelling() {
        let key = VerificationKey::parse(&"12".repeat(32)).unwrap();
        assert_ne!(
            key.vip_identifier("ABC123XYZ0"),
            key.pass_identifier("ABC123XYZ0")
        );
        assert_ne!(key.vip_identifier("Owner"), key.vip_identifier("owner"));
        assert_ne!(key.vip_identifier("é"), key.vip_identifier("e\u{301}"));
        assert!(key.vip_identifier("名前").is_some());
        for value in ["", "0", "owner!other", &"x".repeat(101)] {
            assert!(key.vip_identifier(value).is_none());
        }
    }

    #[test]
    fn pass_tokens_and_identity_hashes_are_separate_and_canonical() {
        let key = VerificationKey::parse(&"12".repeat(32)).unwrap();
        assert_eq!(
            key.pass_identifier("AbC123xyZ0"),
            key.pass_identifier("ABC123XYZ0")
        );
        assert!(key.pass_identifier("too-short").is_none());
        assert!(key.pass_identifier("ABC12!XYZ0").is_none());
        let token = PassSessionToken::generate().unwrap();
        assert_eq!(
            token.storage_hash(),
            PassSessionToken::parse(&token.encode())
                .unwrap()
                .storage_hash()
        );
        assert!(ReceiptToken::parse(&token.encode()).is_none());
        assert!(PassSessionToken::parse(&ReceiptToken::generate().unwrap().encode()).is_none());
    }
    #[test]
    fn pass_networks_preserve_login_and_posting_prefixes() {
        let key = VerificationKey::parse(&"34".repeat(32)).unwrap();
        let get = |ip: &str| key.pass_networks(ip.parse().unwrap()).unwrap();
        let a = get("192.0.2.1");
        let same = get("192.0.2.2");
        let other = get("192.0.3.1");
        assert_eq!(a.login, same.login);
        assert_eq!(a.posting, same.posting);
        assert_ne!(a.peer, same.peer);
        assert_eq!(a.login, other.login);
        assert_ne!(a.posting, other.posting);
        assert_eq!(a.peer, get("::ffff:192.0.2.1").peer);
        assert_eq!(get("2001:db8::1").posting, get("2001:db8::2").posting);
        assert_ne!(get("2001:db8::1").posting, get("2001:db8:0:1::1").posting);
        assert!(key.pass_networks("::".parse().unwrap()).is_none());
    }
}
