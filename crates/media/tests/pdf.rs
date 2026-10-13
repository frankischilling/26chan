use board_media::pdf::RetainedPdf;
use board_media::{
    ApprovedFiles, InputSnapshot, ObjectId, PublicationStore, Quarantine, ValidatedOutput,
};
use std::io::{Seek, SeekFrom, Write};

async fn pixels() -> ValidatedOutput {
    ValidatedOutput::read(b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff".as_slice())
        .await
        .unwrap()
}

async fn snapshot(bytes: &[u8]) -> InputSnapshot {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    InputSnapshot::read(file, bytes.len() as u64).await.unwrap()
}

// This is a byte-retention test, not a document parser or isolated admission
// qualification. Actual PDF validity is tested in the disposable guest suite.
const INPUT: &[u8] = b"%PDF-1.4\nowned byte-retention fixture\n";

#[tokio::test]
async fn original_snapshot_survives_path_mutation_and_is_not_reencoded() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(INPUT).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut mutator = file.try_clone().unwrap();
    let original = InputSnapshot::read(file, INPUT.len() as u64).await.unwrap();
    mutator.seek(SeekFrom::Start(0)).unwrap();
    mutator.write_all(&vec![0; INPUT.len()]).unwrap();
    let document = RetainedPdf::from_processed_snapshot(original, pixels().await).unwrap();
    assert_eq!(document.bytes(), INPUT);
    assert_eq!(document.len(), INPUT.len() as u64);
    assert!(!document.is_empty());
    assert_eq!(document.preview().dimensions(), (1, 1));
    assert!(
        RetainedPdf::from_processed_snapshot(snapshot(b"not PDF").await, pixels().await).is_err()
    );
}

#[tokio::test]
async fn publication_replay_checked_read_and_cleanup_keep_fixed_pdf_paths() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let store = PublicationStore::new(root.path().join("published"), &quarantine).unwrap();
    let files = ApprovedFiles::open(root.path().join("published")).unwrap();
    let id = ObjectId::generate().unwrap();
    let document =
        RetainedPdf::from_processed_snapshot(snapshot(INPUT).await, pixels().await).unwrap();
    let guard = store.try_lock().unwrap();
    assert!(!guard.install_pdf(id, &document).unwrap().already_published);
    assert!(guard.install_pdf(id, &document).unwrap().already_published);
    let thumb = document.preview().thumbnail().unwrap();
    guard.install_thumbnail(id, &thumb).unwrap();
    assert_eq!(
        files
            .read_pdf(id, document.sha256(), document.len())
            .unwrap(),
        INPUT
    );
    assert!(files.read_pdf(id, &"0".repeat(64), document.len()).is_err());
    assert!(
        files
            .read_pdf(id, document.sha256(), document.len() + 1)
            .is_err()
    );
    assert!(files.read(id, document.sha256(), document.len()).is_err());
    assert!(
        guard
            .install(id, &pixels().await.encode().unwrap())
            .is_err()
    );
    let other = ObjectId::generate().unwrap();
    guard
        .install(other, &pixels().await.encode().unwrap())
        .unwrap();
    assert!(guard.install_pdf(other, &document).is_err());
    guard.remove(id).unwrap();
    assert!(!root.path().join(format!("published/{id}.pdf")).exists());
    assert!(
        !root
            .path()
            .join(format!("published/{id}.thumb.png"))
            .exists()
    );
    assert!(root.path().join(format!("published/{other}.png")).exists());
}
