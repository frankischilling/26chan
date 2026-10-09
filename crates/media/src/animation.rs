//! Bounded uncompressed GIF frame data. No GIF parser is used by this module.
use crate::{MediaError, ValidatedOutput};
use md5::{Digest as _, Md5};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAX_FRAMES: usize = 512;
pub const MAX_FRAME_PIXELS: usize = 1024 * 1024;
pub const MAX_TOTAL_PIXELS: usize = 16 * 1024 * 1024;
pub const MAX_WIRE_BYTES: usize = 17 * 1024 * 1024;
pub const MAX_GIF_BYTES: usize = 20 * 1024 * 1024;
pub const OUTPUT_DISK_BYTES: u64 = MAX_WIRE_BYTES as u64;

#[derive(Debug)]
struct Frame {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    delay: u16,
    transparent: Option<u8>,
    disposal: u8,
    palette: Vec<u8>,
    pixels: Vec<u8>,
}

/// Only the neutral protocol validator can construct this value.
#[derive(Debug)]
pub struct ValidatedAnimation {
    width: u16,
    height: u16,
    global_palette: Vec<u8>,
    background: Option<u8>,
    repeat: Option<u16>,
    frames: Vec<Frame>,
}

/// Encoder-owned GIF bytes. This is separate from PNG publication authority.
pub struct EncodedGif {
    bytes: Vec<u8>,
    sha256: String,
    md5: String,
    dimensions: (u32, u32),
}

impl EncodedGif {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn md5(&self) -> &str {
        &self.md5
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }
}

fn invalid() -> MediaError {
    MediaError::InvalidOutput
}

fn word(bytes: &[u8]) -> u16 {
    u16::from_be_bytes(bytes.try_into().expect("two protocol bytes"))
}

fn palette_size(count: u16, optional: bool) -> Result<usize, MediaError> {
    if (optional && count == 0) || ((2..=256).contains(&count) && count.is_power_of_two()) {
        Ok(usize::from(count) * 3)
    } else {
        Err(invalid())
    }
}

impl ValidatedAnimation {
    pub async fn read<R: AsyncRead + Unpin>(mut input: R) -> Result<Self, MediaError> {
        let (animation, _) = Self::read_protocol(&mut input).await?;
        if input.read(&mut [0; 1]).await? != 0 {
            return Err(invalid());
        }
        Ok(animation)
    }

    /// The caller must stop the guest and impose an absolute read deadline.
    pub async fn read_disk<R: AsyncRead + Unpin>(mut input: R) -> Result<Self, MediaError> {
        let (animation, used) = Self::read_protocol(&mut input).await?;
        let mut remaining = MAX_WIRE_BYTES.checked_sub(used).ok_or_else(invalid)?;
        let mut scratch = [0; 8192];
        while remaining != 0 {
            let count = remaining.min(scratch.len());
            input.read_exact(&mut scratch[..count]).await?;
            if scratch[..count].iter().any(|byte| *byte != 0) {
                return Err(invalid());
            }
            remaining -= count;
        }
        if input.read(&mut [0; 1]).await? != 0 {
            return Err(invalid());
        }
        Ok(animation)
    }

    async fn read_protocol<R: AsyncRead + Unpin>(
        input: &mut R,
    ) -> Result<(Self, usize), MediaError> {
        let mut header = [0; 32];
        input.read_exact(&mut header).await?;
        let width = word(&header[8..10]);
        let height = word(&header[10..12]);
        let count = usize::from(word(&header[12..14]));
        let colors = word(&header[14..16]);
        let background = word(&header[16..18]);
        let repeat = u32::from_be_bytes(header[18..22].try_into().expect("four protocol bytes"));
        let palette_bytes = palette_size(colors, true)?;
        if &header[..8] != b"IBGIF001"
            || !(1..=1024).contains(&width)
            || !(1..=1024).contains(&height)
            || !(1..=MAX_FRAMES).contains(&count)
            || (background != 256 && background >= colors)
            || repeat > 65536
            || header[22..].iter().any(|byte| *byte != 0)
        {
            return Err(invalid());
        }
        let mut global_palette = vec![0; palette_bytes];
        input.read_exact(&mut global_palette).await?;
        let mut frames = Vec::with_capacity(count);
        let mut total = 0usize;
        let mut used = header.len() + palette_bytes;
        for _ in 0..count {
            let mut header = [0; 20];
            input.read_exact(&mut header).await?;
            let left = word(&header[..2]);
            let top = word(&header[2..4]);
            let frame_width = word(&header[4..6]);
            let frame_height = word(&header[6..8]);
            let delay = word(&header[8..10]);
            let colors = word(&header[10..12]);
            let transparent = word(&header[12..14]);
            let disposal = header[14];
            let pixel_count = u32::from_be_bytes(header[16..20].try_into().expect("four bytes"));
            let palette_bytes = palette_size(colors, false)?;
            let expected = usize::from(frame_width) * usize::from(frame_height);
            total = total.checked_add(expected).ok_or_else(invalid)?;
            used = used
                .checked_add(20 + palette_bytes + expected)
                .ok_or_else(invalid)?;
            if frame_width == 0
                || frame_height == 0
                || u32::from(left) + u32::from(frame_width) > u32::from(width)
                || u32::from(top) + u32::from(frame_height) > u32::from(height)
                || expected > MAX_FRAME_PIXELS
                || total > MAX_TOTAL_PIXELS
                || pixel_count as usize != expected
                || used > MAX_WIRE_BYTES
                || (transparent != 256 && transparent >= colors)
                || disposal > 3
                || header[15] != 0
            {
                return Err(invalid());
            }
            let mut palette = vec![0; palette_bytes];
            input.read_exact(&mut palette).await?;
            let mut pixels = vec![0; expected];
            input.read_exact(&mut pixels).await?;
            if pixels.iter().any(|pixel| u16::from(*pixel) >= colors) {
                return Err(invalid());
            }
            frames.push(Frame {
                left,
                top,
                width: frame_width,
                height: frame_height,
                delay,
                disposal,
                transparent: (transparent != 256).then_some(transparent as u8),
                palette,
                pixels,
            });
        }
        Ok((
            Self {
                width,
                height,
                global_palette,
                background: (background != 256).then_some(background as u8),
                repeat: (repeat != 65536).then_some(repeat as u16),
                frames,
            },
            used,
        ))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (u32::from(self.width), u32::from(self.height))
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// First displayed image on a transparent canvas, without decoding GIF again.
    pub fn first_frame(&self) -> Result<ValidatedOutput, MediaError> {
        let frame = &self.frames[0];
        let mut pixels = vec![0; usize::from(self.width) * usize::from(self.height) * 4];
        for (offset, index) in frame.pixels.iter().copied().enumerate() {
            if frame.transparent == Some(index) {
                continue;
            }
            let x = offset % usize::from(frame.width) + usize::from(frame.left);
            let y = offset / usize::from(frame.width) + usize::from(frame.top);
            let destination = (y * usize::from(self.width) + x) * 4;
            let color = usize::from(index) * 3;
            pixels[destination..destination + 3].copy_from_slice(&frame.palette[color..color + 3]);
            pixels[destination + 3] = 255;
        }
        ValidatedOutput::from_validated_pixels(self.dimensions(), pixels)
    }

    pub fn encode(&self) -> Result<EncodedGif, MediaError> {
        let mut output = Vec::new();
        output.extend_from_slice(b"GIF89a");
        output.extend_from_slice(&self.width.to_le_bytes());
        output.extend_from_slice(&self.height.to_le_bytes());
        let global_colors = self.global_palette.len() / 3;
        let flags = if global_colors == 0 {
            0
        } else {
            0x80 | 0x70 | (global_colors.ilog2() as u8 - 1)
        };
        output.extend_from_slice(&[flags, self.background.unwrap_or(0), 0]);
        output.extend_from_slice(&self.global_palette);
        if let Some(repeat) = self.repeat {
            output.extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
            output.extend_from_slice(&repeat.to_le_bytes());
            output.push(0);
        }
        for frame in &self.frames {
            output.extend_from_slice(&[
                0x21,
                0xf9,
                4,
                frame.disposal << 2 | u8::from(frame.transparent.is_some()),
            ]);
            output.extend_from_slice(&frame.delay.to_le_bytes());
            output.extend_from_slice(&[frame.transparent.unwrap_or(0), 0, 0x2c]);
            for value in [frame.left, frame.top, frame.width, frame.height] {
                output.extend_from_slice(&value.to_le_bytes());
            }
            output.push(0x87); // Canonical 256-entry local table, noninterlaced.
            output.extend_from_slice(&frame.palette);
            output.resize(output.len() + 768 - frame.palette.len(), 0);
            output.push(8); // Minimum LZW code size for the local table.
            encode_literals(&frame.pixels, &mut output);
            if output.len() >= MAX_GIF_BYTES {
                return Err(invalid());
            }
        }
        output.push(0x3b);
        Ok(EncodedGif {
            sha256: format!("{:x}", Sha256::digest(&output)),
            md5: Md5::digest(&output)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            dimensions: self.dimensions(),
            bytes: output,
        })
    }
}

/// GIF89a permits a clear code at any point. Clear before every 254 literals so
/// the decoder's dictionary never reaches the 9-to-10-bit boundary. There is no
/// compressor dictionary, input decoder, variable code width, or metadata copy.
fn encode_literals(pixels: &[u8], output: &mut Vec<u8>) {
    let mut packed = Vec::with_capacity(pixels.len() * 9 / 8 + pixels.len() / 254 + 4);
    let mut accumulator = 0u32;
    let mut bits = 0u32;
    let mut push = |code: u16| {
        accumulator |= u32::from(code) << bits;
        bits += 9;
        while bits >= 8 {
            packed.push(accumulator as u8);
            accumulator >>= 8;
            bits -= 8;
        }
    };
    for chunk in pixels.chunks(254) {
        push(256);
        for pixel in chunk {
            push(u16::from(*pixel));
        }
    }
    push(257);
    if bits != 0 {
        packed.push(accumulator as u8);
    }
    for block in packed.chunks(255) {
        output.push(block.len() as u8);
        output.extend_from_slice(block);
    }
    output.push(0);
}
