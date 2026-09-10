pub mod config;
#[cfg(target_os = "linux")]
pub mod gateway;
pub mod protocol;
pub mod tls;
pub use config::ClientSettings;
pub use tls::DispatchClient;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("invalid frame")]
    Frame,
    #[error("transport unavailable")]
    Transport,
    #[error("deadline exceeded")]
    Deadline,
    #[error("invalid configuration")]
    Configuration,
    #[error("authentication rejected")]
    Authentication,
    #[error("service busy")]
    Busy,
}

pub type Result<T> = std::result::Result<T, Error>;
