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
