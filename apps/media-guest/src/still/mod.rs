//! Isolated still-image candidate processing. Policy bytes are transport data;
//! the trusted publisher must bind them to the authenticated posting context.
use crate::image_geometry::{self, StillFormat};
use std::io::{self, Read};

mod jpeg;
mod png;
pub(crate) mod raster;
#[cfg(target_os = "linux")]
mod strip_jpeg;

pub const MAX_INPUT_BYTES: usize = 10 * 1024 * 1024;
pub const OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_THUMBNAIL_BYTES: usize = 1024 * 1024;
const MAX_MOBILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Policy {
    pub format: StillFormat,
    pub strip_exif: bool,
    pub mobile: bool,
    pub thread: bool,
    pub blue_background: bool,
    pub thumbnail: (u16, u16),
}

impl Policy {
    pub fn parse(header: &[u8]) -> io::Result<Self> {
        if header.len() != 16
            || &header[..8] != b"IBSTIL06"
            || header[8] & !15 != 0
            || header[14..16] != [0, 0]
        {
            return Err(rejected());
        }
        let format = match header[9] {
            1 => StillFormat::Png,
            2 => StillFormat::Jpeg,
            _ => return Err(rejected()),
        };
        let width = u16::from_be_bytes([header[10], header[11]]);
        let height = u16::from_be_bytes([header[12], header[13]]);
        if !(1..=250).contains(&width) || !(1..=250).contains(&height) {
            return Err(rejected());
        }
        Ok(Self {
            format,
            strip_exif: header[8] & 1 != 0,
            mobile: header[8] & 2 != 0,
            thread: header[8] & 4 != 0,
            blue_background: header[8] & 8 != 0,
            thumbnail: (width, height),
        })
    }

    pub fn header(self) -> io::Result<[u8; 16]> {
        let mut header = [0; 16];
        header[..8].copy_from_slice(b"IBSTIL06");
        header[8] = u8::from(self.strip_exif)
            | (u8::from(self.mobile) << 1)
            | (u8::from(self.thread) << 2)
            | (u8::from(self.blue_background) << 3);
        header[9] = match self.format {
            StillFormat::Png => 1,
            StillFormat::Jpeg => 2,
        };
        header[10..12].copy_from_slice(&self.thumbnail.0.to_be_bytes());
        header[12..14].copy_from_slice(&self.thumbnail.1.to_be_bytes());
        Self::parse(&header)?;
        Ok(header)
    }
}

fn rejected() -> io::Error {
    io::Error::other("still image candidate rejected")
}

pub(crate) fn encode_jpeg(
    pixels: &[u8],
    dimensions: (u32, u32),
    quality: u8,
    maximum: usize,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    jpeg_encoder::Encoder::new(&mut bytes, quality)
        .encode(
            pixels,
            dimensions.0.try_into().map_err(|_| rejected())?,
            dimensions.1.try_into().map_err(|_| rejected())?,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|_| rejected())?;
    if bytes.len() > maximum || !bytes.starts_with(b"\xff\xd8\xff") || !bytes.ends_with(b"\xff\xd9")
    {
        return Err(rejected());
    }
    Ok(bytes)
}

/// Complete a candidate from already bounded input. A decoder panic discards
/// all partial output. Process aborts and exhaustion remain externally bounded.
pub fn process(input: &[u8], policy: Policy) -> io::Result<Vec<u8>> {
    std::panic::catch_unwind(|| process_inner(input, policy)).unwrap_or_else(|_| Err(rejected()))
}

fn process_inner(input: &[u8], policy: Policy) -> io::Result<Vec<u8>> {
    let policy_header = policy.header()?;
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err(rejected());
    }
    let raster = match policy.format {
        StillFormat::Png if input.starts_with(b"\x89PNG\r\n\x1a\n") => png::decode(input)?,
        StillFormat::Jpeg if input.starts_with(b"\xff\xd8\xff") => jpeg::decode(input)?,
        _ => return Err(rejected()),
    };
    let retained = match policy.format {
        StillFormat::Png => png::retained(input)?,
        StillFormat::Jpeg if policy.strip_exif => {
            #[cfg(target_os = "linux")]
            {
                strip_jpeg::strip(input)?
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err(rejected());
            }
        }
        StillFormat::Jpeg => input.to_vec(),
    };
    if retained.is_empty() || retained.len() > MAX_INPUT_BYTES {
        return Err(rejected());
    }
    let dimensions = (raster.width, raster.height);
    let thumbnail_dimensions = image_geometry::thumbnail(
        dimensions,
        (u32::from(policy.thumbnail.0), u32::from(policy.thumbnail.1)),
    )?;
    let rgb = raster.resize(
        thumbnail_dimensions.0,
        thumbnail_dimensions.1,
        image_geometry::thumbnail_background(policy.blue_background, policy.thread),
    )?;
    let thumbnail = encode_jpeg(
        &rgb,
        thumbnail_dimensions,
        if policy.thread { 50 } else { 40 },
        MAX_THUMBNAIL_BYTES,
    )?;
    drop(rgb);
    let mobile_dimensions = if policy.mobile {
        image_geometry::mobile(policy.format, dimensions, retained.len() as u64)?
    } else {
        None
    };
    let mobile = if let Some(size) = mobile_dimensions {
        let rgb = raster.resize(size.0, size.1, [0; 3])?;
        encode_jpeg(&rgb, size, 80, MAX_MOBILE_BYTES)?
    } else {
        Vec::new()
    };
    drop(raster);
    let mut output = vec![0; 64];
    output[..8].copy_from_slice(b"IBIMG006");
    output[8..24].copy_from_slice(&policy_header);
    for (offset, value) in [
        (24, dimensions.0),
        (28, dimensions.1),
        (32, thumbnail_dimensions.0),
        (36, thumbnail_dimensions.1),
        (40, mobile_dimensions.map_or(0, |v| v.0)),
        (44, mobile_dimensions.map_or(0, |v| v.1)),
        (48, retained.len() as u32),
        (52, thumbnail.len() as u32),
        (56, mobile.len() as u32),
    ] {
        output[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    output.extend_from_slice(&retained);
    output.extend_from_slice(&thumbnail);
    output.extend_from_slice(&mobile);
    if output.len() > OUTPUT_BYTES {
        return Err(rejected());
    }
    output.resize(OUTPUT_BYTES, 0);
    Ok(output)
}

/// Validate all request framing and sector padding before invoking a parser.
pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_be_bytes(length);
    if !(17..=MAX_INPUT_BYTES as u64 + 16).contains(&length) {
        return Err(rejected());
    }
    let mut frame = vec![0; length as usize];
    input.read_exact(&mut frame)?;
    let padding_length = (512 - (8 + frame.len()) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_length])?;
    if padding[..padding_length].iter().any(|&v| v != 0) || input.read(&mut [0])? != 0 {
        return Err(rejected());
    }
    let policy = Policy::parse(&frame[..16])?;
    process(&frame[16..], policy)
}
