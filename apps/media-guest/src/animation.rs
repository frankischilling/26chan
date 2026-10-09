//! GIF parsing and decompression remain inside the disposable guest.
use std::{
    io::{self, BufRead, Cursor, Read},
    num::NonZeroU64,
};

const MAX_FRAMES: usize = 512;
const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_WIRE: usize = 17 * 1024 * 1024;

fn rejected() -> io::Error {
    io::Error::other("GIF animation rejected")
}

/// A raw GIF with a bounded u64 byte count, exact sector padding and EOF.
/// Mode comes from the trusted kernel argument, never from these bytes.
pub fn decode_disk(mut input: impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 8];
    input.read_exact(&mut header)?;
    let length = u64::from_be_bytes(header);
    if !(1..=8 * 1024 * 1024).contains(&length) {
        return Err(rejected());
    }
    let mut bytes = vec![0; length as usize];
    input.read_exact(&mut bytes)?;
    let padding_length = (512 - (8 + length as usize) % 512) % 512;
    let mut padding = [0; 511];
    input.read_exact(&mut padding[..padding_length])?;
    if padding[..padding_length].iter().any(|byte| *byte != 0) || input.read(&mut [0; 1])? != 0 {
        return Err(rejected());
    }
    decode(&bytes)
}

/// This candidate API emits only IBGIF001. Image-v1 still emits only IBRGBA01.
/// Callers must select an explicit animation transport before using this API.
pub fn decode(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.len() > 8 * 1024 * 1024
        || !(input.starts_with(b"GIF87a") || input.starts_with(b"GIF89a"))
    {
        return Err(rejected());
    }
    let repeat = scan_blocks(input)?;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        NonZeroU64::new(1024 * 1024).unwrap(),
    ));
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    // Complete-stream decoding below requires Done in a fixed output buffer.
    // This avoids the reader's transient NoProgress at GIF sub-block splits.
    options.skip_frame_decoding(true);
    options.allow_unknown_blocks(false);
    let mut decoder = options
        .read_info(Cursor::new(input))
        .map_err(io::Error::other)?;
    let (width, height) = (decoder.width(), decoder.height());
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return Err(rejected());
    }
    let global = decoder.global_palette().unwrap_or_default().to_vec();
    let background = decoder.bg_color().map_or(256, |index| index as u16);
    let mut output = vec![0; 32];
    output[..8].copy_from_slice(b"IBGIF001");
    output[8..10].copy_from_slice(&width.to_be_bytes());
    output[10..12].copy_from_slice(&height.to_be_bytes());
    output[14..16].copy_from_slice(&((global.len() / 3) as u16).to_be_bytes());
    output[16..18].copy_from_slice(&background.to_be_bytes());
    output[18..22].copy_from_slice(&repeat.to_be_bytes());
    output.extend_from_slice(&global);
    let mut count = 0usize;
    let mut total = 0usize;
    while let Some(frame) = decoder.read_next_frame().map_err(io::Error::other)? {
        count += 1;
        let pixels = usize::from(frame.width) * usize::from(frame.height);
        total = total.checked_add(pixels).ok_or_else(rejected)?;
        let palette = frame.palette.as_deref().unwrap_or(&global);
        let colors = palette.len() / 3;
        let added = 20 + palette.len() + pixels;
        if count > MAX_FRAMES
            || total > MAX_PIXELS
            || pixels == 0
            || pixels > 1024 * 1024
            || !(2..=256).contains(&colors)
            || !colors.is_power_of_two()
            || palette.len() != colors * 3
            || frame.needs_user_input
            || frame
                .transparent
                .is_some_and(|index| usize::from(index) >= colors)
            || output
                .len()
                .checked_add(added)
                .is_none_or(|len| len > MAX_WIRE)
        {
            return Err(rejected());
        }
        let indices = decode_indices(frame, pixels)?;
        if indices.iter().any(|index| usize::from(*index) >= colors) {
            return Err(rejected());
        }
        output.reserve_exact(added);
        for value in [
            frame.left,
            frame.top,
            frame.width,
            frame.height,
            frame.delay,
            colors as u16,
            frame.transparent.map_or(256, u16::from),
        ] {
            output.extend_from_slice(&value.to_be_bytes());
        }
        output.extend_from_slice(&[frame.dispose as u8, 0]);
        output.extend_from_slice(&(pixels as u32).to_be_bytes());
        output.extend_from_slice(palette);
        output.extend_from_slice(&indices);
    }
    if count == 0 || !decoder.into_inner().fill_buf()?.is_empty() {
        return Err(rejected());
    }
    output[12..14].copy_from_slice(&(count as u16).to_be_bytes());
    Ok(output)
}

fn decode_indices(frame: &gif::Frame<'_>, count: usize) -> io::Result<Vec<u8>> {
    let (minimum, compressed) = frame.buffer.split_first().ok_or_else(rejected)?;
    if !(2..=8).contains(minimum) {
        return Err(rejected());
    }
    // One extra byte detects streams that expand beyond their descriptor.
    let mut indices = vec![0; count + 1];
    let mut decoder = weezl::decode::Decoder::new(weezl::BitOrder::Lsb, *minimum);
    let mut read = 0;
    let mut written = 0;
    loop {
        let result = decoder.decode_bytes(&compressed[read..], &mut indices[written..]);
        read += result.consumed_in;
        written += result.consumed_out;
        if written > count {
            return Err(rejected());
        }
        match result.status.map_err(io::Error::other)? {
            weezl::LzwStatus::Done => break,
            weezl::LzwStatus::NoProgress => return Err(rejected()),
            weezl::LzwStatus::Ok if result.consumed_in != 0 || result.consumed_out != 0 => (),
            weezl::LzwStatus::Ok => return Err(rejected()),
        }
    }
    if written != count || read != compressed.len() {
        return Err(rejected());
    }
    indices.truncate(count);
    if !frame.interlaced {
        return Ok(indices);
    }
    let width = usize::from(frame.width);
    let height = usize::from(frame.height);
    let mut ordered = vec![0; count];
    let mut source_row = 0;
    for (start, stride) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        for row in (start..height).step_by(stride) {
            ordered[row * width..(row + 1) * width]
                .copy_from_slice(&indices[source_row * width..(source_row + 1) * width]);
            source_row += 1;
        }
    }
    Ok(ordered)
}

/// Check extension semantics that the decoder otherwise ignores or normalizes.
/// Text rendering and user-input frames cannot be faithfully emitted as pixels.
/// Loop control is a single, bounded numeric field; arbitrary extensions never
/// enter the neutral output. This scan does not decompress any image data.
fn scan_blocks(input: &[u8]) -> io::Result<u32> {
    let mut cursor = Blocks {
        bytes: input,
        offset: 6,
    };
    let logical = cursor.take(7)?;
    if logical[4] & 0x80 != 0 {
        cursor.take(3 * (2usize << (logical[4] & 7)))?;
    }
    let mut repeat = 65536;
    let mut count = 0usize;
    let mut total = 0usize;
    loop {
        match cursor.byte()? {
            0x3b if cursor.offset == input.len() => return Ok(repeat),
            0x2c => {
                count += 1;
                let descriptor = cursor.take(9)?;
                let width = u16::from_le_bytes(descriptor[4..6].try_into().unwrap());
                let height = u16::from_le_bytes(descriptor[6..8].try_into().unwrap());
                let pixels = usize::from(width) * usize::from(height);
                total = total.checked_add(pixels).ok_or_else(rejected)?;
                if count > MAX_FRAMES
                    || total > MAX_PIXELS
                    || pixels == 0
                    || width > 1024
                    || height > 1024
                    || descriptor[8] & 0x18 != 0
                {
                    return Err(rejected());
                }
                if descriptor[8] & 0x80 != 0 {
                    cursor.take(3 * (2usize << (descriptor[8] & 7)))?;
                }
                if !(2..=8).contains(&cursor.byte()?) {
                    return Err(rejected());
                }
                cursor.skip_subblocks()?;
            }
            0x21 => match cursor.byte()? {
                0xf9 => {
                    if cursor.byte()? != 4 {
                        return Err(rejected());
                    }
                    let control = cursor.take(4)?;
                    if control[0] & 0xe2 != 0 || ((control[0] >> 2) & 7) > 3 || cursor.byte()? != 0
                    {
                        return Err(rejected());
                    }
                }
                0xff => {
                    if cursor.byte()? != 11 {
                        return Err(rejected());
                    }
                    let application = cursor.take(11)?;
                    if application == b"NETSCAPE2.0" || application == b"ANIMEXTS1.0" {
                        if repeat != 65536 || cursor.byte()? != 3 {
                            return Err(rejected());
                        }
                        let loop_data = cursor.take(3)?;
                        if loop_data[0] != 1 {
                            return Err(rejected());
                        }
                        repeat = u32::from(u16::from_le_bytes(loop_data[1..].try_into().unwrap()));
                        if cursor.byte()? != 0 {
                            return Err(rejected());
                        }
                    } else {
                        cursor.skip_subblocks()?;
                    }
                }
                0xfe => cursor.skip_subblocks()?,
                _ => return Err(rejected()),
            },
            _ => return Err(rejected()),
        }
    }
}

struct Blocks<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Blocks<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self.offset.checked_add(count).ok_or_else(rejected)?;
        let bytes = self.bytes.get(self.offset..end).ok_or_else(rejected)?;
        self.offset = end;
        Ok(bytes)
    }
    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn skip_subblocks(&mut self) -> io::Result<()> {
        loop {
            let count = usize::from(self.byte()?);
            if count == 0 {
                return Ok(());
            }
            self.take(count)?;
        }
    }
}
