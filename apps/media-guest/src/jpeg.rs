use std::io;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
};

pub(super) fn decode(input: &[u8]) -> io::Result<Vec<u8>> {
    let rejected = || io::Error::other("JPEG decoding rejected");
    // Require a terminal EOI marker in addition to strict decoder validation.
    // This is not an exhaustive container validator: only decoded pixels leave
    // the guest, never source bytes or metadata.
    if !input.ends_with(b"\xff\xd9") {
        return Err(rejected());
    }
    let options = DecoderOptions::new_safe()
        .set_max_width(1024)
        .set_max_height(1024)
        .set_strict_mode(true)
        .set_use_unsafe(false)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(input), options);
    decoder.decode_headers().map_err(|_| rejected())?;
    let info = decoder.info().ok_or_else(rejected)?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    if !(1..=1024).contains(&width)
        || !(1..=1024).contains(&height)
        || decoder.output_colorspace() != Some(ColorSpace::RGB)
    {
        return Err(rejected());
    }
    let count = width as usize * height as usize;
    if decoder.output_buffer_size() != Some(count * 3) {
        return Err(rejected());
    }
    let mut pixels = vec![0; count * 3];
    decoder.decode_into(&mut pixels).map_err(|_| rejected())?;
    let mut output = Vec::with_capacity(16 + count * 4);
    output.extend_from_slice(b"IBRGBA01");
    output.extend_from_slice(&width.to_be_bytes());
    output.extend_from_slice(&height.to_be_bytes());
    for pixel in pixels.chunks_exact(3) {
        output.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    Ok(output)
}
