//! Encrypted, bounded outbox payloads. Mail-sink keys are separate from challenge
//! and identity signing keys. Plaintext values have no Debug/Serialize derives.
use crate::{
    challenge_secrets::{TokenKind, VerificationToken, decode},
    email_verification::PreparedEmail,
};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};

pub const MAX_ENCRYPTED_MAIL_BYTES: usize = 352;
const PREFIX_BYTES: usize = 13;
const TOKEN_BYTES: usize = 67;

pub struct MailKey(aead::LessSafeKey);
pub struct MailPayload {
    recipient: String,
    token: String,
}

impl MailPayload {
    pub fn new(recipient: &str, token: &str) -> Option<Self> {
        let recipient = PreparedEmail::prepare(recipient)
            .ok()?
            .recipient()
            .to_owned();
        let token = VerificationToken::parse(TokenKind::Email, token)?.encode();
        Some(Self { recipient, token })
    }
    pub fn recipient(&self) -> &str {
        &self.recipient
    }
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl MailKey {
    pub fn parse(value: &str) -> Option<Self> {
        let key = aead::UnboundKey::new(&aead::AES_256_GCM, &decode(value)?).ok()?;
        Some(Self(aead::LessSafeKey::new(key)))
    }

    pub fn seal_pass(
        &self,
        context: &[u8; 32],
        payload: &crate::pass_mail::Payload,
    ) -> Option<Vec<u8>> {
        let mut nonce = [0; 12];
        SystemRandom::new().fill(&mut nonce).ok()?;
        let mut encrypted = payload.encode()?;
        let mut aad = b"26chan:pass-mail:v1\0".to_vec();
        aad.extend_from_slice(context);
        self.0
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut encrypted,
            )
            .ok()?;
        let mut envelope = vec![2];
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&encrypted);
        (envelope.len() <= crate::pass_mail::MAX_ENCRYPTED_BYTES).then_some(envelope)
    }
    pub fn open_pass(
        &self,
        context: &[u8; 32],
        envelope: &[u8],
    ) -> Option<crate::pass_mail::Payload> {
        if !(29..=crate::pass_mail::MAX_ENCRYPTED_BYTES).contains(&envelope.len())
            || envelope[0] != 2
        {
            return None;
        }
        let nonce: [u8; 12] = envelope[1..13].try_into().ok()?;
        let mut encrypted = envelope[13..].to_vec();
        let mut aad = b"26chan:pass-mail:v1\0".to_vec();
        aad.extend_from_slice(context);
        let plain = self
            .0
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut encrypted,
            )
            .ok()?;
        crate::pass_mail::Payload::decode(plain)
    }

    pub fn seal(&self, context: &[u8; 32], payload: &MailPayload) -> Option<Vec<u8>> {
        let mut nonce = [0; 12];
        SystemRandom::new().fill(&mut nonce).ok()?;
        let mut encrypted = Vec::with_capacity(MAX_ENCRYPTED_MAIL_BYTES - PREFIX_BYTES);
        encrypted.extend_from_slice(&(payload.recipient.len() as u16).to_be_bytes());
        encrypted.extend_from_slice(payload.recipient.as_bytes());
        encrypted.extend_from_slice(payload.token.as_bytes());
        let mut aad = b"26chan:verification-mail:v1\0".to_vec();
        aad.extend_from_slice(context);
        self.0
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut encrypted,
            )
            .ok()?;
        let mut envelope = Vec::with_capacity(PREFIX_BYTES + encrypted.len());
        envelope.push(1);
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&encrypted);
        (envelope.len() <= MAX_ENCRYPTED_MAIL_BYTES).then_some(envelope)
    }

    pub fn open(&self, context: &[u8; 32], envelope: &[u8]) -> Option<MailPayload> {
        if !(PREFIX_BYTES + 2 + TOKEN_BYTES + 16..=MAX_ENCRYPTED_MAIL_BYTES)
            .contains(&envelope.len())
            || envelope[0] != 1
        {
            return None;
        }
        let nonce: [u8; 12] = envelope[1..PREFIX_BYTES].try_into().ok()?;
        let mut encrypted = envelope[PREFIX_BYTES..].to_vec();
        let mut aad = b"26chan:verification-mail:v1\0".to_vec();
        aad.extend_from_slice(context);
        let plain = self
            .0
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad),
                &mut encrypted,
            )
            .ok()?;
        if plain.len() < 2 {
            return None;
        }
        let recipient_len = usize::from(u16::from_be_bytes(plain[..2].try_into().ok()?));
        if recipient_len > 254 || plain.len() != 2 + recipient_len + TOKEN_BYTES {
            return None;
        }
        MailPayload::new(
            std::str::from_utf8(&plain[2..2 + recipient_len]).ok()?,
            std::str::from_utf8(&plain[2 + recipient_len..]).ok()?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pass_mail_envelopes_reject_substitution_and_verification_tokens() {
        use crate::pass_mail::{Contact, Notice, Payload, Recipient};
        let key = MailKey::parse(&"11".repeat(32)).unwrap();
        let wrong = MailKey::parse(&"22".repeat(32)).unwrap();
        let contact = Contact::new(
            Recipient::parse("Owned+tag@example.test").unwrap(),
            "AB12345678",
            "owned-renewal",
        )
        .unwrap();
        let payload = Payload::notice(contact, Notice::Reminder, 1_700_000_000).unwrap();
        let context = [7; 32];
        let envelope = key.seal_pass(&context, &payload).unwrap();
        let Payload::Notice {
            contact,
            kind,
            expires,
        } = key.open_pass(&context, &envelope).unwrap()
        else {
            panic!("wrong payload")
        };
        assert_eq!(contact.recipient(), "Owned+tag@example.test");
        assert_eq!(kind, Notice::Reminder);
        assert_eq!(expires, 1_700_000_000);
        assert!(key.open(&context, &envelope).is_none());
        assert!(wrong.open_pass(&context, &envelope).is_none());
        assert!(key.open_pass(&[8; 32], &envelope).is_none());
        assert_ne!(envelope, key.seal_pass(&context, &payload).unwrap());
        for length in 0..envelope.len() {
            assert!(key.open_pass(&context, &envelope[..length]).is_none());
        }
        for index in 0..envelope.len() {
            let mut changed = envelope.clone();
            changed[index] ^= 1;
            assert!(key.open_pass(&context, &changed).is_none());
        }
        let email = VerificationToken::generate(TokenKind::Email).unwrap();
        let old = key
            .seal(
                &context,
                &MailPayload::new("owned@gmail.com", &email.encode()).unwrap(),
            )
            .unwrap();
        assert!(key.open_pass(&context, &old).is_none());
        let invalid = Payload::Notice {
            contact,
            kind,
            expires: i64::MAX,
        };
        assert!(key.seal_pass(&context, &invalid).is_none());
    }

    #[test]
    fn local_delivery_roundtrip_keeps_the_full_recipient_and_token() {
        let key = MailKey::parse(&"11".repeat(32)).unwrap();
        let token = VerificationToken::generate(TokenKind::Email).unwrap();
        let payload = MailPayload::new("Owned.Source@gmail.com", &token.encode()).unwrap();
        let context = [9; 32];
        let envelope = key.seal(&context, &payload).unwrap();
        assert!(envelope.len() <= MAX_ENCRYPTED_MAIL_BYTES);
        let restored = key.open(&context, &envelope).unwrap();
        assert_eq!(restored.recipient(), "owned.source@gmail.com");
        assert_eq!(restored.token(), token.encode());
        assert_ne!(
            envelope,
            key.seal(&context, &payload).unwrap(),
            "each delivery envelope needs a fresh nonce"
        );
    }
    #[test]
    fn context_key_version_and_ciphertext_changes_fail_closed() {
        let key = MailKey::parse(&"11".repeat(32)).unwrap();
        let wrong = MailKey::parse(&"22".repeat(32)).unwrap();
        let token = VerificationToken::generate(TokenKind::Email).unwrap();
        let payload = MailPayload::new("owned@gmail.com", &token.encode()).unwrap();
        let envelope = key.seal(&[1; 32], &payload).unwrap();
        assert!(wrong.open(&[1; 32], &envelope).is_none());
        assert!(key.open(&[2; 32], &envelope).is_none());
        for index in [0, 1, PREFIX_BYTES, envelope.len() - 1] {
            let mut changed = envelope.clone();
            changed[index] ^= 1;
            assert!(key.open(&[1; 32], &changed).is_none());
        }
        for len in 0..envelope.len() {
            assert!(key.open(&[1; 32], &envelope[..len]).is_none());
        }
        assert!(
            key.open(&[1; 32], &vec![0; MAX_ENCRYPTED_MAIL_BYTES + 1])
                .is_none()
        );
    }
    #[test]
    fn payload_cannot_carry_a_challenge_or_mail_header_syntax() {
        let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
        assert!(MailPayload::new("owned@gmail.com", &challenge.encode()).is_none());
        let token = VerificationToken::generate(TokenKind::Email).unwrap();
        assert!(MailPayload::new("owned\r\nheader@gmail.com", &token.encode()).is_none());
    }
}
