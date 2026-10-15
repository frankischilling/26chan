//! Fully validated source GIF candidates. This module runs only in the guest.
use crate::{animation, still::raster::Raster};
use std::{
    io::{self, BufRead, Cursor},
    num::NonZeroU64,
};

const MAX_PIXELS: usize = 256 * 1024 * 1024;

fn rejected() -> io::Error {
    io::Error::other("source GIF candidate rejected")
}

pub struct Candidate {
    pub dimensions: (u32, u32),
    pub needs_cleanup: bool,
    pub frame_count: usize,
    first: Raster,
}

impl Candidate {
    /// Pre-JPEG first-frame pixels, using the captured GD palette behavior.
    pub fn thumbnail_pixels(&self, output: (u32, u32), background: [u8; 3]) -> io::Result<Vec<u8>> {
        self.first.resize(output.0, output.1, background)
    }
}

/// Validate every frame, even though only the first contributes thumbnail pixels.
/// Keep one decoded indexed frame at a time under an aggregate animation budget.
pub fn validate(input: &[u8]) -> io::Result<Candidate> {
    let scan = animation::scan_source(input)?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        NonZeroU64::new(100_000_001).unwrap(),
    ));
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    options.skip_frame_decoding(true);
    options.allow_unknown_blocks(false);
    let mut decoder = options
        .read_info(Cursor::new(input))
        .map_err(io::Error::other)?;
    let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
    if !(1..=10_000).contains(&width) || !(1..=10_000).contains(&height) {
        return Err(rejected());
    }
    let global = decoder.global_palette().unwrap_or_default().to_vec();
    let mut first = None;
    let mut count = 0usize;
    let mut total = 0usize;
    while let Some(frame) = decoder.read_next_frame().map_err(io::Error::other)? {
        count += 1;
        let pixels = usize::from(frame.width) * usize::from(frame.height);
        total = total.checked_add(pixels).ok_or_else(rejected)?;
        let palette = frame.palette.as_deref().unwrap_or(&global);
        let colors = palette.len() / 3;
        if count > 512
            || total > MAX_PIXELS
            || pixels == 0
            || pixels > 100_000_000
            || !(2..=256).contains(&colors)
            || !colors.is_power_of_two()
            || palette.len() != colors * 3
            || frame.needs_user_input
            || u32::from(frame.left) + u32::from(frame.width) > width
            || u32::from(frame.top) + u32::from(frame.height) > height
            || frame
                .transparent
                .is_some_and(|index| usize::from(index) >= colors)
        {
            return Err(rejected());
        }
        let indices = animation::decode_indices(frame, pixels)?;
        if indices.iter().any(|index| usize::from(*index) >= colors) {
            return Err(rejected());
        }
        if first.is_none() {
            // GD's GIF loader returns the first descriptor-sized image without
            // applying its offset. Its out-of-bounds palette reads use index 0.
            // The source resampler nevertheless receives logical-screen size.
            let rgba = |index: u8| {
                let start = usize::from(index) * 3;
                [
                    palette[start],
                    palette[start + 1],
                    palette[start + 2],
                    if frame.transparent == Some(index) {
                        0
                    } else {
                        255
                    },
                ]
            };
            let mut canvas = Vec::with_capacity(width as usize * height as usize * 4);
            let outside = rgba(0);
            for _ in 0..width as usize * height as usize {
                canvas.extend_from_slice(&outside);
            }
            for y in 0..usize::from(frame.height) {
                for x in 0..usize::from(frame.width) {
                    let target = (y * width as usize + x) * 4;
                    canvas[target..target + 4]
                        .copy_from_slice(&rgba(indices[y * usize::from(frame.width) + x]));
                }
            }
            first = Some(Raster::new(width, height, 4, canvas)?);
        }
    }
    if count == 0 || !decoder.into_inner().fill_buf()?.is_empty() {
        return Err(rejected());
    }
    Ok(Candidate {
        dimensions: (width, height),
        needs_cleanup: scan.cleanup,
        frame_count: count,
        first: first.ok_or_else(rejected)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/gd-gif-thumbnails.json")).unwrap();
        let hex = fixture["cases"][0]["input_hex"].as_str().unwrap();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn comment(length: usize) -> Vec<u8> {
        let mut block = vec![0x21, 0xfe];
        let data = vec![b'x'; length];
        for piece in data.chunks(255) {
            block.push(piece.len() as u8);
            block.extend_from_slice(piece);
        }
        block.push(0);
        block
    }

    #[test]
    fn cleanup_distinguishes_frame_comments_end_comments_and_loop_control() {
        let original = sample();
        assert!(!validate(&original).unwrap().needs_cleanup);
        let mut before = original.clone();
        before.splice(19..19, comment(1));
        assert!(validate(&before).unwrap().needs_cleanup);
        let mut after = original.clone();
        after.splice(after.len() - 1..after.len() - 1, comment(1));
        assert!(!validate(&after).unwrap().needs_cleanup);
        let mut looped = original.clone();
        looped.splice(
            19..19,
            b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00"
                .iter()
                .copied(),
        );
        assert!(!validate(&looped).unwrap().needs_cleanup);
        let mut unknown = original.clone();
        unknown.splice(19..19, b"\x21\xff\x0bPRIVATE0001\x01x\x00".iter().copied());
        assert!(validate(&unknown).unwrap().needs_cleanup);
    }

    #[test]
    fn cleanup_overhead_includes_framing_and_uses_inclusive_51200_threshold() {
        let original = sample();
        // This literal fixture has 19 header/palette bytes, a ten-byte image
        // descriptor, and a trailer; the rest is gifsicle's compressed length.
        let compressed = original.len() - 30;
        for overhead in [51_199, 51_200, 51_201] {
            let added = compressed + overhead - original.len();
            let length = (0..added)
                .find(|n| 3 + n + n.div_ceil(255) == added)
                .unwrap();
            let mut input = original.clone();
            input.splice(input.len() - 1..input.len() - 1, comment(length));
            assert_eq!(input.len() - compressed, overhead);
            assert_eq!(validate(&input).unwrap().needs_cleanup, overhead >= 51_200);
        }
    }

    #[test]
    fn validation_rejects_late_corruption_and_trailing_bytes() {
        let original = sample();
        for length in 0..original.len() {
            assert!(validate(&original[..length]).is_err());
        }
        let mut trailing = original;
        trailing.extend_from_slice(b"extra;");
        assert!(validate(&trailing).is_err());
    }

    #[test]
    fn first_frame_pixels_match_captured_gd_palette_behavior() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/gd-gif-thumbnails.json")).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let hex = case["input_hex"].as_str().unwrap();
            let bytes: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            let candidate = validate(&bytes).unwrap();
            let output = (
                case["output"][0].as_u64().unwrap() as u32,
                case["output"][1].as_u64().unwrap() as u32,
            );
            let expected: Vec<u8> = case["expected"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(candidate.dimensions, (4, 3));
            assert!(!candidate.needs_cleanup);
            assert_eq!(candidate.frame_count, 1);
            assert_eq!(
                candidate.thumbnail_pixels(output, [255, 255, 238]).unwrap(),
                expected,
                "{case}"
            );
        }
    }
}
