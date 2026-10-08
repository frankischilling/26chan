use board_media::{
    ObjectId, Quarantine,
    paired::{InputKind, decode_input, encode_input},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf};

fn job() -> ObjectId {
    "0102030405060708090a0b0c0d0e0f10".parse().unwrap()
}
fn input(replay: bool) -> Vec<u8> {
    encode_input(
        InputKind::PairedV2,
        job().bytes(),
        b"png",
        replay.then_some(b"replay".as_slice()),
    )
    .unwrap()
}

#[tokio::test]
async fn actual_bytes_determine_all_hashes_and_object_is_installed_once() {
    for present in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let q = Quarantine::new(root.path()).unwrap();
        let bytes = input(present);
        let candidate = decode_input(InputKind::PairedV2, &bytes, job().bytes()).unwrap();
        let receipt = q.receive_pair(job(), bytes.as_slice()).await.unwrap();
        assert_eq!(q.inspect_pair(job()).await.unwrap(), receipt);
        assert_eq!(receipt.sha256, *candidate.frame_sha256());
        assert_eq!(receipt.image_sha256, *candidate.image_sha256());
        assert_eq!(receipt.replay_sha256.as_ref(), candidate.replay_sha256());
        assert_eq!(receipt.bytes, bytes.len() as u64);
        assert_eq!(receipt.image_bytes, 3);
        assert_eq!(receipt.replay_bytes, present.then_some(6));
        assert_eq!(
            std::fs::read(root.path().join(format!("{}.input", job()))).unwrap(),
            bytes
        );
        assert!(!root.path().join(format!("{}.part", job())).exists());
        assert!(
            q.receive_pair(job(), input(present).as_slice())
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read(root.path().join(format!("{}.input", job()))).unwrap(),
            bytes
        );
    }
}

#[tokio::test]
async fn no_truncation_trailing_data_or_corrupted_header_installs_an_object() {
    let bytes = input(true);
    let mut cases: Vec<Vec<u8>> = (0..bytes.len()).map(|end| bytes[..end].to_vec()).collect();
    for position in [0, 8, 10, 12, 16, 24, 32, bytes.len() - 1] {
        let mut modified = bytes.clone();
        modified[position] ^= 128;
        cases.push(modified);
    }
    let mut extra = bytes.clone();
    extra.push(0);
    cases.push(extra);
    for bytes in cases {
        let root = tempfile::tempdir().unwrap();
        let q = Quarantine::new(root.path()).unwrap();
        assert!(q.receive_pair(job(), bytes.as_slice()).await.is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn marker_without_eof_stalls_and_cancellation_removes_only_partial_file() {
    let root = tempfile::tempdir().unwrap();
    let q = Quarantine::new(root.path()).unwrap();
    let (mut tx, rx) = tokio::io::duplex(256);
    tx.write_all(&input(true)).await.unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            q.receive_pair(job(), rx)
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

struct BoundedReader {
    bytes: Vec<u8>,
    position: usize,
}
impl AsyncRead for BoundedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        assert!(
            buffer.remaining() <= 8192,
            "receiver requested a whole-component buffer"
        );
        let count = buffer.remaining().min(self.bytes.len() - self.position);
        buffer.put_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn two_independent_maximum_components_stream_without_whole_pair_buffering() {
    let root = tempfile::tempdir().unwrap();
    let q = Quarantine::new(root.path()).unwrap();
    let component = vec![3; 8_388_608];
    let bytes = encode_input(
        InputKind::PairedV2,
        job().bytes(),
        &component,
        Some(&component),
    )
    .unwrap();
    assert_eq!(bytes.len(), 16_777_272);
    let receipt = q
        .receive_pair(job(), BoundedReader { bytes, position: 0 })
        .await
        .unwrap();
    assert_eq!(receipt.bytes, 16_777_272);
    assert_eq!(receipt.image_sha256, receipt.replay_sha256.unwrap());
}

#[tokio::test]
async fn competing_receive_cannot_unlink_the_owner_partial_file() {
    let root = tempfile::tempdir().unwrap();
    let q = Quarantine::new(root.path()).unwrap();
    let path = root.path().join(format!("{}.part", job()));
    std::fs::write(&path, b"owned").unwrap();
    assert!(
        q.receive_pair(job(), input(false).as_slice())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"owned");
}

#[tokio::test]
async fn reconciliation_rejects_corruption_and_symlinks_without_replacing_input() {
    let root = tempfile::tempdir().unwrap();
    let q = Quarantine::new(root.path()).unwrap();
    let bytes = input(true);
    q.receive_pair(job(), bytes.as_slice()).await.unwrap();
    let path = root.path().join(format!("{}.input", job()));
    let mut changed = bytes;
    changed.push(0);
    std::fs::write(&path, &changed).unwrap();
    assert!(q.inspect_pair(job()).await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    #[cfg(unix)]
    {
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(q.inspect_pair(job()).await.is_err());
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
