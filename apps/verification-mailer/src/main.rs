#![forbid(unsafe_code)]
mod smtp;
use board_domain::{challenge_secrets::ReceiptToken, verification_mail::MailKey};
use board_store::verification::mail::MailStore;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
};
struct Settings {
    database: String,
    key: MailKey,
    relay: SocketAddr,
    origin: String,
}
impl Settings {
    fn parse(values: BTreeMap<String, String>) -> Result<Self, ()> {
        for (name, value) in &values {
            if value.is_empty() {
                continue;
            }
            if (name.ends_with("DATABASE_URL") && name != "VERIFICATION_MAIL_DATABASE_URL")
                || name.starts_with("AWS_")
                || name.starts_with("AZURE_")
                || name.starts_with("GOOGLE_")
                || name.starts_with("DEPLOY_")
                || matches!(name.as_str(), "SSH_AUTH_SOCK" | "DOCKER_HOST")
                || ((name.ends_with("_KEY")
                    || name.ends_with("_TOKEN")
                    || name.ends_with("_PASSWORD")
                    || name.ends_with("_SECRET"))
                    && name != "VERIFICATION_MAIL_KEY")
            {
                return Err(());
            }
        }
        let get = |name| values.get(name).ok_or(());
        let database = get("VERIFICATION_MAIL_DATABASE_URL")?.clone();
        let url = url::Url::parse(&database).map_err(|_| ())?;
        let local = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.username() != "board_mailer"
            || !local
            || url.password().is_none_or(str::is_empty)
            || url.path().len() <= 1
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(());
        }
        let raw_key = get("VERIFICATION_MAIL_KEY")?;
        if url.password() == Some(raw_key.as_str()) {
            return Err(());
        }
        let key = MailKey::parse(raw_key).ok_or(())?;
        let relay: SocketAddr = get("VERIFICATION_SMTP_ADDR")?.parse().map_err(|_| ())?;
        if !relay.ip().is_loopback() || relay.port() == 0 {
            return Err(());
        }
        let origin = get("VERIFICATION_PUBLIC_ORIGIN")?.clone();
        let parsed = url::Url::parse(&origin).map_err(|_| ())?;
        if parsed.origin().ascii_serialization() != origin
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(());
        }
        if parsed.scheme() != "https"
            && !(parsed.scheme() == "http"
                && parsed.host_str().is_some_and(|s| {
                    s.trim_matches(['[', ']'])
                        .parse::<IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
                }))
        {
            return Err(());
        }
        Ok(Self {
            database,
            key,
            relay,
            origin,
        })
    }
}
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let check = args.len() == 1 && args[0] == "--check-config";
    if args.len() != 1 || (!check && args[0] != "--once") {
        eprintln!("mailer arguments rejected");
        return std::process::ExitCode::FAILURE;
    }
    let mut values = BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        let (Ok(name), Ok(value)) = (name.into_string(), value.into_string()) else {
            return std::process::ExitCode::FAILURE;
        };
        if values.insert(name.to_ascii_uppercase(), value).is_some() {
            return std::process::ExitCode::FAILURE;
        }
    }
    let Ok(settings) = Settings::parse(values) else {
        eprintln!("mailer configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    if check {
        return std::process::ExitCode::SUCCESS;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return std::process::ExitCode::FAILURE;
    };
    match runtime.block_on(run(settings)) {
        Ok(count) => {
            println!("verification mail acknowledged: {count}");
            std::process::ExitCode::SUCCESS
        }
        Err(()) => {
            eprintln!("verification mail delivery incomplete");
            std::process::ExitCode::FAILURE
        }
    }
}
async fn run(settings: Settings) -> Result<usize, ()> {
    let store = MailStore::connect(&settings.database)
        .await
        .map_err(|_| ())?;
    let mut sent = 0;
    let result = async {
        for _ in 0..100 {
            // Claim one message at a time so every delivery gets a fresh lease,
            // even when previous deliveries consumed their entire SMTP budget.
            let lease = ReceiptToken::generate().map_err(|_| ())?.storage_hash();
            let jobs = store.claim(&lease, 1).await.map_err(|_| ())?;
            let Some(job) = jobs.into_iter().next() else {
                break;
            };
            let token: [u8; 32] = job.token_hash.as_slice().try_into().map_err(|_| ())?;
            let delivered = if let Some(payload) = settings.key.open(&token, &job.encrypted_payload)
            {
                smtp::deliver(settings.relay, &settings.origin, &job.request_id, &payload)
                    .await
                    .is_ok()
            } else {
                false
            };
            if !store
                .finish(&job.request_id, &lease, delivered)
                .await
                .map_err(|_| ())?
            {
                return Err(());
            }
            if !delivered {
                return Err(());
            }
            sent += 1;
        }
        Ok(sent)
    }
    .await;
    store.close().await;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn values() -> BTreeMap<String, String> {
        [
            (
                "VERIFICATION_MAIL_DATABASE_URL",
                "postgres://board_mailer:synthetic@127.0.0.1:55432/owned".into(),
            ),
            ("VERIFICATION_MAIL_KEY", "ab".repeat(32)),
            ("VERIFICATION_SMTP_ADDR", "127.0.0.1:1025".into()),
            ("VERIFICATION_PUBLIC_ORIGIN", "http://127.0.0.1:3000".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect()
    }
    #[test]
    fn isolated_local_mail_config_and_rejections() {
        assert!(Settings::parse(values()).is_ok());
        for (key, value) in [
            ("VERIFICATION_KEY", "secret"),
            ("DATABASE_URL", "postgres://other"),
            ("VERIFICATION_SMTP_ADDR", "192.0.2.1:25"),
            ("VERIFICATION_PUBLIC_ORIGIN", "http://public.example"),
            ("VERIFICATION_PUBLIC_ORIGIN", "https://public.example/path"),
            (
                "VERIFICATION_PUBLIC_ORIGIN",
                "https://user:secret@public.example",
            ),
        ] {
            let mut v = values();
            v.insert(key.into(), value.into());
            assert!(Settings::parse(v).is_err(), "{key}");
        }
    }
}
