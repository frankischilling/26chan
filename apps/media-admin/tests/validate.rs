use std::{fs, process::Command};

#[test]
fn stopped_disks_require_valid_bounded_pixels_before_approved_storage() {
    let root = tempfile::tempdir().unwrap();
    let private = root.path().join("private");
    let approved = root.path().join("approved");
    fs::create_dir(&private).unwrap();
    let disk = private.join("output.disk");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_media-validate"))
            .env_remove("APP_ENV")
            .arg(&disk)
            .arg(&approved)
            .output()
            .unwrap()
    };
    fs::write(&disk, b"invalid output").unwrap();
    assert!(!run().status.success());
    assert!(!approved.exists());
    let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff".to_vec();
    pixels.resize(4_194_816, 0);
    fs::write(&disk, &pixels).unwrap();
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(receipt["width"], 1);
    assert_eq!(receipt["height"], 1);
    assert_eq!(fs::read_dir(&approved).unwrap().count(), 1);
    pixels[20] = 1;
    fs::write(&disk, &pixels).unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_dir(&approved).unwrap().count(), 1);
}
