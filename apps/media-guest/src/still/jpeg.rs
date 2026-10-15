use super::raster::Raster;
use std::io;
use zune_jpeg::{
    JpegDecoder,
    zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions},
};

pub(super) fn decode(input: &[u8]) -> io::Result<Raster> {
    let rejected = || io::Error::other("JPEG decoding rejected");
    // Reject incomplete/trailing streams even when the decoder tolerates them.
    if !input.starts_with(b"\xff\xd8\xff") || !input.ends_with(b"\xff\xd9") {
        return Err(rejected());
    }
    let options = DecoderOptions::new_safe()
        .set_max_width(10_000)
        .set_max_height(10_000)
        .set_strict_mode(true)
        .set_use_unsafe(false)
        .jpeg_set_max_scans(64)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(input), options);
    decoder.decode_headers().map_err(|_| rejected())?;
    let info = decoder.info().ok_or_else(rejected)?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    if !(1..=10_000).contains(&width)
        || !(1..=10_000).contains(&height)
        || decoder.output_colorspace() != Some(ColorSpace::RGB)
    {
        return Err(rejected());
    }
    let length = width as usize * height as usize * 3;
    if decoder.output_buffer_size() != Some(length) {
        return Err(rejected());
    }
    let mut pixels = vec![0; length];
    decoder.decode_into(&mut pixels).map_err(|_| rejected())?;
    Raster::new(width, height, 3, pixels)
}
