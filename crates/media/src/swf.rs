//! Fixed SWF guest-output framing. No SWF tags, compressed bytes or actions are
//! parsed here. Successful validation grants no posting or serving authority.
use crate::MediaError;
use md5::{Digest as _, Md5};
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAX_INPUT_BYTES: u64 = 10 * 1024 * 1024;
pub const OUTPUT_DISK_BYTES: u64 = 11 * 1024 * 1024;

pub struct ValidatedSwf {
    bytes: Vec<u8>,
    dimensions: (u32, u32),
    sha256: String,
    md5: String,
}

impl ValidatedSwf {
    pub async fn read_disk(mut input: impl AsyncRead + Unpin) -> Result<Self, MediaError> {
        let mut header = [0; 32];
        input.read_exact(&mut header).await?;
        let width = u32::from_be_bytes(header[8..12].try_into().expect("four bytes"));
        let height = u32::from_be_bytes(header[12..16].try_into().expect("four bytes"));
        let length = u64::from_be_bytes(header[16..24].try_into().expect("eight bytes"));
        if &header[..8] != b"IBSWF008"
            || width == 0
            || height == 0
            || !(8..=MAX_INPUT_BYTES).contains(&length)
            || header[24] == 0
            || header[25] > 1
            || header[30..32] != [0, 0]
        {
            return Err(MediaError::InvalidOutput);
        }
        let mut bytes = vec![0; length as usize];
        input.read_exact(&mut bytes).await?;
        let expected = if header[25] == 1 { b"CWS" } else { b"FWS" };
        if &bytes[..3] != expected || bytes[3] != header[24] || (header[25] == 1 && header[24] < 6)
        {
            return Err(MediaError::InvalidOutput);
        }
        let mut remaining = OUTPUT_DISK_BYTES - 32 - length;
        let mut scratch = [0; 8192];
        while remaining > 0 {
            let count = remaining.min(scratch.len() as u64) as usize;
            input.read_exact(&mut scratch[..count]).await?;
            if scratch[..count].iter().any(|&byte| byte != 0) {
                return Err(MediaError::InvalidOutput);
            }
            remaining -= count as u64;
        }
        if input.read(&mut [0]).await? != 0 {
            return Err(MediaError::InvalidOutput);
        }
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        let md5 = Md5::digest(&bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        Ok(Self {
            bytes,
            dimensions: (width, height),
            sha256,
            md5,
        })
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
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}
