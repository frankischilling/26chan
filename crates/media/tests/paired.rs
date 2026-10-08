//! Literal framing controls are independent of the production encoder. No
//! successful parse in this suite is a guest, provenance, or approval claim.

use board_media::paired::{
    INPUT_HEADER_BYTES, INPUT_TRAILER_BYTES, InputKind, PairedError, RESULT_BYTES,
    RESULT_HEADER_BYTES, ReplayPresence, VERSION, decode_input, decode_result, encode_input,
};
use board_media::{MAX_INPUT_BYTES, OUTPUT_DISK_BYTES, ValidatedOutput, replay_state, replay_wire};

const KIND: InputKind = InputKind::PairedV2;
const JOB: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const BINDING: [u8; 32] = [0xa5; 32];
// Independent literal: 48-byte header, three arbitrary image bytes, two arbitrary
// replay-upload bytes, and the exact trailer. Neither component is validated.
const INPUT_HEX: &str = concat!(
    "49425041495230320002003000000001",
    "00000000000000030000000000000002",
    "000102030405060708090a0b0c0d0e0f",
    "deadbeef014942444f4e453032",
);
// Independent literal: no replay, caller-provided opaque binding, 20-byte compact
// RGBA value. This is not an input SHA or a generated dispatch binding.
const RESULT_HEX: &str = concat!(
    "49425245533030320002004000000000",
    "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
    "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
    "00000000000000140000000000000000",
    "49425247424130310000000100000001aabbccdd",
);

fn hex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}

fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
}
fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}
fn set_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}

fn result_without_replay() -> Vec<u8> {
    let mut result = hex(RESULT_HEX);
    result.resize(4_456_960, 0);
    result
}

fn literal_replay() -> Vec<u8> {
    // Independently generated from the frozen production recorder, rather than
    // a round trip through the Rust wire codec. Canvas resized for this control.
    let mut wire = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr").to_vec();
    set_u16(&mut wire, 24, 1);
    set_u16(&mut wire, 26, 1);
    wire
}

fn result_with_wire(wire: &[u8]) -> Vec<u8> {
    let mut result = result_without_replay();
    set_u32(&mut result, 12, 1);
    set_u64(&mut result, 56, wire.len() as u64);
    result[84..84 + wire.len()].copy_from_slice(wire);
    result
}

fn reject_result(result: &[u8], presence: ReplayPresence, error: PairedError) {
    assert_eq!(
        decode_result(KIND, result, BINDING, presence).unwrap_err(),
        error
    );
}

#[test]
fn candidate_constants_do_not_replace_existing_v1_bounds() {
    assert_eq!(VERSION, 2);
    assert_eq!(INPUT_HEADER_BYTES, 48);
    assert_eq!(INPUT_TRAILER_BYTES, 8);
    assert_eq!(RESULT_HEADER_BYTES, 64);
    assert_eq!(RESULT_BYTES, 4_456_960);
    assert_eq!(MAX_INPUT_BYTES, 8_388_608);
    assert_eq!(OUTPUT_DISK_BYTES, 4_194_816);
}

#[test]
fn literal_input_parses_and_encoder_matches_independent_bytes() {
    let input = hex(INPUT_HEX);
    let candidate = decode_input(KIND, &input, JOB).unwrap();
    assert_eq!(candidate.job_id(), &JOB);
    assert_eq!(candidate.image_bytes(), &[0xde, 0xad, 0xbe]);
    assert_eq!(candidate.replay_upload_bytes(), Some(&[0xef, 1][..]));
    assert_eq!(candidate.replay_presence(), ReplayPresence::Present);
    // Independent Python hashlib SHA-256 of the literal frame and components.
    for (actual, expected) in [
        (
            candidate.frame_sha256(),
            "00ab5bc95374af66119bfd2f448a0e131f67e429257cc6bcee795442a0f102fd",
        ),
        (
            candidate.image_sha256(),
            "08d2eb20ba68a2bf98f4b9718c779c2d254b88b67019e710f3065e03063de2bd",
        ),
        (
            candidate.replay_sha256().unwrap(),
            "d7dc23165dfffc97a19610bad8ef701e6dd4ad3daa36369cb6aa347fa9e52309",
        ),
    ] {
        assert_eq!(actual.as_slice(), hex(expected));
    }
    assert_eq!(
        encode_input(KIND, JOB, &[0xde, 0xad, 0xbe], Some(&[0xef, 1])).unwrap(),
        input
    );
}

#[test]
fn input_absence_is_exact_and_present_empty_is_rejected() {
    let mut absent = hex(INPUT_HEX);
    set_u32(&mut absent, 12, 0);
    set_u64(&mut absent, 24, 0);
    absent.drain(51..53);
    assert_eq!(
        encode_input(KIND, JOB, &[0xde, 0xad, 0xbe], None).unwrap(),
        absent
    );
    let candidate = decode_input(KIND, &absent, JOB).unwrap();
    assert_eq!(candidate.replay_presence(), ReplayPresence::Absent);
    assert_eq!(candidate.replay_upload_bytes(), None);
    assert_eq!(candidate.replay_sha256(), None);
    assert_eq!(
        encode_input(KIND, JOB, b"x", Some(&[])),
        Err(PairedError::ReplayPresence)
    );
    set_u32(&mut absent, 12, 1);
    assert_eq!(
        decode_input(KIND, &absent, JOB).unwrap_err(),
        PairedError::ReplayPresence
    );
    let mut hidden_replay = hex(INPUT_HEX);
    set_u32(&mut hidden_replay, 12, 0);
    assert_eq!(
        decode_input(KIND, &hidden_replay, JOB).unwrap_err(),
        PairedError::ReplayPresence
    );
}

#[test]
fn arbitrary_image_and_replay_upload_bytes_remain_unvalidated() {
    for image in [b"not a PNG".as_slice(), b"IBPAIR02", b"GIF89a", b"\xff\xd8"] {
        let frame = encode_input(KIND, JOB, image, Some(b"not TGKR")).unwrap();
        let candidate = decode_input(KIND, &frame, JOB).unwrap();
        assert_eq!(candidate.image_bytes(), image);
        assert_eq!(candidate.replay_upload_bytes(), Some(&b"not TGKR"[..]));
    }
    assert_eq!(
        encode_input(KIND, JOB, &[], None),
        Err(PairedError::EmptyImage)
    );
    let mut input = hex(INPUT_HEX);
    set_u64(&mut input, 16, 0);
    assert_eq!(
        decode_input(KIND, &input, JOB).unwrap_err(),
        PairedError::EmptyImage
    );
}

#[test]
fn explicit_kind_and_expected_job_are_required_without_sniffing() {
    let input = hex(INPUT_HEX);
    assert_eq!(
        decode_input(InputKind::ImageV1, &input, JOB).unwrap_err(),
        PairedError::Kind
    );
    assert_eq!(
        encode_input(InputKind::ImageV1, JOB, b"x", None),
        Err(PairedError::Kind)
    );
    let mut wrong_job = JOB;
    wrong_job[15] ^= 1;
    assert_eq!(
        decode_input(KIND, &input, wrong_job).unwrap_err(),
        PairedError::JobId
    );
    assert!(decode_input(KIND, b"IBJOB001", JOB).is_err());
}

#[test]
fn input_rejects_each_truncation_and_any_trailing_or_padding_bytes() {
    let input = hex(INPUT_HEX);
    // The literal is 61 bytes, so this exhausts truncation without large loops.
    for end in 0..input.len() {
        assert!(decode_input(KIND, &input[..end], JOB).is_err(), "{end}");
    }
    for extra in [0, 1, 255] {
        let mut changed = input.clone();
        changed.push(extra);
        assert_eq!(
            decode_input(KIND, &changed, JOB).unwrap_err(),
            PairedError::Length
        );
    }
    let mut padded = input.clone();
    padded.resize(512, 0);
    assert!(decode_input(KIND, &padded, JOB).is_err());
    for offset in 53..61 {
        let mut changed = input.clone();
        changed[offset] ^= 1;
        assert_eq!(
            decode_input(KIND, &changed, JOB).unwrap_err(),
            PairedError::Header
        );
    }
}

#[test]
fn input_rejects_bad_magic_versions_header_sizes_and_unknown_flags() {
    let input = hex(INPUT_HEX);
    for offset in 0..8 {
        let mut changed = input.clone();
        changed[offset] ^= 1;
        assert_eq!(
            decode_input(KIND, &changed, JOB).unwrap_err(),
            PairedError::Header
        );
    }
    for (offset, values) in [(8, [0, 1, 3, u16::MAX]), (10, [0, 47, 49, u16::MAX])] {
        for value in values {
            let mut changed = input.clone();
            set_u16(&mut changed, offset, value);
            assert_eq!(
                decode_input(KIND, &changed, JOB).unwrap_err(),
                PairedError::Header
            );
        }
    }
    for flags in [2, 3, 0x8000_0001, u32::MAX] {
        let mut changed = input.clone();
        set_u32(&mut changed, 12, flags);
        assert_eq!(
            decode_input(KIND, &changed, JOB).unwrap_err(),
            PairedError::Flags
        );
    }
}

#[test]
fn input_lengths_are_checked_before_any_attacker_sized_allocation() {
    for (image, replay, error) in [
        (4, 2, PairedError::Length),
        (3, 1, PairedError::Length),
        (8_388_608, 2, PairedError::InputSize),
        (1, 8_388_608, PairedError::InputSize),
        (u64::MAX, 1, PairedError::Length),
        (1, u64::MAX, PairedError::Length),
        (u64::MAX - 2, 1, PairedError::Length),
    ] {
        let mut input = hex(INPUT_HEX);
        set_u64(&mut input, 16, image);
        set_u64(&mut input, 24, replay);
        assert_eq!(decode_input(KIND, &input, JOB).unwrap_err(), error);
    }
}

#[test]
fn whole_job_cap_includes_both_components_header_and_trailer() {
    let mut image = vec![7; 8_388_608 - 56];
    let frame = encode_input(KIND, JOB, &image, None).unwrap();
    assert_eq!(frame.len(), 8_388_608);
    assert_eq!(
        decode_input(KIND, &frame, JOB).unwrap().image_bytes(),
        image
    );
    assert_eq!(
        encode_input(KIND, JOB, &image, Some(&[1])),
        Err(PairedError::InputSize)
    );
    image.pop();
    let frame = encode_input(KIND, JOB, &image, Some(&[1])).unwrap();
    assert_eq!(frame.len(), 8_388_608);
    assert!(decode_input(KIND, &frame, JOB).is_ok());
    let mut too_big = frame;
    too_big.push(0);
    assert_eq!(
        decode_input(KIND, &too_big, JOB).unwrap_err(),
        PairedError::InputSize
    );
    image.extend_from_slice(&[1, 2]);
    assert_eq!(
        encode_input(KIND, JOB, &image, None),
        Err(PairedError::InputSize)
    );
}

#[test]
fn literal_result_has_bounded_pixels_without_replay_or_conversion() {
    let result = result_without_replay();
    let candidate = decode_result(KIND, &result, BINDING, ReplayPresence::Absent).unwrap();
    assert_eq!(candidate.binding(), &BINDING);
    assert_eq!(candidate.dimensions(), (1, 1));
    assert_eq!(candidate.rgba_bytes(), &[0xaa, 0xbb, 0xcc, 0xdd]);
    assert!(candidate.untrusted_replay().is_none());
    assert!(candidate.replay_wire_bytes().is_none());
}

#[test]
fn present_empty_drawing_is_distinct_from_absence_and_zero_wire_bytes() {
    let wire = literal_replay();
    let result = result_with_wire(&wire);
    let candidate = decode_result(KIND, &result, BINDING, ReplayPresence::Present).unwrap();
    assert_eq!(candidate.replay_wire_bytes(), Some(wire.as_slice()));
    assert_eq!(candidate.untrusted_replay().unwrap().events.len(), 2);
    reject_result(&result, ReplayPresence::Absent, PairedError::ReplayPresence);
    let mut absent = result_without_replay();
    reject_result(
        &absent,
        ReplayPresence::Present,
        PairedError::ReplayPresence,
    );
    set_u32(&mut absent, 12, 1);
    reject_result(
        &absent,
        ReplayPresence::Present,
        PairedError::ReplayPresence,
    );
    let mut hidden = result;
    set_u32(&mut hidden, 12, 0);
    reject_result(&hidden, ReplayPresence::Absent, PairedError::ReplayPresence);
    let one_byte = result_with_wire(&[0]);
    reject_result(
        &one_byte,
        ReplayPresence::Present,
        PairedError::Replay(replay_wire::ReplayWireError::InputSize),
    );
}

#[test]
fn result_requires_explicit_kind_and_caller_supplied_attempt_binding() {
    let result = result_without_replay();
    assert_eq!(
        decode_result(InputKind::ImageV1, &result, BINDING, ReplayPresence::Absent).unwrap_err(),
        PairedError::Kind
    );
    let mut another_attempt = BINDING;
    another_attempt[31] ^= 1;
    assert_eq!(
        decode_result(KIND, &result, another_attempt, ReplayPresence::Absent).unwrap_err(),
        PairedError::Binding
    );
    // Byte fingerprints are not substituted for the explicit opaque binding.
    let input = hex(INPUT_HEX);
    let frame_sha = *decode_input(KIND, &input, JOB).unwrap().frame_sha256();
    assert_eq!(
        decode_result(KIND, &result, frame_sha, ReplayPresence::Absent).unwrap_err(),
        PairedError::Binding
    );
}

#[test]
fn result_rejects_bad_header_versions_flags_and_binding() {
    let result = result_without_replay();
    for offset in [0, 7, 8, 9, 10, 11] {
        let mut changed = result.clone();
        changed[offset] ^= 1;
        reject_result(&changed, ReplayPresence::Absent, PairedError::Header);
    }
    for flags in [2, 3, 0x8000_0000, u32::MAX] {
        let mut changed = result.clone();
        set_u32(&mut changed, 12, flags);
        reject_result(&changed, ReplayPresence::Absent, PairedError::Flags);
    }
    for offset in [16, 31, 47] {
        let mut changed = result.clone();
        changed[offset] ^= 1;
        reject_result(&changed, ReplayPresence::Absent, PairedError::Binding);
    }
}

#[test]
fn result_requires_exact_fixed_size_and_all_zero_padding() {
    let mut result = result_without_replay();
    // Header, component, sector, and endpoint boundaries; no quadratic sweep
    // over every possible prefix of the 4.25 MiB fixed result.
    for end in [0, 1, 8, 16, 48, 63, 64, 79, 80, 83, 84, 512, 4_456_959] {
        reject_result(
            &result[..end],
            ReplayPresence::Absent,
            PairedError::ResultSize,
        );
    }
    for offset in [84, 512, 2_000_000, 4_456_959] {
        result[offset] = 1;
        reject_result(&result, ReplayPresence::Absent, PairedError::Padding);
        result[offset] = 0;
    }
    result.push(0);
    reject_result(&result, ReplayPresence::Absent, PairedError::ResultSize);
    result[4_456_960] = 255;
    reject_result(&result, ReplayPresence::Absent, PairedError::ResultSize);
}

#[test]
fn result_rejects_bad_compact_magic_dimensions_and_exact_inner_lengths() {
    let result = result_without_replay();
    for offset in [64, 71] {
        let mut changed = result.clone();
        changed[offset] ^= 1;
        reject_result(&changed, ReplayPresence::Absent, PairedError::Pixels);
    }
    for offset in [72, 76] {
        for dimension in [0, 2, 1025, u32::MAX] {
            let mut changed = result.clone();
            set_u32(&mut changed, offset, dimension);
            reject_result(&changed, ReplayPresence::Absent, PairedError::Pixels);
        }
    }
    for length in [0, 8, 15, 16, 19, 21, 512, 4_194_816] {
        let mut changed = result.clone();
        set_u64(&mut changed, 48, length);
        reject_result(&changed, ReplayPresence::Absent, PairedError::Pixels);
    }
    for length in [4_456_897, u64::MAX - 63, u64::MAX] {
        let mut changed = result.clone();
        set_u64(&mut changed, 48, length);
        reject_result(&changed, ReplayPresence::Absent, PairedError::Length);
    }
}

#[test]
fn result_rejects_replay_caps_versions_counts_padding_and_trailing_inner_bytes() {
    let wire = literal_replay();
    let result = result_with_wire(&wire);
    for length in [262_401, 4_456_960, u64::MAX] {
        let mut changed = result.clone();
        set_u64(&mut changed, 56, length);
        reject_result(&changed, ReplayPresence::Present, PairedError::Length);
    }
    for offset in [0, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
        let mut changed_wire = wire.clone();
        changed_wire[offset] ^= 1;
        let changed = result_with_wire(&changed_wire);
        assert!(decode_result(KIND, &changed, BINDING, ReplayPresence::Present).is_err());
    }
    for count in [0, 1, 3, 16_385, u32::MAX] {
        let mut changed_wire = wire.clone();
        set_u32(&mut changed_wire, 20, count);
        let changed = result_with_wire(&changed_wire);
        assert!(decode_result(KIND, &changed, BINDING, ReplayPresence::Present).is_err());
    }
    for end in [0, 1, 63, 64, 255, 256, 272, 287] {
        let changed = result_with_wire(&wire[..end]);
        assert!(decode_result(KIND, &changed, BINDING, ReplayPresence::Present).is_err());
    }
    let mut extra = wire.clone();
    extra.push(0);
    assert!(
        decode_result(
            KIND,
            &result_with_wire(&extra),
            BINDING,
            ReplayPresence::Present
        )
        .is_err()
    );
    let mut wrong_declared = wire.clone();
    set_u32(&mut wrong_declared, 16, 287);
    assert!(
        decode_result(
            KIND,
            &result_with_wire(&wrong_declared),
            BINDING,
            ReplayPresence::Present
        )
        .is_err()
    );
    let mut reserved = wire;
    reserved[48] = 1;
    reject_result(
        &result_with_wire(&reserved),
        ReplayPresence::Present,
        PairedError::Replay(replay_wire::ReplayWireError::Reserved),
    );
    let mut padded = result;
    padded[372] = 1;
    reject_result(&padded, ReplayPresence::Present, PairedError::Padding);
}

#[test]
fn replay_and_pixels_have_independent_bounds_without_a_correspondence_claim() {
    let mut wire = literal_replay();
    set_u16(&mut wire, 24, 2);
    let result = result_with_wire(&wire);
    let candidate = decode_result(KIND, &result, BINDING, ReplayPresence::Present).unwrap();
    assert_eq!(candidate.dimensions(), (1, 1));
    assert_eq!(candidate.untrusted_replay().unwrap().metadata.width, 2);
    assert_eq!(candidate.untrusted_replay().unwrap().metadata.height, 1);
    set_u16(&mut wire, 24, 0);
    reject_result(
        &result_with_wire(&wire),
        ReplayPresence::Present,
        PairedError::Replay(replay_wire::ReplayWireError::Metadata),
    );
    set_u16(&mut wire, 24, 1025);
    assert!(
        decode_result(
            KIND,
            &result_with_wire(&wire),
            BINDING,
            ReplayPresence::Present
        )
        .is_err()
    );
}

#[test]
fn structurally_decoded_replay_is_not_state_or_cost_qualified() {
    let mut wire = literal_replay();
    // Chronologically reversed epochs remain structurally valid. The state
    // checker rejects them, and its required output cannot reach cost checks.
    set_u32(&mut wire, 36, 2);
    set_u32(&mut wire, 40, 1);
    let result = result_with_wire(&wire);
    let candidate = decode_result(KIND, &result, BINDING, ReplayPresence::Present).unwrap();
    assert!(replay_state::check_core_v1(candidate.untrusted_replay().unwrap()).is_err());
    // Any bounded RGBA value is structurally accepted, even though this module
    // has no input image and cannot establish replay/image pixel equality.
    let mut altered_pixels = result;
    altered_pixels[80..84].copy_from_slice(&[1, 2, 3, 4]);
    assert_eq!(
        decode_result(KIND, &altered_pixels, BINDING, ReplayPresence::Present)
            .unwrap()
            .rgba_bytes(),
        &[1, 2, 3, 4]
    );
}

#[test]
fn maximum_pixels_and_maximum_structural_replay_fit_fixed_result() {
    let mut wire = literal_replay();
    let conclusion = wire.split_off(272);
    // Bounded literal HistoryDummy records; state checking is deliberately out
    // of scope. The wire count is capped at 16,384, including endpoint markers.
    let mut dummy = [0; 16];
    dummy[0] = 254;
    for _ in 0..16_382 {
        wire.extend_from_slice(&dummy);
    }
    wire.extend_from_slice(&conclusion);
    set_u32(&mut wire, 16, 262_400);
    set_u32(&mut wire, 20, 16_384);
    set_u16(&mut wire, 24, 1024);
    set_u16(&mut wire, 26, 1024);
    assert_eq!(wire.len(), 262_400);
    let mut result = result_without_replay();
    set_u32(&mut result, 12, 1);
    set_u64(&mut result, 48, 4_194_320);
    set_u64(&mut result, 56, 262_400);
    set_u32(&mut result, 72, 1024);
    set_u32(&mut result, 76, 1024);
    result[4_194_384..4_456_784].copy_from_slice(&wire);
    let candidate = decode_result(KIND, &result, BINDING, ReplayPresence::Present).unwrap();
    assert_eq!(candidate.dimensions(), (1024, 1024));
    assert_eq!(candidate.rgba_bytes().len(), 4_194_304);
    assert_eq!(candidate.untrusted_replay().unwrap().events.len(), 16_384);
    assert_eq!(result[4_456_784..].len(), 176);
}

#[tokio::test]
async fn paired_result_is_not_accepted_by_existing_v1_output_apis() {
    let result = result_without_replay();
    assert!(ValidatedOutput::read(result.as_slice()).await.is_err());
    assert!(ValidatedOutput::read_disk(result.as_slice()).await.is_err());
}
