use crate::MediaError;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId([u8; 16]);

impl ObjectId {
    /// Generate an opaque identifier from operating system randomness.
    pub fn generate() -> Result<Self, MediaError> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| MediaError::Random)?;
        Ok(Self(bytes))
    }
}

impl FromStr for ObjectId {
    type Err = MediaError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(MediaError::InvalidId);
        }
        let mut bytes = [0; 16];
        for (byte, pair) in bytes.iter_mut().zip(value.as_bytes().chunks_exact(2)) {
            let digit = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b - b'a' + 10
                }
            };
            *byte = digit(pair[0]) * 16 + digit(pair[1]);
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
