use std::{
    io::{self, BufRead, Cursor},
    num::NonZeroU64,
};

/// Decode one full-canvas GIF inside the disposable guest. Animation and partial
/// frames need a separately bounded compositor and are deliberately rejected.
pub(super) fn decode(input: &[u8]) -> io::Result<Vec<u8>> {
    if input.last() != Some(&b';') {
        return Err(io::Error::other("GIF trailer missing"));
    }
    let mut options = ::gif::DecodeOptions::new();
    options.set_color_output(::gif::ColorOutput::RGBA);
    options.set_memory_limit(::gif::MemoryLimit::Bytes(
        NonZeroU64::new(4 * 1024 * 1024).expect("nonzero frame limit"),
    ));
    options.check_frame_consistency(true);
    options.check_lzw_end_code(true);
    options.allow_unknown_blocks(false);
    let mut decoder = options
        .read_info(Cursor::new(input))
        .map_err(io::Error::other)?;
    let width = decoder.width();
    let height = decoder.height();
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return Err(io::Error::other("GIF dimensions rejected"));
    }
    let frame = decoder
        .read_next_frame()
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::other("GIF frame missing"))?;
    let size = usize::from(width) * usize::from(height) * 4;
    if frame.left != 0
        || frame.top != 0
        || frame.width != width
        || frame.height != height
        || frame.buffer.len() != size
    {
        return Err(io::Error::other("partial GIF frame rejected"));
    }
    let mut output = Vec::with_capacity(16 + size);
    output.extend_from_slice(b"IBRGBA01");
    output.extend_from_slice(&u32::from(width).to_be_bytes());
    output.extend_from_slice(&u32::from(height).to_be_bytes());
    output.extend_from_slice(&frame.buffer);
    // Read the next descriptor, including intervening extensions and the trailer,
    // before releasing any pixels. Do not decode or publish an animation frame.
    if decoder
        .next_frame_info()
        .map_err(io::Error::other)?
        .is_some()
    {
        return Err(io::Error::other("animated GIF rejected"));
    }
    // The decoder stops at the first trailer. Its buffered reader retains any
    // bytes after that marker, even when the input also ends with a trailer.
    if !decoder.into_inner().fill_buf()?.is_empty() {
        return Err(io::Error::other("trailing GIF data rejected"));
    }
    Ok(output)
}
