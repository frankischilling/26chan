#![cfg(feature = "database-tests")]
// Controlled mTLS envelopes qualify receipt plumbing, not document/container parsing.
use board_media::{PublicationStore, Quarantine};
use board_media_dispatch::{
    ClientSettings, DispatchClient,
    config::GatewaySettings,
    protocol::{Request, read_versioned_request, write_pdf_response, write_video_response},
    tls::server_config,
};
use board_store::media::MediaQueue;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use std::path::Path;
use tokio::{io::AsyncWriteExt, net::TcpListener};
use tokio_rustls::TlsAcceptor;
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

#[tokio::test]
async fn pdf_and_video_receipts_describe_original_uploads_not_worker_envelopes() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    certificates(root);
    let quarantine = Quarantine::new(root.join("private")).unwrap();
    let store = PublicationStore::new(root.join("objects"), &quarantine).unwrap();
    for (extension, input, sha, md5, tag) in [
        (
            "pdf",
            b"%PDF-1.4\nowned byte-retention fixture\n".as_slice(),
            "ac534a329138d070d7f691a233d0f0b2c2d7d0ead0ee2829e48affab04563a85",
            "bef278b940e0a38a51721b7db23ade52",
            0,
        ),
        (
            "webm",
            b"owned original webm input".as_slice(),
            "9bb098d65b095fd5af75504c540d3cbd635460217c55023ce2331abc69987e8e",
            "d7817d2805b1abb11d46ffaddf41e353",
            1,
        ),
        (
            "mp4",
            b"owned original mp4 input".as_slice(),
            "5d969f187b60b4bd7fabffeb044c900827651b06e3f1da6487bf903e07ef104a",
            "54c5fe9302a7728a4389eedbbda315bc",
            2,
        ),
    ] {
        let job = queue
            .reserve(&format!("original.{extension}"))
            .await
            .unwrap();
        let length = quarantine
            .receive(job.id.parse().unwrap(), input)
            .await
            .unwrap();
        queue.queue(&job.id, length).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
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
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut stream = acceptor.accept(socket).await.unwrap();
            let request = read_versioned_request(&mut stream).await.unwrap();
            if tag == 0 {
                assert_eq!(request, Request::PdfV4(input.to_vec()));
                let mut disk = vec![0; board_media::OUTPUT_DISK_BYTES as usize];
                disk[..20].copy_from_slice(b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff");
                write_pdf_response(&disk, &mut stream).await.unwrap();
            } else {
                assert_eq!(
                    request,
                    Request::VideoV5([vec![tag], input.to_vec()].concat())
                );
                let mut disk = vec![0; board_media::video::OUTPUT_DISK_BYTES as usize];
                disk[..8].copy_from_slice(b"IBVID005");
                disk[8] = tag;
                disk[16..20].copy_from_slice(&640_u32.to_be_bytes());
                disk[20..24].copy_from_slice(&480_u32.to_be_bytes());
                disk[24..32].copy_from_slice(&1_250_000_u64.to_be_bytes());
                disk[40..44].copy_from_slice(&1_u32.to_be_bytes());
                disk[44..48].copy_from_slice(&1_u32.to_be_bytes());
                disk[48..56].copy_from_slice(&12_u64.to_be_bytes());
                if tag == 1 {
                    disk[64..68].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
                } else {
                    disk[68..72].copy_from_slice(b"ftyp");
                }
                disk[76..80].copy_from_slice(&[255, 0, 0, 255]);
                write_video_response(&disk, &mut stream).await.unwrap();
            }
            stream.shutdown().await.unwrap();
        });
        let result = board_media_admin::dispatch(&queue, &quarantine, &store, &client).await;
        server.await.unwrap();
        let asset = result.unwrap();
        let original: (String, i64, String, String) = sqlx::query_as(
            "SELECT sha256,bytes,encode(md5,'hex'),component FROM media.input_receipts WHERE asset_id=$1"
        ).bind(&asset.id).fetch_one(&admin).await.unwrap();
        assert_eq!(
            original,
            (sha.into(), input.len() as i64, md5.into(), "upload".into())
        );
        if tag != 0 {
            assert_ne!(asset.sha256, sha);
        }
        sqlx::query("DELETE FROM media.assets WHERE id=$1")
            .bind(&asset.id)
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query("DELETE FROM media.jobs WHERE id=$1")
            .bind(&job.id)
            .execute(&admin)
            .await
            .unwrap();
    }
}
