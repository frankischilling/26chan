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
pub(crate) const STILL_PROCESSING: Duration = Duration::from_secs(65);

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
    /// Explicit paired transport; never retries or accepts an image-v1 response.
    pub async fn process_paired<R: AsyncRead + Unpin>(
        &self,
        input: R,
        length: u64,
        binding: &[u8; 32],
    ) -> Result<Vec<u8>> {
        if !(protocol::MIN_PAIRED_INPUT..=protocol::MAX_PAIRED_INPUT).contains(&length) {
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
        timeout(
            INTAKE,
            protocol::write_paired_request(input, length, binding, &mut stream),
        )
        .await
        .map_err(|_| Error::Deadline)??;
        timeout(PROCESSING, protocol::read_paired_response(&mut stream))
            .await
            .map_err(|_| Error::Deadline)?
    }

    /// The larger deadline applies only to an explicit, bounded still request.
    pub async fn process_still(&self, input: &[u8], policy: &[u8; 16]) -> Result<Vec<u8>> {
        if input.is_empty() || input.len() as u64 > protocol::MAX_STILL_INPUT - 16 {
            return Err(Error::Frame);
        }
        let mut frame = Vec::with_capacity(input.len() + 16);
        frame.extend_from_slice(policy);
        frame.extend_from_slice(input);
        protocol::validate_still_request(&frame)?;
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
        timeout(INTAKE, protocol::write_still_request(&frame, &mut stream))
            .await
            .map_err(|_| Error::Deadline)??;
        timeout(STILL_PROCESSING, protocol::read_still_response(&mut stream))
            .await
            .map_err(|_| Error::Deadline)?
    }

    /// Explicit video transport. The caller supplies one finite container tag;
    /// no format guessing, alternate response version, or retry is permitted.
    pub async fn process_video(&self, input: &[u8], container: u8) -> Result<Vec<u8>> {
        if input.is_empty()
            || input.len() as u64 > protocol::MAX_INPUT
            || !matches!(container, 1 | 2)
        {
            return Err(Error::Frame);
        }
        let mut frame = Vec::with_capacity(input.len() + 1);
        frame.push(container);
        frame.extend_from_slice(input);
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
        timeout(INTAKE, protocol::write_video_request(&frame, &mut stream))
            .await
            .map_err(|_| Error::Deadline)??;
        timeout(PROCESSING, protocol::read_video_response(&mut stream))
            .await
            .map_err(|_| Error::Deadline)?
    }

    /// Explicit PDF transport; no image/GIF response or fallback is accepted.
    pub async fn process_pdf<R: AsyncRead + Unpin>(
        &self,
        input: R,
        length: u64,
    ) -> Result<Vec<u8>> {
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
        timeout(
            INTAKE,
            protocol::write_pdf_request(input, length, &mut stream),
        )
        .await
        .map_err(|_| Error::Deadline)??;
        timeout(PROCESSING, protocol::read_pdf_response(&mut stream))
            .await
            .map_err(|_| Error::Deadline)?
    }

    /// Explicit GIF transport; a v1/v2 response is never accepted or retried.
    pub async fn process_gif<R: AsyncRead + Unpin>(
        &self,
        input: R,
        length: u64,
    ) -> Result<Vec<u8>> {
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
        timeout(
            INTAKE,
            protocol::write_gif_request(input, length, &mut stream),
        )
        .await
        .map_err(|_| Error::Deadline)??;
        timeout(PROCESSING, protocol::read_gif_response(&mut stream))
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
