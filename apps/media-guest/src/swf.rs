//! SWF framing and geometry inspection for the disposable guest only.
//! This does not execute ActionScript or make a movie safe for the board origin.
use std::io::{self, Read};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input_bytes: usize,
    pub expanded_bytes: usize,
    pub dimension: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub width: u32,
    pub height: u32,
    pub version: u8,
    pub compressed: bool,
    pub frame_rate_256: u16,
    pub frames: u16,
}

fn rejected() -> io::Error {
    io::Error::other("SWF framing or resource limit rejected")
}

struct Bits<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl Bits<'_> {
    fn unsigned(&mut self, count: usize) -> io::Result<u32> {
        let mut value = 0;
        for _ in 0..count {
            let byte = self.bytes.get(self.offset / 8).ok_or_else(rejected)?;
            value = (value << 1) | u32::from((byte >> (7 - self.offset % 8)) & 1);
            self.offset += 1;
        }
        Ok(value)
    }
    fn signed(&mut self, count: usize) -> io::Result<i64> {
        let value = self.unsigned(count)?;
        Ok(if value & (1 << (count - 1)) != 0 {
            i64::from(value) - (1_i64 << count)
        } else {
            i64::from(value)
        })
    }
}

/// All allocation and decompression are bounded by caller-supplied guest policy.
/// FWS and zlib-compressed CWS are supported; LZMA ZWS is not a source upload type.
/// The original bytes must remain untrusted even after this function succeeds.
pub fn inspect(input: &[u8], limits: Limits) -> io::Result<Metadata> {
    if input.len() < 8 || input.len() > limits.input_bytes {
        return Err(rejected());
    }
    let compressed = match &input[..3] {
        b"FWS" => false,
        b"CWS" if input[3] >= 6 => true,
        _ => return Err(rejected()),
    };
    let declared = u32::from_le_bytes(input[4..8].try_into().expect("four bytes")) as usize;
    if input[3] == 0 || declared < 8 || declared > limits.expanded_bytes {
        return Err(rejected());
    }
    let expanded;
    let body = if compressed {
        let mut decoder = flate2::bufread::ZlibDecoder::new(&input[8..]);
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(declared - 8 + 1)
            .map_err(|_| rejected())?;
        (&mut decoder)
            .take((declared - 8) as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != declared - 8 || !decoder.get_ref().is_empty() {
            return Err(rejected());
        }
        expanded = bytes;
        expanded.as_slice()
    } else {
        if input.len() != declared {
            return Err(rejected());
        }
        &input[8..]
    };
    let mut bits = Bits {
        bytes: body,
        offset: 0,
    };
    let count = bits.unsigned(5)? as usize;
    if count == 0 {
        return Err(rejected());
    }
    let xmin = bits.signed(count)?;
    let xmax = bits.signed(count)?;
    let ymin = bits.signed(count)?;
    let ymax = bits.signed(count)?;
    // Signed coordinates prevent the old PHP unsigned-wrap geometry bug.
    let width = u32::try_from((xmax - xmin) / 20).map_err(|_| rejected())?;
    let height = u32::try_from((ymax - ymin) / 20).map_err(|_| rejected())?;
    if !(1..=limits.dimension).contains(&width) || !(1..=limits.dimension).contains(&height) {
        return Err(rejected());
    }
    let offset = bits.offset.div_ceil(8);
    let timing = body.get(offset..offset + 4).ok_or_else(rejected)?;
    let frame_rate_256 = u16::from_le_bytes([timing[0], timing[1]]);
    let frames = u16::from_le_bytes([timing[2], timing[3]]);
    let mut cursor = offset + 4;
    loop {
        let word = body.get(cursor..cursor + 2).ok_or_else(rejected)?;
        let word = u16::from_le_bytes([word[0], word[1]]);
        cursor += 2;
        let tag = word >> 6;
        let mut length = usize::from(word & 63);
        if length == 63 {
            let value = body.get(cursor..cursor + 4).ok_or_else(rejected)?;
            length = u32::from_le_bytes(value.try_into().expect("four bytes")) as usize;
            cursor += 4;
        }
        cursor = cursor
            .checked_add(length)
            .filter(|&end| end <= body.len())
            .ok_or_else(rejected)?;
        if tag == 0 {
            if length != 0 || cursor != body.len() {
                return Err(rejected());
            }
            break;
        }
    }
    Ok(Metadata {
        width,
        height,
        version: input[3],
        compressed,
        frame_rate_256,
        frames,
    })
}

pub const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
pub const OUTPUT_BYTES: usize = 11 * 1024 * 1024;

/// A fixed-size envelope carries unchanged bytes and bounded metadata. No
/// executable contents are interpreted by the host-side envelope validator.
pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    let length = u64::from_be_bytes(header);
    if !(8..=MAX_INPUT_BYTES as u64).contains(&length) {
        return Err(rejected());
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    let padding = (512 - (8 + bytes.len()) % 512) % 512;
    let mut tail = [0; 511];
    input.read_exact(&mut tail[..padding])?;
    if tail[..padding].iter().any(|&byte| byte != 0) || input.read(&mut [0])? != 0 {
        return Err(rejected());
    }
    let meta = inspect(
        &bytes,
        Limits {
            input_bytes: MAX_INPUT_BYTES,
            expanded_bytes: 32 * 1024 * 1024,
            // Metadata alone does not allocate a canvas. The player must scale its
            // viewport independently and cannot trust movie-supplied dimensions.
            dimension: u32::MAX,
        },
    )?;
    let mut output = vec![0; OUTPUT_BYTES];
    output[..8].copy_from_slice(b"IBSWF008");
    output[8..12].copy_from_slice(&meta.width.to_be_bytes());
    output[12..16].copy_from_slice(&meta.height.to_be_bytes());
    output[16..24].copy_from_slice(&length.to_be_bytes());
    output[24] = meta.version;
    output[25] = u8::from(meta.compressed);
    output[26..28].copy_from_slice(&meta.frame_rate_256.to_be_bytes());
    output[28..30].copy_from_slice(&meta.frames.to_be_bytes());
    output[32..32 + bytes.len()].copy_from_slice(&bytes);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const LIMITS: Limits = Limits {
        input_bytes: 10 * 1024 * 1024,
        expanded_bytes: 64 * 1024 * 1024,
        dimension: 10_000,
    };

    fn movie(rect: [i32; 4], compressed: bool) -> Vec<u8> {
        let count = 31;
        let mut bits = Vec::new();
        for shift in (0..5).rev() {
            bits.push((count >> shift) & 1);
        }
        for value in rect {
            for shift in (0..count).rev() {
                bits.push(((value as u32 >> shift) & 1) as usize);
            }
        }
        let mut body = vec![0; bits.len().div_ceil(8)];
        for (index, bit) in bits.into_iter().enumerate() {
            body[index / 8] |= (bit as u8) << (7 - index % 8);
        }
        body.extend_from_slice(&[0, 24, 1, 0, 0x40, 0, 0, 0]);
        wrap(&body, compressed)
    }
    fn wrap(body: &[u8], compressed: bool) -> Vec<u8> {
        let mut bytes = if compressed {
            b"CWS\x0a".to_vec()
        } else {
            b"FWS\x0a".to_vec()
        };
        bytes.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
        if compressed {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(body).unwrap();
            bytes.extend(encoder.finish().unwrap());
        } else {
            bytes.extend_from_slice(body);
        }
        bytes
    }

    #[test]
    fn accepted_fws_and_cws_have_identical_geometry() {
        for compressed in [false, true] {
            for (rect, size) in [
                ([0, 12_800, 0, 9_600], (640, 480)),
                ([0, 211, 0, 399], (10, 19)),
                ([-200, 12_600, -100, 9_500], (640, 480)),
            ] {
                let meta = inspect(&movie(rect, compressed), LIMITS).unwrap();
                assert_eq!((meta.width, meta.height), size);
                assert_eq!(meta.frame_rate_256, 24 * 256);
                assert_eq!(meta.frames, 1);
                assert_eq!(meta.compressed, compressed);
                assert_eq!(meta.version, 10);
            }
        }
    }

    #[test]
    fn truncation_trailing_bytes_and_length_confusion_are_rejected() {
        for compressed in [false, true] {
            let bytes = movie([0, 12_800, 0, 9_600], compressed);
            for length in 0..bytes.len() {
                assert!(
                    inspect(&bytes[..length], LIMITS).is_err(),
                    "length {length}"
                );
            }
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(inspect(&trailing, LIMITS).is_err());
            for declared in [0, 7, 8, 20, u32::MAX] {
                let mut bad = bytes.clone();
                bad[4..8].copy_from_slice(&declared.to_le_bytes());
                assert!(inspect(&bad, LIMITS).is_err());
            }
        }
    }

    #[test]
    fn geometry_and_configured_budgets_are_enforced() {
        for compressed in [false, true] {
            for rect in [
                [0, 0, 0, 20],
                [0, 20, 0, 0],
                [20, 0, 0, 20],
                [0, 20, 20, 0],
                [0, 200_020, 0, 20],
            ] {
                assert!(inspect(&movie(rect, compressed), LIMITS).is_err());
            }
            let bytes = movie([0, 12_800, 0, 9_600], compressed);
            assert!(
                inspect(
                    &bytes,
                    Limits {
                        input_bytes: bytes.len() - 1,
                        ..LIMITS
                    }
                )
                .is_err()
            );
            assert!(
                inspect(
                    &bytes,
                    Limits {
                        expanded_bytes: 16,
                        ..LIMITS
                    }
                )
                .is_err()
            );
            assert!(
                inspect(
                    &bytes,
                    Limits {
                        dimension: 639,
                        ..LIMITS
                    }
                )
                .is_err()
            );
        }
    }

    #[test]
    fn unsupported_compression_and_invalid_tag_lengths_are_rejected() {
        let mut bytes = movie([0, 12_800, 0, 9_600], false);
        bytes[0] = b'Z';
        assert!(inspect(&bytes, LIMITS).is_err());
        bytes[0] = b'F';
        bytes[3] = 0;
        assert!(inspect(&bytes, LIMITS).is_err());
        let bytes = movie([0, 12_800, 0, 9_600], false);
        let mut body = bytes[8..bytes.len() - 2].to_vec();
        assert!(inspect(&wrap(&body, false), LIMITS).is_err());
        body.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert!(inspect(&wrap(&body, false), LIMITS).is_err());
        let mut body = bytes[8..].to_vec();
        body.push(0);
        assert!(inspect(&wrap(&body, false), LIMITS).is_err());
    }

    #[test]
    fn compressed_expansion_is_limited_before_allocation() {
        let mut bytes = wrap(&vec![0; 1024 * 1024], true);
        bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
        assert!(inspect(&bytes, LIMITS).is_err());
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_small_inputs_do_not_panic(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..2048)) {
            let _ = inspect(&bytes, Limits { input_bytes: 2048, expanded_bytes: 4096, dimension: 10_000 });
        }
    }
    #[test]
    fn owned_headers_match_php_qualification_geometry() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/swf-header-reference.json"))
                .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let hex = case["bytes_hex"].as_str().unwrap();
            let bytes: Vec<u8> = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            let result = inspect(
                &bytes,
                Limits {
                    dimension: u32::MAX,
                    ..LIMITS
                },
            );
            if case["source_geometry_comparable"] == true {
                let meta = result.unwrap();
                assert_eq!(u64::from(meta.width), case["php"]["0"].as_u64().unwrap());
                assert_eq!(u64::from(meta.height), case["php"]["1"].as_u64().unwrap());
            } else if case["name"] == "negative_origin" {
                // PHP interprets signed RECT fields as unsigned and wraps the
                // subtraction. Preserve the actual positive canvas instead.
                let meta = result.unwrap();
                assert_eq!((meta.width, meta.height), (40, 20));
            } else {
                assert!(result.is_err());
            }
        }
    }
    #[tokio::test]
    async fn stopped_disk_envelope_round_trips_without_modifying_movie() {
        for compressed in [false, true] {
            let bytes = movie([0, 12_800, 0, 9_600], compressed);
            let mut input = (bytes.len() as u64).to_be_bytes().to_vec();
            input.extend_from_slice(&bytes);
            input.resize(input.len().div_ceil(512) * 512, 0);
            let disk = decode_disk(input.as_slice()).unwrap();
            assert_eq!(disk.len() as u64, board_media::swf::OUTPUT_DISK_BYTES);
            let validated = board_media::swf::ValidatedSwf::read_disk(disk.as_slice())
                .await
                .unwrap();
            assert_eq!(validated.bytes(), bytes);
            assert_eq!(validated.dimensions(), (640, 480));
            assert_eq!(validated.len(), bytes.len() as u64);
            assert_eq!(validated.sha256().len(), 64);
            assert_eq!(validated.md5().len(), 32);
            for offset in [0, 25, 30, 31, OUTPUT_BYTES - 1] {
                let mut invalid = disk.clone();
                invalid[offset] = 255;
                assert!(
                    board_media::swf::ValidatedSwf::read_disk(invalid.as_slice())
                        .await
                        .is_err()
                );
            }
            input.push(0);
            assert!(decode_disk(input.as_slice()).is_err());
            input.pop();
            *input.last_mut().unwrap() = 1;
            assert!(decode_disk(input.as_slice()).is_err());
        }
    }
}
