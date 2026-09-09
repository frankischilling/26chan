use board_media::{
    MAX_DIMENSION, MAX_INPUT_BYTES, MediaError, OUTPUT_DISK_BYTES, ValidatedOutput,
    write_input_disk,
};
use std::io::{self, Cursor, Read};

const SECTOR_BYTES: usize = 512;
const HEADER_BYTES: usize = 16;
const EXPECTED_OUTPUT_DISK_BYTES: usize = 4_194_816;

fn output_disk(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut disk = vec![0; EXPECTED_OUTPUT_DISK_BYTES];
    disk[..8].copy_from_slice(b"IBRGBA01");
    disk[8..12].copy_from_slice(&width.to_be_bytes());
    disk[12..16].copy_from_slice(&height.to_be_bytes());
    disk[HEADER_BYTES..HEADER_BYTES + pixels.len()].copy_from_slice(pixels);
    disk
}

#[test]
fn writes_exact_one_byte_input_disk_framing() {
    let mut disk = Vec::new();
    write_input_disk(Cursor::new([0xa5]), 1, &mut disk).unwrap();

    assert_eq!(disk.len(), SECTOR_BYTES);
    assert_eq!(&disk[..8], &1u64.to_be_bytes());
    assert_eq!(disk[8], 0xa5);
    assert!(disk[9..].iter().all(|byte| *byte == 0));
}

#[test]
fn writes_maximum_input_with_sector_padding() {
    let input = vec![0x5a; MAX_INPUT_BYTES as usize];
    let mut disk = Vec::new();
    write_input_disk(input.as_slice(), MAX_INPUT_BYTES, &mut disk).unwrap();

    assert_eq!(disk.len(), MAX_INPUT_BYTES as usize + SECTOR_BYTES);
    assert_eq!(&disk[..8], &MAX_INPUT_BYTES.to_be_bytes());
    assert_eq!(&disk[8..8 + input.len()], input);
    assert!(disk[8 + input.len()..].iter().all(|byte| *byte == 0));
}

#[test]
fn rejects_every_input_length_mismatch() {
    assert!(matches!(
        write_input_disk(&b"abc"[..], 4, Vec::new()),
        Err(MediaError::InputLengthMismatch)
    ));

    let mut longer = Cursor::new(b"abcdef");
    assert!(matches!(
        write_input_disk(&mut longer, 4, Vec::new()),
        Err(MediaError::InputLengthMismatch)
    ));
    assert_eq!(longer.position(), 5);
}

struct NoRead;

impl Read for NoRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("invalid declared sizes must be rejected before reading input")
    }
}

#[test]
fn rejects_invalid_declared_input_sizes_without_reading() {
    assert!(matches!(
        write_input_disk(NoRead, 0, Vec::new()),
        Err(MediaError::Empty)
    ));
    assert!(matches!(
        write_input_disk(NoRead, MAX_INPUT_BYTES + 1, Vec::new()),
        Err(MediaError::InputTooLarge)
    ));
}

#[tokio::test]
async fn reads_exact_one_by_one_red_pixel_disk() {
    let disk = output_disk(1, 1, &[255, 0, 0, 255]);
    let output = ValidatedOutput::read_disk(disk.as_slice()).await.unwrap();

    assert_eq!(output.dimensions(), (1, 1));
}

#[tokio::test]
async fn reads_maximum_output_disk() {
    let pixels = vec![0x7f; MAX_DIMENSION as usize * MAX_DIMENSION as usize * 4];
    let disk = output_disk(MAX_DIMENSION, MAX_DIMENSION, &pixels);
    let output = ValidatedOutput::read_disk(disk.as_slice()).await.unwrap();

    assert_eq!(OUTPUT_DISK_BYTES as usize, EXPECTED_OUTPUT_DISK_BYTES);
    assert_eq!(output.dimensions(), (MAX_DIMENSION, MAX_DIMENSION));
}

#[tokio::test]
async fn rejects_output_disk_truncation_and_trailing_bytes() {
    let mut disk = output_disk(1, 1, &[255, 0, 0, 255]);
    for end in [0, 15, 19, disk.len() - 1] {
        assert!(
            ValidatedOutput::read_disk(&disk[..end]).await.is_err(),
            "accepted truncated disk of {end} bytes"
        );
    }

    disk.push(0);
    let mut reader = Cursor::new(disk);
    assert!(matches!(
        ValidatedOutput::read_disk(&mut reader).await,
        Err(MediaError::InvalidOutput)
    ));
    assert_eq!(reader.position(), EXPECTED_OUTPUT_DISK_BYTES as u64 + 1);
}

#[tokio::test]
async fn rejects_nonzero_output_padding() {
    let mut disk = output_disk(1, 1, &[255, 0, 0, 255]);
    for position in [HEADER_BYTES + 4, disk.len() - 1] {
        disk[position] = 1;
        assert!(matches!(
            ValidatedOutput::read_disk(disk.as_slice()).await,
            Err(MediaError::InvalidOutput)
        ));
        disk[position] = 0;
    }
}

#[tokio::test]
async fn rejects_untrusted_dimensions_before_body_reads() {
    for (width, height) in [
        (0, 1),
        (1, 0),
        (MAX_DIMENSION + 1, 1),
        (1, MAX_DIMENSION + 1),
        (u32::MAX, u32::MAX),
    ] {
        let mut reader = Cursor::new(output_disk(width, height, &[]));
        assert!(matches!(
            ValidatedOutput::read_disk(&mut reader).await,
            Err(MediaError::InvalidOutput)
        ));
        assert_eq!(reader.position(), HEADER_BYTES as u64);
    }
}
