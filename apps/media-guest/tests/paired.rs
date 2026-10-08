use board_media_guest::paired::{
    InputKind, MAX_FRAME_BYTES, RESULT_BYTES, decode_disk, kernel_input_kind,
};
use std::io::Cursor;

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[1, 2, 3, 255])
            .unwrap();
    }
    bytes
}

fn disk(image: &[u8], replay: Option<&[u8]>) -> Vec<u8> {
    let mut frame = b"IBPAIR02".to_vec();
    frame.extend_from_slice(&2_u16.to_be_bytes());
    frame.extend_from_slice(&48_u16.to_be_bytes());
    frame.extend_from_slice(&u32::from(replay.is_some()).to_be_bytes());
    frame.extend_from_slice(&(image.len() as u64).to_be_bytes());
    frame.extend_from_slice(&(replay.map_or(0, <[u8]>::len) as u64).to_be_bytes());
    frame.extend_from_slice(&[0x33; 16]);
    frame.extend_from_slice(image);
    frame.extend_from_slice(replay.unwrap_or_default());
    frame.extend_from_slice(b"IBDONE02");
    let mut disk = b"IBJOB002".to_vec();
    disk.extend_from_slice(&(frame.len() as u64).to_be_bytes());
    disk.extend_from_slice(&[0x75; 32]);
    disk.extend_from_slice(&frame);
    disk.resize(disk.len().div_ceil(512) * 512, 0);
    disk
}

#[test]
fn absent_and_present_replay_controls_with_independent_dimensions() {
    for replay in [
        None,
        Some(include_bytes!("fixtures/replay/empty.tgkr").as_slice()),
    ] {
        let output = decode_disk(Cursor::new(disk(&png(), replay))).unwrap();
        assert_eq!(output.len(), RESULT_BYTES);
        assert_eq!(&output[..12], b"IBRES002\0\x02\0\x40");
        assert_eq!(&output[12..16], &u32::from(replay.is_some()).to_be_bytes());
        assert_eq!(&output[16..48], &[0x75; 32]);
        assert_eq!(u64::from_be_bytes(output[48..56].try_into().unwrap()), 20);
        assert_eq!(
            &output[64..84],
            b"IBRGBA01\0\0\0\x01\0\0\0\x01\x01\x02\x03\xff"
        );
        let wire_len = u64::from_be_bytes(output[56..64].try_into().unwrap()) as usize;
        assert_eq!(wire_len != 0, replay.is_some());
        if replay.is_some() {
            assert_eq!(&output[84..92], b"IBRPLY01");
            // Fixture canvas is 640x480, while PNG is 1x1.
            assert_eq!(&output[108..112], &[2, 128, 1, 224]);
        }
        assert!(output[84 + wire_len..].iter().all(|&b| b == 0));
    }
}

#[test]
fn rejects_truncation_extension_nonzero_padding_and_wrong_envelope() {
    let valid = disk(&png(), None);
    for end in [0, 8, 47, 48, 49, valid.len() - 1] {
        assert!(decode_disk(Cursor::new(&valid[..end])).is_err());
    }
    let mut extra = valid.clone();
    extra.push(0);
    assert!(decode_disk(Cursor::new(extra)).is_err());
    let mut padding = valid.clone();
    *padding.last_mut().unwrap() = 1;
    assert!(decode_disk(Cursor::new(padding)).is_err());
    for offset in [0, 48, 56, 58, 60] {
        let mut bad = valid.clone();
        bad[offset] ^= 0x80;
        assert!(decode_disk(Cursor::new(bad)).is_err());
    }
    for size in [0, 56, MAX_FRAME_BYTES + 1, u64::MAX] {
        let mut bad = valid.clone();
        bad[8..16].copy_from_slice(&size.to_be_bytes());
        assert!(decode_disk(Cursor::new(bad)).is_err());
    }
}

#[test]
fn rejects_component_overflows_presence_mismatch_and_trailer() {
    let valid = disk(&png(), None);
    for (offset, size) in [
        (64, 0),
        (64, 8 * 1024 * 1024 + 1),
        (64, u64::MAX),
        (72, 1),
        (72, u64::MAX),
    ] {
        let mut bad = valid.clone();
        bad[offset..offset + 8].copy_from_slice(&size.to_be_bytes());
        assert!(decode_disk(Cursor::new(bad)).is_err());
    }
    let mut present_empty = valid.clone();
    present_empty[63] = 1;
    assert!(decode_disk(Cursor::new(present_empty)).is_err());
    let mut bad_trailer = valid.clone();
    bad_trailer[96 + png().len()] ^= 1;
    assert!(decode_disk(Cursor::new(bad_trailer)).is_err());
    assert!(decode_disk(Cursor::new(disk(&png(), Some(b"invalid TGKR")))).is_err());
    assert!(decode_disk(Cursor::new(disk(b"GIF89a", None))).is_err());
    assert!(decode_disk(Cursor::new(disk(b"\xff\xd8\xff", None))).is_err());
}

#[test]
fn trusted_mode_selection_does_not_sniff_and_rejects_duplicates() {
    assert_eq!(
        kernel_input_kind("quiet reboot=k").unwrap(),
        InputKind::ImageV1
    );
    assert_eq!(
        kernel_input_kind("board_media_input_kind=image-v1").unwrap(),
        InputKind::ImageV1
    );
    assert_eq!(
        kernel_input_kind("quiet board_media_input_kind=paired-v2").unwrap(),
        InputKind::PairedV2
    );
    for args in [
        "board_media_input_kind",
        "board_media_input_kind=",
        "board_media_input_kind=unknown",
        "board_media_input_kind=paired-v2 board_media_input_kind=paired-v2",
        "board_media_input_kind=image-v1 board_media_input_kind=paired-v2",
    ] {
        assert!(kernel_input_kind(args).is_err());
    }
    assert!(InputKind::parse("IBJOB002").is_err());
    assert_eq!(InputKind::ImageV1.output_bytes(), 4_194_816);
    assert_eq!(InputKind::PairedV2.output_bytes(), RESULT_BYTES as u64);
}

#[test]
fn valid_v1_jpeg_and_gif_do_not_enable_paired_format_fallback() {
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&[20, 40, 60], 1, 1, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let mut encoder = gif::Encoder::new(Vec::new(), 1, 1, &[255, 0, 0, 0, 0, 255]).unwrap();
    encoder
        .write_frame(&gif::Frame {
            width: 1,
            height: 1,
            buffer: std::borrow::Cow::Borrowed(&[0]),
            ..Default::default()
        })
        .unwrap();
    let gif = encoder.into_inner().unwrap();
    for image in [jpeg, gif] {
        assert!(board_media_guest::decode_image(&image).is_ok());
        assert!(decode_disk(Cursor::new(disk(&image, None))).is_err());
    }
}

#[test]
fn disk_reader_handles_short_reads_without_accepting_short_disks() {
    struct ShortReads(Cursor<Vec<u8>>);
    impl std::io::Read for ShortReads {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let count = bytes.len().min(3);
            std::io::Read::read(&mut self.0, &mut bytes[..count])
        }
    }
    assert!(decode_disk(ShortReads(Cursor::new(disk(&png(), None)))).is_ok());
}
