use board_media_dispatch::protocol::{self, Request};

fn frame(magic: &[u8; 8], length: u64, bytes: &[u8]) -> Vec<u8> {
    [magic.as_slice(), &length.to_be_bytes(), bytes].concat()
}

#[tokio::test]
async fn swf_transport_is_explicit_bounded_and_opaque() {
    // The broker does not sniff or parse SWF bytes, including the signature.
    let body = [0; 8];
    let mut wire = Vec::new();
    protocol::write_swf_request(&body, &mut wire).await.unwrap();
    assert_eq!(wire, frame(b"IBJOB008", 8, &body));
    assert_eq!(
        protocol::read_versioned_request(&mut wire.as_slice())
            .await
            .unwrap(),
        Request::SwfV8(body.to_vec())
    );
    assert!(protocol::read_request(&mut wire.as_slice()).await.is_err());
    for length in [0, 1, 7, protocol::MAX_SWF_INPUT + 1, u64::MAX] {
        assert!(
            protocol::read_versioned_request(&mut frame(b"IBJOB008", length, &[]).as_slice())
                .await
                .is_err()
        );
    }
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
    assert!(
        protocol::write_swf_request(&body[..7], &mut tokio::io::sink())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn swf_response_requires_its_version_fixed_size_and_exact_eof() {
    let body = vec![0; protocol::SWF_OUTPUT_LENGTH as usize];
    let mut response = Vec::new();
    protocol::write_swf_response(&body, &mut response)
        .await
        .unwrap();
    assert_eq!(
        protocol::read_swf_response(&mut response.as_slice())
            .await
            .unwrap(),
        body
    );
    assert!(
        protocol::read_response(&mut response.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::read_video_response(&mut response.as_slice())
            .await
            .is_err()
    );
    for magic in [
        b"IBOUT001",
        b"IBOUT002",
        b"IBOUT003",
        b"IBOUT004",
        b"IBOUT005",
        b"IBOUT006",
        b"IBOUT007",
    ] {
        assert!(
            protocol::read_swf_response(
                &mut frame(magic, protocol::SWF_OUTPUT_LENGTH, &[]).as_slice()
            )
            .await
            .is_err()
        );
    }
    response.push(0);
    assert!(
        protocol::read_swf_response(&mut response.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::write_swf_response(&body[..body.len() - 1], &mut tokio::io::sink())
            .await
            .is_err()
    );
}
