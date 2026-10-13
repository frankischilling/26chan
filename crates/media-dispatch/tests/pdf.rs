use board_media_dispatch::protocol::{self, Request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn frame(magic: &[u8; 8], length: u64, body: &[u8]) -> Vec<u8> {
    [magic.as_slice(), &length.to_be_bytes(), body].concat()
}

#[tokio::test]
async fn version_four_is_explicit_and_never_downgrades_or_sniffs_payload() {
    let mut writer = Vec::new();
    protocol::write_pdf_request(b"IBPAIR02".as_slice(), 8, &mut writer)
        .await
        .unwrap();
    assert_eq!(writer, frame(b"IBJOB004", 8, b"IBPAIR02"));
    assert_eq!(
        protocol::read_versioned_request(&mut writer.as_slice())
            .await
            .unwrap(),
        Request::PdfV4(b"IBPAIR02".to_vec())
    );
    assert!(
        protocol::read_request(&mut writer.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::read_paired_request(&mut writer.as_slice())
            .await
            .is_err()
    );
    let disk = vec![0x5a; protocol::PDF_OUTPUT_LENGTH as usize];
    let mut response = Vec::new();
    protocol::write_pdf_response(&disk, &mut response)
        .await
        .unwrap();
    assert_eq!(response[..16], frame(b"IBOUT004", 4_194_816, b""));
    assert_eq!(
        protocol::read_pdf_response(&mut response.as_slice())
            .await
            .unwrap(),
        disk
    );
    assert!(
        protocol::read_response(&mut response.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::read_paired_response(&mut response.as_slice())
            .await
            .is_err()
    );
    for magic in [b"IBOUT001", b"IBOUT002", b"IBOUT003", b"IBOUT005"] {
        assert!(
            protocol::read_pdf_response(&mut frame(magic, 4_194_816, b"").as_slice())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn request_and_response_limits_completion_and_trailing_bytes_are_checked() {
    for length in [0, 8_388_609, u64::MAX] {
        assert!(
            protocol::read_versioned_request(&mut frame(b"IBJOB004", length, b"").as_slice())
                .await
                .is_err()
        );
        assert!(
            protocol::write_pdf_request(b"".as_slice(), length, &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
    let wire = frame(b"IBJOB004", 3, b"abc");
    for end in 0..wire.len() {
        assert!(
            protocol::read_versioned_request(&mut &wire[..end])
                .await
                .is_err()
        );
    }
    let mut trailing = wire;
    trailing.push(0);
    assert!(
        protocol::read_versioned_request(&mut trailing.as_slice())
            .await
            .is_err()
    );
    for (body, length) in [(b"ab".as_slice(), 3), (b"abcd", 3)] {
        assert!(
            protocol::write_pdf_request(body, length, &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
    for (length, size) in [
        (4_194_815, 0),
        (4_194_817, 0),
        (4_194_816, 3),
        (4_194_816, 4_194_817),
    ] {
        assert!(
            protocol::read_pdf_response(&mut frame(b"IBOUT004", length, &vec![0; size]).as_slice())
                .await
                .is_err()
        );
    }
    assert!(
        protocol::write_pdf_response(&[0; 20], &mut tokio::io::sink())
            .await
            .is_err()
    );
    let maximum = frame(b"IBJOB004", 8_388_608, &vec![0; 8_388_608]);
    assert!(
        matches!(protocol::read_versioned_request(&mut maximum.as_slice()).await.unwrap(),Request::PdfV4(body) if body.len()==8_388_608)
    );
}

#[tokio::test]
async fn fragmented_writer_and_missing_eof_do_not_change_the_contract() {
    let (mut writer, mut reader) = tokio::io::duplex(1);
    let sender = tokio::spawn(async move {
        for byte in frame(b"IBJOB004", 3, b"abc") {
            writer.write_all(&[byte]).await.unwrap();
        }
        writer.shutdown().await.unwrap();
    });
    assert_eq!(
        protocol::read_versioned_request(&mut reader).await.unwrap(),
        Request::PdfV4(b"abc".to_vec())
    );
    sender.await.unwrap();
    let (mut writer, mut reader) = tokio::io::duplex(64);
    writer
        .write_all(&frame(b"IBJOB004", 3, b"abc"))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            protocol::read_versioned_request(&mut reader)
        )
        .await
        .is_err()
    );
    writer.shutdown().await.unwrap();
    let mut extra = Vec::new();
    reader.read_to_end(&mut extra).await.unwrap();
    assert!(extra.is_empty());
}
