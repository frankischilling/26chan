//! Guest-only paired decoding. The binding is opaque transport data, not authority.
use std::io::{self, Read};

pub const RESULT_BYTES: usize = 4_456_960;
pub const MAX_FRAME_BYTES: u64 = 16_777_272;
const COMPONENT_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    ImageV1,
    PairedV2,
    GifV3,
}

impl InputKind {
    pub fn parse(value: &str) -> io::Result<Self> {
        match value {
            "image-v1" => Ok(Self::ImageV1),
            "paired-v2" => Ok(Self::PairedV2),
            "gif-v3" => Ok(Self::GifV3),
            _ => Err(rejected()),
        }
    }

    pub fn argument(self) -> &'static str {
        match self {
            Self::ImageV1 => "image-v1",
            Self::PairedV2 => "paired-v2",
            Self::GifV3 => "gif-v3",
        }
    }

    pub fn output_bytes(self) -> u64 {
        match self {
            Self::ImageV1 => 4_194_816,
            Self::PairedV2 => RESULT_BYTES as u64,
            Self::GifV3 => 17_825_792,
        }
    }
}

/// Older image-only boot configurations omit the argument. Selection is never
/// inferred from attacker-controlled disk magic; duplicate selections fail.
pub fn kernel_input_kind(command_line: &str) -> io::Result<InputKind> {
    let mut selected = None;
    for argument in command_line.split_ascii_whitespace() {
        if argument == "board_media_input_kind" {
            return Err(rejected());
        }
        if let Some(value) = argument.strip_prefix("board_media_input_kind=") {
            if selected.is_some() {
                return Err(rejected());
            }
            selected = Some(InputKind::parse(value)?);
        }
    }
    Ok(selected.unwrap_or(InputKind::ImageV1))
}

/// Read exactly the declared bounded frame, sector padding, and EOF before
/// invoking either complex decoder. Works on the guest's read-only block disk.
pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0_u8; 48];
    input.read_exact(&mut header)?;
    if &header[..8] != b"IBJOB002" {
        return Err(rejected());
    }
    let length = u64_at(&header, 8);
    if !(57..=MAX_FRAME_BYTES).contains(&length) {
        return Err(rejected());
    }
    let mut frame = Vec::new();
    let length = usize::try_from(length).map_err(|_| rejected())?;
    frame.try_reserve_exact(length).map_err(|_| rejected())?;
    frame.resize(length, 0);
    input.read_exact(&mut frame)?;
    let padding_length = (512 - (48 + length) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_length])?;
    if padding[..padding_length].iter().any(|&byte| byte != 0) {
        return Err(rejected());
    }
    let mut extra = [0];
    if input.read(&mut extra)? != 0 {
        return Err(rejected());
    }
    decode_frame(&frame, header[16..48].try_into().map_err(|_| rejected())?)
}

fn decode_frame(frame: &[u8], binding: [u8; 32]) -> io::Result<Vec<u8>> {
    if frame.len() < 57
        || &frame[..8] != b"IBPAIR02"
        || frame[8..10] != 2_u16.to_be_bytes()
        || frame[10..12] != 48_u16.to_be_bytes()
    {
        return Err(rejected());
    }
    let flags = u32::from_be_bytes(frame[12..16].try_into().map_err(|_| rejected())?);
    let image_length = u64_at(frame, 16);
    let replay_length = u64_at(frame, 24);
    if flags > 1
        || !(1..=COMPONENT_LIMIT).contains(&image_length)
        || replay_length > COMPONENT_LIMIT
        || (flags == 1) != (replay_length != 0)
        || image_length
            .checked_add(replay_length)
            .and_then(|n| n.checked_add(56))
            != Some(frame.len() as u64)
    {
        return Err(rejected());
    }
    let image_end = 48 + image_length as usize;
    let replay_end = image_end + replay_length as usize;
    if &frame[replay_end..] != b"IBDONE02" {
        return Err(rejected());
    }
    // Parsers execute sequentially under the same preexisting guest limits.
    // Each component has independent dimensions; equality is not required.
    let pixels = crate::decode_png(&frame[48..image_end])?;
    let wire = if flags == 1 {
        let candidate = crate::replay::parse_candidate(&frame[image_end..replay_end])
            .map_err(io::Error::other)?;
        crate::replay_output::encode_untrusted_candidate(&candidate).map_err(io::Error::other)?
    } else {
        Vec::new()
    };
    let used = 64_usize
        .checked_add(pixels.len())
        .and_then(|n| n.checked_add(wire.len()))
        .filter(|&n| n <= RESULT_BYTES)
        .ok_or_else(rejected)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(RESULT_BYTES)
        .map_err(|_| rejected())?;
    result.extend_from_slice(b"IBRES002");
    result.extend_from_slice(&2_u16.to_be_bytes());
    result.extend_from_slice(&64_u16.to_be_bytes());
    result.extend_from_slice(&flags.to_be_bytes());
    result.extend_from_slice(&binding);
    result.extend_from_slice(&(pixels.len() as u64).to_be_bytes());
    result.extend_from_slice(&(wire.len() as u64).to_be_bytes());
    result.extend_from_slice(&pixels);
    result.extend_from_slice(&wire);
    debug_assert_eq!(result.len(), used);
    result.resize(RESULT_BYTES, 0);
    Ok(result)
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("bounded header"),
    )
}

fn rejected() -> io::Error {
    io::Error::other("paired input rejected")
}
