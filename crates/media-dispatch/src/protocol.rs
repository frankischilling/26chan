//! One frame per connection. Callers apply an absolute deadline to the whole exchange.
use crate::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_INPUT: u64 = 8_388_608;
pub const OUTPUT_LENGTH: u64 = 4_194_816;
pub const MIN_PAIRED_INPUT: u64 = 57;
pub const MAX_PAIRED_INPUT: u64 = 16_777_272;
pub const PAIRED_OUTPUT_LENGTH: u64 = 4_456_960;
pub const GIF_OUTPUT_LENGTH: u64 = 17_825_792;

/// Transport framing only: the binding and completed pair remain opaque here.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    ImageV1(Vec<u8>),
    GifV3(Vec<u8>),
    PairedV2 { binding: [u8; 32], input: Vec<u8> },
}

/// Select solely from the explicit outer request version, never payload sniffing.
pub async fn read_versioned_request<R: AsyncRead + Unpin>(input: &mut R) -> Result<Request> {
    let (magic, length) = read_header(input).await?;
    match &magic {
        b"IBJOB001" if (1..=MAX_INPUT).contains(&length) => {
            Ok(Request::ImageV1(read_body(input, length).await?))
        }
        b"IBJOB003" if (1..=MAX_INPUT).contains(&length) => {
            Ok(Request::GifV3(read_body(input, length).await?))
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
