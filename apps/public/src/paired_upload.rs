//! Inactive drawing-upload transport. Production routers do not call this module.
//!
//! Ordered ordinary FormData fields: resto, png_bytes, replay_bytes, upfile,
//! optional replay. Filenames are tegaki.png and tegaki.tgkr. Length declarations
//! bound streaming; intake derives hashes from actual received bytes. Only an
//! exact final boundary and actual body EOF authorize the completion marker.
//!
//! This deliberately does not use multer: its terminal-boundary state can return
//! None before HTTP EOF, and its buffer drains all immediately-ready chunks.
use board_media::{
    ObjectId,
    paired::{INPUT_COMPLETION, InputHeader},
};
use bytes::Bytes;
use futures_util::StreamExt;
use std::{io, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::mpsc,
    time::{Instant, timeout_at},
};

const CHUNK: usize = 16_384;
const HEADER_LIMIT: usize = 1024;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid paired multipart")
}

/// A strict, bounded multipart subset produced by browser FormData. There are no
/// per-part Content-Length headers, no preamble, and no multipart epilogue.
pub struct PairedMultipart<R> {
    reader: R,
    boundary: Vec<u8>,
    deadline: Instant,
    declaration: Option<Declaration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declaration {
    pub resto: i64,
    pub image_bytes: u64,
    pub replay_bytes: Option<u64>,
}

impl<R: AsyncRead + Unpin> PairedMultipart<R> {
    pub fn new(reader: R, boundary: &str) -> io::Result<Self> {
        // A deliberately small RFC-compatible subset used by browser FormData.
        if boundary.is_empty()
            || boundary.len() > 70
            || !boundary
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"'()+_,-./:=?".contains(&b))
        {
            return Err(invalid());
        }
        Ok(Self {
            reader,
            boundary: boundary.as_bytes().to_vec(),
            deadline: Instant::now() + Duration::from_secs(10),
            declaration: None,
        })
    }

    pub async fn read_declaration(&mut self) -> io::Result<Declaration> {
        let deadline = self.deadline;
        timeout_at(deadline, self.declaration_inner())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "paired upload expired"))?
    }

    async fn declaration_inner(&mut self) -> io::Result<Declaration> {
        if self.declaration.is_some() {
            return Err(invalid());
        }
        let mut first = b"--".to_vec();
        first.extend_from_slice(&self.boundary);
        first.extend_from_slice(b"\r\n");
        self.expect(&first).await?;
        let resto = self.number("resto").await?;
        let image_bytes = self.number("png_bytes").await?;
        let replay_bytes = self.number("replay_bytes").await?;
        let replay_bytes = (replay_bytes != 0).then_some(replay_bytes);
        InputHeader::new([0; 16], image_bytes, replay_bytes).map_err(|_| invalid())?;
        let declaration = Declaration {
            resto: i64::try_from(resto).map_err(|_| invalid())?,
            image_bytes,
            replay_bytes,
        };
        self.declaration = Some(declaration);
        Ok(declaration)
    }

    /// The caller supplies a one-slot channel and drives its receiving upload
    /// concurrently. Errors/drop close without a marker; intake cannot finalize.
    pub async fn forward(
        mut self,
        job: ObjectId,
        sender: mpsc::Sender<Result<Bytes, io::Error>>,
    ) -> io::Result<()> {
        let deadline = self.deadline;
        timeout_at(deadline, self.forward_inner(job, sender))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "paired upload expired"))?
    }

    /// Connect the bounded forwarder to the fixed-endpoint intake client. Board,
    /// thread, and form checks belong before reservation in a future route. The
    /// reservation remains a bearer; this helper invents no actor binding.
    pub async fn upload(
        self,
        client: &board_intake_client::IntakeClient,
        reservation: &board_intake_client::Reservation,
    ) -> io::Result<()> {
        let job = reservation.id.parse().map_err(|_| invalid())?;
        let (sender, receiver) = mpsc::channel(1);
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|chunk| (chunk, receiver))
        });
        let remote = async {
            client
                .upload_pair(
                    &reservation.id,
                    &reservation.capability,
                    axum::body::Body::from_stream(stream),
                )
                .await
                .map_err(|_| io::Error::other("paired intake unavailable"))
        };
        tokio::try_join!(remote, self.forward(job, sender))?;
        Ok(())
    }

    async fn forward_inner(
        &mut self,
        job: ObjectId,
        sender: mpsc::Sender<Result<Bytes, io::Error>>,
    ) -> io::Result<()> {
        let d = self.declaration.take().ok_or_else(invalid)?;
        let header =
            InputHeader::new(job.bytes(), d.image_bytes, d.replay_bytes).map_err(|_| invalid())?;
        self.headers("upfile", Some("tegaki.png")).await?;
        send(&sender, header.bytes()).await?;
        self.component(d.image_bytes, &sender).await?;
        self.delimiter().await?;
        if let Some(length) = d.replay_bytes {
            self.expect(b"\r\n").await?;
            self.headers("replay", Some("tegaki.tgkr")).await?;
            self.component(length, &sender).await?;
            self.delimiter().await?;
        }
        self.expect(b"--").await?;
        // A final CRLF is normal browser FormData syntax. Nothing else, including
        // whitespace epilogues, is accepted. Probe the real reader to EOF.
        let mut byte = [0];
        if self.reader.read(&mut byte).await? != 0 {
            if byte != [b'\r'] {
                return Err(invalid());
            }
            self.expect(b"\n").await?;
            if self.reader.read(&mut byte).await? != 0 {
                return Err(invalid());
            }
        }
        send(&sender, INPUT_COMPLETION).await
    }

    async fn number(&mut self, name: &str) -> io::Result<u64> {
        self.headers(name, None).await?;
        let mut value = Vec::with_capacity(20);
        loop {
            let byte = self.reader.read_u8().await?;
            if byte == b'\r' {
                self.expect(b"\n").await?;
                break;
            }
            if !byte.is_ascii_digit() || value.len() == 20 {
                return Err(invalid());
            }
            value.push(byte);
        }
        if value.is_empty() || (value.len() > 1 && value[0] == b'0') {
            return Err(invalid());
        }
        let number = std::str::from_utf8(&value)
            .map_err(|_| invalid())?
            .parse()
            .map_err(|_| invalid())?;
        let mut boundary = b"--".to_vec();
        boundary.extend_from_slice(&self.boundary);
        boundary.extend_from_slice(b"\r\n");
        self.expect(&boundary).await?;
        Ok(number)
    }

    async fn headers(&mut self, name: &str, filename: Option<&str>) -> io::Result<()> {
        let mut headers = Vec::with_capacity(128);
        loop {
            if headers.len() == HEADER_LIMIT {
                return Err(invalid());
            }
            headers.push(self.reader.read_u8().await?);
            if headers.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let text = std::str::from_utf8(&headers[..headers.len() - 4]).map_err(|_| invalid())?;
        let disposition = match filename {
            Some(filename) => format!("form-data; name=\"{name}\"; filename=\"{filename}\""),
            None => format!("form-data; name=\"{name}\""),
        };
        let mut saw_disposition = false;
        let mut saw_type = false;
        for line in text.split("\r\n") {
            let (key, value) = line.split_once(':').ok_or_else(invalid)?;
            let value = value.trim_start_matches(' ');
            if key.eq_ignore_ascii_case("content-disposition")
                && !saw_disposition
                && value == disposition
            {
                saw_disposition = true;
            } else if filename.is_some()
                && key.eq_ignore_ascii_case("content-type")
                && !saw_type
                && matches!(value, "image/png" | "application/octet-stream")
            {
                saw_type = true;
            } else {
                return Err(invalid());
            }
        }
        if !saw_disposition {
            return Err(invalid());
        }
        Ok(())
    }

    async fn component(
        &mut self,
        mut remaining: u64,
        sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    ) -> io::Result<()> {
        let mut buffer = [0; CHUNK];
        while remaining != 0 {
            let limit = remaining.min(CHUNK as u64) as usize;
            let n = self.reader.read(&mut buffer[..limit]).await?;
            if n == 0 {
                return Err(invalid());
            }
            send(sender, &buffer[..n]).await?;
            remaining -= n as u64;
        }
        Ok(())
    }

    async fn delimiter(&mut self) -> io::Result<()> {
        let mut delimiter = b"\r\n--".to_vec();
        delimiter.extend_from_slice(&self.boundary);
        self.expect(&delimiter).await
    }

    async fn expect(&mut self, expected: &[u8]) -> io::Result<()> {
        // All expected values are fixed delimiters under 76 bytes.
        for expected in expected {
            if self.reader.read_u8().await? != *expected {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

async fn send(sender: &mpsc::Sender<Result<Bytes, io::Error>>, bytes: &[u8]) -> io::Result<()> {
    sender
        .send(Ok(Bytes::copy_from_slice(bytes)))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "paired intake closed"))
}

/// Build from the entire framework request body, never a length-limited component
/// reader. The stream is pulled on demand; it is not drained into a whole-body
/// buffer. StreamReader retains only the current HTTP data frame in addition to
/// the parser's fixed chunk and bounded headers.
pub fn from_request(
    request: axum::extract::Request,
) -> io::Result<PairedMultipart<impl AsyncRead + Unpin>> {
    let mut types = request.headers().get_all("content-type").iter();
    let content_type = types
        .next()
        .ok_or_else(invalid)?
        .to_str()
        .map_err(|_| invalid())?;
    if types.next().is_some() || request.headers().contains_key("content-encoding") {
        return Err(invalid());
    }
    let boundary = content_type
        .strip_prefix("multipart/form-data; boundary=")
        .ok_or_else(invalid)?;
    let boundary = boundary
        .strip_prefix('"')
        .and_then(|b| b.strip_suffix('"'))
        .unwrap_or(boundary)
        .to_owned();
    // Aggregate framing overhead is independently bounded by five 1024-byte
    // header blocks, three 20-byte numbers, and seven short delimiters. Reject
    // oversized HTTP frames before retaining them in StreamReader.
    const MAX_HTTP_BYTES: usize = 16_777_272 + 8192;
    let mut count = 0usize;
    let stream = request.into_body().into_data_stream().map(move |chunk| {
        let chunk = chunk.map_err(io::Error::other)?;
        count = count
            .checked_add(chunk.len())
            .filter(|n| *n <= MAX_HTTP_BYTES)
            .ok_or_else(invalid)?;
        Ok::<_, io::Error>(chunk)
    });
    PairedMultipart::new(tokio_util::io::StreamReader::new(stream), &boundary)
}
