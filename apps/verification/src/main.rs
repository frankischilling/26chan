#![forbid(unsafe_code)]
use board_store::verification::VerificationStore;
use board_verification::{AppState, config::Settings, router};

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let check = args.len() == 1 && args[0] == "--check-config";
    let recipient_hash = args.len() == 1 && args[0] == "--recipient-hash";
    if !args.is_empty() && !check && !recipient_hash {
        eprintln!("verification arguments rejected");
        return std::process::ExitCode::FAILURE;
    }
    let Ok(settings) = Settings::from_env() else {
        eprintln!("verification configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    if check {
        return std::process::ExitCode::SUCCESS;
    }
    if recipient_hash {
        return match hash_recipient(&settings.verification_key) {
            Ok(value) => {
                println!("{value}");
                std::process::ExitCode::SUCCESS
            }
            Err(()) => {
                eprintln!("verification recipient rejected");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
    else {
        eprintln!("verification runtime unavailable");
        return std::process::ExitCode::FAILURE;
    };
    if runtime.block_on(run(settings)).is_err() {
        eprintln!("verification service stopped with an error");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn hash_recipient(key: &board_domain::challenge_secrets::VerificationKey) -> Result<String, ()> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(321)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 320 {
        return Err(());
    }
    let value = std::str::from_utf8(&bytes).map_err(|_| ())?;
    let email = board_domain::email_verification::PreparedEmail::prepare(value).map_err(|_| ())?;
    Ok(key
        .email_recipient(&email)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

async fn run(settings: Settings) -> Result<(), ()> {
    let store = VerificationStore::connect(&settings.database_url)
        .await
        .map_err(|_| ())?;
    let app = router(
        AppState::new(
            store.clone(),
            settings.verification_key,
            settings.mail_key,
            &settings.service_token,
        )
        .map_err(|_| ())?,
    );
    let listener = tokio::net::TcpListener::bind(settings.bind)
        .await
        .map_err(|_| ())?;
    eprintln!("verification service ready");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await;
    store.close().await;
    result.map_err(|_| ())
}
async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {_=interrupt=>(),_=terminate=>()}
}
