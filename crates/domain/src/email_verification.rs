//! Private email preparation for public verification. This is not posting options.
//! Values deliberately have no Debug or serialization implementation.

pub const ALLOWED_DOMAINS: &[&str] = &[
    "gmail.com",
    "hotmail.com",
    "yahoo.com",
    "proton.me",
    "protonmail.com",
    "outlook.com",
    "live.com",
    "icloud.com",
    "yandex.com",
    "tutanota.com",
    "tutamail.com",
    "tuta.io",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailError {
    Invalid,
    Subaddress,
    Provider,
}

impl EmailError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => "Invalid email address.",
            Self::Subaddress => "Email subaddressing is not allowed",
            Self::Provider => "This email provider is not allowed.",
        }
    }
}

pub struct PreparedEmail {
    recipient: String,
    identity: String,
}

impl PreparedEmail {
    pub fn prepare(raw: &str) -> Result<Self, EmailError> {
        // Bound allocation before cleanup. PHP trim's ASCII set is intentional.
        if raw.len() > 320 {
            return Err(EmailError::Invalid);
        }
        let email = raw.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{000b}']);
        if email.is_empty() || email.len() > 254 || !email.is_ascii() {
            return Err(EmailError::Invalid);
        }
        let email = email.to_ascii_lowercase();
        let (local, domain) = email.split_once('@').ok_or(EmailError::Invalid)?;
        if local.is_empty()
            || domain.contains('@')
            || !domain.contains('.')
            || !domain.rsplit('.').next().is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_lowercase())
            })
        {
            return Err(EmailError::Invalid);
        }
        if email.contains('+') {
            return Err(EmailError::Subaddress);
        }
        if !ALLOWED_DOMAINS.contains(&domain) {
            return Err(EmailError::Provider);
        }
        // The old regex accepted control/whitespace-containing recipients.
        // Restrict delivery to a bounded ASCII dot-atom; never build mail headers
        // from an unchecked address or accept quoted/display-name syntax.
        if local.len() > 64
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !local
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".!#$%&'*-/=?^_`{|}~".contains(&byte))
        {
            return Err(EmailError::Invalid);
        }
        let identity = if domain == "gmail.com" {
            format!("{}@gmail.com", local.replace('.', ""))
        } else {
            email.clone()
        };
        Ok(Self {
            recipient: email,
            identity,
        })
    }

    /// Use only for the private, expiring delivery queue.
    pub fn recipient(&self) -> &str {
        &self.recipient
    }

    /// Use only as input to the operator-keyed private identity hash.
    pub fn identity(&self) -> &str {
        &self.identity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_domain_and_gmail_rules_preserve_delivery_address() {
        for domain in ALLOWED_DOMAINS {
            let raw = format!(" Owned.Source.200@{} ", domain.to_ascii_uppercase());
            let email = PreparedEmail::prepare(&raw).unwrap();
            assert_eq!(email.recipient(), format!("owned.source.200@{domain}"));
            let local = if *domain == "gmail.com" {
                "ownedsource200"
            } else {
                "owned.source.200"
            };
            assert_eq!(email.identity(), format!("{local}@{domain}"));
        }
    }

    #[test]
    fn source_visible_errors_have_the_original_order() {
        for (input, expected) in [
            ("", EmailError::Invalid),
            ("owned@", EmailError::Invalid),
            ("owned@example.invalid", EmailError::Provider),
            ("owned+200@gmail.com", EmailError::Subaddress),
            ("owned@@gmail.com", EmailError::Invalid),
            ("owned@gmail.com.evil", EmailError::Provider),
            ("owned+200@example.invalid", EmailError::Subaddress),
        ] {
            assert_eq!(PreparedEmail::prepare(input).err(), Some(expected));
        }
    }

    #[test]
    fn delivery_bounds_reject_non_atomic_recipients() {
        for input in [
            "a b@gmail.com",
            "a\nb@gmail.com",
            "a\0b@gmail.com",
            "a..b@gmail.com",
            ".ab@gmail.com",
            "ab.@gmail.com",
            "\"ab\"@gmail.com",
            "é@gmail.com",
            "a\\b@gmail.com",
        ] {
            assert_eq!(
                PreparedEmail::prepare(input).err(),
                Some(EmailError::Invalid)
            );
        }
        assert!(PreparedEmail::prepare(&format!("{}@gmail.com", "a".repeat(64))).is_ok());
        assert_eq!(
            PreparedEmail::prepare(&format!("{}@gmail.com", "a".repeat(65))).err(),
            Some(EmailError::Invalid)
        );
        assert_eq!(
            PreparedEmail::prepare(&" ".repeat(321)).err(),
            Some(EmailError::Invalid)
        );
        assert_eq!(
            PreparedEmail::prepare("\tA.B@gmail.com\r\n")
                .unwrap()
                .identity(),
            "ab@gmail.com"
        );
    }
}
