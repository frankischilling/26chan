//! Bounded Pass mail data. No plaintext address or identifier implements Debug.
use crate::pass_policy::Status;

pub const REMINDER_SECONDS: i64 = 7 * 86_400;
pub const MAX_ENCRYPTED_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    Reminder,
    Expired,
}

/// `tasks/pass_mailer.php`: only active/new accounts, seven-day lookahead,
/// expired-mail suppression before reminder suppression, expiry inclusive.
pub fn due_notice(
    status: Status,
    expires: i64,
    now: i64,
    reminder_sent: bool,
    expired_sent: bool,
) -> Option<Notice> {
    if !matches!(status, Status::Active | Status::New)
        || expired_sent
        || expires > now.saturating_add(REMINDER_SECONDS)
    {
        None
    } else if expires <= now {
        Some(Notice::Expired)
    } else if !reminder_sent {
        Some(Notice::Reminder)
    } else {
        None
    }
}

pub struct Recipient {
    address: String,
}
impl Recipient {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.is_empty() || raw.len() > 254 || !raw.is_ascii() {
            return None;
        }
        let (local, domain) = raw.split_once('@')?;
        if local.is_empty()
            || local.len() > 64
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
            || !domain.contains('.')
            || domain.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return None;
        }
        Some(Self {
            address: format!("{local}@{}", domain.to_ascii_lowercase()),
        })
    }
    pub fn address(&self) -> &str {
        &self.address
    }
    /// Native recovery equality; no provider-specific dot or subaddress folding.
    pub fn identity(&self) -> String {
        self.address.to_ascii_lowercase()
    }
    pub fn owner(primary: &str, gift: Option<&str>) -> Option<Self> {
        Self::parse(gift.filter(|value| !value.is_empty()).unwrap_or(primary))
    }
}

pub struct Contact {
    recipient: Recipient,
    identifier: String,
    renewal: String,
}
impl Contact {
    pub fn new(recipient: Recipient, identifier: &str, renewal: &str) -> Option<Self> {
        if identifier.len() != 10
            || !identifier.bytes().all(|b| b.is_ascii_alphanumeric())
            || renewal.is_empty()
            || renewal.len() > 128
            || !renewal
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return None;
        }
        Some(Self {
            recipient,
            identifier: identifier.to_ascii_uppercase(),
            renewal: renewal.into(),
        })
    }
    pub fn recipient(&self) -> &str {
        self.recipient.address()
    }
    pub fn recipient_identity(&self) -> String {
        self.recipient.identity()
    }
    pub fn identifier(&self) -> &str {
        &self.identifier
    }
    pub fn renewal(&self) -> &str {
        &self.renewal
    }
}

pub enum Payload {
    Contact(Contact),
    Setup {
        contact: Contact,
        token: crate::challenge_secrets::PassRecoveryToken,
    },
    Recovery {
        recipient: Recipient,
        token: crate::challenge_secrets::PassRecoveryToken,
    },
    Notice {
        contact: Contact,
        kind: Notice,
        expires: i64,
    },
}
impl Payload {
    pub fn notice(contact: Contact, kind: Notice, expires: i64) -> Option<Self> {
        (0..=253_402_300_799)
            .contains(&expires)
            .then_some(Self::Notice {
                contact,
                kind,
                expires,
            })
    }
    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        if let Self::Recovery { recipient, token } = self {
            let mut out = vec![4];
            out.extend_from_slice(&(recipient.address().len() as u16).to_be_bytes());
            out.extend_from_slice(recipient.address().as_bytes());
            out.extend_from_slice(token.encode().as_bytes());
            return Some(out);
        }
        let (tag, contact, expires) = match self {
            Self::Recovery { .. } => unreachable!("handled separately"),
            Self::Contact(contact) => (1, contact, 0i64),
            Self::Setup { contact, .. } => (5, contact, 0i64),
            Self::Notice {
                contact,
                kind: Notice::Reminder,
                expires,
            } => (2, contact, *expires),
            Self::Notice {
                contact,
                kind: Notice::Expired,
                expires,
            } => (3, contact, *expires),
        };
        if tag != 1 && !(0..=253_402_300_799).contains(&expires) {
            return None;
        }
        let mut out = vec![tag];
        out.extend_from_slice(&expires.to_be_bytes());
        out.extend_from_slice(&(contact.recipient().len() as u16).to_be_bytes());
        out.extend_from_slice(contact.recipient().as_bytes());
        out.extend_from_slice(contact.identifier.as_bytes());
        out.push(contact.renewal.len() as u8);
        out.extend_from_slice(contact.renewal.as_bytes());
        if let Self::Setup { token, .. } = self {
            out.extend_from_slice(token.encode().as_bytes());
        }
        Some(out)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 23 || bytes.len() > MAX_ENCRYPTED_BYTES - 29 {
            return None;
        }
        if bytes[0] == 4 {
            let length = usize::from(u16::from_be_bytes(bytes.get(1..3)?.try_into().ok()?));
            if length > 254 || bytes.len() != 3 + length + 68 {
                return None;
            }
            let recipient = Recipient::parse(std::str::from_utf8(bytes.get(3..3 + length)?).ok()?)?;
            let token = crate::challenge_secrets::PassRecoveryToken::parse(
                std::str::from_utf8(bytes.get(3 + length..)?).ok()?,
            )?;
            return Some(Self::Recovery { recipient, token });
        }
        let expires = i64::from_be_bytes(bytes.get(1..9)?.try_into().ok()?);
        let recipient_len = usize::from(u16::from_be_bytes(bytes.get(9..11)?.try_into().ok()?));
        if recipient_len > 254 {
            return None;
        }
        let recipient =
            Recipient::parse(std::str::from_utf8(bytes.get(11..11 + recipient_len)?).ok()?)?;
        let identifier =
            std::str::from_utf8(bytes.get(11 + recipient_len..21 + recipient_len)?).ok()?;
        let renewal_len = usize::from(*bytes.get(21 + recipient_len)?);
        let contact_end = 22 + recipient_len + renewal_len;
        let expected = contact_end + if bytes[0] == 5 { 68 } else { 0 };
        if bytes.len() != expected {
            return None;
        }
        let renewal = std::str::from_utf8(bytes.get(22 + recipient_len..contact_end)?).ok()?;
        let contact = Contact::new(recipient, identifier, renewal)?;
        if bytes[0] == 5 {
            if expires != 0 {
                return None;
            }
            let token = crate::challenge_secrets::PassRecoveryToken::parse(
                std::str::from_utf8(bytes.get(contact_end..)?).ok()?,
            )?;
            return Some(Self::Setup { contact, token });
        }
        match bytes[0] {
            1 if expires == 0 => Some(Self::Contact(contact)),
            2 => Self::notice(contact, Notice::Reminder, expires),
            3 => Self::notice(contact, Notice::Expired, expires),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_setup_mail_is_typed_bounded_and_contains_no_initial_pin() {
        use crate::{challenge_secrets::PassRecoveryToken, verification_mail::MailKey};
        let address = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        assert_eq!(address.len(), 254);
        let contact = Contact::new(
            Recipient::parse(&address).unwrap(),
            "ABCDEFGHIJ",
            &"r".repeat(128),
        )
        .unwrap();
        let token = PassRecoveryToken::generate().unwrap();
        let context = token.storage_hash();
        let payload = Payload::Setup { contact, token };
        let key = MailKey::parse(&"33".repeat(32)).unwrap();
        let encrypted = key.seal_pass(&context, &payload).unwrap();
        assert_eq!(encrypted.len(), 501);
        let Some(Payload::Setup { contact, token }) = key.open_pass(&context, &encrypted) else {
            panic!("setup payload expected")
        };
        assert_eq!(contact.identifier(), "ABCDEFGHIJ");
        assert_eq!(token.storage_hash(), context);
        let bytes = payload.encode().unwrap();
        for len in 0..bytes.len() {
            assert!(Payload::decode(&bytes[..len]).is_none());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(Payload::decode(&extra).is_none());
    }

    #[test]
    fn recovery_material_is_distinct_and_roundtrips_without_session_authority() {
        use crate::challenge_secrets::{PassRecoveryToken, PassSessionToken, ReceiptToken};
        let token = PassRecoveryToken::generate().unwrap();
        let encoded = token.encode();
        let hash = token.storage_hash();
        assert!(PassSessionToken::parse(&encoded).is_none());
        assert!(ReceiptToken::parse(&encoded).is_none());
        let payload = Payload::Recovery {
            recipient: Recipient::parse("owner@example.org").unwrap(),
            token,
        };
        let key = crate::verification_mail::MailKey::parse(&"44".repeat(32)).unwrap();
        let sealed = key.seal_pass(&hash, &payload).unwrap();
        assert!(matches!(
            key.open_pass(&hash, &sealed),
            Some(Payload::Recovery { .. })
        ));
        assert!(key.open(&hash, &sealed).is_none());
        assert!(key.open_pass(&[0; 32], &sealed).is_none());
        let bytes = payload.encode().unwrap();
        let Payload::Recovery { recipient, token } = Payload::decode(&bytes).unwrap() else {
            panic!("recovery expected")
        };
        assert_eq!(recipient.address(), "owner@example.org");
        assert_eq!(token.storage_hash(), hash);
        for length in 0..bytes.len() {
            assert!(Payload::decode(&bytes[..length]).is_none());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(Payload::decode(&extra).is_none());
    }

    #[test]
    fn source_notice_boundaries_and_sent_flags() {
        let now = 1_700_000_000;
        for status in 0..=7 {
            let status = Status::try_from(status).unwrap();
            for reminder in [false, true] {
                for expired in [false, true] {
                    for offset in [-1, 0, 1, REMINDER_SECONDS, REMINDER_SECONDS + 1] {
                        let expected = if !matches!(status, Status::Active | Status::New)
                            || expired
                            || offset > REMINDER_SECONDS
                        {
                            None
                        } else if offset <= 0 {
                            Some(Notice::Expired)
                        } else if reminder {
                            None
                        } else {
                            Some(Notice::Reminder)
                        };
                        assert_eq!(
                            due_notice(status, now + offset, now, reminder, expired),
                            expected
                        );
                    }
                }
            }
        }
        assert_eq!(
            due_notice(Status::Active, i64::MAX, i64::MAX, false, false),
            Some(Notice::Expired)
        );
    }
    #[test]
    fn recipient_accepts_gifts_and_subaddresses_without_header_syntax() {
        assert_eq!(
            Recipient::owner("owner@example.test", Some("Gift+Tag@EXAMPLE.TEST"))
                .unwrap()
                .address(),
            "Gift+Tag@example.test"
        );
        assert_eq!(
            Recipient::owner("owner@example.test", Some(""))
                .unwrap()
                .address(),
            "owner@example.test"
        );
        assert!(Recipient::owner("owner@example.test", Some(" ")).is_none());
        for address in [
            "",
            "a@b",
            "a\r\nBcc:x@example.test",
            "a b@example.test",
            "a@@example.test",
            ".a@example.test",
            "a..b@example.test",
            "a@-example.test",
            "a@example..test",
            "a@example.test.",
            "é@example.test",
        ] {
            assert!(Recipient::parse(address).is_none(), "{address:?}");
        }
    }
    #[test]
    fn payload_roundtrip_is_bounded_and_typed() {
        for kind in [Notice::Reminder, Notice::Expired] {
            let contact = Contact::new(
                Recipient::parse("Owned+tag@example.test").unwrap(),
                "aB12345678",
                "owned-renewal_1",
            )
            .unwrap();
            let payload = Payload::notice(contact, kind, 1_700_000_000).unwrap();
            let encoded = payload.encode().unwrap();
            let Payload::Notice {
                contact,
                kind: actual,
                expires,
            } = Payload::decode(&encoded).unwrap()
            else {
                panic!("wrong kind")
            };
            assert_eq!(actual, kind);
            assert_eq!(contact.identifier(), "AB12345678");
            assert_eq!(contact.recipient(), "Owned+tag@example.test");
            assert_eq!(expires, 1_700_000_000);
            for len in 0..encoded.len() {
                assert!(Payload::decode(&encoded[..len]).is_none());
            }
            let mut extra = encoded.clone();
            extra.push(0);
            assert!(Payload::decode(&extra).is_none());
            let mut wrong = encoded.clone();
            wrong[0] = 9;
            assert!(Payload::decode(&wrong).is_none());
        }
    }
}
