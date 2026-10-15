use board_media::{MediaError, ObjectId, Quarantine};

#[tokio::test]
async fn still_intake_has_its_own_exact_bound_without_changing_legacy_intake() {
    use board_media::{InputSnapshot, still::MAX_IMAGE_BYTES};
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path()).unwrap();
    let maximum = MAX_IMAGE_BYTES as usize;
    let bytes = vec![42; maximum];
    let legacy = ObjectId::generate().unwrap();
    assert!(matches!(
        quarantine.receive(legacy, bytes.as_slice()).await,
        Err(MediaError::InputTooLarge)
    ));
    assert!(!directory.path().join(format!("{legacy}.part")).exists());
    let id = ObjectId::generate().unwrap();
    assert_eq!(
        quarantine
            .receive_still(id, bytes.as_slice())
            .await
            .unwrap(),
        maximum as u64
    );
    assert!(quarantine.open_input(id, maximum as u64).is_err());
    let input = quarantine.open_still_input(id, maximum as u64).unwrap();
    assert!(InputSnapshot::read(input, maximum as u64).await.is_err());
    let input = quarantine.open_still_input(id, maximum as u64).unwrap();
    assert_eq!(
        InputSnapshot::read_still(input, maximum as u64)
            .await
            .unwrap()
            .bytes(),
        bytes
    );
    let oversized = ObjectId::generate().unwrap();
    let bytes = vec![42; maximum + 1];
    assert!(matches!(
        quarantine.receive_still(oversized, bytes.as_slice()).await,
        Err(MediaError::InputTooLarge)
    ));
    assert!(!directory.path().join(format!("{oversized}.part")).exists());
    assert!(!directory.path().join(format!("{oversized}.input")).exists());
}
