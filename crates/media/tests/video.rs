use board_media::video::{OUTPUT_DISK_BYTES, ValidatedVideo, VideoFormat};
use std::io::Cursor;

fn disk(format: VideoFormat) -> Vec<u8> {
    let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
    disk[..8].copy_from_slice(b"IBVID005");
    disk[8] = format.tag();
    disk[16..20].copy_from_slice(&640_u32.to_be_bytes());
    disk[20..24].copy_from_slice(&480_u32.to_be_bytes());
    disk[24..32].copy_from_slice(&1_250_000_u64.to_be_bytes());
    disk[40..44].copy_from_slice(&1_u32.to_be_bytes());
    disk[44..48].copy_from_slice(&1_u32.to_be_bytes());
    disk[48..56].copy_from_slice(&12_u64.to_be_bytes());
    match format {
        VideoFormat::Webm => disk[64..68].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]),
        VideoFormat::Mp4 => disk[68..72].copy_from_slice(b"ftyp"),
    }
    disk[76..80].copy_from_slice(&[255, 0, 0, 255]);
    disk
}

#[tokio::test]
async fn exact_envelope_is_bounded_media_not_a_container_parser() {
    for format in [VideoFormat::Webm, VideoFormat::Mp4] {
        let bytes = disk(format);
        let video = ValidatedVideo::read_disk(Cursor::new(&bytes), format)
            .await
            .unwrap();
        assert_eq!(video.format(), format);
        assert_eq!(video.bytes(), &bytes[64..76]);
        assert_eq!(video.dimensions(), (640, 480));
        assert_eq!(video.duration_micros(), 1_250_000);
        assert!(!video.has_audio());
        assert_eq!(video.sample_aspect_ratio(), None);
        assert_eq!(video.sha256().len(), 64);
        assert_eq!(video.md5().len(), 32);
        assert!(video.preview().encode().is_ok());
    }
}

#[tokio::test]
async fn malformed_headers_padding_and_kind_mismatch_fail() {
    let valid = disk(VideoFormat::Webm);
    for offset in [0, 8, 9, 10, 15, 16, 20, 24, 32, 40, 44, 48, 56, 63, 64, 80] {
        let mut bytes = valid.clone();
        bytes[offset] = 255;
        assert!(
            ValidatedVideo::read_disk(Cursor::new(bytes), VideoFormat::Webm)
                .await
                .is_err(),
            "offset {offset}"
        );
    }
    for length in [0, 63, 64, 75, valid.len() - 1] {
        assert!(
            ValidatedVideo::read_disk(Cursor::new(&valid[..length]), VideoFormat::Webm)
                .await
                .is_err()
        );
    }
    let mut trailing = valid.clone();
    trailing.push(0);
    assert!(
        ValidatedVideo::read_disk(Cursor::new(trailing), VideoFormat::Webm)
            .await
            .is_err()
    );
    assert!(
        ValidatedVideo::read_disk(Cursor::new(valid), VideoFormat::Mp4)
            .await
            .is_err()
    );
}
