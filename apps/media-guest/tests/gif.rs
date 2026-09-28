use board_media_guest::decode_image;
use std::borrow::Cow;

fn encode(width: u16, height: u16, frames: &[gif::Frame<'_>], comment: bool) -> Vec<u8> {
    let mut encoder =
        gif::Encoder::new(Vec::new(), width, height, &[255, 0, 0, 0, 0, 255]).unwrap();
    if comment {
        encoder
            .write_raw_extension(gif::AnyExtension(0xfe), &[b"owned synthetic comment"])
            .unwrap();
    }
    for frame in frames {
        encoder.write_frame(frame).unwrap();
    }
    encoder.into_inner().unwrap()
}

fn frame(width: u16, height: u16) -> gif::Frame<'static> {
    gif::Frame {
        width,
        height,
        buffer: Cow::Owned(vec![0; usize::from(width) * usize::from(height)]),
        ..Default::default()
    }
}

#[test]
fn single_frame_and_transparency_produce_bounded_rgba_without_metadata() {
    let opaque = frame(1, 1);
    let ordinary = encode(1, 1, std::slice::from_ref(&opaque), false);
    let pixels = decode_image(&ordinary).unwrap();
    assert_eq!(pixels, b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff");
    assert_eq!(
        decode_image(&encode(1, 1, &[opaque], true)).unwrap(),
        pixels
    );
    let mut old_version = ordinary;
    old_version[..6].copy_from_slice(b"GIF87a");
    assert_eq!(decode_image(&old_version).unwrap(), pixels);
    let transparent = gif::Frame {
        width: 2,
        height: 1,
        transparent: Some(1),
        buffer: Cow::Borrowed(&[0, 1]),
        ..Default::default()
    };
    let pixels = decode_image(&encode(2, 1, &[transparent], false)).unwrap();
    assert_eq!(&pixels[16..20], &[255, 0, 0, 255]);
    assert_eq!(&pixels[20..24], &[0, 0, 255, 0]);
    assert_eq!(pixels.len(), 24);
}

#[test]
fn interlaced_rows_are_decoded_in_display_order() {
    let rows = [0, 4, 2, 6, 1, 3, 5, 7];
    let interlaced = gif::Frame {
        width: 2,
        height: 8,
        interlaced: true,
        buffer: Cow::Owned(rows.into_iter().flat_map(|row| [row % 2; 2]).collect()),
        ..Default::default()
    };
    let pixels = decode_image(&encode(2, 8, &[interlaced], false)).unwrap();
    for (row, bytes) in pixels[16..].chunks_exact(8).enumerate() {
        let pixel = if row % 2 == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert_eq!(bytes, pixel.repeat(2));
    }
}

#[test]
fn animation_partial_frames_and_excess_dimensions_are_rejected() {
    assert!(decode_image(&encode(1, 1, &[frame(1, 1), frame(1, 1)], false)).is_err());
    assert!(decode_image(&encode(2, 1, &[frame(1, 1)], false)).is_err());
    let offset = gif::Frame {
        left: 1,
        ..frame(1, 1)
    };
    assert!(decode_image(&encode(2, 1, &[offset], false)).is_err());
    assert!(decode_image(&encode(1025, 1, &[frame(1025, 1)], false)).is_err());
    assert!(decode_image(&encode(1, 1, &[], false)).is_err());
    let mut oversized = b"GIF89a".to_vec();
    oversized.resize(8 * 1024 * 1024 + 1, 0);
    assert!(decode_image(&oversized).is_err());
}

#[test]
fn every_truncation_is_rejected_and_a_healthy_decode_still_works() {
    let input = encode(2, 2, &[frame(2, 2)], false);
    for end in 0..input.len() {
        assert!(decode_image(&input[..end]).is_err(), "length {end}");
    }
    let mut trailing = input.clone();
    trailing.push(0);
    assert!(decode_image(&trailing).is_err());
    assert_eq!(decode_image(&input).unwrap().len(), 16 + 2 * 2 * 4);
}

#[test]
fn maximum_accepted_canvas_stays_within_the_pixel_protocol() {
    let input = encode(1024, 1024, &[frame(1024, 1024)], false);
    let output = decode_image(&input).unwrap();
    assert_eq!(output.len(), 16 + 4 * 1024 * 1024);
    assert_eq!(&output[..16], b"IBRGBA01\0\0\x04\0\0\0\x04\0");
}

#[test]
fn committed_native_fixtures_reproduce_the_owned_generator() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/media/fixtures/gif");
    let transparent = gif::Frame {
        width: 2,
        height: 1,
        transparent: Some(1),
        buffer: Cow::Borrowed(&[0, 1]),
        ..Default::default()
    };
    let interlaced = gif::Frame {
        width: 2,
        height: 8,
        interlaced: true,
        buffer: Cow::Owned(
            [0, 4, 2, 6, 1, 3, 5, 7]
                .into_iter()
                .flat_map(|row| [row % 2; 2])
                .collect(),
        ),
        ..Default::default()
    };
    for (name, width, height, frames) in [
        ("static", 1, 1, vec![frame(1, 1)]),
        ("transparent", 2, 1, vec![transparent]),
        ("interlaced", 2, 8, vec![interlaced]),
        ("animated", 1, 1, vec![frame(1, 1), frame(1, 1)]),
        ("partial", 2, 1, vec![frame(1, 1)]),
        ("too-wide", 1025, 1, vec![frame(1025, 1)]),
    ] {
        assert_eq!(
            std::fs::read(directory.join(format!("{name}.gif"))).unwrap(),
            encode(width, height, &frames, false),
            "{name}"
        );
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn bounded_gif_mutations_never_escape_the_output_contract(
        changes in proptest::collection::vec((0usize..128, proptest::num::u8::ANY), 0..5)
    ) {
        let mut input = encode(2, 4, &[frame(2, 4)], true);
        for (index, byte) in changes { let index = index % input.len(); input[index] = byte; }
        if let Ok(output) = decode_image(&input) {
            proptest::prop_assert!(output.len() >= 20 && output.len() <= 16 + 4 * 1024 * 1024);
            proptest::prop_assert_eq!(&output[..8], b"IBRGBA01");
            let width = u32::from_be_bytes(output[8..12].try_into().unwrap());
            let height = u32::from_be_bytes(output[12..16].try_into().unwrap());
            proptest::prop_assert!((1..=1024).contains(&width) && (1..=1024).contains(&height));
            proptest::prop_assert_eq!(output.len(), 16 + width as usize * height as usize * 4);
        }
    }
}
