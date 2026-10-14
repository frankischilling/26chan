use board_media_dispatch::protocol::{self, Request};

fn frame(magic: &[u8; 8], length: u64, bytes: &[u8]) -> Vec<u8> {
    [magic.as_slice(), &length.to_be_bytes(), bytes].concat()
}

#[tokio::test]
async fn video_has_an_explicit_tagged_version_without_image_fallback() {
    for tag in [1, 2] {
        let body = [tag, b'x'];
        let mut wire = Vec::new();
        protocol::write_video_request(&body, &mut wire)
            .await
            .unwrap();
        assert_eq!(wire, frame(b"IBJOB005", 2, &body));
        assert_eq!(
            protocol::read_versioned_request(&mut wire.as_slice())
                .await
                .unwrap(),
            Request::VideoV5(body.to_vec())
        );
        assert!(protocol::read_request(&mut wire.as_slice()).await.is_err());
    }
    for body in [vec![], vec![1], vec![0, 1], vec![3, 1], vec![255, 1]] {
        assert!(
            protocol::write_video_request(&body, &mut tokio::io::sink())
                .await
                .is_err()
        );
        assert!(
            protocol::read_versioned_request(
                &mut frame(b"IBJOB005", body.len() as u64, &body).as_slice()
            )
            .await
            .is_err()
        );
    }
    for length in [0, 1, protocol::MAX_INPUT + 2, u64::MAX] {
        assert!(
            protocol::read_versioned_request(&mut frame(b"IBJOB005", length, b"").as_slice())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn fixed_disk_size_exact_completion_and_response_version_are_required() {
    let body = vec![0; protocol::VIDEO_OUTPUT_LENGTH as usize];
    let mut response = Vec::new();
    protocol::write_video_response(&body, &mut response)
        .await
        .unwrap();
    assert_eq!(
        protocol::read_video_response(&mut response.as_slice())
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
        protocol::read_pdf_response(&mut response.as_slice())
            .await
            .is_err()
    );
    for magic in [b"IBOUT001", b"IBOUT002", b"IBOUT003", b"IBOUT004"] {
        assert!(
            protocol::read_video_response(
                &mut frame(magic, protocol::VIDEO_OUTPUT_LENGTH, &[]).as_slice()
            )
            .await
            .is_err()
        );
    }
    response.push(0);
    assert!(
        protocol::read_video_response(&mut response.as_slice())
            .await
            .is_err()
    );
    assert!(
        protocol::write_video_response(&body[..body.len() - 1], &mut tokio::io::sink())
            .await
            .is_err()
    );
    let request = frame(b"IBJOB005", 3, &[1, 2, 3]);
    for end in 0..request.len() {
        assert!(
            protocol::read_versioned_request(&mut &request[..end])
                .await
                .is_err()
        );
    }
    let mut trailing = request;
    trailing.push(0);
    assert!(
        protocol::read_versioned_request(&mut trailing.as_slice())
            .await
            .is_err()
    );
}
