//! Linux transport boundary. The CLI validates the process before building a Gateway.
use crate::{
    Error, Result,
    config::{GatewaySettings, authorizations},
    protocol,
    tls::{INTAKE, PROCESSING, server_config},
};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};
use tokio::{
    net::{TcpListener, TcpStream, UnixStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::timeout,
};
use tokio_rustls::TlsAcceptor;

pub struct Gateway {
    acceptor: TlsAcceptor,
    authorization_file: PathBuf,
    broker_socket: PathBuf,
}
impl Gateway {
    pub fn new(settings: &GatewaySettings) -> Result<Self> {
        let acceptor = TlsAcceptor::from(server_config(settings)?);
        authorizations(&settings.authorization_file)?;
        Ok(Self {
            acceptor,
            authorization_file: settings.authorization_file.clone(),
            broker_socket: settings.broker_socket.clone(),
        })
    }

    pub async fn serve(self, listener: TcpListener) -> Result<()> {
        let gateway = Arc::new(self);
        let handshakes = Arc::new(Semaphore::new(4));
        let work = Arc::new(Semaphore::new(1));
        let mut tasks = JoinSet::new();
        loop {
            // Reap every completed task before accepting, keeping owned task storage bounded.
            while tasks.try_join_next().is_some() {}
            let (socket, _) = listener.accept().await.map_err(|_| Error::Transport)?;
            let Ok(handshake) = handshakes.clone().try_acquire_owned() else {
                drop(socket);
                continue;
            };
            let gateway = gateway.clone();
            let work = work.clone();
            tasks.spawn(async move {
                if let Err(error) = gateway.connection(socket, handshake, work).await {
                    eprintln!("{error}");
                }
            });
        }
    }

    fn authorize(&self, fingerprint: &[u8; 32]) -> Result<()> {
        if !authorizations(&self.authorization_file)?.contains(fingerprint) {
            return Err(Error::Authentication);
        }
        Ok(())
    }

    async fn connection(
        &self,
        socket: TcpStream,
        handshake: OwnedSemaphorePermit,
        work: Arc<Semaphore>,
    ) -> Result<()> {
        let mut stream = timeout(INTAKE, self.acceptor.accept(socket))
            .await
            .map_err(|_| Error::Deadline)?
            .map_err(|_| Error::Authentication)?;
        let leaf = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|chain| chain.first())
            .ok_or(Error::Authentication)?;
        let fingerprint: [u8; 32] = Sha256::digest(leaf).into();
        self.authorize(&fingerprint)?;
        let _work = work.try_acquire_owned().map_err(|_| Error::Busy)?;
        drop(handshake);
        let input = timeout(INTAKE, protocol::read_request(&mut stream))
            .await
            .map_err(|_| Error::Deadline)??;
        self.authorize(&fingerprint)?;
        timeout(PROCESSING, async {
            let mut broker = UnixStream::connect(&self.broker_socket)
                .await
                .map_err(|_| Error::Transport)?;
            if broker.peer_cred().map_err(|_| Error::Authentication)?.uid() != 0 {
                return Err(Error::Authentication);
            }
            protocol::write_request(input.as_slice(), input.len() as u64, &mut broker).await?;
            let disk = protocol::read_response(&mut broker).await?;
            self.authorize(&fingerprint)?;
            protocol::write_response(&disk, &mut stream).await
        })
        .await
        .map_err(|_| Error::Deadline)?
    }
}

pub fn validate_process() -> Result<()> {
    if rustix::process::getuid().is_root()
        || rustix::process::geteuid().is_root()
        || rustix::process::getgid().is_root()
        || rustix::process::getegid().is_root()
        || rustix::process::getuid() != rustix::process::geteuid()
        || rustix::process::getgid() != rustix::process::getegid()
    {
        return Err(Error::Configuration);
    }
    for (name, _) in std::env::vars_os() {
        if !matches!(
            name.to_str(),
            Some("APP_ENV" | "PATH" | "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ")
        ) {
            return Err(Error::Configuration);
        }
    }
    if std::env::var("APP_ENV").ok().as_deref() != Some("development") {
        return Err(Error::Configuration);
    }
    Ok(())
}
