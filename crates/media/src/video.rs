//! Bounded stopped-disk envelope for guest-remuxed video and neutral preview
//! pixels. Compressed bytes remain untrusted media for a separate origin. This
//! module never parses a container or invokes a codec.
use crate::{MediaError, ValidatedOutput};
use md5::{Digest as _, Md5};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const OUTPUT_DISK_BYTES: u64 = 9 * 1024 * 1024;
pub const MAX_VIDEO_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoFormat {
    Webm,
    Mp4,
}

impl VideoFormat {
    pub fn tag(self) -> u8 {
        match self {
            Self::Webm => 1,
            Self::Mp4 => 2,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
        }
    }
}

pub struct ValidatedVideo {
    format: VideoFormat,
    bytes: Vec<u8>,
    preview: ValidatedOutput,
    width: u32,
    height: u32,
    duration_micros: u64,
    has_audio: bool,
    sample_aspect_ratio: Option<f64>,
    sha256: String,
    md5: String,
}

fn invalid() -> MediaError {
    MediaError::InvalidOutput
}
fn word(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four bytes"))
}
fn long(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("eight bytes"))
}

impl ValidatedVideo {
    /// The requested kind binds the typed transport to this output. A successful
    /// call validates framing and resource limits; it does not trust executable
    /// content or establish posting/serving authority.
    pub async fn read_disk(
        mut input: impl AsyncRead + Unpin,
        expected: VideoFormat,
    ) -> Result<Self, MediaError> {
        let mut header = [0; 64];
        input.read_exact(&mut header).await?;
        let width = word(&header[16..20]);
        let height = word(&header[20..24]);
        let duration_micros = long(&header[24..32]);
        let sar = f64::from_be_bytes(header[32..40].try_into().expect("eight bytes"));
        let thumb_width = word(&header[40..44]);
        let thumb_height = word(&header[44..48]);
        let length = long(&header[48..56]);
        if &header[..8] != b"IBVID005"
            || header[8] != expected.tag()
            || header[9] > 1
            || header[10..16].iter().any(|&v| v != 0)
            || header[56..].iter().any(|&v| v != 0)
            || !(1..=2048).contains(&width)
            || !(1..=2048).contains(&height)
            || !(1..=400_000_000).contains(&duration_micros)
            || !sar.is_finite()
            || !(sar == 0.0 || (sar > 0.5 && sar < 2.0 && sar != 1.0))
            || !(1..=250).contains(&thumb_width)
            || !(1..=250).contains(&thumb_height)
            || !(12..=MAX_VIDEO_BYTES).contains(&length)
        {
            return Err(invalid());
        }
        let mut bytes = vec![0; length as usize];
        input.read_exact(&mut bytes).await?;
        // These fixed signatures only catch transport/format confusion. All
        // container structure and stream parsing remain in the isolated guest.
        if match expected {
            VideoFormat::Webm => !bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
            VideoFormat::Mp4 => bytes.get(4..8) != Some(b"ftyp"),
        } {
            return Err(invalid());
        }
        let mut pixels = vec![0; thumb_width as usize * thumb_height as usize * 4];
        input.read_exact(&mut pixels).await?;
        let mut remaining = OUTPUT_DISK_BYTES - 64 - length - pixels.len() as u64;
        let mut scratch = [0; 8192];
        while remaining > 0 {
            let count = remaining.min(scratch.len() as u64) as usize;
            input.read_exact(&mut scratch[..count]).await?;
            if scratch[..count].iter().any(|&byte| byte != 0) {
                return Err(invalid());
            }
            remaining -= count as u64;
        }
        if input.read(&mut [0; 1]).await? != 0 {
            return Err(invalid());
        }
        let preview = ValidatedOutput::from_validated_pixels((thumb_width, thumb_height), pixels)?;
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        let md5 = Md5::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        Ok(Self {
            format: expected,
            bytes,
            preview,
            width,
            height,
            duration_micros,
            has_audio: header[9] == 1,
            sample_aspect_ratio: (sar != 0.0).then_some(sar),
            sha256,
            md5,
        })
    }

    pub fn format(&self) -> VideoFormat {
        self.format
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub fn preview(&self) -> &ValidatedOutput {
        &self.preview
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn duration_micros(&self) -> u64 {
        self.duration_micros
    }
    pub fn has_audio(&self) -> bool {
        self.has_audio
    }
    pub fn sample_aspect_ratio(&self) -> Option<f64> {
        self.sample_aspect_ratio
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn md5(&self) -> &str {
        &self.md5
    }
}
