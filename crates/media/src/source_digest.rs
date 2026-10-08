//! Source-compatible PNG retained-byte digest, deliberately separate from the
//! encoder-output checksum. This is a bounded framing scanner, NOT image or
//! decoder admission: it does not validate CRCs, chunk semantics, or pixels.
//!
//! Profile PNG v1 mirrors `strip_png_chunks` in 4chan-old/lib/postfilter.php:
//! case-insensitive whitelist, original retained bytes, first IEND termination.
//! In particular the source's `$chink_type` typo drops fcTL/fdAT but rejects
//! acTL. This behavior must never be interpreted as admitting malformed/APNG
//! input. Unlike PHP's seek/EOF paths, truncated dropped chunks and missing
//! IEND are rejected. Compatibility claims apply only to valid admitted PNGs.
//!
//! A future caller must scan immutable, trusted intake bytes with its own
//! admission-derived limits. Never accept a guest/client-asserted digest. No
//! source bytes are returned and no publication/admission policy is established.
use md5::{Digest, Md5};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// The detected format and exact retained-byte transformation version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceDigestProfile {
    PngV1,
}

/// Limits come from the trusted caller's admission policy, not uploaded data.
/// All byte ceilings must be at most the current trusted 8 MiB intake ceiling.
#[derive(Clone, Copy, Debug)]
pub struct PngSourceDigestLimits {
    max_source_bytes: usize,
    max_processed_bytes: usize,
    max_chunk_bytes: usize,
}

impl PngSourceDigestLimits {
    pub fn new(
        max_source_bytes: usize,
        max_processed_bytes: usize,
        max_chunk_bytes: usize,
    ) -> Result<Self, SourceDigestError> {
        if [max_source_bytes, max_processed_bytes, max_chunk_bytes]
            .iter()
            .any(|&limit| limit as u64 > crate::MAX_INPUT_BYTES)
        {
            return Err(SourceDigestError::InvalidLimits);
        }
        Ok(Self {
            max_source_bytes,
            max_processed_bytes,
            max_chunk_bytes,
        })
    }
}

/// Computed only by the scanner; MD5 is legacy interoperability data, never an
/// integrity, authorization, or duplicate-admission decision by itself.
#[derive(Debug, Eq, PartialEq)]
pub struct PngSourceProcessedDigest {
    md5: [u8; 16],
    processed_bytes: usize,
    profile: SourceDigestProfile,
}

impl PngSourceProcessedDigest {
    /// Fixed-length binary legacy digest for bytewise comparison, not collation.
    pub fn md5(&self) -> &[u8; 16] {
        &self.md5
    }

    pub fn processed_bytes(&self) -> usize {
        self.processed_bytes
    }

    pub fn profile(&self) -> SourceDigestProfile {
        self.profile
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SourceDigestError {
    #[error("source digest limits exceed the trusted intake ceiling")]
    InvalidLimits,
    #[error("source exceeds byte limit")]
    SourceTooLarge,
    #[error("retained PNG exceeds byte limit")]
    ProcessedTooLarge,
    #[error("PNG chunk exceeds byte limit")]
    ChunkTooLarge,
    #[error("JPEG source-processed digest is unavailable")]
    JpegUnavailable,
    #[error("GIF source-processed digest is unavailable")]
    GifUnavailable,
    #[error("invalid or unsupported PNG signature")]
    InvalidSignature,
    #[error("truncated PNG chunk length")]
    TruncatedLength,
    #[error("truncated PNG chunk type")]
    TruncatedType,
    #[error("truncated PNG chunk data or CRC")]
    TruncatedChunk,
    #[error("PNG has no complete IEND chunk")]
    MissingIend,
    #[error("APNG acTL chunk is unsupported")]
    ApngUnsupported,
}

/// Hash signature plus complete original whitelisted chunks without allocating
/// an output image, decompressing, reencoding, or trusting an asserted hash.
/// Chunk count/work is bounded by source bytes (each framed chunk uses >=12).
/// JPEG/GIF are explicitly unavailable; there is no raw/encoder-hash fallback.
pub fn png_source_processed_digest(
    source: &[u8],
    limits: PngSourceDigestLimits,
) -> Result<PngSourceProcessedDigest, SourceDigestError> {
    if source.len() > limits.max_source_bytes {
        return Err(SourceDigestError::SourceTooLarge);
    }
    if source.starts_with(b"\xff\xd8") {
        return Err(SourceDigestError::JpegUnavailable);
    }
    if source.starts_with(b"GIF87a") || source.starts_with(b"GIF89a") {
        return Err(SourceDigestError::GifUnavailable);
    }
    if !source.starts_with(SIGNATURE) {
        return Err(SourceDigestError::InvalidSignature);
    }
    let mut processed_bytes = SIGNATURE.len();
    if processed_bytes > limits.max_processed_bytes {
        return Err(SourceDigestError::ProcessedTooLarge);
    }
    let mut digest = Md5::new();
    digest.update(SIGNATURE);
    let mut offset = SIGNATURE.len();
    loop {
        let remaining = &source[offset..];
        if remaining.is_empty() {
            return Err(SourceDigestError::MissingIend);
        }
        let length = remaining
            .get(..4)
            .ok_or(SourceDigestError::TruncatedLength)?;
        let chunk_bytes = u32::from_be_bytes(length.try_into().expect("four length bytes"));
        let chunk_bytes =
            usize::try_from(chunk_bytes).map_err(|_| SourceDigestError::ChunkTooLarge)?;
        if chunk_bytes > limits.max_chunk_bytes {
            return Err(SourceDigestError::ChunkTooLarge);
        }
        let kind = remaining
            .get(4..8)
            .ok_or(SourceDigestError::TruncatedType)?;
        if kind.eq_ignore_ascii_case(b"acTL") {
            return Err(SourceDigestError::ApngUnsupported);
        }
        let framed_bytes = chunk_bytes
            .checked_add(12)
            .ok_or(SourceDigestError::ChunkTooLarge)?;
        // Bounds-check dropped chunks too: PHP's fseek could seek past EOF.
        let chunk = remaining
            .get(..framed_bytes)
            .ok_or(SourceDigestError::TruncatedChunk)?;
        let keep = [
            b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"sBIT", b"pHYs", b"sRGB",
            b"bKGD", b"tIME", b"cHRM", b"iCCP",
        ]
        .iter()
        .any(|allowed| kind.eq_ignore_ascii_case(*allowed));
        if keep {
            processed_bytes = processed_bytes
                .checked_add(framed_bytes)
                .filter(|&size| size <= limits.max_processed_bytes)
                .ok_or(SourceDigestError::ProcessedTooLarge)?;
            digest.update(chunk);
            if kind.eq_ignore_ascii_case(b"IEND") {
                return Ok(PngSourceProcessedDigest {
                    md5: digest.finalize().into(),
                    processed_bytes,
                    profile: SourceDigestProfile::PngV1,
                });
            }
        }
        // framed_bytes <= remaining.len(), so addition cannot overflow.
        offset += framed_bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: &str = "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c48900000010494441547801010500faff00ff0000ff050001fffa5c88d10000000049454e44ae426082";
    const RED_MD5: &str = "b4e7464f29bcc44451c570504d61030b";
    // Independent fixture construction: network-order length + literal type/data
    // + Python zlib.crc32. Expected digests use Python hashlib.md5 on explicit
    // retained-byte concatenations, never this scanner or its whitelist.
    // The combined metadata and case/CRC fixtures test framing semantics only;
    // they deliberately make no claim of decoder admission.
    const METADATA: [&str; 10] = [
        "00000003504c5445ff000019e20937",
        "0000000174524e53ff6de437eb",
        "0000000467414d410000b18f0bfc6105",
        "0000000473424954080808087c086488",
        "00000009704859730000000100000001004f25c4d6",
        "000000017352474200aece1ce9",
        "00000006624b4744000000000000f943bb7f",
        "0000000774494d4507e80102030405b7ea5da6",
        "000000206348524d0000000000000000000000000000000000000000000000000000000000000000a0e6b5a7",
        "0000001269434350700000789c2b28ca4fcbcc4905000bfe02f239d8401c",
    ];

    fn hex(value: &str) -> Vec<u8> {
        assert_eq!(value.len() % 2, 0);
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }

    fn limits() -> PngSourceDigestLimits {
        PngSourceDigestLimits::new(8 * 1024 * 1024, 8 * 1024 * 1024, 8 * 1024 * 1024).unwrap()
    }

    fn scan(source: &[u8]) -> Result<PngSourceProcessedDigest, SourceDigestError> {
        png_source_processed_digest(source, limits())
    }

    fn digest_hex(result: &PngSourceProcessedDigest) -> String {
        result.md5().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn insert(chunks: &[u8]) -> Vec<u8> {
        let red = hex(RED);
        [red[..33].to_vec(), chunks.to_vec(), red[33..].to_vec()].concat()
    }

    #[test]
    fn baseline_has_independent_golden_digest_and_png_v1_profile() {
        let result = scan(&hex(RED)).unwrap();
        let expected: [u8; 16] = hex(RED_MD5).try_into().unwrap();
        assert_eq!(result.md5(), &expected);
        assert_eq!(digest_hex(&result), RED_MD5);
        assert_eq!(result.processed_bytes(), 73);
        assert_eq!(result.profile(), SourceDigestProfile::PngV1);
    }

    #[test]
    fn keeps_every_whitelisted_metadata_chunk_and_original_order() {
        for (chunks, expected) in [
            (METADATA.to_vec(), "7e85c61670b6e8a76df458bef25faa16"),
            (
                METADATA.into_iter().rev().collect(),
                "213886a2711d18929eb013c9ff0c8c1c",
            ),
        ] {
            let bytes = insert(&chunks.into_iter().flat_map(hex).collect::<Vec<_>>());
            let result = scan(&bytes).unwrap();
            assert_eq!(digest_hex(&result), expected);
            assert_eq!(result.processed_bytes(), 278);
        }
    }

    #[test]
    fn drops_text_unknown_chunks_and_source_typo_fctl_fdat() {
        for chunk in [
            "00000003744558746e0076cdcf317b",
            "000000017670416778deb5a177",
            "0000001a6663544c00000000000000000000000000000000000000000000000000000491c706",
            "00000004666441540000000005c8ae61",
        ] {
            let result = scan(&insert(&hex(chunk))).unwrap();
            assert_eq!(digest_hex(&result), RED_MD5);
            assert_eq!(result.processed_bytes(), 73);
        }
    }

    #[test]
    fn rejects_actl_case_insensitively_before_first_iend() {
        for kind in [b"acTL", b"ACTL", b"aCtL"] {
            let mut chunk = hex("000000086163544c0000000100000000b42de9a0");
            chunk[4..8].copy_from_slice(kind);
            assert_eq!(
                scan(&insert(&chunk)),
                Err(SourceDigestError::ApngUnsupported)
            );
        }
    }

    #[test]
    fn preserves_original_case_and_crc_without_claiming_validation() {
        let mut bytes = hex(RED);
        bytes[12..16].copy_from_slice(b"iHdR");
        assert_eq!(
            digest_hex(&scan(&bytes).unwrap()),
            "572d3a4d40a13dea475af8a7b9d7d97d"
        );
        let mut bytes = hex(RED);
        bytes[29..33].copy_from_slice(&[1, 2, 3, 4]);
        assert_eq!(
            digest_hex(&scan(&bytes).unwrap()),
            "3b543dd67f74550c72dfd0edadadb5de"
        );
    }

    #[test]
    fn idat_segmentation_changes_digest_without_changing_compressed_payload() {
        let split = hex(
            "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c48900000008494441547801010500faff001f418bb70000000849444154ff0000ff050001ff01a0b2130000000049454e44ae426082",
        );
        let red = hex(RED);
        assert_eq!([&split[41..49], &split[61..69]].concat(), red[41..57]);
        let result = scan(&split).unwrap();
        assert_eq!(digest_hex(&result), "930fbb043a80853b10c9e02a7ca3a1c5");
        assert_eq!(result.processed_bytes(), 85);
        assert_ne!(digest_hex(&result), RED_MD5);
    }

    #[test]
    fn stops_at_first_iend_dropping_arbitrary_trailing_bytes() {
        let mut bytes = hex(RED);
        bytes.extend(hex("000000086163544c0000000100000000b42de9a0"));
        bytes.extend_from_slice(b"truncated junk");
        assert_eq!(digest_hex(&scan(&bytes).unwrap()), RED_MD5);
    }

    #[test]
    fn rejects_truncations_in_all_fields_and_missing_iend() {
        let red = hex(RED);
        for end in 0..8 {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::InvalidSignature));
        }
        for end in [8, 33, 61] {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::MissingIend));
        }
        for end in 9..12 {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::TruncatedLength));
        }
        for end in 12..16 {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::TruncatedType));
        }
        for end in 16..33 {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::TruncatedChunk));
        }
        for end in 69..73 {
            assert_eq!(scan(&red[..end]), Err(SourceDigestError::TruncatedChunk));
        }
        // Safer than PHP's unchecked fseek over a dropped chunk.
        let mut dropped = red[..33].to_vec();
        dropped.extend(hex("00000003744558746e00"));
        assert_eq!(scan(&dropped), Err(SourceDigestError::TruncatedChunk));
    }

    #[test]
    fn bounds_source_processed_and_all_chunks_including_dropped_chunks() {
        let red = hex(RED);
        for (source, processed, chunk, expected) in [
            (72, 73, 16, SourceDigestError::SourceTooLarge),
            (73, 72, 16, SourceDigestError::ProcessedTooLarge),
            (73, 7, 16, SourceDigestError::ProcessedTooLarge),
            (73, 73, 15, SourceDigestError::ChunkTooLarge),
        ] {
            assert_eq!(
                png_source_processed_digest(
                    &red,
                    PngSourceDigestLimits::new(source, processed, chunk).unwrap()
                ),
                Err(expected)
            );
        }
        assert_eq!(
            digest_hex(
                &png_source_processed_digest(&red, PngSourceDigestLimits::new(73, 73, 16).unwrap())
                    .unwrap()
            ),
            RED_MD5
        );
        let dropped = insert(&hex(
            "0000001a6663544c00000000000000000000000000000000000000000000000000000491c706",
        ));
        assert_eq!(
            png_source_processed_digest(&dropped, PngSourceDigestLimits::new(111, 73, 16).unwrap()),
            Err(SourceDigestError::ChunkTooLarge)
        );
        let mut huge = SIGNATURE.to_vec();
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(scan(&huge), Err(SourceDigestError::ChunkTooLarge));
        for excessive in [
            (usize::MAX, 73, 16),
            (73, usize::MAX, 16),
            (73, 73, usize::MAX),
        ] {
            assert!(matches!(
                PngSourceDigestLimits::new(excessive.0, excessive.1, excessive.2),
                Err(SourceDigestError::InvalidLimits)
            ));
        }
    }

    #[test]
    fn unsupported_formats_are_explicit_and_never_fall_back_to_raw_md5() {
        assert_eq!(
            scan(b"\xff\xd8\xff"),
            Err(SourceDigestError::JpegUnavailable)
        );
        for gif in [b"GIF87a", b"GIF89a"] {
            assert_eq!(scan(gif), Err(SourceDigestError::GifUnavailable));
        }
        assert_eq!(
            scan(b"not an image"),
            Err(SourceDigestError::InvalidSignature)
        );
    }
}
