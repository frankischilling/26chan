use board_media_dispatch::protocol::{read_request, read_response, write_request};
use proptest::prelude::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn frame(magic: &[u8; 8], length: u64, body: &[u8]) -> Vec<u8> {
    [magic.as_slice(), &length.to_be_bytes(), body].concat()
}

#[tokio::test]
async fn exact_request_and_response_are_accepted() {
    assert_eq!(
        read_request(&mut frame(b"IBJOB001", 3, b"abc").as_slice())
            .await
            .unwrap(),
        b"abc"
    );
    let disk = vec![0x5a; 4_194_816];
    assert_eq!(
        read_response(&mut frame(b"IBOUT001", 4_194_816, &disk).as_slice())
            .await
            .unwrap(),
        disk
    );
    assert_eq!(
        read_request(&mut frame(b"IBJOB001", 8_388_608, &vec![7; 8_388_608]).as_slice())
            .await
            .unwrap()
            .len(),
        8_388_608
    );
}

#[tokio::test]
async fn malformed_lengths_magic_truncation_and_trailing_bytes_reject() {
    for value in [
        frame(b"IBJOB001", 0, b""),
        frame(b"IBJOB001", 8_388_609, b""),
        frame(b"IBJOB001", u64::MAX, b""),
        frame(b"IBBAD001", 1, b"a"),
        frame(b"IBJOB001", 2, b"a"),
        frame(b"IBJOB001", 1, b"ab"),
        vec![0; 15],
    ] {
        assert!(read_request(&mut value.as_slice()).await.is_err());
    }
    for value in [
        frame(b"IBOUT001", 4_194_815, b""),
        frame(b"IBOUT001", 4_194_817, b""),
        frame(b"IBOUT001", 4_194_816, b"short"),
        frame(b"IBJOB001", 4_194_816, &vec![0; 4_194_816]),
        frame(b"IBOUT001", 4_194_816, &vec![0; 4_194_817]),
    ] {
        assert!(read_response(&mut value.as_slice()).await.is_err());
    }
}

#[tokio::test]
async fn writer_enforces_source_length_and_sends_eof() {
    let (mut tx, mut rx) = tokio::io::duplex(64);
    write_request(b"abc".as_slice(), 3, &mut tx).await.unwrap();
    let mut bytes = Vec::new();
    rx.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"IBJOB001\0\0\0\0\0\0\0\x03abc");
    for (input, length) in [
        (b"abc".as_slice(), 2),
        (b"abc".as_slice(), 4),
        (b"".as_slice(), 0),
        (b"".as_slice(), 8_388_609),
    ] {
        assert!(
            write_request(input, length, &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn fragmented_async_reader_preserves_framing() {
    let (mut tx, mut rx) = tokio::io::duplex(1);
    let sender = tokio::spawn(async move {
        for byte in b"IBJOB001\0\0\0\0\0\0\0\x03abc" {
            tx.write_all(&[*byte]).await.unwrap();
        }
        tx.shutdown().await.unwrap();
    });
    assert_eq!(read_request(&mut rx).await.unwrap(), b"abc");
    sender.await.unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, .. ProptestConfig::default() })]
    #[test]
    fn arbitrary_short_frames_never_produce_payload(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        prop_assert!(runtime.block_on(read_request(&mut bytes.as_slice())).is_err());
    }
}

fn paired_frame(length: u64, binding: &[u8], body: &[u8]) -> Vec<u8> {
    frame(b"IBJOB002", length, &[binding, body].concat())
}

#[tokio::test]
async fn paired_bounds_binding_and_selected_version_roundtrip() {
    use board_media_dispatch::protocol::*;
    for length in [MIN_PAIRED_INPUT, MAX_PAIRED_INPUT] {
        let body = vec![7; length as usize];
        let binding = [0xa5; 32];
        let wire = paired_frame(length, &binding, &body);
        assert_eq!(
            read_paired_request(&mut wire.as_slice()).await.unwrap(),
            (binding, body.clone())
        );
        assert_eq!(
            read_versioned_request(&mut wire.as_slice()).await.unwrap(),
            Request::PairedV2 {
                binding,
                input: body.clone()
            }
        );
        let mut encoded = Vec::new();
        write_paired_request(body.as_slice(), length, &binding, &mut encoded)
            .await
            .unwrap();
        assert_eq!(encoded, wire);
        assert!(read_request(&mut wire.as_slice()).await.is_err());
    }
    // A legacy request remains legacy even when its payload starts with paired magic.
    let body = b"IBPAIR02opaque";
    let wire = frame(b"IBJOB001", body.len() as u64, body);
    assert_eq!(
        read_versioned_request(&mut wire.as_slice()).await.unwrap(),
        Request::ImageV1(body.to_vec())
    );
    assert!(read_paired_request(&mut wire.as_slice()).await.is_err());
}

#[tokio::test]
async fn paired_rejects_lengths_truncation_trailing_and_unknown_versions() {
    use board_media_dispatch::protocol::*;
    for length in [0, MIN_PAIRED_INPUT - 1, MAX_PAIRED_INPUT + 1, u64::MAX] {
        let wire = paired_frame(length, &[0; 32], &[]);
        assert!(read_paired_request(&mut wire.as_slice()).await.is_err());
        assert!(read_versioned_request(&mut wire.as_slice()).await.is_err());
        let mut output = Vec::new();
        assert!(
            write_paired_request(&[][..], length, &[0; 32], &mut output)
                .await
                .is_err()
        );
        assert!(output.is_empty());
    }
    let wire = paired_frame(57, &[0; 32], &[1; 57]);
    for end in 0..wire.len() {
        assert!(read_paired_request(&mut &wire[..end]).await.is_err());
    }
    let mut trailing = wire.clone();
    trailing.push(0);
    assert!(read_paired_request(&mut trailing.as_slice()).await.is_err());
    let mut unknown = wire;
    unknown[7] = b'3';
    assert!(
        read_versioned_request(&mut unknown.as_slice())
            .await
            .is_err()
    );
    for body in [vec![0; 56], vec![0; 58]] {
        assert!(
            write_paired_request(body.as_slice(), 57, &[0; 32], &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn paired_response_is_exact_and_cannot_downgrade_either_direction() {
    use board_media_dispatch::protocol::*;
    let disk = vec![0xa5; PAIRED_OUTPUT_LENGTH as usize];
    let mut wire = Vec::new();
    write_paired_response(&disk, &mut wire).await.unwrap();
    assert_eq!(wire, frame(b"IBOUT002", PAIRED_OUTPUT_LENGTH, &disk));
    assert_eq!(
        read_paired_response(&mut wire.as_slice()).await.unwrap(),
        disk
    );
    assert!(read_response(&mut wire.as_slice()).await.is_err());
    assert!(
        read_paired_response(
            &mut frame(b"IBOUT001", OUTPUT_LENGTH, &vec![0; OUTPUT_LENGTH as usize]).as_slice()
        )
        .await
        .is_err()
    );
    for value in [
        frame(b"IBOUT002", PAIRED_OUTPUT_LENGTH - 1, &[]),
        frame(b"IBOUT002", PAIRED_OUTPUT_LENGTH + 1, &[]),
        frame(b"IBOUT002", PAIRED_OUTPUT_LENGTH, b"short"),
        frame(
            b"IBOUT002",
            PAIRED_OUTPUT_LENGTH,
            &vec![0; PAIRED_OUTPUT_LENGTH as usize + 1],
        ),
    ] {
        assert!(read_paired_response(&mut value.as_slice()).await.is_err());
    }
    for length in [0, PAIRED_OUTPUT_LENGTH - 1, PAIRED_OUTPUT_LENGTH + 1] {
        assert!(
            write_paired_response(&vec![0; length as usize], &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn paired_fragmented_binding_and_body_preserve_exact_bytes_and_eof() {
    use board_media_dispatch::protocol::*;
    let (mut tx, mut rx) = tokio::io::duplex(1);
    let sender = tokio::spawn(async move {
        write_paired_request(&[7; 57][..], 57, &[9; 32], &mut tx)
            .await
            .unwrap();
    });
    assert_eq!(
        read_paired_request(&mut rx).await.unwrap(),
        ([9; 32], vec![7; 57])
    );
    sender.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn paired_absolute_deadline_covers_header_binding_body_and_eof() {
    use board_media_dispatch::protocol::*;
    use std::time::Duration;
    let wire = paired_frame(57, &[0; 32], &[1; 57]);
    for count in [7, 16, 47, 48, wire.len() - 1, wire.len()] {
        let (mut tx, mut rx) = tokio::io::duplex(256);
        tx.write_all(&wire[..count]).await.unwrap();
        // Sender stays open: even all B bytes do not satisfy EOF.
        assert!(
            tokio::time::timeout(Duration::from_secs(3), read_versioned_request(&mut rx))
                .await
                .is_err()
        );
        drop(tx);
    }
    let (mut tx, mut rx) = tokio::io::duplex(256);
    let sender = tokio::spawn(async move {
        for byte in wire {
            if tx.write_all(&[byte]).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    assert!(
        tokio::time::timeout(Duration::from_secs(3), read_versioned_request(&mut rx))
            .await
            .is_err()
    );
    sender.abort();
}
