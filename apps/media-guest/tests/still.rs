use board_media_guest::{
    image_geometry::StillFormat,
    still::{self, Policy},
};

fn policy(format: StillFormat) -> Policy {
    Policy {
        format,
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
