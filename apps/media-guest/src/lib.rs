#![forbid(unsafe_code)]

use std::io::{self, Cursor};

mod jpeg;

/// Format selection and every complex parser stay inside the disposable guest.
/// Extensions and client content types are never used to select a decoder.
pub fn decode_image(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.is_empty() || input.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("input size rejected"));
    }
    if input.starts_with(b"\x89PNG\r\n\x1a\n") {
        decode_png(input)
    } else if input.starts_with(b"\xff\xd8\xff") {
        jpeg::decode(input)
    } else {
        Err(io::Error::other("unsupported image format"))
    }
}

/// The PNG parser belongs only in the disposable guest. Host applications must
/// consume the bounded pixel protocol rather than link this crate.
pub fn decode_png(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.is_empty() || input.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other("input size rejected"));
    }
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(input),
        png::Limits {
            bytes: 32 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    if !(1..=1024).contains(&info.width)
        || !(1..=1024).contains(&info.height)
        || info.animation_control.is_some()
    {
        return Err(io::Error::other("image dimensions or animation rejected"));
    }
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= 4 * 1024 * 1024)
        .ok_or_else(|| io::Error::other("decoded size rejected"))?;
    let mut pixels = vec![0; size];
    let frame = reader.next_frame(&mut pixels)?;
    let mut output = Vec::with_capacity(16 + frame.width as usize * frame.height as usize * 4);
    output.extend_from_slice(b"IBRGBA01");
    output.extend_from_slice(&frame.width.to_be_bytes());
    output.extend_from_slice(&frame.height.to_be_bytes());
    for pixel in pixels[..frame.buffer_size()].chunks_exact(frame.color_type.samples()) {
        let rgba = match frame.color_type {
            png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
            png::ColorType::GrayscaleAlpha => [pixel[0], pixel[0], pixel[0], pixel[1]],
            png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
            png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
            png::ColorType::Indexed => return Err(io::Error::other("unexpanded pixels")),
        };
        output.extend_from_slice(&rgba);
    }
    reader.finish()?;
    Ok(output)
}
