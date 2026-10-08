use axum::{body::Body, http::Request};
use board_media::{
    ObjectId, Quarantine,
    paired::{InputKind, decode_input},
};
use board_public::paired_upload::{PairedMultipart, from_request};
use bytes::Bytes;
use futures_util::{StreamExt, stream};
use std::{io, time::Duration};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWriteExt, ReadBuf},
    sync::mpsc,
};

fn job() -> ObjectId {
    "0102030405060708090a0b0c0d0e0f10".parse().unwrap()
}

fn form(image: &[u8], replay: Option<&[u8]>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (name, value) in [
        ("resto", "19".to_owned()),
        ("png_bytes", image.len().to_string()),
        ("replay_bytes", replay.map_or(0, <[u8]>::len).to_string()),
    ] {
        bytes.extend_from_slice(
            format!(
                "--boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    bytes.extend_from_slice(b"--boundary\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"tegaki.png\"\r\nContent-Type: image/png\r\n\r\n");
    bytes.extend_from_slice(image);
    if let Some(replay) = replay {
        bytes.extend_from_slice(b"\r\n--boundary\r\nContent-Disposition: form-data; name=\"replay\"; filename=\"tegaki.tgkr\"\r\nContent-Type: application/octet-stream\r\n\r\n");
        bytes.extend_from_slice(replay);
    }
    bytes.extend_from_slice(b"\r\n--boundary--\r\n");
    bytes
}

async fn collect<R: AsyncRead + Unpin>(
    mut parser: PairedMultipart<R>,
) -> (io::Result<()>, Vec<u8>) {
    if let Err(e) = parser.read_declaration().await {
        return (Err(e), vec![]);
    }
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let collect = async {
        let mut bytes = Vec::new();
        while let Some(chunk) = rx.recv().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        bytes
    };
    tokio::join!(parser.forward(job(), tx), collect)
}

#[tokio::test]
async fn normal_formdata_contract_streams_png_with_optional_replay() {
    for replay in [None, Some(b"replay".as_slice())] {
        for final_crlf in [true, false] {
            let mut bytes = form(b"png", replay);
            if !final_crlf {
                bytes.truncate(bytes.len() - 2);
            }
            let (result, wire) =
                collect(PairedMultipart::new(bytes.as_slice(), "boundary").unwrap()).await;
            result.unwrap();
            let parsed = decode_input(InputKind::PairedV2, &wire, job().bytes()).unwrap();
            assert_eq!(parsed.image_bytes(), b"png");
            assert_eq!(parsed.replay_upload_bytes(), replay);
        }
    }
}

#[tokio::test]
async fn every_byte_split_of_an_http_body_is_accepted_without_prefetching() {
    let bytes = form(b"png", Some(b"replay"));
    for split in 0..=bytes.len() {
        let chunks = vec![
            Ok::<_, io::Error>(Bytes::copy_from_slice(&bytes[..split])),
            Ok(Bytes::copy_from_slice(&bytes[split..])),
        ];
        let request = Request::builder()
            .header("content-type", "multipart/form-data; boundary=boundary")
            .body(Body::from_stream(stream::iter(chunks)))
            .unwrap();
        let (result, wire) = collect(from_request(request).unwrap()).await;
        result.unwrap();
        assert!(decode_input(InputKind::PairedV2, &wire, job().bytes()).is_ok());
    }
}

#[tokio::test]
async fn malformed_final_boundary_epilogue_or_late_body_error_never_emits_marker() {
    let good = form(b"png", Some(b"replay"));
    let mut cases = Vec::new();
    for cut in 0..good.len() - 2 {
        cases.push(good[..cut].to_vec());
    }
    for epilogue in [b"x".as_slice(), b"\r\n", b" ", b"--boundary\r\n"] {
        let mut bytes = good.clone();
        bytes.extend_from_slice(epilogue);
        cases.push(bytes);
    }
    for bytes in cases {
        let (result, wire) =
            collect(PairedMultipart::new(bytes.as_slice(), "boundary").unwrap()).await;
        assert!(result.is_err());
        assert!(!wire.ends_with(b"IBDONE02"));
    }
    let chunks = stream::iter(vec![
        Ok(Bytes::from(good)),
        Err(io::Error::other("late body failure")),
    ]);
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=boundary")
        .body(Body::from_stream(chunks))
        .unwrap();
    let (result, wire) = collect(from_request(request).unwrap()).await;
    assert!(result.is_err());
    assert!(!wire.ends_with(b"IBDONE02"));
}

#[tokio::test(start_paused = true)]
async fn terminal_delimiter_without_body_eof_times_out_without_a_marker() {
    let bytes = form(b"png", None);
    let chunks =
        stream::iter(vec![Ok::<_, io::Error>(Bytes::from(bytes))]).chain(stream::pending());
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=boundary")
        .body(Body::from_stream(chunks))
        .unwrap();
    let start = tokio::time::Instant::now();
    let (result, wire) = collect(from_request(request).unwrap()).await;
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert_eq!(tokio::time::Instant::now() - start, Duration::from_secs(10));
    assert!(!wire.ends_with(b"IBDONE02"));
}

#[tokio::test]
async fn stalled_outer_body_cannot_install_a_quarantine_object() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path()).unwrap();
    let (mut writer, reader) = tokio::io::duplex(1024);
    writer
        .write_all(&form(b"png", Some(b"replay")))
        .await
        .unwrap();
    let mut parser = PairedMultipart::new(reader, "boundary").unwrap();
    parser.read_declaration().await.unwrap();
    let (tx, rx) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let stream = stream::unfold(rx, |mut rx| async { rx.recv().await.map(|v| (v, rx)) });
    let receive =
        quarantine.receive_pair(job(), tokio_util::io::StreamReader::new(Box::pin(stream)));
    let pending = async { tokio::join!(parser.forward(job(), tx), receive) };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), pending)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn duplicate_reordered_empty_replay_or_false_lengths_are_rejected() {
    let good = String::from_utf8(form(b"png", Some(b"replay"))).unwrap();
    let cases = [
        good.replace("name=\"png_bytes\"", "name=\"resto\""),
        good.replace("\r\n\r\n3\r\n", "\r\n\r\n2\r\n"),
        good.replace("\r\n\r\n3\r\n", "\r\n\r\n4\r\n"),
        good.replace("\r\n\r\n3\r\n", "\r\n\r\n8388609\r\n"),
        good.replace("\r\n\r\n6\r\n", "\r\n\r\n8388609\r\n"),
        good.replace("\r\n\r\n6\r\n", "\r\n\r\n0\r\n"),
        good.replace("\r\n\r\n3\r\n", "\r\n\r\n03\r\n"),
        good.replace("tegaki.png", "../tegaki.png"),
        format!("preamble{good}"),
        good.replace(
            "Content-Type: image/png",
            &format!("X-Oversize: {}", "x".repeat(1024)),
        ),
    ];
    for bytes in cases {
        let (result, wire) =
            collect(PairedMultipart::new(bytes.as_bytes(), "boundary").unwrap()).await;
        assert!(result.is_err());
        assert!(!wire.ends_with(b"IBDONE02"));
    }
}

struct SmallReads {
    bytes: Vec<u8>,
    at: usize,
}
impl AsyncRead for SmallReads {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        b: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        assert!(b.remaining() <= 16_384);
        let count = b.remaining().min(self.bytes.len() - self.at);
        b.put_slice(&self.bytes[self.at..self.at + count]);
        self.at += count;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn maximum_components_use_bounded_reads_and_channel_chunks() {
    let component = vec![9; 8_388_608];
    let reader = SmallReads {
        bytes: form(&component, Some(&component)),
        at: 0,
    };
    let mut parser = PairedMultipart::new(reader, "boundary").unwrap();
    let declaration = parser.read_declaration().await.unwrap();
    assert_eq!(declaration.image_bytes, 8_388_608);
    assert_eq!(declaration.replay_bytes, Some(8_388_608));
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let consume = async {
        let mut count = 0;
        while let Some(chunk) = rx.recv().await {
            let chunk: Bytes = chunk.unwrap();
            assert!(chunk.len() <= 16_384);
            count += chunk.len();
        }
        count
    };
    let (result, count) = tokio::join!(parser.forward(job(), tx), consume);
    result.unwrap();
    assert_eq!(count, 16_777_272);
}

#[tokio::test]
async fn standard_formdata_generated_body_is_accepted() {
    // Generated with the standard FormData/Blob/Request APIs, not our parser.
    let request = Request::builder()
        .header(
            "content-type",
            include_str!("fixtures/paired-intake/content-type.txt"),
        )
        .body(Body::from(
            include_bytes!("fixtures/paired-intake/formdata.bin").as_slice(),
        ))
        .unwrap();
    let (result, wire) = collect(from_request(request).unwrap()).await;
    result.unwrap();
    let parsed = decode_input(InputKind::PairedV2, &wire, job().bytes()).unwrap();
    assert_eq!(parsed.image_bytes(), b"png");
    assert_eq!(parsed.replay_upload_bytes(), Some(b"replay".as_slice()));
}

#[tokio::test]
async fn blocked_downstream_does_not_drain_immediately_ready_input() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Counted {
        bytes: Vec<u8>,
        read: Arc<AtomicUsize>,
    }
    impl AsyncRead for Counted {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            b: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let at = self.read.load(Ordering::Relaxed);
            let n = b.remaining().min(self.bytes.len() - at);
            b.put_slice(&self.bytes[at..at + n]);
            self.read.store(at + n, Ordering::Relaxed);
            Poll::Ready(Ok(()))
        }
    }
    let read = Arc::new(AtomicUsize::new(0));
    let reader = Counted {
        bytes: form(&vec![1; 8_388_608], None),
        read: read.clone(),
    };
    let mut parser = PairedMultipart::new(reader, "boundary").unwrap();
    parser.read_declaration().await.unwrap();
    let (tx, _rx) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), parser.forward(job(), tx))
            .await
            .is_err()
    );
    // One header packet occupies the channel; at most one copied component chunk
    // can be pending. The immediately-ready source is not drained into a buffer.
    assert!(read.load(Ordering::Relaxed) < 20_000);
}

#[tokio::test]
async fn request_metadata_and_http_overhead_are_bounded_before_forwarding() {
    for boundary in ["", "bad boundary", "bad\r\nboundary"] {
        assert!(PairedMultipart::new(b"".as_slice(), boundary).is_err());
    }
    assert!(PairedMultipart::new(b"".as_slice(), &"a".repeat(71)).is_err());
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=boundary")
        .header("content-type", "multipart/form-data; boundary=other")
        .body(Body::empty())
        .unwrap();
    assert!(from_request(request).is_err());
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=boundary")
        .header("content-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    assert!(from_request(request).is_err());
    let request = Request::builder()
        .header("content-type", "multipart/form-data; boundary=boundary")
        .body(Body::from(vec![0; 16_777_272 + 8193]))
        .unwrap();
    let (result, wire) = collect(from_request(request).unwrap()).await;
    assert!(result.is_err());
    assert!(wire.is_empty());
}
