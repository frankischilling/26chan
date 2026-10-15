use board_media::still::{OUTPUT_DISK_BYTES, StillFormat, StillPolicy, ValidatedStill};
use std::io::Cursor;

fn policy(format: StillFormat) -> StillPolicy {
    StillPolicy {
        format,
        strip_exif: format != StillFormat::Gif,
        mobile: format != StillFormat::Gif,
        thread: true,
        blue_background: false,
        thumbnail: (250, 250),
    }
}

// Only framing is under test here. These are intentionally not decodable media.
fn disk(format: StillFormat) -> Vec<u8> {
    let mut output = vec![0; OUTPUT_DISK_BYTES as usize];
    output[..8].copy_from_slice(if format == StillFormat::Gif {
        b"IBIMG007"
    } else {
        b"IBIMG006"
    });
    output[8..24].copy_from_slice(&policy(format).header().unwrap());
    for (offset, value) in [
        (24, 640u32),
        (28, 480),
        (32, 250),
        (36, 187),
        (48, 12),
        (52, 5),
    ] {
        output[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    match format {
        StillFormat::Png => output[64..72].copy_from_slice(b"\x89PNG\r\n\x1a\n"),
        StillFormat::Jpeg => {
            output[64..67].copy_from_slice(b"\xff\xd8\xff");
            output[74..76].copy_from_slice(b"\xff\xd9");
        }
        StillFormat::Gif => {
            output[64..70].copy_from_slice(b"GIF89a");
            output[75] = b';';
        }
    }
    output[76..81].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
    output
}

#[tokio::test]
async fn exact_envelope_preserves_bytes_and_hashes_only_served_content() {
    for format in [StillFormat::Png, StillFormat::Jpeg, StillFormat::Gif] {
        let bytes = disk(format);
        let result = ValidatedStill::read_disk(Cursor::new(&bytes), policy(format))
            .await
            .unwrap();
        assert_eq!(result.policy(), policy(format));
        assert_eq!(result.retained().bytes(), &bytes[64..76]);
        assert_eq!(result.thumbnail().bytes(), &bytes[76..81]);
        assert_eq!(result.retained().dimensions(), (640, 480));
        assert_eq!(result.thumbnail().dimensions(), (250, 187));
        assert!(result.mobile().is_none());
        assert_eq!(result.retained().sha256().len(), 64);
        assert_eq!(result.retained().md5().len(), 32);
        let mut changed = bytes.clone();
        changed[73] = 42;
        let other = ValidatedStill::read_disk(Cursor::new(changed), policy(format))
            .await
            .unwrap();
        assert_ne!(result.retained().md5(), other.retained().md5());
        assert_ne!(result.retained().sha256(), other.retained().sha256());
        assert_eq!(result.thumbnail().sha256(), other.thumbnail().sha256());
    }
}

#[tokio::test]
async fn source_gif_has_its_own_version_byte_cap_and_no_mobile_branch() {
    let expected = policy(StillFormat::Gif);
    let bytes = disk(StillFormat::Gif);
    for flag in [1, 2] {
        let mut invalid = expected;
        invalid.strip_exif = flag == 1;
        invalid.mobile = flag == 2;
        assert!(invalid.header().is_err());
    }
    for magic in [b"IBIMG006", b"IBGIF001"] {
        let mut invalid = bytes.clone();
        invalid[..8].copy_from_slice(magic);
        assert!(
            ValidatedStill::read_disk(Cursor::new(invalid), expected)
                .await
                .is_err()
        );
    }
    for (offset, value) in [(48, 8 * 1024 * 1024 + 1), (40, 1024), (56, 5)] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 4].copy_from_slice(&(value as u32).to_be_bytes());
        assert!(
            ValidatedStill::read_disk(Cursor::new(invalid), expected)
                .await
                .is_err()
        );
    }
    let mut truncated = bytes;
    truncated[75] = 0;
    assert!(
        ValidatedStill::read_disk(Cursor::new(truncated), expected)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn header_policy_geometry_signatures_and_padding_are_bound() {
    let valid = disk(StillFormat::Png);
    for offset in [
        0, 8, 16, 17, 18, 20, 22, 23, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 63, 64, 76, 80, 81,
    ] {
        let mut bytes = valid.clone();
        bytes[offset] ^= 0xff;
        assert!(
            ValidatedStill::read_disk(Cursor::new(bytes), policy(StillFormat::Png))
                .await
                .is_err(),
            "{offset}"
        );
    }
    for length in [0, 63, 64, 75, 80, valid.len() - 1] {
        assert!(
            ValidatedStill::read_disk(Cursor::new(&valid[..length]), policy(StillFormat::Png))
                .await
                .is_err()
        );
    }
    let mut extra = valid.clone();
    extra.push(0);
    assert!(
        ValidatedStill::read_disk(Cursor::new(extra), policy(StillFormat::Png))
            .await
            .is_err()
    );
    assert!(
        ValidatedStill::read_disk(Cursor::new(&valid), policy(StillFormat::Jpeg))
            .await
            .is_err()
    );
    let mut other = policy(StillFormat::Png);
    other.strip_exif = false;
    assert!(
        ValidatedStill::read_disk(Cursor::new(&valid), other)
            .await
            .is_err()
    );
    other = policy(StillFormat::Png);
    other.thumbnail = (0, 250);
    assert!(
        ValidatedStill::read_disk(Cursor::new(&valid), other)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mobile_is_required_exactly_when_policy_and_geometry_require_it() {
    let mut bytes = disk(StillFormat::Jpeg);
    for (offset, value) in [
        (24, 2048u32),
        (28, 1024),
        (32, 250),
        (36, 125),
        (40, 1024),
        (44, 512),
        (56, 5),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[81..86].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
    let result = ValidatedStill::read_disk(Cursor::new(&bytes), policy(StillFormat::Jpeg))
        .await
        .unwrap();
    assert_eq!(result.mobile().unwrap().dimensions(), (1024, 512));
    for offset in [40, 44, 56] {
        let mut missing = bytes.clone();
        missing[offset..offset + 4].fill(0);
        assert!(
            ValidatedStill::read_disk(Cursor::new(missing), policy(StillFormat::Jpeg))
                .await
                .is_err()
        );
    }
    let mut disabled = policy(StillFormat::Jpeg);
    disabled.mobile = false;
    bytes[8..24].copy_from_slice(&disabled.header().unwrap());
    assert!(
        ValidatedStill::read_disk(Cursor::new(bytes), disabled)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn still_files_have_fixed_names_exact_receipts_and_owned_cleanup() {
    use board_media::{ApprovedFiles, MediaError, ObjectId, PublicationStore, Quarantine};
    let mut bytes = disk(StillFormat::Jpeg);
    for (offset, value) in [
        (24, 2048u32),
        (28, 1024),
        (32, 250),
        (36, 125),
        (40, 1024),
        (44, 512),
        (56, 5),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    bytes[81..86].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
    let output = ValidatedStill::read_disk(Cursor::new(&bytes), policy(StillFormat::Jpeg))
        .await
        .unwrap();
    let png = ValidatedStill::read_disk(
        Cursor::new(disk(StillFormat::Png)),
        policy(StillFormat::Png),
    )
    .await
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
    let root = directory.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    let other = ObjectId::generate().unwrap();
    std::fs::write(
        root.join(format!("{other}.png")),
        b"unrelated owned fixture",
    )
    .unwrap();
    let guard = store.try_lock().unwrap();
    assert!(!guard.install_still(id, &output).unwrap().already_published);
    assert!(guard.install_still(id, &output).unwrap().already_published);
    guard.install_still_thumbnail(id, &output).unwrap();
    guard.install_still_mobile(id, &output).unwrap().unwrap();
    assert!(matches!(
        guard.install_still(id, &png),
        Err(MediaError::Conflict)
    ));
    let reader = ApprovedFiles::open(&root).unwrap();
    assert_eq!(
        reader
            .read_still(
                id,
                output.retained().sha256(),
                output.retained().bytes().len() as u64,
                StillFormat::Jpeg
            )
            .unwrap(),
        output.retained().bytes()
    );
    assert_eq!(
        reader
            .read_still_thumbnail(
                id,
                output.thumbnail().sha256(),
                output.thumbnail().bytes().len() as u64
            )
            .unwrap(),
        output.thumbnail().bytes()
    );
    let mobile = output.mobile().unwrap();
    assert_eq!(
        reader
            .read_still_mobile(id, mobile.sha256(), mobile.bytes().len() as u64)
            .unwrap(),
        mobile.bytes()
    );
    assert!(
        reader
            .read_still_mobile(id, &"0".repeat(64), mobile.bytes().len() as u64)
            .is_err()
    );
    assert!(
        reader
            .read_still(
                id,
                output.retained().sha256(),
                output.retained().bytes().len() as u64,
                StillFormat::Png
            )
            .is_err()
    );
    for suffix in ["jpg", "thumb.jpg", "mobile.jpg"] {
        assert!(root.join(format!("{id}.{suffix}")).is_file());
    }
    std::fs::write(root.join(format!("{id}.mobile.part")), b"partial").unwrap();
    guard.remove(id).unwrap();
    for entry in std::fs::read_dir(&root).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_str()
                .unwrap()
                .starts_with(&id.to_string())
        );
    }
    assert_eq!(
        std::fs::read(root.join(format!("{other}.png"))).unwrap(),
        b"unrelated owned fixture"
    );
}
