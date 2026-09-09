use crate::{
    Error, Result,
    config::{ClientSettings, GatewaySettings, read_file},
    protocol,
};
use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};
use tokio::io::AsyncRead;
use tokio::{net::TcpStream, time::timeout};
use tokio_rustls::TlsConnector;

pub(crate) const INTAKE: Duration = Duration::from_secs(3);
pub(crate) const PROCESSING: Duration = Duration::from_secs(25);

fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let bytes = read_file(path, false, false, 65_536)?;
    let certificates = CertificateDer::pem_slice_iter(&bytes)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::Configuration)?;
    if certificates.is_empty() {
        return Err(Error::Configuration);
    }
    Ok(certificates)
}

fn key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_slice(&read_file(path, true, false, 65_536)?)
        .map_err(|_| Error::Configuration)
}

fn roots(path: &Path) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates(path)? {
        roots.add(certificate).map_err(|_| Error::Configuration)?;
    }
    Ok(roots)
}

pub struct DispatchClient {
    endpoint: SocketAddr,
    name: ServerName<'static>,
    connector: TlsConnector,
}
impl DispatchClient {
    pub fn new(settings: &ClientSettings) -> Result<Self> {
        settings.validate()?;
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| Error::Configuration)?
        .with_root_certificates(roots(&settings.server_ca)?)
        .with_client_auth_cert(
            certificates(&settings.client_certificate)?,
            key(&settings.client_key)?,
        )
        .map_err(|_| Error::Configuration)?;
        config.resumption = rustls::client::Resumption::disabled();
        config.enable_early_data = false;
        Ok(Self {
            endpoint: settings.endpoint,
            name: ServerName::try_from(settings.server_name.clone())
                .map_err(|_| Error::Configuration)?,
            connector: TlsConnector::from(Arc::new(config)),
        })
    }
    pub async fn process<R: AsyncRead + Unpin>(&self, input: R, length: u64) -> Result<Vec<u8>> {
        if !(1..=protocol::MAX_INPUT).contains(&length) {
            return Err(Error::Frame);
        }
        let mut stream = timeout(INTAKE, async {
            let socket = TcpStream::connect(self.endpoint)
                .await
                .map_err(|_| Error::Transport)?;
            self.connector
                .connect(self.name.clone(), socket)
                .await
                .map_err(|_| Error::Authentication)
        })
        .await
        .map_err(|_| Error::Deadline)??;
        timeout(INTAKE, protocol::write_request(input, length, &mut stream))
            .await
            .map_err(|_| Error::Deadline)??;
        timeout(PROCESSING, protocol::read_response(&mut stream))
            .await
            .map_err(|_| Error::Deadline)?
    }
}

pub fn server_config(settings: &GatewaySettings) -> Result<Arc<rustls::ServerConfig>> {
    settings.validate()?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots(&settings.client_ca)?),
        provider.clone(),
    )
    .build()
    .map_err(|_| Error::Configuration)?;
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| Error::Configuration)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            certificates(&settings.server_certificate)?,
            key(&settings.server_key)?,
        )
        .map_err(|_| Error::Configuration)?;
    config.send_tls13_tickets = 0;
    config.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}
