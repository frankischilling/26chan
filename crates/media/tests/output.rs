use board_media::{
    MAX_DIMENSION, MAX_PNG_BYTES, MediaError, ObjectId, Promoter, Quarantine, ValidatedOutput,
};
use sha2::{Digest, Sha256};
use std::io::Cursor;

fn protocol(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = b"IBRGBA01".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend(pixels);
    bytes
}

#[tokio::test]
async fn rejects_zero_excessive_dimensions_and_invalid_magic_before_pixel_reads() {
    for (width, height) in [(0, 1), (1, 0), (1025, 1), (1, 1025), (u32::MAX, u32::MAX)] {
        let mut reader = Cursor::new(protocol(width, height, &[255; 4]));
        assert!(matches!(
            ValidatedOutput::read(&mut reader).await,
            Err(MediaError::InvalidOutput)
        ));
        assert_eq!(reader.position(), 16);
    }
    let mut bytes = protocol(1, 1, &[255; 4]);
    bytes[0] = b'X';
    let mut reader = Cursor::new(bytes);
    assert!(matches!(
        ValidatedOutput::read(&mut reader).await,
        Err(MediaError::InvalidOutput)
    ));
    assert_eq!(reader.position(), 16);
}

#[tokio::test]
async fn rejects_every_truncation_and_trailing_bytes() {
    let valid = protocol(1, 1, &[10, 20, 30, 40]);
    for end in 0..valid.len() {
        assert!(
            ValidatedOutput::read(&valid[..end]).await.is_err(),
            "accepted prefix length {end}"
        );
    }
    for count in [1, 5000] {
        let mut trailing = valid.clone();
        trailing.extend(vec![0; count]);
        let mut reader = Cursor::new(trailing);
        assert!(matches!(
            ValidatedOutput::read(&mut reader).await,
            Err(MediaError::InvalidOutput)
        ));
        assert_eq!(
            reader.position(),
            21,
            "trailing bytes must not be collected"
        );
    }
}

#[tokio::test]
async fn known_rgba_fixture_produces_exact_png_pixels_and_receipt() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let public = root.path().join("public");
    let promoter = Promoter::new(&public, &quarantine).unwrap();
    let pixels = [10, 20, 30, 40];
    let output = ValidatedOutput::read(protocol(1, 1, &pixels).as_slice())
        .await
        .unwrap();
    assert_eq!(output.dimensions(), (1, 1));
    let id = ObjectId::generate().unwrap();
    let receipt = promoter.promote(id, &output).unwrap();
    assert!(!receipt.already_published);
    let png_bytes = std::fs::read(public.join(format!("{id}.png"))).unwrap();
    assert_eq!(receipt.bytes, png_bytes.len() as u64);
    assert_eq!(receipt.sha256, format!("{:x}", Sha256::digest(&png_bytes)));
    assert!(png_bytes.len() <= MAX_PNG_BYTES);
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder.read_info().unwrap();
    assert_eq!(reader.info().color_type, png::ColorType::Rgba);
    assert_eq!(reader.info().bit_depth, png::BitDepth::Eight);
    assert!(reader.info().uncompressed_latin1_text.is_empty());
    assert!(reader.info().utf8_text.is_empty());
    let mut actual = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut actual).unwrap();
    assert_eq!((info.width, info.height), (1, 1));
    assert_eq!(&actual[..info.buffer_size()], pixels);
    assert_eq!(std::fs::read_dir(public).unwrap().count(), 1);
}

#[tokio::test]
async fn maximum_dimensions_encode_within_independent_byte_bound() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let promoter = Promoter::new(root.path().join("public"), &quarantine).unwrap();
    let mut state = 0x12345678u32;
    let pixels: Vec<u8> = (0..MAX_DIMENSION * MAX_DIMENSION * 4)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let output = ValidatedOutput::read(protocol(MAX_DIMENSION, MAX_DIMENSION, &pixels).as_slice())
        .await
        .unwrap();
    assert_eq!(output.dimensions(), (MAX_DIMENSION, MAX_DIMENSION));
    let receipt = promoter
        .promote(ObjectId::generate().unwrap(), &output)
        .unwrap();
    assert!(receipt.bytes <= MAX_PNG_BYTES as u64);
}

#[tokio::test]
async fn identical_replay_is_idempotent_and_conflicting_replay_preserves_original() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let public = root.path().join("public");
    let promoter = Promoter::new(&public, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    let first = ValidatedOutput::read(protocol(1, 1, &[10, 20, 30, 40]).as_slice())
        .await
        .unwrap();
    let other = ValidatedOutput::read(protocol(1, 1, &[50, 60, 70, 80]).as_slice())
        .await
        .unwrap();
    let original = promoter.promote(id, &first).unwrap();
    let replay = promoter.promote(id, &first).unwrap();
    assert!(replay.already_published);
    assert_eq!(replay.sha256, original.sha256);
    assert_eq!(replay.bytes, original.bytes);
    let bytes = std::fs::read(public.join(format!("{id}.png"))).unwrap();
    assert!(matches!(
        promoter.promote(id, &other),
        Err(MediaError::Conflict)
    ));
    assert_eq!(
        std::fs::read(public.join(format!("{id}.png"))).unwrap(),
        bytes
    );
    assert_eq!(std::fs::read_dir(public).unwrap().count(), 1);
}

#[tokio::test]
async fn concurrent_promotion_publishes_one_complete_artifact() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let public = root.path().join("public");
    let promoter = Promoter::new(&public, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    let output = ValidatedOutput::read(protocol(1, 1, &[1, 2, 3, 4]).as_slice())
        .await
        .unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            promoter.promote(id, &output).unwrap()
        });
        let b = scope.spawn(|| {
            barrier.wait();
            promoter.promote(id, &output).unwrap()
        });
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        assert_ne!(a.already_published, b.already_published);
        assert_eq!(a.sha256, b.sha256);
    });
    assert_eq!(std::fs::read_dir(public).unwrap().count(), 1);
}

#[tokio::test]
async fn failed_promotion_leaves_no_partial_public_file() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    let public = root.path().join("public");
    let promoter = Promoter::new(&public, &quarantine).unwrap();
    let id = ObjectId::generate().unwrap();
    let occupied = public.join(format!("{id}.png"));
    std::fs::create_dir(&occupied).unwrap();
    let output = ValidatedOutput::read(protocol(1, 1, &[1, 2, 3, 4]).as_slice())
        .await
        .unwrap();
    assert!(promoter.promote(id, &output).is_err());
    assert_eq!(std::fs::read_dir(&public).unwrap().count(), 1);
    assert_eq!(std::fs::read_dir(occupied).unwrap().count(), 0);
}

#[test]
fn roots_must_be_distinct_and_not_nested() {
    let root = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(root.path().join("private")).unwrap();
    for public in [
        root.path().join("private"),
        root.path().join("private").join("."),
        root.path().join("private").join("public"),
        root.path().to_path_buf(),
    ] {
        assert!(matches!(
            Promoter::new(public, &quarantine),
            Err(MediaError::OverlappingRoots)
        ));
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(128))]
    #[test]
    fn arbitrary_invalid_headers_are_rejected_before_pixel_reads(header in proptest::array::uniform16(proptest::num::u8::ANY)) {
        let width = u32::from_be_bytes(header[8..12].try_into().unwrap());
        let height = u32::from_be_bytes(header[12..16].try_into().unwrap());
        proptest::prop_assume!(&header[..8] != b"IBRGBA01" || !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height));
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut bytes = header.to_vec();
        bytes.extend([255; 64]);
        let mut reader = Cursor::new(bytes);
        proptest::prop_assert!(matches!(runtime.block_on(ValidatedOutput::read(&mut reader)), Err(MediaError::InvalidOutput)));
        proptest::prop_assert_eq!(reader.position(), 16);
    }
    #[test]
    fn arbitrary_invalid_dimensions_are_checked_before_body_reads(width in proptest::num::u32::ANY, height in proptest::num::u32::ANY) {
        proptest::prop_assume!(!(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height));
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut reader = Cursor::new(protocol(width, height, &[255; 64]));
        proptest::prop_assert!(matches!(runtime.block_on(ValidatedOutput::read(&mut reader)), Err(MediaError::InvalidOutput)));
        proptest::prop_assert_eq!(reader.position(), 16);
    }
}
