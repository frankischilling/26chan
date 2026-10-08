#![cfg(feature = "database-tests")]

//! Real SQL intake and mTLS transport, with hand-built untrusted responses.
//! No guest crate, VM, encoder, asset approval, or publication is invoked.
use board_media::{
    ObjectId, Quarantine,
    paired::{self, InputKind},
};
use board_media_admin::paired::dispatch_candidate;
use board_media_dispatch::{
    ClientSettings, DispatchClient, config::GatewaySettings, protocol::read_paired_request,
    tls::server_config,
};
use board_store::{
    media::MediaQueue,
    media_assets::SourceProfile,
    media_intake::{IntakeStore, PairedInputDescriptor},
};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::oneshot};
use tokio_rustls::TlsAcceptor;

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}
fn digest(value: &[u8]) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}
fn private(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
fn certificates(root: &Path) {
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca_key = KeyPair::generate().unwrap();
    private(&root.join("ca.pem"), ca.self_signed(&ca_key).unwrap().pem());
    let issuer = Issuer::new(ca, ca_key);
    for (name, usage) in [
        ("server", ExtendedKeyUsagePurpose::ServerAuth),
        ("client", ExtendedKeyUsagePurpose::ClientAuth),
    ] {
        let mut params = CertificateParams::new(vec!["dispatch.test".into()]).unwrap();
        params.extended_key_usages = vec![usage];
        let key = KeyPair::generate().unwrap();
        private(
            &root.join(format!("{name}.pem")),
            params.signed_by(&key, &issuer).unwrap().pem(),
        );
        private(&root.join(format!("{name}.key")), key.serialize_pem());
    }
}
fn source_png() -> Vec<u8> {
    // Frozen red pixel with removable text and trailer. Independent golden
    // source digests match the image-v1 provenance controls.
    let red = hex(
        "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c48900000010494441547801010500faff00ff0000ff050001fffa5c88d10000000049454e44ae426082",
    );
    [
        red[..33].to_vec(),
        hex("00000003744558746e0076cdcf317b"),
        red[33..].to_vec(),
        b"ignored trailer".to_vec(),
    ]
    .concat()
}
fn response(binding: [u8; 32], present: bool, case: &str) -> Vec<u8> {
    // This frozen wire was independently produced from the recorder fixture.
    // Its 2x3 canvas intentionally differs from the returned 1x1 PNG pixels.
    let mut wire = include_bytes!("../../../tests/media/fixtures/replay-wire/empty.ibr").to_vec();
    wire[24..26].copy_from_slice(&2_u16.to_be_bytes());
    wire[26..28].copy_from_slice(&3_u16.to_be_bytes());
    let mut disk = vec![0; 4_456_960];
    disk[..8].copy_from_slice(b"IBRES002");
    disk[8..10].copy_from_slice(&2_u16.to_be_bytes());
    disk[10..12].copy_from_slice(&64_u16.to_be_bytes());
    disk[12..16].copy_from_slice(&u32::from(present).to_be_bytes());
    disk[16..48].copy_from_slice(&binding);
    disk[48..56].copy_from_slice(&20_u64.to_be_bytes());
    disk[64..84].copy_from_slice(b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff");
    if present {
        disk[56..64].copy_from_slice(&(wire.len() as u64).to_be_bytes());
        disk[84..84 + wire.len()].copy_from_slice(&wire);
    }
    match case {
        "wrong-binding" => disk[16] ^= 1,
        "wrong-presence" => disk[12..16].copy_from_slice(&u32::from(!present).to_be_bytes()),
        "nonzero-padding" => *disk.last_mut().unwrap() = 1,
        _ => (),
    }
    disk
}

#[tokio::test]
async fn paired_sql_snapshot_and_mtls_candidates_remain_nonpublishable() {
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    certificates(root);
    let quarantine = Quarantine::new(root.join("quarantine")).unwrap();
    let mut ids = Vec::new();
    let mut bindings = Vec::new();
    for case in [
        "absent",
        "present",
        "wrong-binding",
        "wrong-presence",
        "nonzero-padding",
        "bundle-digest",
        "image-digest",
        "replay-digest",
        "file-substitution",
        "snapshot-mutation",
    ] {
        let present = case != "absent";
        let reservation = intake
            .reserve_pair("private-filename-not-transported.png")
            .await
            .unwrap();
        ids.push(reservation.id.clone());
        let id: ObjectId = reservation.id.parse().unwrap();
        let png = source_png();
        let replay = include_bytes!("../../media-guest/tests/fixtures/replay/empty.tgkr");
        let input = paired::encode_input(
            InputKind::PairedV2,
            id.bytes(),
            &png,
            present.then_some(replay.as_slice()),
        )
        .unwrap();
        intake
            .begin_pair_upload(&reservation.id, &reservation.capability)
            .await
            .unwrap();
        let receipt = quarantine.receive_pair(id, input.as_slice()).await.unwrap();
        let mut descriptor = PairedInputDescriptor {
            bytes: receipt.bytes,
            sha256: digest(&receipt.sha256),
            image_bytes: receipt.image_bytes,
            image_sha256: digest(&receipt.image_sha256),
            replay_bytes: receipt.replay_bytes,
            replay_sha256: receipt.replay_sha256.as_ref().map(|d| digest(d)),
        };
        // Persist deliberately corrupted intake metadata through the actual
        // intake boundary. Later UPDATE is forbidden by the immutable SQL guard.
        match case {
            "bundle-digest" => descriptor.sha256 = "0".repeat(64),
            "image-digest" => descriptor.image_sha256 = "0".repeat(64),
            "replay-digest" => descriptor.replay_sha256 = Some("0".repeat(64)),
            _ => (),
        }
        intake
            .finish_pair_upload(&reservation.id, &reservation.capability, &descriptor)
            .await
            .unwrap();
        let input_path = root.join("quarantine").join(format!("{id}.input"));
        if case == "file-substitution" {
            // Equal length and valid frame: length checks alone cannot detect it.
            let mut substituted = input.clone();
            substituted[48 + 29] ^= 1;
            let replacement = root.join("replacement");
            private(&replacement, substituted);
            std::fs::rename(replacement, &input_path).unwrap();
        }
        let listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
        let endpoint = listener.local_addr().unwrap();
        let client = DispatchClient::new(&ClientSettings {
            endpoint,
            server_name: "dispatch.test".into(),
            server_ca: root.join("ca.pem"),
            client_certificate: root.join("client.pem"),
            client_key: root.join("client.key"),
        })
        .unwrap();
        let acceptor = TlsAcceptor::from(
            server_config(&GatewaySettings {
                listen: endpoint,
                server_certificate: root.join("server.pem"),
                server_key: root.join("server.key"),
                client_ca: root.join("ca.pem"),
                authorization_file: root.join("unused"),
                broker_socket: root.join("unused.sock"),
            })
            .unwrap(),
        );
        let accepted = Arc::new(AtomicBool::new(false));
        let accepted_server = accepted.clone();
        let server_listener = listener.clone();
        let (arrival_tx, arrival) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let expected_bundle = receipt.sha256;
        let server = tokio::spawn(async move {
            let (socket, _) = server_listener.accept().await.unwrap();
            accepted_server.store(true, Ordering::SeqCst);
            let mut stream = acceptor.accept(socket).await.unwrap();
            assert!(!stream.get_ref().1.peer_certificates().unwrap().is_empty());
            let (binding, received) = read_paired_request(&mut stream).await.unwrap();
            assert_eq!(received, input, "case {case}");
            assert_ne!(
                binding, expected_bundle,
                "attempt binding is not a source fingerprint"
            );
            arrival_tx.send(binding).unwrap();
            released.await.unwrap();
            let disk = response(binding, present, case);
            stream.write_all(b"IBOUT002").await.unwrap();
            stream
                .write_all(&(disk.len() as u64).to_be_bytes())
                .await
                .unwrap();
            stream.write_all(&disk).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let before_transport = matches!(
            case,
            "bundle-digest" | "image-digest" | "replay-digest" | "file-substitution"
        );
        let dispatch = dispatch_candidate(&queue, &quarantine, &client);
        let result = if before_transport {
            let result = tokio::time::timeout(Duration::from_secs(5), dispatch)
                .await
                .unwrap();
            server.abort();
            let _ = server.await;
            assert!(
                !accepted.load(Ordering::SeqCst),
                "case {case} reached transport"
            );
            // Also inspect the kernel backlog; task scheduling cannot hide a connect.
            let listener = Arc::try_unwrap(listener).unwrap().into_std().unwrap();
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
            );
            result
        } else {
            let observe = async {
                let binding = tokio::time::timeout(Duration::from_secs(5), arrival)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(
                    !bindings.contains(&binding),
                    "fresh binding on every attempt"
                );
                bindings.push(binding);
                let processing = queue.get(&reservation.id).await.unwrap();
                assert_eq!(processing.state, "processing");
                assert_eq!(processing.attempts, 1);
                assert!(processing.lease_token.is_some());
                if case == "snapshot-mutation" {
                    std::fs::write(&input_path, b"mutated opened inode").unwrap();
                    std::fs::rename(&input_path, root.join("retired-input")).unwrap();
                    private(&input_path, b"new pathname contents");
                }
                release.send(()).unwrap();
            };
            let (result, ()) = tokio::join!(dispatch, observe);
            server.await.unwrap();
            result
        };
        let succeeds = matches!(case, "absent" | "present" | "snapshot-mutation");
        assert_eq!(result.is_ok(), succeeds, "case {case}");
        if let Ok(checked) = result {
            assert_eq!(checked.job_id(), id);
            assert_eq!(checked.bundle_sha256(), &receipt.sha256);
            let source = checked.png_source();
            assert_eq!(
                source.input_sha256,
                "f482035298dcdf31dca1ce576adea047626ddb9deff6c5bfce3cf1ea68b15357"
            );
            assert_ne!(source.input_sha256, digest(checked.bundle_sha256()));
            assert_eq!(source.input_bytes, 103);
            assert_eq!(source.profile, SourceProfile::PngV1);
            assert_eq!(source.retained_bytes, 73);
            assert_eq!(
                source.md5.as_slice(),
                hex("b4e7464f29bcc44451c570504d61030b")
            );
            let candidate = checked.candidate().unwrap();
            assert_eq!(candidate.dimensions(), (1, 1));
            assert_eq!(candidate.rgba_bytes(), &[255, 0, 0, 255]);
            assert_eq!(candidate.replay_wire_bytes().is_some(), present);
            if let Some(replay) = candidate.untrusted_replay() {
                assert_eq!((replay.metadata.width, replay.metadata.height), (2, 3));
            }
        }
        let finished = queue.get(&reservation.id).await.unwrap();
        assert_eq!(
            finished.state, "failed",
            "terminal candidate is not publication"
        );
        assert_eq!(
            finished.failure.as_deref(),
            Some(if succeeds {
                "candidate_checked"
            } else {
                "processing_failed"
            })
        );
        assert!(finished.lease_token.is_none());
        assert!(finished.expires_at.is_none());
        assert!(finished.output_sha256.is_none());
        assert!(finished.output_bytes.is_none());
        let assets: i64 = sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=$1")
            .bind(&reservation.id)
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(assets, 0, "case {case}");
        assert!(queue.claim_paired_candidate().await.unwrap().is_none());
    }
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(ids)
        .execute(&admin)
        .await
        .unwrap();
    intake.close().await.unwrap();
    admin.close().await;
}
