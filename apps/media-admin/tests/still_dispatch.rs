#![cfg(feature = "database-tests")]
// Controlled mTLS responses qualify coordinator policy binding and publication.
// Real parser and isolation qualification runs in the disposable VM suite.
use board_media::{
    ApprovedFiles, PublicationStore, Quarantine,
    still::{OUTPUT_DISK_BYTES, StillFormat, source_policy},
};
use board_media_dispatch::{
    ClientSettings, DispatchClient,
    config::GatewaySettings,
    protocol::{Request, read_versioned_request, write_still_response},
    tls::server_config,
};
use board_store::{media::MediaQueue, media_intake::IntakeStore};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, net::TcpListener, sync::oneshot};
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
async fn still_context_is_bound_through_transport_approval_and_retained_identity() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let owned = ids.clone();
    let owner = admin.clone();
    let result=tokio::spawn(async move {
        let intake=IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await.unwrap();
        let queue=MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap()).await.unwrap();
        let temp=tempfile::tempdir().unwrap();let root=temp.path();certificates(root);
        let quarantine=Quarantine::new(root.join("private")).unwrap();
        let store=PublicationStore::new(root.join("objects"),&quarantine).unwrap();
        for case in ["valid","wrong-policy","expired"] {
            let upload=intake.reserve_image("name-not-transported.jpg","p",0,false).await.unwrap();
            owned.lock().unwrap().push(upload.id.clone());
            let mut input=vec![0;32];input[..3].copy_from_slice(b"\xff\xd8\xff");input[30..].copy_from_slice(b"\xff\xd9");
            intake.begin_upload(&upload.id,&upload.capability).await.unwrap();
            let length=quarantine.receive_still(upload.id.parse().unwrap(),input.as_slice()).await.unwrap();
            intake.finish_upload(&upload.id,&upload.capability,length).await.unwrap();
            let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let endpoint=listener.local_addr().unwrap();
            let client=DispatchClient::new(&ClientSettings{endpoint,server_name:"dispatch.test".into(),server_ca:root.join("ca.pem"),client_certificate:root.join("client.pem"),client_key:root.join("client.key")}).unwrap();
            let acceptor=TlsAcceptor::from(server_config(&GatewaySettings{listen:endpoint,server_certificate:root.join("server.pem"),server_key:root.join("server.key"),client_ca:root.join("ca.pem"),authorization_file:root.join("unused"),broker_socket:root.join("unused.sock")}).unwrap());
            let (arrival_tx,arrival)=oneshot::channel();let (release,released)=oneshot::channel();
            let expected=input.clone();
            let server=tokio::spawn(async move {
                let (socket,_)=listener.accept().await.unwrap();let mut stream=acceptor.accept(socket).await.unwrap();
                let Request::StillV6(request)=read_versioned_request(&mut stream).await.unwrap() else {panic!("wrong transport")};
                let policy=source_policy("p",StillFormat::Jpeg,true,false).unwrap();
                assert!(!policy.strip_exif);assert_eq!(&request[..16],&policy.header().unwrap());assert_eq!(&request[16..],expected);
                arrival_tx.send(()).unwrap();released.await.unwrap();
                let mut disk=vec![0;OUTPUT_DISK_BYTES as usize];disk[..8].copy_from_slice(b"IBIMG006");disk[8..24].copy_from_slice(&policy.header().unwrap());
                for (offset,value) in [(24,17u32),(28,9),(32,17),(36,9),(48,32),(52,5)] {disk[offset..offset+4].copy_from_slice(&value.to_be_bytes());}
                disk[64..96].copy_from_slice(&expected);disk[96..101].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
                if case=="wrong-policy" {disk[16]^=1;}
                write_still_response(&disk,&mut stream).await.unwrap();stream.shutdown().await.unwrap();
            });
            let dispatch=board_media_admin::dispatch(&queue,&quarantine,&store,&client);tokio::pin!(dispatch);
            tokio::select! { result=&mut dispatch=>panic!("dispatch returned before fixture reply: {}",result.is_ok()),
                result=arrival=>result.unwrap(), _=tokio::time::sleep(Duration::from_secs(10))=>panic!("transport did not arrive") }
            if case=="expired" {sqlx::query("UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(&upload.id).execute(&owner).await.unwrap();}
            release.send(()).unwrap();let result=dispatch.await;server.await.unwrap();
            if case=="valid" {
                let asset=result.unwrap();assert_eq!(asset.output_format,board_store::media_assets::MediaFormat::Jpeg);
                let files=ApprovedFiles::open(root.join("objects")).unwrap();
                assert_eq!(files.read_still(asset.id.parse().unwrap(),&asset.sha256,asset.bytes as u64,StillFormat::Jpeg).unwrap(),input);
                let profile:String=sqlx::query_scalar("SELECT source_profile FROM media.assets WHERE id=$1").bind(&asset.id).fetch_one(&owner).await.unwrap();assert_eq!(profile,"still-v6");
                let original:(String,i64,String,String)=sqlx::query_as("SELECT sha256,bytes,encode(md5,'hex'),component FROM media.input_receipts WHERE asset_id=$1").bind(&asset.id).fetch_one(&owner).await.unwrap();
                assert_eq!(original,("201e57bb8f1737c3c940e0a29508dba8a2273e44fb83aaf5af5bda37b8678449".into(),32,"a54cc8e5e5474a7c00fcb0ee01d717c8".into(),"upload".into()));
            } else {
                assert!(result.is_err());
                let count:i64=sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=$1 AND state='approved'").bind(&upload.id).fetch_one(&owner).await.unwrap();assert_eq!(count,0);
            }
        }
    }).await;
    let ids = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}
