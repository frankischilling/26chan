//! Bounded still-image output framing. Encoded files remain untrusted media;
//! this module does not parse images or grant posting or serving authority.
use crate::MediaError;
use md5::{Digest as _, Md5};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

mod board_policy;
pub use board_policy::source_policy;

pub const OUTPUT_DISK_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_EXIF_BYTES: u32 = 16_384;
pub const MAX_IMAGE_BYTES: u32 = 10 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StillFormat {
    Png,
    Jpeg,
    Gif,
}

impl StillFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
        }
    }
}

/// The caller must derive this from authenticated posting context, never from
/// a guest reply. The exact header is echoed by the disposable worker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StillPolicy {
    pub format: StillFormat,
    pub strip_exif: bool,
    /// 0: disabled, 1: cleaned file, 2: before capcode-only cleanup.
    pub exif_mode: u8,
    pub mobile: bool,
    pub thread: bool,
    pub blue_background: bool,
    pub thumbnail: (u16, u16),
}

fn invalid() -> MediaError {
    MediaError::InvalidOutput
}
fn word(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four bytes"))
}

impl StillPolicy {
    pub fn header(self) -> Result<[u8; 16], MediaError> {
        if self.exif_mode > 2
            || (self.exif_mode != 0 && self.format != StillFormat::Jpeg)
            || !(1..=250).contains(&self.thumbnail.0)
            || !(1..=250).contains(&self.thumbnail.1)
            || (self.format == StillFormat::Gif && (self.strip_exif || self.mobile))
        {
            return Err(invalid());
        }
        let mut output = [0; 16];
        output[..8].copy_from_slice(if self.format == StillFormat::Gif {
            b"IBGIF007"
        } else {
            b"IBSTIL06"
        });
        output[8] = u8::from(self.strip_exif)
            | (u8::from(self.mobile) << 1)
            | (u8::from(self.thread) << 2)
            | (u8::from(self.blue_background) << 3);
        output[9] = match self.format {
            StillFormat::Png => 1,
            StillFormat::Jpeg => 2,
            StillFormat::Gif => 3,
        };
        output[10..12].copy_from_slice(&self.thumbnail.0.to_be_bytes());
        output[12..14].copy_from_slice(&self.thumbnail.1.to_be_bytes());
        output[14] = self.exif_mode;
        Ok(output)
    }

    fn thumbnail_dimensions(self, size: (u32, u32)) -> (u32, u32) {
        let scale = (f64::from(self.thumbnail.0) / f64::from(size.0))
            .min(f64::from(self.thumbnail.1) / f64::from(size.1))
            .min(1.0);
        (
            (f64::from(size.0) * scale).floor() as u32,
            (f64::from(size.1) * scale).floor() as u32,
        )
    }

    fn mobile_dimensions(self, size: (u32, u32), retained_bytes: u32) -> (u32, u32) {
        if self.format == StillFormat::Gif
            || !self.mobile
            || (self.format == StillFormat::Png && retained_bytes <= 524_288)
            || (size.0 <= 1024 && size.1 <= 1024)
            || u64::from(size.0) * u64::from(size.1) <= 524_288
        {
            return (0, 0);
        }
        let ratio = f64::from(size.0) / f64::from(size.1);
        if ratio > 1.0 {
            (1024, (1024.0 / ratio).round() as u32)
        } else {
            ((1024.0 * ratio).round() as u32, 1024)
        }
    }
}

pub struct EncodedStillFile {
    bytes: Vec<u8>,
    dimensions: (u32, u32),
    sha256: String,
    md5: String,
}

impl EncodedStillFile {
    fn new(bytes: Vec<u8>, dimensions: (u32, u32)) -> Self {
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        let md5 = Md5::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        Self {
            bytes,
            dimensions,
            sha256,
            md5,
        }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn md5(&self) -> &str {
        &self.md5
    }
}

pub struct ValidatedStill {
    policy: StillPolicy,
    retained: EncodedStillFile,
    thumbnail: EncodedStillFile,
    mobile: Option<EncodedStillFile>,
    exif: Option<String>,
}

fn jpeg_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\xff\xd8\xff") && bytes.ends_with(b"\xff\xd9")
}

impl ValidatedStill {
    /// Read only after the worker and all descendants have stopped. Fixed
    /// signatures catch transport confusion; image validity belongs to the
    /// isolated decoder, and separate-origin serving remains necessary.
    pub async fn read_disk(
        mut input: impl AsyncRead + Unpin,
        expected: StillPolicy,
    ) -> Result<Self, MediaError> {
        let policy_header = expected.header()?;
        let mut header = [0; 64];
        input.read_exact(&mut header).await?;
        let size = (word(&header[24..28]), word(&header[28..32]));
        let thumb_size = (word(&header[32..36]), word(&header[36..40]));
        let mobile_size = (word(&header[40..44]), word(&header[44..48]));
        let retained_length = word(&header[48..52]);
        let thumbnail_length = word(&header[52..56]);
        let mobile_length = word(&header[56..60]);
        let exif_length = word(&header[60..64]);
        if &header[..8]
            != if expected.format == StillFormat::Gif {
                b"IBIMG007"
            } else {
                b"IBIMG006"
            }
            || header[8..24] != policy_header
            || exif_length > MAX_EXIF_BYTES
            || (expected.exif_mode != 2 && exif_length != 0)
            || !(1..=10_000).contains(&size.0)
            || !(1..=10_000).contains(&size.1)
            || !(12..=if expected.format == StillFormat::Gif {
                8 * 1024 * 1024
            } else {
                MAX_IMAGE_BYTES
            })
                .contains(&retained_length)
            || !(5..=1024 * 1024).contains(&thumbnail_length)
            || !(1..=250).contains(&thumb_size.0)
            || !(1..=250).contains(&thumb_size.1)
            || thumb_size != expected.thumbnail_dimensions(size)
            || mobile_size != expected.mobile_dimensions(size, retained_length)
            || if mobile_size == (0, 0) {
                mobile_length != 0
            } else {
                !(1..=1024).contains(&mobile_size.0)
                    || !(1..=1024).contains(&mobile_size.1)
                    || !(5..=4 * 1024 * 1024).contains(&mobile_length)
            }
        {
            return Err(invalid());
        }
        let total = 64
            + u64::from(retained_length)
            + u64::from(thumbnail_length)
            + u64::from(mobile_length)
            + u64::from(exif_length);
        if total > OUTPUT_DISK_BYTES {
            return Err(invalid());
        }
        let mut retained = vec![0; retained_length as usize];
        let mut thumbnail = vec![0; thumbnail_length as usize];
        let mut mobile = vec![0; mobile_length as usize];
        input.read_exact(&mut retained).await?;
        input.read_exact(&mut thumbnail).await?;
        input.read_exact(&mut mobile).await?;
        let mut exif = vec![0; exif_length as usize];
        input.read_exact(&mut exif).await?;
        let exif = String::from_utf8(exif).map_err(|_| invalid())?;
        if exif.contains('\0') || exif.split('\n').count() > 256 {
            return Err(invalid());
        }
        if !jpeg_signature(&thumbnail)
            || (!mobile.is_empty() && !jpeg_signature(&mobile))
            || match expected.format {
                StillFormat::Png => !retained.starts_with(b"\x89PNG\r\n\x1a\n"),
                StillFormat::Jpeg => !jpeg_signature(&retained),
                StillFormat::Gif => {
                    !(retained.starts_with(b"GIF87a") || retained.starts_with(b"GIF89a"))
                        || !retained.ends_with(b";")
                }
            }
        {
            return Err(invalid());
        }
        let mut remaining = OUTPUT_DISK_BYTES - total;
        let mut scratch = [0; 8192];
        while remaining > 0 {
            let count = remaining.min(scratch.len() as u64) as usize;
            input.read_exact(&mut scratch[..count]).await?;
            if scratch[..count].iter().any(|&v| v != 0) {
                return Err(invalid());
            }
            remaining -= count as u64;
        }
        if input.read(&mut [0; 1]).await? != 0 {
            return Err(invalid());
        }
        Ok(Self {
            policy: expected,
            exif: (expected.exif_mode != 0).then_some(exif),
            retained: EncodedStillFile::new(retained, size),
            thumbnail: EncodedStillFile::new(thumbnail, thumb_size),
            mobile: (!mobile.is_empty()).then(|| EncodedStillFile::new(mobile, mobile_size)),
        })
    }
    /// Bounded untrusted text. This grants no HTML or posting authority.
    pub fn exif(&self) -> Option<&str> {
        self.exif.as_deref()
    }
    pub fn policy(&self) -> StillPolicy {
        self.policy
    }
    pub fn retained(&self) -> &EncodedStillFile {
        &self.retained
    }
    pub fn thumbnail(&self) -> &EncodedStillFile {
        &self.thumbnail
    }
    pub fn mobile(&self) -> Option<&EncodedStillFile> {
        self.mobile.as_ref()
    }
}
