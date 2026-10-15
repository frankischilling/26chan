//! Fully validated source GIF candidates. This module runs only in the guest.
use crate::{animation, still::raster::Raster};
use std::{
    io::{self, BufRead, Cursor},
    num::NonZeroU64,
};

const MAX_PIXELS: usize = 256 * 1024 * 1024;
const MAX_INPUT: usize = 8 * 1024 * 1024;
pub const OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[cfg(target_os = "linux")]
mod cleanup;

#[derive(Clone, Copy)]
pub struct Policy {
    pub thread: bool,
    pub blue_background: bool,
    pub thumbnail: (u16, u16),
}

impl Policy {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() != 16
            || &bytes[..8] != b"IBGIF007"
            || bytes[8] & !12 != 0
            || bytes[9] != 3
            || bytes[14..] != [0; 2]
        {
            return Err(rejected());
        }
        let thumbnail = (
            u16::from_be_bytes([bytes[10], bytes[11]]),
            u16::from_be_bytes([bytes[12], bytes[13]]),
        );
        if !(1..=250).contains(&thumbnail.0) || !(1..=250).contains(&thumbnail.1) {
            return Err(rejected());
        }
        Ok(Self {
            thread: bytes[8] & 4 != 0,
            blue_background: bytes[8] & 8 != 0,
            thumbnail,
        })
    }

    pub fn header(self) -> io::Result<[u8; 16]> {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(b"IBGIF007");
        bytes[8] = (u8::from(self.thread) << 2) | (u8::from(self.blue_background) << 3);
        bytes[9] = 3;
        bytes[10..12].copy_from_slice(&self.thumbnail.0.to_be_bytes());
        bytes[12..14].copy_from_slice(&self.thumbnail.1.to_be_bytes());
        Self::parse(&bytes)?;
        Ok(bytes)
    }
}

pub fn process(input: &[u8], policy: Policy) -> io::Result<Vec<u8>> {
    std::panic::catch_unwind(|| process_inner(input, policy)).unwrap_or_else(|_| Err(rejected()))
}

fn strip(input: &[u8]) -> io::Result<Vec<u8>> {
    #[cfg(target_os = "linux")]
    {
        cleanup::strip(input)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = input;
        Err(rejected())
    }
}

fn process_inner(input: &[u8], policy: Policy) -> io::Result<Vec<u8>> {
    let header = policy.header()?;
    let candidate = validate(input)?;
    let (retained, candidate) = if candidate.needs_cleanup {
        drop(candidate);
        let retained = strip(input)?;
        let candidate = validate(&retained)?;
        (retained, candidate)
    } else {
        (input.to_vec(), candidate)
    };
    let size = candidate.dimensions;
    let thumbnail_size = crate::image_geometry::thumbnail(
        size,
        (u32::from(policy.thumbnail.0), u32::from(policy.thumbnail.1)),
    )?;
    let pixels = candidate.thumbnail_pixels(
        thumbnail_size,
        crate::image_geometry::thumbnail_background(policy.blue_background, policy.thread),
    )?;
    let thumbnail = crate::still::encode_jpeg(
        &pixels,
        thumbnail_size,
        if policy.thread { 50 } else { 40 },
        1024 * 1024,
    )?;
    drop(candidate);
    let mut result = vec![0; 64];
    result[..8].copy_from_slice(b"IBIMG007");
    result[8..24].copy_from_slice(&header);
    for (offset, value) in [
        (24, size.0),
        (28, size.1),
        (32, thumbnail_size.0),
        (36, thumbnail_size.1),
        (48, retained.len() as u32),
        (52, thumbnail.len() as u32),
    ] {
        result[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    result.extend_from_slice(&retained);
    result.extend_from_slice(&thumbnail);
    if result.len() > OUTPUT_BYTES {
        return Err(rejected());
    }
    result.resize(OUTPUT_BYTES, 0);
    Ok(result)
}

pub fn decode_disk(mut input: impl io::Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);
    if !(17..=MAX_INPUT as u64 + 16).contains(&length) {
        return Err(rejected());
    }
    let mut frame = vec![0; length as usize];
    input.read_exact(&mut frame)?;
    let padding_length = (512 - (8 + frame.len()) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_length])?;
    if padding[..padding_length].iter().any(|&value| value != 0) || input.read(&mut [0; 1])? != 0 {
        return Err(rejected());
    }
    process(&frame[16..], Policy::parse(&frame[..16])?)
}

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
    fn partial_frames_cannot_hide_excess_canvas_or_frame_work() {
        let original = sample();
        for (canvas, count, accepted) in [(4096u16, 16, true), (4096, 17, false),
            (4, 512, true), (4, 513, false)] {
            let mut input = original[..19].to_vec();
            input[6..8].copy_from_slice(&canvas.to_le_bytes());
            input[8..10].copy_from_slice(&canvas.to_le_bytes());
            for _ in 0..count { input.extend_from_slice(&original[19..original.len() - 1]); }
            input.push(b';');
            assert_eq!(animation::scan_source(&input).is_ok(), accepted);
            if !accepted { assert!(validate(&input).is_err()); }
        }
    }

    #[test]
    fn clean_candidate_retains_exact_bytes_and_returns_only_a_jpeg_thumbnail() {
        let input = sample();
        let policy = Policy {
            thread: true,
            blue_background: true,
            thumbnail: (250, 250),
        };
        let disk = process(&input, policy).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let expected = board_media::still::StillPolicy {
            format: board_media::still::StillFormat::Gif,
            strip_exif: false,
            mobile: false,
            thread: true,
            blue_background: true,
            thumbnail: (250, 250),
        };
        let output = runtime
            .block_on(board_media::still::ValidatedStill::read_disk(
                Cursor::new(disk),
                expected,
            ))
            .unwrap();
        assert_eq!(output.retained().bytes(), input);
        assert_eq!(output.retained().dimensions(), (4, 3));
        assert_eq!(output.thumbnail().dimensions(), (4, 3));
        assert!(output.mobile().is_none());
    }

    #[test]
    fn disk_mode_requires_the_exact_gif_policy_padding_and_eof() {
        let policy = Policy {
            thread: false,
            blue_background: false,
            thumbnail: (125, 125),
        };
        let body = [policy.header().unwrap().as_slice(), sample().as_slice()].concat();
        let mut disk = (body.len() as u64).to_be_bytes().to_vec();
        disk.extend_from_slice(&body);
        disk.resize(disk.len().div_ceil(512) * 512, 0);
        assert!(decode_disk(Cursor::new(&disk)).is_ok());
        for offset in [0, 8, 16, 17, 22, disk.len() - 1] {
            let mut changed = disk.clone();
            changed[offset] ^= 0xff;
            assert!(
                decode_disk(Cursor::new(changed)).is_err(),
                "offset {offset}"
            );
        }
        disk.push(0);
        assert!(decode_disk(Cursor::new(disk)).is_err());
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
