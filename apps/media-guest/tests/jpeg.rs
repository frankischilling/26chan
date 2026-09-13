use board_media_guest::decode_image;
use jpeg_encoder::{ColorType, Encoder};
use proptest::prelude::*;

fn encoded(width: u16, height: u16, color: ColorType, pixels: &[u8], progressive: bool) -> Vec<u8> {
    let mut bytes = vec![];
    let mut encoder = Encoder::new(&mut bytes, 100);
    encoder.set_progressive(progressive);
    encoder.encode(pixels, width, height, color).unwrap();
    bytes
}

fn frame(bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    let pixels = decode_image(bytes).unwrap();
    assert_eq!(&pixels[..8], b"IBRGBA01");
    assert_eq!(&pixels[8..12], &width.to_be_bytes());
    assert_eq!(&pixels[12..16], &height.to_be_bytes());
    assert_eq!(pixels.len(), 16 + width as usize * height as usize * 4);
    assert!(pixels[16..].chunks_exact(4).all(|pixel| pixel[3] == 255));
    pixels
}

#[test]
fn baseline_and_progressive_rgb_and_gray_become_opaque_rgba() {
    for progressive in [false, true] {
        for (color, sample, expected) in [
            (ColorType::Rgb, vec![230, 40, 25], [230u8, 40, 25]),
            (ColorType::Luma, vec![80], [80, 80, 80]),
        ] {
            let input = encoded(17, 9, color, &sample.repeat(17 * 9), progressive);
            for pixel in frame(&input, 17, 9)[16..].chunks_exact(4) {
                assert!(
                    pixel[..3]
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| actual.abs_diff(expected) <= 3)
                );
            }
        }
    }
}

#[test]
fn cmyk_and_ycck_convert_to_rgb() {
    for color in [ColorType::Cmyk, ColorType::CmykAsYcck] {
        let input = encoded(8, 8, color, &[0, 255, 255, 0].repeat(64), false);
        let pixels = frame(&input, 8, 8);
        for pixel in pixels[16..].chunks_exact(4) {
            assert!(
                pixel[0] >= 250 && pixel[1] <= 5 && pixel[2] <= 5,
                "{pixel:?}"
            );
        }
    }
}

#[test]
fn metadata_is_not_part_of_the_pixel_response() {
    let input = encoded(8, 8, ColorType::Rgb, &[230, 40, 25].repeat(64), false);
    let expected = decode_image(&input).unwrap();
    // Synthetic APP1 metadata, not a real camera/user record. Metadata remains
    // inside the guest and is neither interpreted by the host nor published.
    let metadata = b"Exif\0\0SYNTHETIC-PRIVATE-METADATA";
    let mut tagged = input[..2].to_vec();
    tagged.extend_from_slice(b"\xff\xe1");
    tagged.extend_from_slice(&((metadata.len() + 2) as u16).to_be_bytes());
    tagged.extend_from_slice(metadata);
    tagged.extend_from_slice(&input[2..]);
    assert_eq!(decode_image(&tagged).unwrap(), expected);
}

#[test]
fn rejects_excess_dimensions_truncation_trailers_and_unsupported_input() {
    for (width, height) in [(1025, 1), (1, 1025)] {
        assert!(
            decode_image(&encoded(
                width,
                height,
                ColorType::Luma,
                &vec![80; width as usize * height as usize],
                false
            ))
            .is_err()
        );
    }
    let input = encoded(8, 8, ColorType::Rgb, &[230, 40, 25].repeat(64), true);
    for end in [
        0,
        1,
        2,
        3,
        input.len() / 2,
        input.len() - 2,
        input.len() - 1,
    ] {
        assert!(decode_image(&input[..end]).is_err(), "{end}");
    }
    let mut trailing = input.clone();
    trailing.extend_from_slice(b"trailing bytes");
    assert!(decode_image(&trailing).is_err());
    for input in [
        b"GIF89a".as_slice(),
        b"%PDF-1.7",
        b"RIFF",
        b"\xff\xd8\xff\xd9",
        b"",
    ] {
        assert!(decode_image(input).is_err());
    }
    let mut oversized = vec![0; 8 * 1024 * 1024 + 1];
    oversized[..3].copy_from_slice(b"\xff\xd8\xff");
    assert!(decode_image(&oversized).is_err());
}

#[test]
fn maximum_dimensions_fit_the_existing_output_protocol() {
    let input = encoded(1024, 1024, ColorType::Luma, &vec![80; 1024 * 1024], false);
    frame(&input, 1024, 1024);
}

#[test]
fn excessive_progressive_scans_are_rejected_with_a_healthy_control() {
    let data = [230, 40, 25].repeat(64);
    frame(&encoded(8, 8, ColorType::Rgb, &data, true), 8, 8);
    let mut bytes = vec![];
    let mut encoder = Encoder::new(&mut bytes, 100);
    encoder.set_progressive_scans(64);
    encoder.encode(&data, 8, 8, ColorType::Rgb).unwrap();
    assert!(decode_image(&bytes).is_err());
}

#[test]
fn committed_synthetic_fixtures_match_the_pinned_encoder() {
    for (bytes, color, sample, progressive, width) in [
        (
            include_bytes!("../../../tests/media/fixtures/jpeg/baseline.jpg").as_slice(),
            ColorType::Rgb,
            vec![255, 0, 0],
            false,
            1u16,
        ),
        (
            include_bytes!("../../../tests/media/fixtures/jpeg/progressive.jpg").as_slice(),
            ColorType::Rgb,
            vec![255, 0, 0],
            true,
            1,
        ),
        (
            include_bytes!("../../../tests/media/fixtures/jpeg/grayscale.jpg").as_slice(),
            ColorType::Luma,
            vec![80],
            false,
            1,
        ),
        (
            include_bytes!("../../../tests/media/fixtures/jpeg/cmyk.jpg").as_slice(),
            ColorType::Cmyk,
            vec![0, 255, 255, 0],
            false,
            1,
        ),
        (
            include_bytes!("../../../tests/media/fixtures/jpeg/too-wide.jpg").as_slice(),
            ColorType::Luma,
            vec![80; 1025],
            false,
            1025,
        ),
    ] {
        assert_eq!(bytes, encoded(width, 1, color, &sample, progressive));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn bounded_jpeg_mutations_never_escape_the_pixel_protocol(index in 0usize..1200, byte in any::<u8>()) {
        let mut input = encoded(8, 8, ColorType::Rgb, &[230, 40, 25].repeat(64), true);
        let position = index % input.len();
        input[position] = byte;
        if let Ok(output) = decode_image(&input) {
            prop_assert_eq!(&output[..8], b"IBRGBA01");
            let width = u32::from_be_bytes(output[8..12].try_into().unwrap());
            let height = u32::from_be_bytes(output[12..16].try_into().unwrap());
            prop_assert!((1..=1024).contains(&width) && (1..=1024).contains(&height));
            prop_assert_eq!(output.len(), 16 + width as usize * height as usize * 4);
        }
    }
}
