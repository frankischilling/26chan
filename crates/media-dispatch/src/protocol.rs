//! One frame per connection. Callers apply an absolute deadline to the whole exchange.
use crate::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_INPUT: u64 = 8_388_608;
pub const OUTPUT_LENGTH: u64 = 4_194_816;
const SCRATCH: usize = 16_384;

pub async fn read_request<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBJOB001", 1, MAX_INPUT).await
}

pub async fn read_response<R: AsyncRead + Unpin>(input: &mut R) -> Result<Vec<u8>> {
    read_frame(input, b"IBOUT001", OUTPUT_LENGTH, OUTPUT_LENGTH).await
}

async fn read_frame<R: AsyncRead + Unpin>(
    input: &mut R,
    magic: &[u8; 8],
    min: u64,
    max: u64,
) -> Result<Vec<u8>> {
    let mut header = [0; 16];
    input
        .read_exact(&mut header)
        .await
        .map_err(|_| Error::Frame)?;
    let length = u64::from_be_bytes(header[8..].try_into().map_err(|_| Error::Frame)?);
    if &header[..8] != magic || !(min..=max).contains(&length) {
        return Err(Error::Frame);
    }
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

async fn write_frame<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut input: R,
    length: u64,
    output: &mut W,
    magic: &[u8; 8],
) -> Result<()> {
    output
        .write_all(magic)
        .await
        .map_err(|_| Error::Transport)?;
    output
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|_| Error::Transport)?;
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
