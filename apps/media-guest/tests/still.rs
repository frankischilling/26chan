use board_media_guest::{
    image_geometry::StillFormat,
    still::{self, Policy},
};

fn policy(format: StillFormat) -> Policy {
    Policy {
        format,
        exif_mode: 0,
        strip_exif: false,
        mobile: true,
        thread: true,
        blue_background: false,
        thumbnail: (250, 250),
    }
}

fn png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
    bytes
}

fn jpeg(width: u16, height: u16, progressive: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut bytes, 90);
    encoder.set_progressive(progressive);
    encoder
        .encode(
            &[220, 40, 25].repeat(width as usize * height as usize),
            width,
            height,
            jpeg_encoder::ColorType::Rgb,
        )
        .unwrap();
    bytes
}

fn number(output: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(output[offset..offset + 4].try_into().unwrap())
}

fn retained(output: &[u8]) -> &[u8] {
    &output[64..64 + number(output, 48) as usize]
}

fn verify_output(output: &[u8], policy: Policy) {
    assert_eq!(output.len(), still::OUTPUT_BYTES);
    assert_eq!(&output[..8], b"IBIMG006");
    assert_eq!(&output[8..24], &policy.header().unwrap());
    assert_eq!(&output[60..64], &[0; 4]);
    let thumbnail_start = 64 + number(output, 48) as usize;
    let mobile_start = thumbnail_start + number(output, 52) as usize;
    let end = mobile_start + number(output, 56) as usize;
    let thumbnail = &output[thumbnail_start..mobile_start];
    assert!(thumbnail.starts_with(b"\xff\xd8\xff"));
    assert!(thumbnail.ends_with(b"\xff\xd9"));
    if mobile_start != end {
        assert!(output[mobile_start..end].starts_with(b"\xff\xd8\xff"));
        assert!(output[mobile_start..end].ends_with(b"\xff\xd9"));
    } else {
        assert_eq!((number(output, 40), number(output, 44)), (0, 0));
    }
    assert!(output[end..].iter().all(|&byte| byte == 0));
}

#[test]
fn png_is_retained_and_small_thumbnails_are_not_enlarged() {
    let input = png(
        17,
        9,
        png::ColorType::Rgba,
        &[80, 40, 20, 100].repeat(17 * 9),
    );
    let policy = policy(StillFormat::Png);
    let output = still::process(&input, policy).unwrap();
    verify_output(&output, policy);
    assert_eq!(retained(&output), input);
    assert_eq!((number(&output, 24), number(&output, 28)), (17, 9));
    assert_eq!((number(&output, 32), number(&output, 36)), (17, 9));
}

#[test]
fn jpeg_retention_baseline_and_progressive_and_mobile_geometry() {
    for progressive in [false, true] {
        let input = jpeg(1100, 500, progressive);
        let policy = policy(StillFormat::Jpeg);
        let output = still::process(&input, policy).unwrap();
        verify_output(&output, policy);
        assert_eq!(retained(&output), input);
        assert_eq!((number(&output, 32), number(&output, 36)), (250, 113));
        assert_eq!((number(&output, 40), number(&output, 44)), (1024, 465));
    }
}

#[test]
fn wide_compressed_png_does_not_get_a_mobile_image() {
    let input = png(2048, 512, png::ColorType::Grayscale, &vec![90; 2048 * 512]);
    assert!(input.len() <= 524_288);
    let policy = policy(StillFormat::Png);
    let output = still::process(&input, policy).unwrap();
    verify_output(&output, policy);
    assert_eq!((number(&output, 32), number(&output, 36)), (250, 62));
    assert_eq!(number(&output, 56), 0);
}

#[test]
fn strict_policy_roundtrip_and_rejections() {
    for format in [StillFormat::Png, StillFormat::Jpeg] {
        for flags in 0..16 {
            let mut bytes = policy(format).header().unwrap();
            bytes[8] = flags;
            assert_eq!(Policy::parse(&bytes).unwrap().header().unwrap(), bytes);
        }
    }
    let valid = policy(StillFormat::Png).header().unwrap();
    for (offset, value) in [(0, 0), (8, 16), (9, 0), (10, 1), (11, 0), (14, 1), (15, 1)] {
        let mut bytes = valid;
        bytes[offset] = value;
        assert!(Policy::parse(&bytes).is_err(), "{offset}");
    }
    assert!(Policy::parse(&valid[..15]).is_err());
}

#[test]
fn malformed_images_and_format_mismatch_fail() {
    let png = png(8, 8, png::ColorType::Rgb, &[80, 40, 20].repeat(64));
    let jpeg = jpeg(8, 8, false);
    for (input, format) in [(&png, StillFormat::Png), (&jpeg, StillFormat::Jpeg)] {
        for end in [0, 1, input.len() / 2, input.len() - 1] {
            assert!(still::process(&input[..end], policy(format)).is_err());
        }
    }
    assert!(still::process(&png, policy(StillFormat::Jpeg)).is_err());
    assert!(still::process(&jpeg, policy(StillFormat::Png)).is_err());
    let mut bad_crc = png;
    bad_crc[29] ^= 1;
    assert!(still::process(&bad_crc, policy(StillFormat::Png)).is_err());
    let mut trailer = jpeg;
    trailer.extend_from_slice(b"untrusted trailer");
    assert!(still::process(&trailer, policy(StillFormat::Jpeg)).is_err());
}

#[test]
fn disk_frame_padding_and_end_are_checked() {
    let input = png(8, 8, png::ColorType::Rgb, &[80, 40, 20].repeat(64));
    let policy = policy(StillFormat::Png);
    let mut disk = ((16 + input.len()) as u64).to_be_bytes().to_vec();
    disk.extend_from_slice(&policy.header().unwrap());
    disk.extend_from_slice(&input);
    disk.resize(disk.len().div_ceil(512) * 512, 0);
    let output = still::decode_disk(disk.as_slice()).unwrap();
    verify_output(&output, policy);
    let mut extra = disk.clone();
    extra.push(0);
    assert!(still::decode_disk(extra.as_slice()).is_err());
    assert!(still::decode_disk(&disk[..disk.len() - 1]).is_err());
    *disk.last_mut().unwrap() = 1;
    assert!(still::decode_disk(disk.as_slice()).is_err());
}

#[tokio::test]
async fn guest_output_matches_independent_host_envelope_checks() {
    use board_media::still::{StillFormat as HostFormat, StillPolicy, ValidatedStill};
    let input = png(17, 9, png::ColorType::Rgb, &[80, 40, 20].repeat(17 * 9));
    let guest_policy = policy(StillFormat::Png);
    let host_policy = StillPolicy {
        format: HostFormat::Png,
        exif_mode: 0,
        strip_exif: guest_policy.strip_exif,
        mobile: guest_policy.mobile,
        thread: guest_policy.thread,
        blue_background: guest_policy.blue_background,
        thumbnail: guest_policy.thumbnail,
    };
    assert_eq!(
        guest_policy.header().unwrap(),
        host_policy.header().unwrap()
    );
    let output = still::process(&input, guest_policy).unwrap();
    let checked = ValidatedStill::read_disk(std::io::Cursor::new(output), host_policy)
        .await
        .unwrap();
    assert_eq!(checked.retained().bytes(), input);
    assert_eq!(checked.retained().dimensions(), (17, 9));
    assert_eq!(checked.thumbnail().dimensions(), (17, 9));
}

#[test]
fn authenticated_board_policy_matches_all_captured_source_defaults() {
    use board_media::still::{StillFormat as HostFormat, source_policy};
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let boards = reference["source_image_presentation"].as_object().unwrap();
    assert_eq!(boards.len(), 82);
    for (board, expected) in boards {
        for thread in [false, true] {
            for capcode in [false, true] {
                for format in [HostFormat::Png, HostFormat::Jpeg] {
                    let policy = source_policy(board, format, thread, capcode).unwrap();
                    let bounds = &expected[if thread {
                        "thread_thumbnail"
                    } else {
                        "reply_thumbnail"
                    }];
                    assert_eq!(
                        policy.thumbnail,
                        (
                            bounds[0].as_u64().unwrap() as u16,
                            bounds[1].as_u64().unwrap() as u16
                        )
                    );
                    assert_eq!(policy.mobile, expected["mobile_resize"].as_bool().unwrap());
                    assert_eq!(
                        policy.blue_background,
                        expected["blue_thumbnail_background"].as_bool().unwrap()
                    );
                    assert_eq!(
                        policy.strip_exif,
                        expected["strip_exif"].as_bool().unwrap()
                            || (capcode && expected["strip_exif_on_capcode"].as_bool().unwrap())
                    );
                    assert_eq!(policy.thread, thread);
                    assert_eq!(policy.format, format);
                    assert!(policy.header().is_ok());
                }
            }
        }
    }
    for unknown in ["", "/p/", "P", "p\0", "unknown", "../p"] {
        assert!(source_policy(unknown, HostFormat::Jpeg, true, false).is_none());
    }
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    [
        &u32::try_from(data.len()).unwrap().to_be_bytes(),
        kind,
        data,
        &(!crc).to_be_bytes(),
    ]
    .concat()
}

#[test]
fn png_cleanup_preserves_allowed_chunks_and_drops_text_and_trailing_bytes() {
    let original = png(8, 8, png::ColorType::Rgb, &[80, 40, 20].repeat(64));
    let gamma = png_chunk(b"gAMA", &45_455u32.to_be_bytes());
    let text = png_chunk(b"tEXt", b"Comment\0SYNTHETIC-PRIVATE-METADATA");
    let expected = [
        original[..33].as_ref(),
        gamma.as_slice(),
        original[33..].as_ref(),
    ]
    .concat();
    let input = [
        original[..33].as_ref(),
        gamma.as_slice(),
        text.as_slice(),
        original[33..].as_ref(),
        b"synthetic trailing bytes",
    ]
    .concat();
    let output = still::process(&input, policy(StillFormat::Png)).unwrap();
    assert_eq!(retained(&output), expected);
    let animation = png_chunk(b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
    let animated = [
        original[..33].as_ref(),
        animation.as_slice(),
        original[33..].as_ref(),
    ]
    .concat();
    assert!(still::process(&animated, policy(StillFormat::Png)).is_err());
}

#[test]
fn retained_jpeg_orientation_does_not_rotate_source_pixels_or_dimensions() {
    let original = jpeg(17, 9, false);
    // Valid little-endian TIFF orientation 6, entirely synthetic.
    let metadata = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let tagged = [
        original[..2].as_ref(),
        b"\xff\xe1",
        &((metadata.len() + 2) as u16).to_be_bytes(),
        metadata,
        original[2..].as_ref(),
    ]
    .concat();
    let expected = still::process(&original, policy(StillFormat::Jpeg)).unwrap();
    let output = still::process(&tagged, policy(StillFormat::Jpeg)).unwrap();
    assert_eq!(retained(&output), tagged);
    assert_eq!((number(&output, 24), number(&output, 28)), (17, 9));
    let thumbnail = |output: &[u8]| {
        let start = 64 + number(output, 48) as usize;
        output[start..start + number(output, 52) as usize].to_vec()
    };
    assert_eq!(thumbnail(&output), thumbnail(&expected));
}
