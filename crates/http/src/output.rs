use std::{
    fmt, io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::Body;
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const BLOCK_BYTES: usize = 4096;

/// Errors returned while configuring or building a bounded encoded response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputError {
    InvalidCapacity,
    OutputLimitExceeded,
    BudgetExhausted,
}

impl fmt::Display for OutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCapacity => "response budget capacity is outside the supported range",
            Self::OutputLimitExceeded => "response exceeds its encoded byte limit",
            Self::BudgetExhausted => "aggregate response buffer budget is exhausted",
        })
    }
}

impl std::error::Error for OutputError {}

/// A process-local pool shared by dynamic response encoders.
///
/// Encoded output is charged in 4096-byte blocks. A short final block therefore
/// conservatively occupies one full block until its final owner is released.
#[derive(Clone)]
pub struct ResponseBudget {
    permits: Arc<Semaphore>,
}

impl ResponseBudget {
    pub fn new(capacity: usize) -> Result<Self, OutputError> {
        let maximum = maximum_capacity_bytes();
        if !(BLOCK_BYTES..=maximum).contains(&capacity) {
            return Err(OutputError::InvalidCapacity);
        }
        let blocks = capacity / BLOCK_BYTES;
        Ok(Self {
            permits: Arc::new(Semaphore::new(blocks)),
        })
    }

    pub fn writer(&self, limit: usize) -> ResponseWriter {
        ResponseWriter {
            budget: self.clone(),
            limit,
            len: 0,
            blocks: Vec::new(),
            error: None,
        }
    }

    pub fn available_bytes(&self) -> usize {
        self.permits.available_permits() * BLOCK_BYTES
    }
}

fn maximum_capacity_bytes() -> usize {
    usize::try_from(u32::MAX)
        .unwrap_or(usize::MAX)
        .min(Semaphore::MAX_PERMITS.saturating_mul(BLOCK_BYTES))
}

/// A non-waiting writer for one encoded response.
pub struct ResponseWriter {
    budget: ResponseBudget,
    limit: usize,
    len: usize,
    blocks: Vec<ChargedBlock>,
    error: Option<OutputError>,
}

impl ResponseWriter {
    pub fn finish(self) -> Result<EncodedResponse, OutputError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(EncodedResponse {
            len: self.len,
            blocks: self.blocks,
        })
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), OutputError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if bytes.is_empty() {
            return Ok(());
        }

        let Some(new_len) = self.len.checked_add(bytes.len()) else {
            return self.fail(OutputError::OutputLimitExceeded);
        };
        if new_len > self.limit {
            return self.fail(OutputError::OutputLimitExceeded);
        }

        let tail_space = self
            .blocks
            .last()
            .map_or(0, |block| BLOCK_BYTES - block.len);
        let remaining = bytes.len().saturating_sub(tail_space);
        let new_blocks = remaining.div_ceil(BLOCK_BYTES);

        let mut acquired = if new_blocks == 0 {
            None
        } else {
            let Ok(charge) = u32::try_from(new_blocks) else {
                return self.fail(OutputError::BudgetExhausted);
            };
            match self.budget.permits.clone().try_acquire_many_owned(charge) {
                Ok(permit) => Some(permit),
                Err(_) => return self.fail(OutputError::BudgetExhausted),
            }
        };

        // Acquire every required block before allocating any payload storage. A
        // failed aggregate admission therefore cannot allocate an uncharged block.
        let mut additions = Vec::with_capacity(new_blocks);
        for _ in 0..new_blocks {
            let permit = acquired
                .as_mut()
                .and_then(|permit| permit.split(1))
                .expect("one permit was acquired for every pending block");
            additions.push(ChargedBlock {
                bytes: Box::new([0; BLOCK_BYTES]),
                len: 0,
                _permit: permit,
            });
        }
        drop(acquired);

        self.blocks.reserve(additions.len());
        let mut input = bytes;
        if let Some(block) = self.blocks.last_mut() {
            let copied = block.extend(input);
            input = &input[copied..];
        }
        for mut block in additions {
            let copied = block.extend(input);
            input = &input[copied..];
            self.blocks.push(block);
        }
        debug_assert!(input.is_empty());
        self.len = new_len;
        Ok(())
    }

    fn fail<T>(&mut self, error: OutputError) -> Result<T, OutputError> {
        self.error = Some(error);
        Err(error)
    }
}

impl io::Write for ResponseWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.append(bytes)
            .map(|()| bytes.len())
            .map_err(io::Error::other)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.append(bytes).map_err(io::Error::other)
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(error) = self.error {
            Err(io::Error::other(error))
        } else {
            Ok(())
        }
    }
}

impl fmt::Write for ResponseWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.append(value.as_bytes()).map_err(|_| fmt::Error)
    }
}

struct ChargedBlock {
    bytes: Box<[u8; BLOCK_BYTES]>,
    len: usize,
    _permit: OwnedSemaphorePermit,
}

impl ChargedBlock {
    fn extend(&mut self, input: &[u8]) -> usize {
        let count = input.len().min(BLOCK_BYTES - self.len);
        self.bytes[self.len..self.len + count].copy_from_slice(&input[..count]);
        self.len += count;
        count
    }
}

impl AsRef<[u8]> for ChargedBlock {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Fully encoded response bytes whose aggregate charge follows their ownership.
pub struct EncodedResponse {
    len: usize,
    blocks: Vec<ChargedBlock>,
}

impl EncodedResponse {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn chunks(&self) -> impl Iterator<Item = &[u8]> {
        self.blocks.iter().map(AsRef::as_ref)
    }

    pub fn into_body(self) -> Body {
        Body::new(EncodedBody {
            remaining: self.len,
            blocks: self.blocks.into_iter(),
        })
    }
}

struct EncodedBody {
    remaining: usize,
    blocks: std::vec::IntoIter<ChargedBlock>,
}

impl HttpBody for EncodedBody {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let Some(block) = self.blocks.next() else {
            self.remaining = 0;
            return Poll::Ready(None);
        };
        self.remaining = self.remaining.saturating_sub(block.len);
        Poll::Ready(Some(Ok(Frame::data(Bytes::from_owner(block)))))
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining as u64)
    }
}

#[cfg(test)]
mod tests;
