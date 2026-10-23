//! One frame per connection. Callers apply an absolute deadline to the whole exchange.
use crate::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_INPUT: u64 = 8_388_608;
pub const OUTPUT_LENGTH: u64 = 4_194_816;
pub const MIN_PAIRED_INPUT: u64 = 57;
pub const MAX_PAIRED_INPUT: u64 = 16_777_272;
pub const PAIRED_OUTPUT_LENGTH: u64 = 4_456_960;
pub const GIF_OUTPUT_LENGTH: u64 = 17_825_792;
pub const PDF_OUTPUT_LENGTH: u64 = OUTPUT_LENGTH;
pub const MAX_SWF_INPUT: u64 = 10 * 1024 * 1024;
pub const SWF_OUTPUT_LENGTH: u64 = 11 * 1024 * 1024;
pub const VIDEO_OUTPUT_LENGTH: u64 = 9 * 1024 * 1024;
pub const MAX_STILL_INPUT: u64 = 10 * 1024 * 1024 + 16;
pub const STILL_OUTPUT_LENGTH: u64 = 16 * 1024 * 1024;
pub const MAX_GIF_SOURCE_INPUT: u64 = MAX_INPUT + 16;

/// Transport framing only: the binding and completed pair remain opaque here.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    ImageV1(Vec<u8>),
    GifV3(Vec<u8>),
    PdfV4(Vec<u8>),
    VideoV5(Vec<u8>),
    SwfV8(Vec<u8>),
    StillV6(Vec<u8>),
    GifV7(Vec<u8>),
    PairedV2 { binding: [u8; 32], input: Vec<u8> },
}

/// Select solely from the explicit outer request version, never payload sniffing.
pub async fn read_versioned_request<R: AsyncRead + Unpin>(input: &mut R) -> Result<Request> {
    let (magic, length) = read_header(input).await?;
    match &magic {
        b"IBJOB008" if (8..=MAX_SWF_INPUT).contains(&length) => {
            Ok(Request::SwfV8(read_body(input, length).await?))
        }
        b"IBJOB001" if (1..=MAX_INPUT).contains(&length) => {
            Ok(Request::ImageV1(read_body(input, length).await?))
        }
        b"IBJOB003" if (1..=MAX_INPUT).contains(&length) => {
            Ok(Request::GifV3(read_body(input, length).await?))
        }
        b"IBJOB004" if (1..=MAX_INPUT).contains(&length) => {
            Ok(Request::PdfV4(read_body(input, length).await?))
        }
        b"IBJOB005" if (2..=MAX_INPUT + 1).contains(&length) => {
            let body = read_body(input, length).await?;
            if !matches!(body[0], 1 | 2) {
                return Err(Error::Frame);
            }
            Ok(Request::VideoV5(body))
        }
        b"IBJOB006" if (17..=MAX_STILL_INPUT).contains(&length) => {
            let body = read_body(input, length).await?;
            validate_still_request(&body)?;
            Ok(Request::StillV6(body))
        }
        b"IBJOB007" if (17..=MAX_GIF_SOURCE_INPUT).contains(&length) => {
            let body = read_body(input, length).await?;
            validate_source_gif_request(&body)?;
            Ok(Request::GifV7(body))
        }
        b"IBJOB002" if (MIN_PAIRED_INPUT..=MAX_PAIRED_INPUT).contains(&length) => {
            let (binding, input) = read_paired_body(input, length).await?;
            Ok(Request::PairedV2 { binding, input })
        }
        _ => Err(Error::Frame),
    }
}

pub async fn read_paired_request<R: AsyncRead + Unpin>(
    input: &mut R,
) -> Result<([u8; 32], Vec<u8>)> {
    let (magic, length) = read_header(input).await?;
    if &magic != b"IBJOB002" || !(MIN_PAIRED_INPUT..=MAX_PAIRED_INPUT).contains(&length) {
        return Err(Error::Frame);
    }
    read_paired_body(input, length).await
}

async fn read_paired_body<R: AsyncRead + Unpin>(
    input: &mut R,
    length: u64,
) -> Result<([u8; 32], Vec<u8>)> {
    let mut binding = [0; 32];
    input
        .read_exact(&mut binding)
        .await
        .map_err(|_| Error::Frame)?;
    Ok((binding, read_body(input, length).await?))
}

pub async fn read_paired_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(
        input,
        b"IBOUT002",
        PAIRED_OUTPUT_LENGTH,
        PAIRED_OUTPUT_LENGTH,
    )
    .await
}
const SCRATCH: usize = 16_384;

pub async fn read_request<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBJOB001", 1, MAX_INPUT).await
}

pub async fn read_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT001", OUTPUT_LENGTH, OUTPUT_LENGTH).await
}

pub async fn read_gif_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT003", GIF_OUTPUT_LENGTH, GIF_OUTPUT_LENGTH).await
}

pub async fn read_pdf_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT004", PDF_OUTPUT_LENGTH, PDF_OUTPUT_LENGTH).await
}

pub async fn read_video_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT005", VIDEO_OUTPUT_LENGTH, VIDEO_OUTPUT_LENGTH).await
}

/// Validate only the fixed policy header. Image parsing belongs to the guest.
pub fn validate_still_request(bytes: &[u8]) -> Result<()> {
    if !(17..=MAX_STILL_INPUT as usize).contains(&bytes.len())
        || &bytes[..8] != b"IBSTIL06"
        || bytes[8] & !15 != 0
        || !matches!(bytes[9], 1 | 2)
        || bytes[15] != 0
        || bytes[14] > 2
        || (bytes[14] != 0 && bytes[9] != 2)
        || !(1..=250).contains(&u16::from_be_bytes([bytes[10], bytes[11]]))
        || !(1..=250).contains(&u16::from_be_bytes([bytes[12], bytes[13]]))
    {
        return Err(Error::Frame);
    }
    Ok(())
}

pub async fn write_still_request<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    validate_still_request(bytes)?;
    write_frame(bytes, bytes.len() as u64, output, b"IBJOB006").await
}

pub async fn read_still_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT006", STILL_OUTPUT_LENGTH, STILL_OUTPUT_LENGTH).await
}

pub async fn write_still_response<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    if bytes.len() as u64 != STILL_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, STILL_OUTPUT_LENGTH, output, b"IBOUT006").await
}

/// Validate only the fixed policy header. Image parsing belongs to the guest.
pub fn validate_source_gif_request(bytes: &[u8]) -> Result<()> {
    if !(17..=MAX_GIF_SOURCE_INPUT as usize).contains(&bytes.len())
        || &bytes[..8] != b"IBGIF007"
        || bytes[8] & !12 != 0
        || bytes[9] != 3
        || bytes[14..16] != [0; 2]
        || !(1..=250).contains(&u16::from_be_bytes([bytes[10], bytes[11]]))
        || !(1..=250).contains(&u16::from_be_bytes([bytes[12], bytes[13]]))
    {
        return Err(Error::Frame);
    }
    Ok(())
}

pub async fn write_source_gif_request<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    validate_source_gif_request(bytes)?;
    write_frame(bytes, bytes.len() as u64, output, b"IBJOB007").await
}

pub async fn read_source_gif_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT007", STILL_OUTPUT_LENGTH, STILL_OUTPUT_LENGTH).await
}

pub async fn write_source_gif_response<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    if bytes.len() as u64 != STILL_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, STILL_OUTPUT_LENGTH, output, b"IBOUT007").await
}

/// The bounded frame begins with a fixed container tag, followed by the upload.
pub async fn write_video_request<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    if !(2..=MAX_INPUT as usize + 1).contains(&bytes.len()) || !matches!(bytes[0], 1 | 2) {
        return Err(Error::Frame);
    }
    write_frame(bytes, bytes.len() as u64, output, b"IBJOB005").await
}

pub async fn write_video_response<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    if bytes.len() as u64 != VIDEO_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, VIDEO_OUTPUT_LENGTH, output, b"IBOUT005").await
}

pub async fn write_pdf_request<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    length: u64,
    output: &mut W,
) -> Result<()> {
    if !(1..=MAX_INPUT).contains(&length) {
        return Err(Error::Frame);
    }
    write_frame(input, length, output, b"IBJOB004").await
}

pub async fn write_pdf_response<W: AsyncWrite + Unpin>(bytes: &[u8], output: &mut W) -> Result<()> {
    if bytes.len() as u64 != PDF_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, PDF_OUTPUT_LENGTH, output, b"IBOUT004").await
}

pub async fn write_gif_request<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    length: u64,
    output: &mut W,
) -> Result<()> {
    if !(1..=MAX_INPUT).contains(&length) {
        return Err(Error::Frame);
    }
    write_frame(input, length, output, b"IBJOB003").await
}

pub async fn write_gif_response<W: AsyncWrite + Unpin>(bytes: &[u8], output: &mut W) -> Result<()> {
    if bytes.len() as u64 != GIF_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, GIF_OUTPUT_LENGTH, output, b"IBOUT003").await
}

async fn read_frame<R: AsyncRead + Unpin>(
    input: &mut R,
    magic: &[u8; 8],
    min: u64,
    max: u64,
) -> Result<Vec<u8>> {
    let (actual_magic, length) = read_header(input).await?;
    if &actual_magic != magic || !(min..=max).contains(&length) {
        return Err(Error::Frame);
    }
    read_body(input, length).await
}

async fn read_header<R: AsyncRead + Unpin>(input: &mut R) -> Result<([u8; 8], u64)> {
    let mut header = [0; 16];
    input
        .read_exact(&mut header)
        .await
        .map_err(|_| Error::Frame)?;
    Ok((
        header[..8].try_into().map_err(|_| Error::Frame)?,
        u64::from_be_bytes(header[8..].try_into().map_err(|_| Error::Frame)?),
    ))
}

async fn read_body<R: AsyncRead + Unpin>(input: &mut R, length: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(length as usize);
    let mut scratch = [0; SCRATCH];
    while bytes.len() < length as usize {
        let size = scratch.len().min(length as usize - bytes.len());
        input
            .read_exact(&mut scratch[..size])
            .await
            .map_err(|_| Error::Frame)?;
        bytes.extend_from_slice(&scratch[..size]);
    }
    require_eof(input).await?;
    Ok(bytes)
}

async fn require_eof<R: AsyncRead + Unpin>(input: &mut R) -> Result<()> {
    let mut extra = [0];
    if input.read(&mut extra).await.map_err(|_| Error::Frame)? != 0 {
        return Err(Error::Frame);
    }
    Ok(())
}

pub async fn write_request<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    length: u64,
    output: &mut W,
) -> Result<()> {
    if !(1..=MAX_INPUT).contains(&length) {
        return Err(Error::Frame);
    }
    write_frame(input, length, output, b"IBJOB001").await
}

pub async fn write_response<W: AsyncWrite + Unpin>(bytes: &[u8], output: &mut W) -> Result<()> {
    if bytes.len() as u64 != OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, OUTPUT_LENGTH, output, b"IBOUT001").await
}

pub async fn write_paired_request<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    length: u64,
    binding: &[u8; 32],
    output: &mut W,
) -> Result<()> {
    if !(MIN_PAIRED_INPUT..=MAX_PAIRED_INPUT).contains(&length) {
        return Err(Error::Frame);
    }
    write_header(output, b"IBJOB002", length).await?;
    output
        .write_all(binding)
        .await
        .map_err(|_| Error::Transport)?;
    write_body(input, length, output).await
}

pub async fn write_paired_response<W: AsyncWrite + Unpin>(
    bytes: &[u8],
    output: &mut W,
) -> Result<()> {
    if bytes.len() as u64 != PAIRED_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, PAIRED_OUTPUT_LENGTH, output, b"IBOUT002").await
}

async fn write_frame<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    length: u64,
    output: &mut W,
    magic: &[u8; 8],
) -> Result<()> {
    write_header(output, magic, length).await?;
    write_body(input, length, output).await
}

async fn write_header<W: AsyncWrite + Unpin>(
    output: &mut W,
    magic: &[u8; 8],
    length: u64,
) -> Result<()> {
    output
        .write_all(magic)
        .await
        .map_err(|_| Error::Transport)?;
    output
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|_| Error::Transport)?;
    Ok(())
}

async fn write_body<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut input: R,
    length: u64,
    output: &mut W,
) -> Result<()> {
    let mut remaining = length;
    let mut scratch = [0; SCRATCH];
    while remaining != 0 {
        let size = remaining.min(SCRATCH as u64) as usize;
        input
            .read_exact(&mut scratch[..size])
            .await
            .map_err(|_| Error::Frame)?;
        output
            .write_all(&scratch[..size])
            .await
            .map_err(|_| Error::Transport)?;
        remaining -= size as u64;
    }
    require_eof(&mut input).await?;
    output.flush().await.map_err(|_| Error::Transport)?;
    output.shutdown().await.map_err(|_| Error::Transport)
}

pub async fn write_swf_request<W: AsyncWrite + Unpin>(bytes: &[u8], output: &mut W) -> Result<()> {
    if !(8..=MAX_SWF_INPUT).contains(&(bytes.len() as u64)) {
        return Err(Error::Frame);
    }
    write_frame(bytes, bytes.len() as u64, output, b"IBJOB008").await
}

pub async fn read_swf_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT008", SWF_OUTPUT_LENGTH, SWF_OUTPUT_LENGTH).await
}

pub async fn write_swf_response<W: AsyncWrite + Unpin>(bytes: &[u8], output: &mut W) -> Result<()> {
    if bytes.len() as u64 != SWF_OUTPUT_LENGTH {
        return Err(Error::Frame);
    }
    write_frame(bytes, SWF_OUTPUT_LENGTH, output, b"IBOUT008").await
}
