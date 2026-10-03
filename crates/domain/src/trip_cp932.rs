// Source's Windows-Japanese conversion, qualified over every Unicode scalar.
use crate::trip_cp932_data::PAIRS;

pub(crate) fn encode(text: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let point = ch as u32;
        if point < 128 {
            output.push(point as u8);
        } else if let Ok(index) = PAIRS.binary_search_by_key(&point, |&(point, _)| point) {
            let encoded = PAIRS[index].1;
            if encoded > 255 {
                output.push((encoded >> 8) as u8);
            }
            output.push(encoded as u8);
        } else {
            output.push(b'?');
        }
    }
    output
}

pub(crate) fn escape_compat(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    for byte in bytes {
        match byte {
            b'&' => output.extend_from_slice(b"&amp;"),
            b'"' => output.extend_from_slice(b"&quot;"),
            b'<' => output.extend_from_slice(b"&lt;"),
            b'>' => output.extend_from_slice(b"&gt;"),
            _ => output.push(*byte),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn conversion_matches_mbstring_for_every_unicode_scalar() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/trip-encoding.json")).unwrap();
        assert_eq!(fixture["scalars"], 1_112_064);
        assert_eq!(fixture["pairs"].as_array().unwrap().len(), 9_278);
        assert!(fixture["invalid"].as_array().unwrap().is_empty());
        let pairs: std::collections::BTreeMap<_, _> = fixture["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    pair[0].as_u64().unwrap() as u32,
                    decode(pair[1].as_str().unwrap()),
                )
            })
            .collect();
        let mut count = 0;
        for point in 0..=0x10ffff {
            let Some(ch) = char::from_u32(point) else {
                continue;
            };
            count += 1;
            let expected = if point < 128 {
                vec![point as u8]
            } else {
                pairs.get(&point).cloned().unwrap_or_else(|| vec![b'?'])
            };
            assert_eq!(encode(ch.encode_utf8(&mut [0; 4])), expected, "{point:x}");
        }
        assert_eq!(count, 1_112_064);
        assert!(PAIRS.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn multi_scalar_conversion_and_html_bytes_match_independent_vectors() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/trip-encoding.json")).unwrap();
        for vector in fixture["vectors"].as_array().unwrap() {
            let encoded = encode(vector["input"].as_str().unwrap());
            assert_eq!(encoded, decode(vector["cp932_hex"].as_str().unwrap()));
            assert_eq!(
                escape_compat(&encoded),
                decode(vector["escaped_hex"].as_str().unwrap())
            );
        }
    }
}
