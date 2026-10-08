//! Public pseudonyms. Legacy DES trips never authorize deletion or staff access.
use base64::{Engine, engine::general_purpose::STANDARD};

pub struct SecureKey([u8; 32]);

impl SecureKey {
    pub fn parse(value: &str) -> Result<Self, crate::ValidationError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(crate::ValidationError(
                "TRIPCODE_KEY must contain 64 hexadecimal digits.",
            ));
        }
        let mut key = [0; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| crate::ValidationError("Invalid TRIPCODE_KEY."))?;
        }
        if key.iter().all(|byte| *byte == 0) {
            return Err(crate::ValidationError("TRIPCODE_KEY cannot be all zeroes."));
        }
        Ok(Self(key))
    }
}

pub struct Identity {
    pub name: String,
    pub trip: Option<String>,
}

pub const MAX_DISPLAY_NAME_BYTES: usize = 255;

pub fn prepare(raw: &str, key: Option<&SecureKey>) -> Result<Identity, crate::ValidationError> {
    prepare_with_spacing(
        raw,
        key,
        crate::CommentSpacing::for_board("g", false, false),
    )
}

pub fn prepare_with_spacing(
    raw: &str,
    key: Option<&SecureKey>,
    spacing: crate::CommentSpacing<'_>,
) -> Result<Identity, crate::ValidationError> {
    prepare_for_board(raw, key, spacing, false)
}

pub fn prepare_for_board(
    raw: &str,
    key: Option<&SecureKey>,
    spacing: crate::CommentSpacing<'_>,
    strip_tripcode: bool,
) -> Result<Identity, crate::ValidationError> {
    prepare_for_board_with_limits(
        raw,
        key,
        spacing,
        strip_tripcode,
        crate::PostLimits::ordinary(crate::MAX_COMMENT_CHARS),
    )
}

pub fn prepare_for_board_with_limits(
    raw: &str,
    key: Option<&SecureKey>,
    spacing: crate::CommentSpacing<'_>,
    strip_tripcode: bool,
    limits: crate::PostLimits,
) -> Result<Identity, crate::ValidationError> {
    if raw.len() > limits.field_bytes()
        || raw
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\r' | '\n' | '\t'))
    {
        return Err(crate::ValidationError("Invalid name."));
    }

    // Source cleans the whole field before conversion, including private input.
    let mut field: String = raw
        .chars()
        .filter(|&ch| {
            !crate::comment_spacing::zero_width(ch)
                && !crate::comment_unicode::emoticon(ch, spacing.sjis)
        })
        .collect();
    if field.chars().all(|ch| matches!(ch, ' ' | '|' | '\u{3000}')) {
        field.clear();
    }
    field.retain(|ch| !matches!(ch, '\r' | '\n'));

    let display = field.split('#').next().unwrap_or("");
    let normalized: String = display
        .chars()
        .filter(|ch| !matches!(*ch, '\u{2318}' | '\u{ff03}' | '\u{fe5f}'))
        .map(crate::comment_ascii::similar_to_ascii)
        .collect();
    let mut name = crate::comment_spacing::sanitize_spacing(&normalized, spacing);
    name.retain(|ch| ch as u32 <= 0x3134f);
    if name
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12))
    {
        name.clear();
    }
    name.retain(|ch| ch != '!');

    let mut encoded = crate::trip_cp932::encode(&field);
    while encoded.last() == Some(&b'#') {
        encoded.pop();
    }
    let escaped = crate::trip_cp932::escape_compat(&encoded);
    let mut parts = escaped.splitn(3, |&byte| byte == b'#');
    parts.next();
    let normal = parts.next();
    let secure = parts.next();
    let trip = if strip_tripcode {
        None
    } else {
        match (normal, secure) {
            (_, Some(password)) if !password.is_empty() => {
                let key = key.ok_or(crate::ValidationError("Secure tripcodes are unavailable."))?;
                // Keep secure secrets in UTF-8 so unmappable CP932 characters cannot
                // collapse distinct modern credentials into the same key preimage.
                let password = field
                    .trim_end_matches('#')
                    .splitn(3, '#')
                    .nth(2)
                    .ok_or(crate::ValidationError("Invalid name."))?;
                let password = crate::trip_cp932::escape_compat(password.as_bytes());
                let signing = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key.0);
                let signature = ring::hmac::sign(&signing, &password);
                Some(format!("!!{}", &STANDARD.encode(signature.as_ref())[..11]))
            }
            (Some(password), _) if !password.is_empty() => {
                Some(format!("!{}", legacy_trip_bytes(password)))
            }
            _ => None,
        }
    };

    // Source's second bound includes escaped text and its generated trip wrapper.
    let wrapper = if !strip_tripcode && normal.is_some() {
        "</span>".len()
            + trip
                .as_ref()
                .map_or(0, |trip| " <span class=\"postertrip\">".len() + trip.len())
    } else {
        0
    };
    if crate::source_html_entities(&name).len() + wrapper > MAX_DISPLAY_NAME_BYTES {
        return Err(crate::ValidationError("Name or subject is too long."));
    }
    if name.is_empty() && (strip_tripcode || normal.is_none()) {
        name = "Anonymous".into();
    }
    Ok(Identity { name, trip })
}

// Standard DES tables, with the Unix crypt salt applied to the expansion.
// This bounded compatibility hash is intentionally unsuitable for authentication.
const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61,
    53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60,
    52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29,
    21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52,
    31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const EXPANSION: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];
const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
const S: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 1,
        13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];
const ALPHABET: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn permute(value: u64, width: u8, table: &[u8]) -> u64 {
    table.iter().fold(0, |output, bit| {
        (output << 1) | ((value >> (width - bit)) & 1)
    })
}

fn salt_byte(value: u8) -> u8 {
    let value = if (b'.'..=b'z').contains(&value) {
        value
    } else {
        b'.'
    };
    if let Some(index) = b":;<=>?@[\\]^_`".iter().position(|byte| *byte == value) {
        b"ABCDEFGabcdef"[index]
    } else {
        value
    }
}

#[cfg(test)]
fn legacy_trip(password: &str) -> String {
    legacy_trip_bytes(&crate::trip_cp932::escape_compat(
        &crate::trip_cp932::encode(password),
    ))
}

fn legacy_trip_bytes(escaped: &[u8]) -> String {
    let mut key = [0u8; 8];
    for (slot, byte) in key
        .iter_mut()
        .zip(escaped.iter().copied().take_while(|byte| *byte != 0))
    {
        *slot = byte.wrapping_shl(1);
    }
    let raw_salt = match escaped {
        [] | [_] => [b'H', b'.'],
        [_, second] => [*second, b'H'],
        [_, second, third, ..] => [*second, *third],
    };
    let salt = raw_salt.map(salt_byte);
    let bits = |byte| {
        ALPHABET
            .iter()
            .position(|value| *value == byte)
            .unwrap_or(0) as u16
    };
    let salt = bits(salt[0]) | (bits(salt[1]) << 6);
    let mut expansion = EXPANSION;
    for index in 0..12 {
        if salt & (1 << index) != 0 {
            expansion.swap(index, index + 24);
        }
    }
    let key = permute(u64::from_be_bytes(key), 64, &PC1);
    let mut left = key >> 28;
    let mut right = key & 0x0fff_ffff;
    let mut keys = [0u64; 16];
    for (slot, shift) in keys.iter_mut().zip(SHIFTS) {
        left = ((left << shift) | (left >> (28 - shift))) & 0x0fff_ffff;
        right = ((right << shift) | (right >> (28 - shift))) & 0x0fff_ffff;
        *slot = permute((left << 28) | right, 56, &PC2);
    }
    let mut block = 0;
    for _ in 0..25 {
        let expanded = permute(block, 64, &IP);
        let mut left = (expanded >> 32) as u32;
        let mut right = expanded as u32;
        for key in keys {
            let expanded = permute(u64::from(right), 32, &expansion) ^ key;
            let mut substituted = 0u32;
            for (index, table) in S.iter().enumerate() {
                let chunk = ((expanded >> (42 - index * 6)) & 63) as usize;
                let row = ((chunk & 32) >> 4) | (chunk & 1);
                let column = (chunk >> 1) & 15;
                substituted = (substituted << 4) | u32::from(table[row * 16 + column]);
            }
            (left, right) = (right, left ^ permute(u64::from(substituted), 32, &P) as u32);
        }
        block = permute((u64::from(right) << 32) | u64::from(left), 64, &FP);
    }
    // crypt's first hash character is discarded by the ten-character trip format.
    (1..11)
        .map(|index| {
            let value = if index == 10 {
                (block & 15) << 2
            } else {
                (block >> (58 - index * 6)) & 63
            };
            char::from(ALPHABET[value as usize])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_faq_vector_and_identity_separation() {
        let identity = prepare("User#password", None).unwrap();
        assert_eq!(identity.name, "User");
        assert_eq!(identity.trip.as_deref(), Some("!ozOtJW9BFA"));
        assert_eq!(
            prepare("Another#password", None).unwrap().trip,
            identity.trip
        );
        assert_eq!(prepare("#password", None).unwrap().name, "");
        assert_eq!(prepare(" Anonymous ", None).unwrap().trip, None);
        assert_eq!(prepare("#", None).unwrap().trip, None);
        assert_eq!(
            prepare("#a", None).unwrap().trip.as_ref().unwrap().len(),
            11
        );
    }

    #[test]
    fn secure_trips_require_valid_keys_and_do_not_depend_on_display_names() {
        assert!(prepare("User##password", None).is_err());
        assert!(SecureKey::parse(&"0".repeat(64)).is_err());
        assert!(SecureKey::parse(&"g".repeat(64)).is_err());
        let key = SecureKey::parse(&"1".repeat(64)).unwrap();
        let other = SecureKey::parse(&"2".repeat(64)).unwrap();
        let identity = prepare("User##password", Some(&key)).unwrap();
        let trip = identity.trip.as_ref().unwrap();
        assert!(trip.starts_with("!!"));
        assert_eq!(trip.len(), 13);
        // Independently checked with Python's stdlib HMAC-SHA256 and base64.
        assert_eq!(trip, "!!XYWOFgjf7hP");
        assert_eq!(
            prepare("Other##password", Some(&key)).unwrap().trip,
            identity.trip
        );
        assert_ne!(
            prepare("User##password", Some(&other)).unwrap().trip,
            identity.trip
        );
    }

    #[test]
    fn legacy_hash_matches_independent_libxcrypt_vectors() {
        // ASCII vectors generated with Ubuntu libcrypt.so.1, independently of
        // the Rust DES implementation. CP932 vectors are qualified separately.
        for (password, expected) in [
            ("", "jPpg5.obl6"),
            ("a", "ZnBI2EKkq."),
            ("ab", "85qvGhCCNc"),
            ("abc", "GmgU93SCyE"),
            ("password", "ozOtJW9BFA"),
            ("12345678", "WBRXcNtpf."),
            ("123456789", "WBRXcNtpf."),
            ("a:b", "5G6R5bcZ.A"),
            ("a;b", "t1v/9v8/H6"),
            ("a<b", "8Y5BINvmbw"),
            ("a=b", "5nWaJjbeYc"),
            ("a>b", "labYBrneog"),
            ("a?b", "VTjjj4nGeo"),
            ("a@b", "P1UQB/oPwU"),
            ("a[b", "t2.tko2NFA"),
            ("a\\b", "3lE0LvF2.Q"),
            ("a]b", "P5ZUENRgGw"),
            ("a^b", "A9rbR/OfJw"),
            ("a_b", "LyZE5V//ls"),
            ("a`b", ".b8U2g5acY"),
            ("a&b", "vbZwEe8/SY"),
            ("a\"b", ".5HnWOZ4h6"),
            ("a b", "x.YiIbWIis"),
            ("a~b", ".9dz0SmY.c"),
            ("a!b", "kaOTmOZZBw"),
            ("#password", "VhXEJkFkS."),
        ] {
            assert_eq!(legacy_trip(password), expected, "{password:?}");
        }
    }

    #[test]
    fn bounded_names_and_keys_reject_controls_and_never_echo_secret_errors() {
        for raw in ["User#\0secret", "User\u{7f}", "User\u{000b}"] {
            assert_eq!(prepare(raw, None).err().unwrap().0, "Invalid name.");
        }
        assert!(prepare(&"n".repeat(crate::MAX_PUBLIC_FIELD_BYTES), None).is_ok());
        assert!(prepare(&"n".repeat(crate::MAX_PUBLIC_FIELD_BYTES + 1), None).is_err());
        assert!(
            prepare("User##owned-private-secret", None)
                .err()
                .unwrap()
                .0
                .find("owned-private-secret")
                .is_none()
        );
        assert_eq!(prepare("  User #password", None).unwrap().name, "User");
        assert_eq!(
            prepare("User#\nsecret", None).unwrap().trip,
            prepare("User#secret", None).unwrap().trip
        );
        for value in ["", "untrusted-secret", &"a".repeat(63), &"a".repeat(65)] {
            assert!(SecureKey::parse(value).is_err());
        }
    }

    #[test]
    fn public_names_and_parser_branches_match_selected_source_bodies() {
        check_source_names(
            include_str!("../tests/fixtures/public-name.json"),
            crate::PostLimits::ordinary(crate::MAX_COMMENT_CHARS),
            469,
        );
    }

    #[test]
    fn authorized_names_keep_raw_and_finished_bounds_from_selected_source_bodies() {
        check_source_names(
            include_str!("../tests/fixtures/staff-name.json"),
            crate::PostLimits::authorized(10_000).unwrap(),
            553,
        );
    }

    fn check_source_names(source: &str, limits: crate::PostLimits, expected: usize) {
        let fixture: serde_json::Value = serde_json::from_str(source).unwrap();
        let key = SecureKey::parse(&"1".repeat(64)).unwrap();
        let mut count = 0;
        for group in fixture["groups"].as_array().unwrap() {
            let spacing = crate::CommentSpacing::for_board(
                group["board"].as_str().unwrap(),
                group["code"].as_bool().unwrap(),
                group["sjis"].as_bool().unwrap(),
            );
            for case in group["cases"].as_array().unwrap() {
                count += 1;
                let result = prepare_for_board_with_limits(
                    case["input"].as_str().unwrap(),
                    Some(&key),
                    spacing,
                    group["strip"].as_bool().unwrap(),
                    limits,
                );
                if case["outcome"] == "too_long" {
                    assert!(result.is_err(), "{}", case["input"]);
                } else {
                    let identity = result.unwrap();
                    assert_eq!(
                        identity.name,
                        case["name"].as_str().unwrap(),
                        "{}",
                        case["input"]
                    );
                    assert_eq!(
                        crate::source_html_entities(&identity.name),
                        case["name_html"].as_str().unwrap()
                    );
                    assert_eq!(
                        identity.trip.as_deref(),
                        case["modern_trip"].as_str(),
                        "{}",
                        case["input"]
                    );
                }
            }
        }
        assert_eq!(count, expected);
    }

    #[test]
    fn legacy_des_hash_matches_cp932_reference_vectors() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/trip-encoding.json")).unwrap();
        let vectors = fixture["vectors"].as_array().unwrap();
        assert_eq!(vectors.len(), 48);
        for vector in vectors {
            if vector["trip"].is_null() {
                assert_eq!(vector["input"], "");
                assert_eq!(prepare("Name#", None).unwrap().trip, None);
                continue;
            }
            assert_eq!(
                legacy_trip(vector["input"].as_str().unwrap()),
                vector["trip"].as_str().unwrap()
            );
        }
    }

    #[test]
    fn modern_secure_trips_keep_unicode_secrets_distinct_and_source_delimiters() {
        let key = SecureKey::parse(&"1".repeat(64)).unwrap();
        assert_ne!(
            prepare("User##é", Some(&key)).unwrap().trip,
            prepare("User##€", Some(&key)).unwrap().trip
        );
        assert_eq!(
            prepare("User##password", Some(&key)).unwrap().trip,
            prepare("User#ignored#password", Some(&key)).unwrap().trip
        );
        assert_eq!(
            prepare("User#password###", None).unwrap().trip,
            prepare("User#password", None).unwrap().trip
        );
        assert_eq!(prepare("User##", None).unwrap().trip, None);
    }

    #[test]
    fn suppressed_trips_skip_keys_and_wrappers_but_keep_input_and_display_bounds() {
        let spacing = crate::CommentSpacing::for_board("b", false, false);
        for (raw, name) in [
            ("#password", "Anonymous"),
            ("#かみ", "Anonymous"),
            ("Name#password", "Name"),
            ("Name##owned-private-secret", "Name"),
            ("Name#ignored#owned-private-secret", "Name"),
        ] {
            let identity = prepare_for_board(raw, None, spacing, true).unwrap();
            assert_eq!(identity.name, name);
            assert_eq!(identity.trip, None);
        }
        let name = format!("{}#password", "\"".repeat(37));
        assert!(prepare_for_board(&name, None, spacing, false).is_err());
        let identity = prepare_for_board(&name, None, spacing, true).unwrap();
        assert_eq!(identity.name, "\"".repeat(37));
        assert_eq!(identity.trip, None);
        assert!(prepare_for_board(&"n".repeat(101), None, spacing, true).is_err());
        assert!(prepare_for_board(&"\"".repeat(43), None, spacing, true).is_err());
        assert!(prepare_for_board("Name#\0private", None, spacing, true).is_err());
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(96))]
        #[test]
        fn bounded_unicode_identity_parser(raw in proptest::collection::vec(proptest::char::any(), 0..120)) {
            let raw: String = raw.into_iter().collect();
            let key = SecureKey::parse(&"1".repeat(64)).unwrap();
            if let Ok(identity) = prepare(&raw, Some(&key)) {
                proptest::prop_assert!(raw.len() <= crate::MAX_PUBLIC_FIELD_BYTES);
                proptest::prop_assert!(!raw.chars().any(|ch| ch.is_control() && !matches!(ch, '\r' | '\n' | '\t')));
                proptest::prop_assert!(identity.name.len() <= MAX_DISPLAY_NAME_BYTES);
                if let Some(trip) = identity.trip {
                    proptest::prop_assert!(matches!(trip.len(), 11 | 13));
                    proptest::prop_assert!(trip.starts_with('!'));
                    proptest::prop_assert!(trip.is_ascii());
                }
            }
        }
    }
}
