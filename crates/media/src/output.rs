use crate::{MAX_DIMENSION, MAX_PNG_BYTES, MediaError, OUTPUT_DISK_BYTES};
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use tokio::io::{AsyncRead, AsyncReadExt};

const PROTOCOL_HEADER_BYTES: u64 = 16;

/// Pixels accepted only through the fixed IBRGBA01 protocol. The fields and PNG
/// encoder are private so callers cannot construct an unvalidated output.
#[derive(Debug)]
pub struct ValidatedOutput {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// An encoder-produced PNG, never arbitrary worker bytes. Metadata is computed
/// on the host from validated dimensions and the exact encoded bytes.
pub struct EncodedOutput {
    pub(crate) bytes: Vec<u8>,
    sha256: String,
    dimensions: (u32, u32),
}

impl EncodedOutput {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }
}

impl ValidatedOutput {
    /// Read a 16-byte header, exactly width * height * 4 bytes, and EOF.
    /// Callers must impose their job deadline on streams that may never end.
    pub async fn read<R: AsyncRead + Unpin>(mut reader: R) -> Result<Self, MediaError> {
        let output = Self::read_protocol(&mut reader).await?;
        if reader.read(&mut [0; 1]).await? != 0 {
            return Err(MediaError::InvalidOutput);
        }
        Ok(output)
    }

    /// Read the fixed output block format after guest termination.
    pub async fn read_disk<R: AsyncRead + Unpin>(mut reader: R) -> Result<Self, MediaError> {
        let output = Self::read_protocol(&mut reader).await?;
        let used = PROTOCOL_HEADER_BYTES + output.pixels.len() as u64;
        let mut remaining = OUTPUT_DISK_BYTES
            .checked_sub(used)
            .ok_or(MediaError::InvalidOutput)?;
        let mut padding = [0; 8192];
        while remaining != 0 {
            let limit = usize::try_from(remaining.min(padding.len() as u64))
                .expect("padding read limit fits in usize");
            reader.read_exact(&mut padding[..limit]).await?;
            if padding[..limit].iter().any(|byte| *byte != 0) {
                return Err(MediaError::InvalidOutput);
            }
            remaining -= limit as u64;
        }
        if reader.read(&mut [0; 1]).await? != 0 {
            return Err(MediaError::InvalidOutput);
        }
        Ok(output)
    }

    async fn read_protocol<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Self, MediaError> {
        let mut header = [0; 16];
        reader.read_exact(&mut header).await?;
        let width = u32::from_be_bytes(header[8..12].try_into().expect("four width bytes"));
        let height = u32::from_be_bytes(header[12..16].try_into().expect("four height bytes"));
        if &header[..8] != b"IBRGBA01"
            || !(1..=MAX_DIMENSION).contains(&width)
            || !(1..=MAX_DIMENSION).contains(&height)
        {
            return Err(MediaError::InvalidOutput);
        }
        let size = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or(MediaError::InvalidOutput)?;
        let mut pixels = vec![0; size];
        // Even a very large validated image is read in fixed-size chunks.
        for chunk in pixels.chunks_mut(8192) {
            reader.read_exact(chunk).await?;
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn encode(&self) -> Result<EncodedOutput, MediaError> {
        let bytes = self.encode_png()?;
        Ok(EncodedOutput {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes,
            dimensions: self.dimensions(),
        })
    }

    pub(crate) fn encode_png(&self) -> Result<Vec<u8>, MediaError> {
        let mut bytes = BoundedPng(Vec::new());
        {
            let mut encoder = png::Encoder::new(&mut bytes, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&self.pixels)?;
            writer.finish()?;
        }
        Ok(bytes.0)
    }
}

struct BoundedPng(Vec<u8>);

impl Write for BoundedPng {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let size = self
            .0
            .len()
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_PNG_BYTES)
            .ok_or_else(|| io::Error::other("encoded PNG exceeds byte limit"))?;
        self.0.reserve_exact(size - self.0.len());
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_sink_enforces_five_mib_before_extending_buffer() {
        let mut sink = BoundedPng(Vec::new());
        for _ in 0..1280 {
            sink.write_all(&[0; 4096]).unwrap();
        }
        assert_eq!(sink.0.len(), 5 * 1024 * 1024);
        let capacity = sink.0.capacity();
        assert!(sink.write_all(&[1]).is_err());
        assert_eq!(sink.0.len(), 5 * 1024 * 1024);
        assert_eq!(sink.0.capacity(), capacity);
    }
}
