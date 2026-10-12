//! PHP string integer coercion for source request fields.
//! Bound each raw request field before calling this parser.

pub fn integer(value: &str) -> i64 {
    let value = value.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let bytes = value.as_bytes();
    let mut at = usize::from(
        bytes
            .first()
            .is_some_and(|byte| *byte == b'+' || *byte == b'-'),
    );
    let mut digits = 0;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        digits += 1;
        at += 1;
    }
    let mut float = false;
    if bytes.get(at) == Some(&b'.') {
        float = true;
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            digits += 1;
            at += 1;
        }
    }
    if digits == 0 {
        return 0;
    }
    if bytes
        .get(at)
        .is_some_and(|byte| *byte == b'e' || *byte == b'E')
    {
        let mut next = at + 1;
        if bytes
            .get(next)
            .is_some_and(|byte| *byte == b'+' || *byte == b'-')
        {
            next += 1;
        }
        let start = next;
        while bytes.get(next).is_some_and(u8::is_ascii_digit) {
            next += 1;
        }
        if next > start {
            at = next;
            float = true;
        }
    }
    let prefix = &value[..at];
    if float {
        prefix.parse::<f64>().map_or(
            0,
            |number| {
                if number.is_finite() { number as i64 } else { 0 }
            },
        )
    } else {
        prefix.parse::<i64>().unwrap_or_else(|_| {
            if prefix.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            }
        })
    }
}
