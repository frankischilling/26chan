use crate::{MAX_INPUT_BYTES, MediaError};
use std::io::{Read, Write};

const INPUT_HEADER_BYTES: u64 = 8;
const SECTOR_BYTES: u64 = 512;

/// Fixed byte length of a guest output block device.
pub const OUTPUT_DISK_BYTES: u64 = 4_194_816;

/// Write a bounded input as a length-prefixed, sector-aligned block image.
///
/// The source must contain exactly `bytes` bytes. A mismatch can leave a
/// partial image in `writer`; callers own cleanup after an error.
pub fn write_input_disk<R: Read, W: Write>(
    mut reader: R,
    bytes: u64,
    mut writer: W,
) -> Result<(), MediaError> {
    if bytes == 0 {
        return Err(MediaError::Empty);
    }
    if bytes > MAX_INPUT_BYTES {
        return Err(MediaError::InputTooLarge);
    }

    writer.write_all(&bytes.to_be_bytes())?;

    let mut remaining = bytes;
    let mut buffer = [0; 8192];
    while remaining != 0 {
        let limit =
            usize::try_from(remaining.min(buffer.len() as u64)).expect("read limit fits in usize");
        match reader.read(&mut buffer[..limit]) {
            Ok(0) => return Err(MediaError::InputLengthMismatch),
            Ok(read) => {
                writer.write_all(&buffer[..read])?;
                remaining -= read as u64;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }

    let mut lookahead = [0; 1];
    loop {
        match reader.read(&mut lookahead) {
            Ok(0) => break,
            Ok(_) => return Err(MediaError::InputLengthMismatch),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }

    let framed_bytes = INPUT_HEADER_BYTES + bytes;
    let padding = (SECTOR_BYTES - framed_bytes % SECTOR_BYTES) % SECTOR_BYTES;
    writer.write_all(&[0; SECTOR_BYTES as usize][..padding as usize])?;
    Ok(())
}
