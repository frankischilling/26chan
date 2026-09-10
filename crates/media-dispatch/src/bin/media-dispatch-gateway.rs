#[cfg(target_os = "linux")]
fn run() -> board_media_dispatch::Result<()> {
    use board_media_dispatch::{
        Error,
        config::GatewaySettings,
        gateway::{Gateway, validate_process},
    };
    validate_process()?;
    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or(Error::Configuration)?;
    if args.next().is_some() {
        return Err(Error::Configuration);
    }
    let settings = GatewaySettings::read(std::path::Path::new(&path))?;
    let gateway = Gateway::new(&settings)?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Error::Configuration)?
        .block_on(async move {
            let listener = tokio::net::TcpListener::bind(settings.listen)
                .await
                .map_err(|_| Error::Transport)?;
            gateway.serve(listener).await
        })
}

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "linux")]
    let result = run();
    #[cfg(not(target_os = "linux"))]
    let result: board_media_dispatch::Result<()> = Err(board_media_dispatch::Error::Configuration);
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
