use crate::{MAX_DIMENSION, MAX_PNG_BYTES, MediaError, OUTPUT_DISK_BYTES};
use md5::{Digest as _, Md5};
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
    md5: String,
    dimensions: (u32, u32),
}

impl EncodedOutput {
    /// Legacy interoperability only; never an integrity or authorization check.
    pub fn md5(&self) -> &str {
        &self.md5
    }
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
            md5: Md5::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            bytes,
            dimensions: self.dimensions(),
        })
    }

    /// Resize already validated RGBA pixels, without decoding an image again.
    /// Integer area averaging preserves alpha through premultiplied channels.
    pub fn thumbnail(&self) -> Result<EncodedOutput, MediaError> {
        let longest = self.width.max(self.height);
        if longest <= 250 {
            return self.encode();
        }
        let width = (self.width * 250 / longest).max(1);
        let height = (self.height * 250 / longest).max(1);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            let top = y * self.height / height;
            let bottom = ((y + 1) * self.height / height).max(top + 1);
            for x in 0..width {
                let left = x * self.width / width;
                let right = ((x + 1) * self.width / width).max(left + 1);
                let mut rgba = [0u64; 4];
                for sy in top..bottom {
                    for sx in left..right {
                        let offset = ((sy * self.width + sx) * 4) as usize;
                        let alpha = u64::from(self.pixels[offset + 3]);
                        for (channel, sum) in rgba[..3].iter_mut().enumerate() {
                            *sum += u64::from(self.pixels[offset + channel]) * alpha;
                        }
                        rgba[3] += alpha;
                    }
                }
                for value in &rgba[..3] {
                    pixels.push(if rgba[3] == 0 {
                        0
                    } else {
                        (value / rgba[3]) as u8
                    });
                }
                pixels.push((rgba[3] / u64::from((right - left) * (bottom - top))) as u8);
            }
        }
        Self {
            width,
            height,
            pixels,
        }
        .encode()
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

    fn decoded(output: &EncodedOutput) -> Vec<u8> {
        let mut reader = png::Decoder::new(std::io::Cursor::new(&output.bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), output.dimensions());
        assert_eq!(info.color_type, png::ColorType::Rgba);
        pixels.truncate(info.buffer_size());
        pixels
    }

    #[test]
    fn thumbnail_preserves_small_images_and_bounds_extreme_aspect_ratios() {
        for (width, height, expected) in [
            (1, 1, (1, 1)),
            (250, 249, (250, 249)),
            (500, 300, (250, 150)),
            (300, 500, (150, 250)),
            (MAX_DIMENSION, 1, (250, 1)),
            (1, MAX_DIMENSION, (1, 250)),
        ] {
            let pixels = [24, 87, 150, 255].repeat((width * height) as usize);
            let output = ValidatedOutput {
                width,
                height,
                pixels,
            };
            let thumbnail = output.thumbnail().unwrap();
            assert_eq!(thumbnail.dimensions(), expected);
            assert!(thumbnail.len() <= MAX_PNG_BYTES as u64);
            assert_eq!(
                decoded(&thumbnail),
                [24, 87, 150, 255].repeat((expected.0 * expected.1) as usize)
            );
            if width.max(height) <= 250 {
                assert_eq!(thumbnail.bytes, output.encode().unwrap().bytes);
            }
        }
    }

    #[test]
    fn thumbnail_averages_premultiplied_alpha_without_transparent_color_bleed() {
        for (pair, expected) in [
            ([255, 0, 0, 255, 0, 0, 255, 0], [255, 0, 0, 127]),
            ([255, 0, 0, 255, 0, 0, 255, 255], [127, 0, 127, 255]),
            ([255, 0, 0, 0, 0, 0, 255, 0], [0, 0, 0, 0]),
        ] {
            let output = ValidatedOutput {
                width: 500,
                height: 1,
                pixels: pair.repeat(250),
            };
            assert_eq!(decoded(&output.thumbnail().unwrap()), expected.repeat(250));
        }
    }

    #[test]
    fn legacy_checksum_describes_encoded_bytes_not_pixel_input() {
        let output = ValidatedOutput {
            width: 1,
            height: 1,
            pixels: vec![255, 0, 0, 255],
        }
        .encode()
        .unwrap();
        assert_eq!(output.md5().len(), 32);
        let expected: String = Md5::digest(&output.bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(output.md5(), expected);
        assert_eq!(
            output.sha256(),
            format!("{:x}", Sha256::digest(&output.bytes))
        );
    }

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
