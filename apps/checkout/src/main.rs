use board_checkout::{AppState, config::Settings, router};
use board_store::checkout::CheckoutStore;
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("checkout adapter unavailable");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), ()> {
    let settings = Settings::from_env().map_err(|_| ())?;
    let store = CheckoutStore::connect(&settings.database)
        .await
        .map_err(|_| ())?;
    let listener = tokio::net::TcpListener::bind(settings.bind)
        .await
        .map_err(|_| ())?;
    let result = axum::serve(listener, router(AppState::new(store.clone(), settings.key)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| ());
    store.close().await;
    result
}
