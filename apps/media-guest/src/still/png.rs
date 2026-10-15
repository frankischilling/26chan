use super::{MAX_INPUT_BYTES, raster::Raster};
use std::io::{self, Cursor};

pub(super) fn decode(input: &[u8]) -> io::Result<Raster> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(input),
        png::Limits {
            bytes: 32 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    if !(1..=10_000).contains(&info.width)
        || !(1..=10_000).contains(&info.height)
        || info.animation_control.is_some()
    {
        return Err(io::Error::other("PNG dimensions or animation rejected"));
    }
    let count = reader
        .output_buffer_size()
        .filter(|size| *size <= 400_000_000)
        .ok_or_else(|| io::Error::other("PNG pixel allocation rejected"))?;
    let mut pixels = vec![0; count];
    let frame = reader.next_frame(&mut pixels)?;
    if frame.bit_depth != png::BitDepth::Eight || frame.color_type == png::ColorType::Indexed {
        return Err(io::Error::other("PNG output samples rejected"));
    }
    pixels.truncate(frame.buffer_size());
    reader.finish()?;
    Raster::new(
        frame.width,
        frame.height,
        frame.color_type.samples(),
        pixels,
    )
}

/// Apply source chunk retention only after the isolated decoder accepts pixels.
/// All retained chunk bytes, including CRCs, remain identical to the upload.
pub(super) fn retained(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.len() > MAX_INPUT_BYTES || !input.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(io::Error::other("PNG cleanup input rejected"));
    }
    let mut output = input[..8].to_vec();
    let mut offset = 8usize;
    while offset < input.len() {
        let header = input
            .get(offset..offset + 8)
            .ok_or_else(|| io::Error::other("PNG chunk truncated"))?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four bytes")) as usize;
        if length > MAX_INPUT_BYTES {
            return Err(io::Error::other("PNG chunk too large"));
        }
        let end = offset
            .checked_add(12)
            .and_then(|n| n.checked_add(length))
            .ok_or_else(|| io::Error::other("PNG chunk overflow"))?;
        let chunk = input
            .get(offset..end)
            .ok_or_else(|| io::Error::other("PNG chunk truncated"))?;
        let kind: Vec<_> = header[4..8].iter().map(u8::to_ascii_lowercase).collect();
        if kind == b"actl" {
            return Err(io::Error::other("animated PNG rejected"));
        }
        if matches!(
            kind.as_slice(),
            b"ihdr"
                | b"plte"
                | b"idat"
                | b"iend"
                | b"trns"
                | b"gama"
                | b"sbit"
                | b"phys"
                | b"srgb"
                | b"bkgd"
                | b"time"
                | b"chrm"
                | b"iccp"
        ) {
            output.extend_from_slice(chunk);
        }
        if kind == b"iend" {
            return Ok(output);
        }
        offset = end;
    }
    Err(io::Error::other("PNG IEND missing"))
}
