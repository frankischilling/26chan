use board_media_dispatch::protocol::{self, Request};

fn frame(magic: &[u8; 8], length: u64, bytes: &[u8]) -> Vec<u8> {
    [magic.as_slice(), &length.to_be_bytes(), bytes].concat()
}
fn body() -> Vec<u8> {
    [b"IBGIF007".as_slice(), &[4, 3, 0, 250, 0, 250, 0, 0, 42]].concat()
}

#[tokio::test]
async fn explicit_source_gif_request_never_becomes_a_legacy_image() {
    let body = body();
    let mut wire = Vec::new();
    protocol::write_source_gif_request(&body, &mut wire)
        .await
        .unwrap();
    assert_eq!(wire, frame(b"IBJOB007", 17, &body));
    assert_eq!(
        protocol::read_versioned_request(&mut wire.as_slice())
            .await
            .unwrap(),
        Request::GifV7(body)
    );
    assert!(protocol::read_request(&mut wire.as_slice()).await.is_err());
    for end in 0..wire.len() {
        assert!(
            protocol::read_versioned_request(&mut &wire[..end])
                .await
                .is_err()
        );
    }
    wire.push(0);
    assert!(
        protocol::read_versioned_request(&mut wire.as_slice())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn policy_header_and_length_fail_before_forwarding() {
    for (offset, value) in [
        (0, 0),
        (8, 1),
        (8, 2),
        (8, 16),
        (9, 0),
        (10, 1),
        (11, 0),
        (14, 1),
        (15, 1),
    ] {
        let mut body = body();
        body[offset] = value;
        assert!(
            protocol::write_source_gif_request(&body, &mut tokio::io::sink())
                .await
                .is_err()
        );
        assert!(
            protocol::read_versioned_request(&mut frame(b"IBJOB007", 17, &body).as_slice())
                .await
                .is_err()
        );
    }
    for length in [0, 16, protocol::MAX_GIF_SOURCE_INPUT + 1, u64::MAX] {
        assert!(
            protocol::read_versioned_request(&mut frame(b"IBJOB007", length, &[]).as_slice())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn fixed_response_requires_exact_length_version_and_eof() {
    let body = vec![0; protocol::STILL_OUTPUT_LENGTH as usize];
    let mut response = Vec::new();
    protocol::write_source_gif_response(&body, &mut response)
        .await
        .unwrap();
    assert_eq!(
        protocol::read_source_gif_response(&mut response.as_slice())
            .await
            .unwrap(),
        body
    );
    for magic in [
        b"IBOUT001",
        b"IBOUT002",
        b"IBOUT003",
        b"IBOUT004",
        b"IBOUT005",
        b"IBOUT006",
    ] {
        assert!(
            protocol::read_source_gif_response(
                &mut frame(magic, protocol::STILL_OUTPUT_LENGTH, &[]).as_slice()
            )
            .await
            .is_err()
        );
    }
    assert!(
        protocol::read_source_gif_response(&mut &response[..response.len() - 1])
            .await
            .is_err()
    );
    response.push(0);
    assert!(
        protocol::read_source_gif_response(&mut response.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::write_source_gif_response(&body[..body.len() - 1], &mut tokio::io::sink())
            .await
            .is_err()
    );
}
