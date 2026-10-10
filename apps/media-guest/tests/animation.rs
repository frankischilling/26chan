use board_media::{
    ValidatedOutput,
    animation::{self, ValidatedAnimation},
};
use board_media_guest::animation::decode;
use std::{borrow::Cow, io::Cursor};

fn frame(width: u16, height: u16, pixels: Vec<u8>) -> gif::Frame<'static> {
    gif::Frame {
        width,
        height,
        buffer: Cow::Owned(pixels),
        ..Default::default()
    }
}

fn encode(
    width: u16,
    height: u16,
    frames: &[gif::Frame<'_>],
    repeat: Option<gif::Repeat>,
    comment: bool,
) -> Vec<u8> {
    let mut encoder =
        gif::Encoder::new(Vec::new(), width, height, &[255, 0, 0, 0, 0, 255]).unwrap();
    if let Some(repeat) = repeat {
        encoder.set_repeat(repeat).unwrap();
    }
    if comment {
        encoder
            .write_raw_extension(gif::AnyExtension(0xfe), &[b"private comment"])
            .unwrap();
    }
    for frame in frames {
        encoder.write_frame(frame).unwrap();
    }
    encoder.into_inner().unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn validate(wire: &[u8]) -> ValidatedAnimation {
    runtime().block_on(ValidatedAnimation::read(wire)).unwrap()
}

type InspectedFrame = (
    u16,
    u16,
    u16,
    u16,
    u16,
    gif::DisposalMethod,
    Option<u8>,
    Vec<u8>,
    Vec<u8>,
);

fn inspect(input: &[u8]) -> Vec<InspectedFrame> {
    let mut decoder = gif::Decoder::new(Cursor::new(input)).unwrap();
    let global = decoder.global_palette().unwrap_or_default().to_vec();
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        frames.push((
            frame.left,
            frame.top,
            frame.width,
            frame.height,
            frame.delay,
            frame.dispose,
            frame.transparent,
            frame.palette.as_deref().unwrap_or(&global).to_vec(),
            frame.buffer.to_vec(),
        ));
    }
    frames
}

#[test]
fn preserves_local_colors_offsets_delays_transparency_disposal_and_loop_control() {
    let mut frames = vec![frame(3, 2, vec![0, 1, 0, 1, 0, 1])];
    frames[0].delay = 7;
    for (index, disposal) in [
        gif::DisposalMethod::Any,
        gif::DisposalMethod::Keep,
        gif::DisposalMethod::Background,
        gif::DisposalMethod::Previous,
    ]
    .into_iter()
    .enumerate()
    {
        frames.push(gif::Frame {
            left: 1,
            top: 1,
            delay: 13 + index as u16,
            transparent: Some(1),
            dispose: disposal,
            palette: Some(vec![0, 255, 0, 255, 255, 0]),
            ..frame(2, 1, vec![0, 1])
        });
    }
    for repeat in [
        None,
        Some(gif::Repeat::Infinite),
        Some(gif::Repeat::Finite(1)),
        Some(gif::Repeat::Finite(65535)),
    ] {
        let input = encode(3, 2, &frames, repeat, true);
        let wire = decode(&input).unwrap();
        let animation = validate(&wire);
        assert_eq!(animation.dimensions(), (3, 2));
        assert_eq!(animation.frame_count(), 5);
        let encoded = animation.encode().unwrap();
        let output = encoded.bytes();
        assert!(
            !output
                .windows(b"private comment".len())
                .any(|part| part == b"private comment")
        );
        assert!(!output.windows(2).any(|part| part == [0x21, 0xfe]));
        assert_eq!(
            gif::Decoder::new(Cursor::new(output)).unwrap().repeat(),
            repeat.unwrap_or(gif::Repeat::Finite(0))
        );
        let original = inspect(&input);
        let normalized = inspect(output);
        for (original, normalized) in original.iter().zip(&normalized) {
            assert_eq!(
                (
                    &original.0,
                    &original.1,
                    &original.2,
                    &original.3,
                    &original.4,
                    &original.5,
                    &original.6,
                    &original.8
                ),
                (
                    &normalized.0,
                    &normalized.1,
                    &normalized.2,
                    &normalized.3,
                    &normalized.4,
                    &normalized.5,
                    &normalized.6,
                    &normalized.8
                )
            );
            assert_eq!(original.7, normalized.7[..original.7.len()]);
        }
        assert_eq!(original.len(), normalized.len());
        assert_eq!(encoded.sha256().len(), 64);
        assert_eq!(encoded.md5().len(), 32);
        assert_eq!(encoded.dimensions(), (3, 2));
        assert_eq!(animation.encode().unwrap().bytes(), output);
        // The encoder pads local tables to 256 entries, so compare semantics
        // above and independently validate the regenerated wire below.
        assert_eq!(validate(&decode(output).unwrap()).frame_count(), 5);
    }
}

#[test]
fn loop_extensions_after_a_frame_and_alternate_identifier_are_kept() {
    let mut input = encode(
        1,
        1,
        &[frame(1, 1, vec![0]), frame(1, 1, vec![1])],
        None,
        false,
    );
    let trailer = input.len() - 1;
    input.splice(
        trailer..trailer,
        b"\x21\xff\x0bANIMEXTS1.0\x03\x01\x05\x00\x00"
            .iter()
            .copied(),
    );
    let encoded = validate(&decode(&input).unwrap()).encode().unwrap();
    assert_eq!(
        gif::Decoder::new(Cursor::new(encoded.bytes()))
            .unwrap()
            .repeat(),
        gif::Repeat::Finite(5)
    );
    let mut duplicate = input.clone();
    let trailer = duplicate.len() - 1;
    duplicate.splice(
        trailer..trailer,
        b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x01\x00\x00"
            .iter()
            .copied(),
    );
    assert!(decode(&duplicate).is_err());
}

#[test]
fn partial_first_frame_thumbnail_has_correct_offsets_transparency_and_static_png() {
    let input = encode(
        3,
        2,
        &[
            gif::Frame {
                left: 1,
                top: 1,
                transparent: Some(1),
                ..frame(2, 1, vec![0, 1])
            },
            frame(3, 2, vec![1; 6]),
        ],
        Some(gif::Repeat::Infinite),
        false,
    );
    let animation = validate(&decode(&input).unwrap());
    let thumbnail = animation.first_frame().unwrap().thumbnail().unwrap();
    assert_eq!(thumbnail.dimensions(), (3, 2));
    // Verify first-frame RGBA using the public pixel protocol rather than a new
    // production GIF parser in the host dependency graph.
    let mut expected = b"IBRGBA01\0\0\0\x03\0\0\0\x02".to_vec();
    let mut pixels = vec![0; 24];
    pixels[16..20].copy_from_slice(&[255, 0, 0, 255]);
    expected.extend_from_slice(&pixels);
    let png = runtime()
        .block_on(ValidatedOutput::read(expected.as_slice()))
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(png.sha256(), thumbnail.sha256());
    assert_eq!(png.md5(), thumbnail.md5());
}

#[test]
fn interlaced_input_is_normalized_in_display_order() {
    let input = encode(
        2,
        8,
        &[gif::Frame {
            interlaced: true,
            ..frame(
                2,
                8,
                [0, 4, 2, 6, 1, 3, 5, 7]
                    .into_iter()
                    .flat_map(|row| [row % 2; 2])
                    .collect(),
            )
        }],
        None,
        false,
    );
    let encoded = validate(&decode(&input).unwrap()).encode().unwrap();
    let frames = inspect(encoded.bytes());
    assert_eq!(
        frames[0].8,
        (0..8).flat_map(|row| [row % 2; 2]).collect::<Vec<u8>>()
    );
}

#[test]
fn literal_encoding_handles_every_byte_and_repeated_clear_code_boundaries() {
    for size in [1, 2, 253, 254, 255, 256, 511, 512, 1024] {
        let palette = (0..=255)
            .flat_map(|value| [value, value, 255 - value])
            .collect();
        let pixels: Vec<u8> = (0..size).map(|i| i as u8).collect();
        let input = encode(
            size,
            1,
            &[gif::Frame {
                palette: Some(palette),
                ..frame(size, 1, pixels.clone())
            }],
            None,
            false,
        );
        let encoded =
            validate(&decode(&input).unwrap_or_else(|error| panic!("input size {size}: {error}")))
                .encode()
                .unwrap();
        assert_eq!(inspect(encoded.bytes())[0].8, pixels);
    }
}

#[test]
fn rejects_incomplete_trailing_unknown_text_user_input_reserved_control_and_invalid_pixels() {
    let input = encode(
        2,
        2,
        &[frame(2, 2, vec![0, 1, 1, 0]), frame(2, 2, vec![1, 0, 0, 1])],
        None,
        true,
    );
    for end in 0..input.len() {
        assert!(decode(&input[..end]).is_err(), "GIF truncation {end}");
    }
    for suffix in [b";".as_slice(), b"JUNK;", input.as_slice()] {
        let mut corrupted = input.clone();
        corrupted.extend_from_slice(suffix);
        assert!(decode(&corrupted).is_err());
    }
    for extension in [
        b"\x21\x01\x00".as_slice(),
        b"\x21\x77\x00",
        b"\x21\xf9\x04\x02\0\0\0\0",
        b"\x21\xf9\x04\x10\0\0\0\0",
        b"\x21\xf9\x04\x80\0\0\0\0",
    ] {
        let mut corrupted = input.clone();
        let end = corrupted.len() - 1;
        corrupted.splice(end..end, extension.iter().copied());
        assert!(decode(&corrupted).is_err());
    }
    let broken = encode(1, 1, &[frame(1, 1, vec![7])], None, false);
    assert!(decode(&broken).is_err());
    assert!(
        decode(&encode(
            1025,
            1,
            &[frame(1025, 1, vec![0; 1025])],
            None,
            false
        ))
        .is_err()
    );
    assert!(decode(&encode(2, 2, &[], None, false)).is_err());
    assert!(
        board_media_guest::decode_image(&input).is_err(),
        "legacy output must not silently accept animation"
    );
    assert_eq!(validate(&decode(&input).unwrap()).frame_count(), 2);
}

#[test]
fn host_rejects_every_truncation_and_independently_checks_all_neutral_fields() {
    let input = encode(2, 2, &[frame(2, 2, vec![0, 1, 1, 0])], None, false);
    let wire = decode(&input).unwrap();
    let runtime = runtime();
    for end in 0..wire.len() {
        assert!(
            runtime
                .block_on(ValidatedAnimation::read(&wire[..end]))
                .is_err()
        );
    }
    let frame_offset = 32 + 6;
    let cases = [
        (0, 0),
        (8, 255),
        (9, 0),
        (10, 255),
        (11, 0),
        (12, 255),
        (13, 255),
        (14, 1),
        (15, 0),
        (16, 2),
        (17, 2),
        (18, 255),
        (22, 1),
        (31, 1),
        (frame_offset, 255),
        (frame_offset + 4, 255),
        (frame_offset + 5, 255),
        (frame_offset + 7, 0),
        (frame_offset + 11, 3),
        (frame_offset + 12, 2),
        (frame_offset + 13, 1),
        (frame_offset + 14, 4),
        (frame_offset + 15, 1),
        (frame_offset + 19, 5),
        (frame_offset + 20 + 6, 2),
    ];
    for (offset, value) in cases {
        let mut corrupted = wire.clone();
        corrupted[offset] = value;
        assert!(
            runtime
                .block_on(ValidatedAnimation::read(corrupted.as_slice()))
                .is_err(),
            "field offset {offset} value {value}"
        );
    }
    let mut trailing = wire.clone();
    trailing.push(0);
    assert!(
        runtime
            .block_on(ValidatedAnimation::read(trailing.as_slice()))
            .is_err()
    );
    assert!(
        runtime
            .block_on(ValidatedOutput::read(wire.as_slice()))
            .is_err()
    );
}

#[test]
fn fixed_disk_checks_zero_padding_exact_length_and_eof() {
    let wire = decode(&encode(1, 1, &[frame(1, 1, vec![0])], None, false)).unwrap();
    let mut disk = wire.clone();
    disk.resize(animation::OUTPUT_DISK_BYTES as usize, 0);
    let runtime = runtime();
    assert_eq!(
        runtime
            .block_on(ValidatedAnimation::read_disk(disk.as_slice()))
            .unwrap()
            .frame_count(),
        1
    );
    for offset in [wire.len(), disk.len() - 1] {
        disk[offset] = 1;
        assert!(
            runtime
                .block_on(ValidatedAnimation::read_disk(disk.as_slice()))
                .is_err()
        );
        disk[offset] = 0;
    }
    assert!(
        runtime
            .block_on(ValidatedAnimation::read_disk(&disk[..disk.len() - 1]))
            .is_err()
    );
    disk.push(0);
    assert!(
        runtime
            .block_on(ValidatedAnimation::read_disk(disk.as_slice()))
            .is_err()
    );
}

#[test]
fn guest_gif_disk_requires_length_padding_eof_and_an_explicit_mode() {
    let input = encode(1, 1, &[frame(1, 1, vec![0])], None, false);
    let mut disk = (input.len() as u64).to_be_bytes().to_vec();
    disk.extend_from_slice(&input);
    let used = disk.len();
    disk.resize(used.next_multiple_of(512), 0);
    let expected = decode(&input).unwrap();
    assert_eq!(
        board_media_guest::animation::decode_disk(disk.as_slice()).unwrap(),
        expected
    );
    for end in 0..disk.len() {
        assert!(board_media_guest::animation::decode_disk(&disk[..end]).is_err());
    }
    disk[used] = 1;
    assert!(board_media_guest::animation::decode_disk(disk.as_slice()).is_err());
    disk[used] = 0;
    disk.push(0);
    assert!(board_media_guest::animation::decode_disk(disk.as_slice()).is_err());
    for count in [0u64, 8_388_609, u64::MAX] {
        assert!(board_media_guest::animation::decode_disk(count.to_be_bytes().as_slice()).is_err());
    }
    assert!(board_media_guest::animation::decode_disk(b"IBJOB003".as_slice()).is_err());
}

#[test]
fn complete_gif_framing_cannot_hide_missing_end_codes_or_wrong_decoded_lengths() {
    // Independent one-pixel stream: clear(4), literal(0), end(5), 3-bit LSB.
    let header = b"GIF89a\x01\0\x01\0\x80\0\0\xff\0\0\0\0\xff\x2c\0\0\0\0\x01\0\x01\0\0\x02";
    let valid = [header.as_slice(), b"\x02\x44\x01\0\x3b"].concat();
    assert_eq!(validate(&decode(&valid).unwrap()).dimensions(), (1, 1));
    let absent = [header.as_slice(), b"\x01\x04\0\x3b"].concat();
    assert!(decode(&absent).is_err());
    let original = encode(2, 1, &[frame(2, 1, vec![0, 1])], None, false);
    for width in [1u8, 3] {
        let mut changed = original.clone();
        changed[6] = 3; // Larger canvas keeps both descriptors in bounds.
        let descriptor = changed
            .windows(8)
            .position(|part| part == b"\x2c\0\0\0\0\x02\0\x01")
            .unwrap();
        changed[descriptor + 5] = width;
        assert!(decode(&changed).is_err(), "decoded width {width}");
    }
}

#[test]
fn finite_frame_and_aggregate_pixel_budgets_are_enforced_before_guest_decompression() {
    let tiny = frame(1, 1, vec![0]);
    let frames = vec![tiny.clone(); 512];
    assert_eq!(
        validate(&decode(&encode(1, 1, &frames, None, false)).unwrap()).frame_count(),
        512
    );
    assert!(decode(&encode(1, 1, &vec![tiny; 513], None, false)).is_err());
    let large = frame(1024, 1024, vec![0; 1024 * 1024]);
    let input = encode(1024, 1024, &vec![large.clone(); 16], None, false);
    let wire = decode(&input).unwrap();
    assert!(wire.len() <= animation::MAX_WIRE_BYTES);
    let animation = validate(&wire);
    let encoded = animation.encode().unwrap();
    assert!(encoded.bytes().len() <= animation::MAX_GIF_BYTES);
    assert_eq!(inspect(encoded.bytes()).len(), 16);
    assert!(decode(&encode(1024, 1024, &vec![large; 17], None, false)).is_err());
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn untrusted_neutral_mutations_never_escape_the_host_bounds(changes in proptest::collection::vec((0usize..256,proptest::num::u8::ANY),0..5)) {
        let mut wire = decode(&encode(2,2,&[frame(2,2,vec![0,1,1,0])],None,false)).unwrap();
        for (offset,value) in changes { let index = offset % wire.len(); wire[index] = value; }
        if let Ok(animation) = runtime().block_on(ValidatedAnimation::read(wire.as_slice())) {
            let (width,height) = animation.dimensions();
            proptest::prop_assert!((1..=1024).contains(&width) && (1..=1024).contains(&height));
            proptest::prop_assert!((1..=512).contains(&animation.frame_count()));
            let encoded = animation.encode().unwrap();
            proptest::prop_assert!(encoded.bytes().len() <= animation::MAX_GIF_BYTES);
            proptest::prop_assert_eq!(inspect(encoded.bytes()).len(),animation.frame_count());
        }
    }
}
