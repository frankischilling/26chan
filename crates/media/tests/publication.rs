use board_media::{
    ApprovedFiles, MediaError, ObjectId, PublicationStore, Quarantine, ValidatedOutput,
};
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

async fn pixels(red: u8) -> ValidatedOutput {
    let mut bytes = b"IBRGBA01".to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&[red, 0, 0, 255]);
    ValidatedOutput::read(bytes.as_slice()).await.unwrap()
}

#[tokio::test]
async fn complete_files_replay_without_overwrite_and_recover_fixed_staging() {
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
    let root = directory.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    assert!(PublicationStore::new(directory.path(), &quarantine).is_err());
    let id = ObjectId::generate().unwrap();
    let output = pixels(7).await.encode().unwrap();
    let guard = store.try_lock().unwrap();
    fs::write(root.join(format!("{id}.part")), b"abandoned partial write").unwrap();
    assert!(!guard.install(id, &output).unwrap().already_published);
    assert!(!root.join(format!("{id}.part")).exists());
    assert!(guard.install(id, &output).unwrap().already_published);
    assert!(matches!(
        guard.install(id, &pixels(8).await.encode().unwrap()),
        Err(MediaError::Conflict)
    ));
    let reader = ApprovedFiles::open(&root).unwrap();
    let saved = reader.read(id, output.sha256(), output.len()).unwrap();
    assert!(saved.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(reader.read(id, &"0".repeat(64), output.len()).is_err());
    assert!(reader.read(id, output.sha256(), output.len() + 1).is_err());
    fs::write(root.join(format!("{id}.png")), vec![0; saved.len()]).unwrap();
    assert!(reader.read(id, output.sha256(), output.len()).is_err());
    guard.remove(id).unwrap();
    guard.remove(id).unwrap();
    assert!(!root.join(format!("{id}.png")).exists());
    assert!(root.join(".publication.lock").is_file());
    let pending = ObjectId::generate().unwrap();
    fs::write(root.join(format!("{pending}.part")), b"partial").unwrap();
    guard.remove(pending).unwrap();
    assert!(!root.join(format!("{pending}.part")).exists());
}

#[test]
fn readers_create_nothing_and_reject_nonregular_storage_entries() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent");
    assert!(ApprovedFiles::open(&absent).is_err());
    assert!(!absent.exists());
    let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
    let root = directory.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    fs::create_dir(root.join(format!("{id}.png"))).unwrap();
    let reader = ApprovedFiles::open(&root).unwrap();
    assert!(reader.read(id, &"a".repeat(64), 1).is_err());
    assert!(store.try_lock().unwrap().remove(id).is_err());
    assert!(root.join(format!("{id}.png")).is_dir());
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_without_reading_or_removing_the_target() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
    let root = directory.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    let target = directory.path().join("witness");
    fs::write(&target, b"preserve").unwrap();
    symlink(&target, root.join(format!("{id}.png"))).unwrap();
    assert!(
        ApprovedFiles::open(&root)
            .unwrap()
            .read(id, &"a".repeat(64), 8)
            .is_err()
    );
    assert!(store.try_lock().unwrap().remove(id).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserve");
    symlink(&target, root.join(".publication.lock.bad")).unwrap();
    fs::remove_file(root.join(".publication.lock")).unwrap();
    fs::rename(
        root.join(".publication.lock.bad"),
        root.join(".publication.lock"),
    )
    .unwrap();
    assert!(store.try_lock().is_err());
}

#[test]
fn publication_lock_excludes_other_processes_and_is_released_after_process_death() {
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
    let store = PublicationStore::new(directory.path().join("objects"), &quarantine).unwrap();
    let guard = store.try_lock().unwrap();
    assert!(matches!(store.try_lock(), Err(MediaError::Busy)));
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "publication_lock_child", "--nocapture"])
        .env("BOARD_LOCK_TEST_ROOT", directory.path())
        .env("BOARD_LOCK_TEST_MODE", "busy")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    drop(guard);
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "publication_lock_child", "--nocapture"])
        .env("BOARD_LOCK_TEST_ROOT", directory.path())
        .env("BOARD_LOCK_TEST_MODE", "hold")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    while !directory.path().join("ready").exists() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let ready = directory.path().join("ready").exists();
    let was_busy = matches!(store.try_lock(), Err(MediaError::Busy));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(ready && was_busy);
    assert!(store.try_lock().is_ok());
}

#[test]
fn publication_lock_child() {
    let Some(root) = std::env::var_os("BOARD_LOCK_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let quarantine = Quarantine::new(root.join("input")).unwrap();
    let store = PublicationStore::new(root.join("objects"), &quarantine).unwrap();
    if std::env::var("BOARD_LOCK_TEST_MODE").as_deref() == Ok("busy") {
        assert!(matches!(store.try_lock(), Err(MediaError::Busy)));
    } else {
        let _guard = store.try_lock().unwrap();
        fs::write(root.join("ready"), b"locked").unwrap();
        std::thread::park();
    }
}
