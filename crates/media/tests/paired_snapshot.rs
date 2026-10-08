use board_media::{
    ObjectId,
    paired::{self, InputKind},
    paired_snapshot::{PairedInputDescriptor, PairedInputSnapshot},
};
use std::io::{Seek, SeekFrom, Write};

fn fixture(png: &[u8], replay: Option<&[u8]>) -> (Vec<u8>, PairedInputDescriptor) {
    let id: ObjectId = "00112233445566778899aabbccddeeff".parse().unwrap();
    let bytes = paired::encode_input(InputKind::PairedV2, id.bytes(), png, replay).unwrap();
    let decoded = paired::decode_input(InputKind::PairedV2, &bytes, id.bytes()).unwrap();
    let d = PairedInputDescriptor {
        job_id: id,
        bytes: bytes.len() as u64,
        sha256: *decoded.frame_sha256(),
        image_bytes: png.len() as u64,
        image_sha256: *decoded.image_sha256(),
        replay: replay.map(|r| (r.len() as u64, *decoded.replay_sha256().unwrap())),
    };
    (bytes, d)
}
async fn read(
    bytes: &[u8],
    d: PairedInputDescriptor,
) -> Result<PairedInputSnapshot, board_media::MediaError> {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    PairedInputSnapshot::read(file, d).await
}
#[tokio::test]
async fn immutable_source_and_fresh_attempt_binding() {
    let (bytes, d) = fixture(b"png source", Some(b"raw TGKR"));
    let snapshot = read(&bytes, d).await.unwrap();
    assert_eq!(snapshot.bytes(), bytes);
    assert_eq!(snapshot.image_bytes(), b"png source");
    assert_eq!(snapshot.replay_presence(), paired::ReplayPresence::Present);
    assert_ne!(
        snapshot.new_attempt_binding().unwrap(),
        snapshot.new_attempt_binding().unwrap()
    );
    assert_ne!(
        snapshot.new_attempt_binding().unwrap(),
        snapshot.descriptor().sha256
    );
}
#[tokio::test]
async fn every_persisted_descriptor_is_checked() {
    let (bytes, d) = fixture(b"png", Some(b"tgkr"));
    for field in 0..8 {
        let mut changed = d.clone();
        match field {
            0 => changed.bytes += 1,
            1 => changed.sha256[0] ^= 1,
            2 => changed.image_bytes += 1,
            3 => changed.image_sha256[0] ^= 1,
            4 => changed.replay.as_mut().unwrap().0 += 1,
            5 => changed.replay.as_mut().unwrap().1[0] ^= 1,
            6 => changed.replay = None,
            _ => changed.job_id = "ffeeddccbbaa99887766554433221100".parse().unwrap(),
        }
        assert!(read(&bytes, changed).await.is_err(), "field {field}");
    }
    let mut changed = bytes.clone();
    changed[48] ^= 1;
    assert!(read(&changed, d.clone()).await.is_err());
    assert!(read(&bytes[..bytes.len() - 1], d.clone()).await.is_err());
    let mut changed = bytes.clone();
    changed.push(0);
    assert!(read(&changed, d).await.is_err());
}
#[tokio::test]
async fn maximum_pair_is_separate_from_v1_budget() {
    let (bytes, d) = fixture(&vec![1; 8_388_608], Some(&vec![2; 8_388_608]));
    assert_eq!(bytes.len(), 16_777_272);
    assert!(read(&bytes, d).await.is_ok());
    let (bytes, d) = fixture(b"png", None);
    assert_eq!(
        read(&bytes, d).await.unwrap().replay_presence(),
        paired::ReplayPresence::Absent
    );
}

#[tokio::test]
async fn snapshot_does_not_follow_later_inode_mutation() {
    let (bytes, d) = fixture(b"png source", Some(b"replay"));
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut mutator = file.try_clone().unwrap();
    let snapshot = PairedInputSnapshot::read(file, d).await.unwrap();
    mutator.seek(SeekFrom::Start(0)).unwrap();
    mutator.write_all(&vec![0; bytes.len()]).unwrap();
    assert_eq!(snapshot.bytes(), bytes);
    assert_eq!(snapshot.image_bytes(), b"png source");
}
