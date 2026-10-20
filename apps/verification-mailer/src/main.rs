#![forbid(unsafe_code)]
mod smtp;
use board_domain::{challenge_secrets::ReceiptToken, verification_mail::MailKey};
use board_store::verification::mail::{MailJob, MailStore};
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
#[derive(Clone, Copy, PartialEq, Eq)]
enum Queue {
    Verification,
    Notice,
    Recovery,
}
async fn next_job(
    store: &MailStore,
    lease: &[u8; 32],
    turn: usize,
) -> Result<Option<(MailJob, Queue)>, ()> {
    let queues = [Queue::Verification, Queue::Notice, Queue::Recovery];
    for offset in 0..3 {
        let queue = queues[(turn + offset) % 3];
        let jobs = match queue {
            Queue::Verification => store.claim(lease, 1).await,
            Queue::Notice => store.claim_pass(lease, 1).await,
            Queue::Recovery => store.claim_recovery(lease, 1).await,
        }
        .map_err(|_| ())?;
        if let Some(job) = jobs.into_iter().next() {
            return Ok(Some((job, queue)));
        }
    }
    Ok(None)
}
async fn run(settings: Settings) -> Result<usize, ()> {
    let store = MailStore::connect(&settings.database)
        .await
        .map_err(|_| ())?;
    let mut sent = 0;
    let result = async {
        let mut incomplete = false;
        for turn in 0..100 {
            // Claim one message at a time so every delivery gets a fresh lease,
            // even when previous deliveries consumed their entire SMTP budget.
            let lease = ReceiptToken::generate().map_err(|_| ())?.storage_hash();
            let Some((job, queue)) = next_job(&store, &lease, turn).await? else {
                break;
            };
            let token: [u8; 32] = job.token_hash.as_slice().try_into().map_err(|_| ())?;
            let delivered = if queue != Queue::Verification {
                if let Some(payload) = settings.key.open_pass(&token, &job.encrypted_payload) {
                    smtp::deliver_pass(settings.relay, &settings.origin, &job.request_id, &payload)
                        .await
                        .is_ok()
                } else {
                    false
                }
            } else if let Some(payload) = settings.key.open(&token, &job.encrypted_payload) {
                smtp::deliver(settings.relay, &settings.origin, &job.request_id, &payload)
                    .await
                    .is_ok()
            } else {
                false
            };
            let finished = match queue {
                Queue::Notice => store.finish_pass(&job.request_id, &lease, delivered).await,
                Queue::Recovery => {
                    store
                        .finish_recovery(&job.request_id, &lease, delivered)
                        .await
                }
                Queue::Verification => store.finish(&job.request_id, &lease, delivered).await,
            }
            .map_err(|_| ())?;
            if !finished {
                return Err(());
            }
            if queue == Queue::Notice {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
            if !delivered {
                if queue == Queue::Verification {
                    return Err(());
                }
                incomplete = true;
                continue;
            }
            sent += 1;
        }
        if incomplete { Err(()) } else { Ok(sent) }
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

#[cfg(all(test, feature = "database-tests"))]
mod delivery_tests {
    use super::*;
    use board_domain::{
        challenge_secrets::{TokenKind, VerificationToken},
        verification_mail::MailPayload,
    };
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    static DELIVERY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    #[tokio::test]
    #[ignore = "Run explicitly with isolated mailer test credentials."]
    async fn delivery_acknowledges_only_accepted_local_mail() {
        let _guard = DELIVERY_LOCK.lock().await;
        let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        for accepted in [false, true] {
            let material = ReceiptToken::generate().unwrap().encode();
            let bytes = &material[3..35];
            let id = format!(
                "{}-{}-{}-{}-{}",
                &bytes[..8],
                &bytes[8..12],
                &bytes[12..16],
                &bytes[16..20],
                &bytes[20..]
            );
            let token = VerificationToken::generate(TokenKind::Email)
                .unwrap()
                .encode();
            let payload = MailPayload::new("owned.delivery.fixture@gmail.com", &token).unwrap();
            let key = MailKey::parse(&material[3..]).unwrap();
            let hash = ReceiptToken::generate().unwrap().storage_hash();
            let encrypted = key.seal(&hash, &payload).unwrap();
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("INSERT INTO verification_private.email_requests(id,token_hash,email_hash,recipient_hash,peer_hash,country,created_at,expires_at,captcha_failure_limit) VALUES($1::text::uuid,$2,$2,$2,$2,'XX',statement_timestamp(),statement_timestamp()+interval '24 hours',6)")
                .bind(&id).bind(hash.as_slice()).execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO verification_private.mail_outbox(request_id,recipient_hash,encrypted_payload,created_at,expires_at,retry_after) VALUES($1::text::uuid,$2,$3,statement_timestamp(),statement_timestamp()+interval '1 hour',statement_timestamp())")
                .bind(&id).bind(hash.as_slice()).bind(encrypted).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let relay = listener.local_addr().unwrap();
            let receiver = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                stream
                    .get_mut()
                    .write_all(b"220 owned fixture\r\n")
                    .await
                    .unwrap();
                for response in ["250 OK\r\n", "250 OK\r\n", "250 OK\r\n", "354 DATA\r\n"] {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    stream
                        .get_mut()
                        .write_all(response.as_bytes())
                        .await
                        .unwrap();
                }
                let mut data = String::new();
                loop {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    if line == ".\r\n" {
                        break;
                    }
                    assert!(data.len() < 8192);
                    data.push_str(&line);
                }
                stream
                    .get_mut()
                    .write_all(if accepted {
                        b"250 accepted\r\n"
                    } else {
                        b"451 rejected\r\n"
                    })
                    .await
                    .unwrap();
                data
            });
            let result = run(Settings {
                database: std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap(),
                key,
                relay,
                origin: "http://127.0.0.1:3000".into(),
            })
            .await;
            let mail = tokio::time::timeout(std::time::Duration::from_secs(8), receiver)
                .await
                .unwrap()
                .unwrap();
            let delivered:bool=sqlx::query_scalar("SELECT delivered_at IS NOT NULL FROM verification_private.email_requests WHERE id=$1::text::uuid").bind(&id).fetch_one(&owner).await.unwrap();
            let queued:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.mail_outbox WHERE request_id=$1::text::uuid AND lease_hash IS NULL AND retry_after>clock_timestamp())").bind(&id).fetch_one(&owner).await.unwrap();
            sqlx::query("DELETE FROM verification_private.email_requests WHERE id=$1::text::uuid")
                .bind(&id)
                .execute(&owner)
                .await
                .unwrap();
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(delivered, accepted);
            assert_eq!(queued, !accepted);
            assert!(mail.contains(&format!("/signin?action=verify&tkn={token}")));
        }
        owner.close().await;
    }

    #[tokio::test]
    #[ignore = "Run explicitly with isolated mailer test credentials."]
    async fn pass_delivery_changes_expiry_state_only_after_relay_acceptance() {
        use board_domain::pass_mail::{Contact, Notice, Payload, Recipient};
        let _guard = DELIVERY_LOCK.lock().await;
        let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        for (accepted, recovery, setup) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, true, true),
            (true, true, true),
        ] {
            let material = ReceiptToken::generate().unwrap().encode();
            let bytes = &material[3..35];
            let id = format!(
                "{}-{}-{}-{}-{}",
                &bytes[..8],
                &bytes[8..12],
                &bytes[12..16],
                &bytes[16..20],
                &bytes[20..]
            );
            let key = MailKey::parse(&material[3..]).unwrap();
            let hash = ReceiptToken::generate().unwrap().storage_hash();
            let contact = Contact::new(
                Recipient::parse("owned.pass.fixture@example.org").unwrap(),
                "ABCDEFGHIJ",
                "owned_renewal",
            )
            .unwrap();
            let contact_envelope = key.seal_pass(&hash, &Payload::Contact(contact)).unwrap();
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let (account, expiry): (i64, chrono::DateTime<chrono::Utc>) = sqlx::query_as("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),6,clock_timestamp()-interval '1 year',clock_timestamp()-interval '1 second') RETURNING id,expires_at")
                .bind(hash.as_slice()).fetch_one(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO verification_private.pass_contacts(entitlement_id,recipient_hash,encrypted_contact) VALUES($1,$2,$3)")
                .bind(account).bind(hash.as_slice()).bind(contact_envelope).execute(&mut *tx).await.unwrap();
            let recovery_token =
                board_domain::challenge_secrets::PassRecoveryToken::generate().unwrap();
            let recovery_text = recovery_token.encode();
            let recovery_hash = recovery_token.storage_hash();
            let payload = if setup {
                Payload::Setup {
                    contact: Contact::new(
                        Recipient::parse("owned.pass.fixture@example.org").unwrap(),
                        "ABCDEFGHIJ",
                        "owned_renewal",
                    )
                    .unwrap(),
                    token: recovery_token,
                }
            } else if recovery {
                Payload::Recovery {
                    recipient: Recipient::parse("owned.pass.fixture@example.org").unwrap(),
                    token: recovery_token,
                }
            } else {
                let contact = Contact::new(
                    Recipient::parse("owned.pass.fixture@example.org").unwrap(),
                    "ABCDEFGHIJ",
                    "owned_renewal",
                )
                .unwrap();
                Payload::notice(contact, Notice::Expired, expiry.timestamp()).unwrap()
            };
            let encrypted = key
                .seal_pass(if recovery { &recovery_hash } else { &hash }, &payload)
                .unwrap();
            tx.commit().await.unwrap();
            let verifier =
                sqlx::PgPool::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
                    .await
                    .unwrap();
            if recovery {
                let prepared: String = sqlx::query_scalar(
                    "SELECT status FROM content.pass_begin_recovery($1,$1,$1,$1)",
                )
                .bind(hash.as_slice())
                .fetch_one(&verifier)
                .await
                .unwrap();
                assert_eq!(prepared, "prepare");
                let queued: bool = sqlx::query_scalar(
                    "SELECT content.pass_queue_recovery($1,$2,$3::text::uuid,$4)",
                )
                .bind(hash.as_slice())
                .bind(recovery_hash.as_slice())
                .bind(&id)
                .bind(encrypted)
                .fetch_one(&verifier)
                .await
                .unwrap();
                assert!(queued);
            } else {
                let queued: String = sqlx::query_scalar(
                    "SELECT content.pass_queue_notice($1,1,$2,'expired',$3::text::uuid,$4,$5)",
                )
                .bind(account)
                .bind(expiry)
                .bind(&id)
                .bind(hash.as_slice())
                .bind(encrypted)
                .fetch_one(&verifier)
                .await
                .unwrap();
                assert_eq!(queued, "queued");
            }
            verifier.close().await;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let relay = listener.local_addr().unwrap();
            let receiver = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                stream
                    .get_mut()
                    .write_all(b"220 owned fixture\r\n")
                    .await
                    .unwrap();
                for response in ["250 OK\r\n", "250 OK\r\n", "250 OK\r\n", "354 DATA\r\n"] {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    stream
                        .get_mut()
                        .write_all(response.as_bytes())
                        .await
                        .unwrap();
                }
                let mut data = String::new();
                loop {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    if line == ".\r\n" {
                        break;
                    }
                    assert!(data.len() < 8192);
                    data.push_str(&line);
                }
                stream
                    .get_mut()
                    .write_all(if accepted {
                        b"250 accepted\r\n"
                    } else {
                        b"451 rejected\r\n"
                    })
                    .await
                    .unwrap();
                data
            });

            let result = run(Settings {
                database: std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap(),
                key,
                relay,
                origin: "http://127.0.0.1:3000".into(),
            })
            .await;
            let mail = tokio::time::timeout(std::time::Duration::from_secs(8), receiver)
                .await
                .unwrap()
                .unwrap();
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let status: i16 = sqlx::query_scalar(
                "SELECT status FROM verification_private.pass_entitlements WHERE id=$1",
            )
            .bind(account)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
            let marked:bool=sqlx::query_scalar(if recovery {"SELECT delivered_at IS NOT NULL FROM verification_private.pass_recoveries WHERE entitlement_id=$1"} else {"SELECT c.expired_expiry IS NOT DISTINCT FROM e.expires_at FROM verification_private.pass_contacts c JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE e.id=$1"}).bind(account).fetch_one(&mut *tx).await.unwrap();
            let retry:bool=sqlx::query_scalar(if recovery {"SELECT EXISTS(SELECT 1 FROM verification_private.pass_recoveries WHERE entitlement_id=$1 AND delivered_at IS NULL AND lease_hash IS NULL AND retry_after>clock_timestamp())"} else {"SELECT EXISTS(SELECT 1 FROM verification_private.pass_mail_outbox WHERE entitlement_id=$1 AND lease_hash IS NULL AND retry_after>clock_timestamp())"}).bind(account).fetch_one(&mut *tx).await.unwrap();
            sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
                .bind(account)
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(status, if accepted && !recovery { 1 } else { 6 });
            assert_eq!(marked, accepted);
            assert_eq!(retry, !accepted);
            if recovery {
                assert!(mail.contains(if setup {
                    "Subject: Set up your 4chan Pass"
                } else {
                    "Subject: Reset your Pass PIN"
                }));
                if setup {
                    assert!(mail.contains("Your Pass Token is: ABCDEFGHIJ"));
                }
                assert!(mail.contains(&format!("/pass?reset={recovery_text}")));
            } else {
                assert!(mail.contains("Subject: Your 4chan Pass has expired!"));
                assert!(mail.contains("/pass?renew=owned_renewal"));
                assert!(mail.contains("Token: ABCDEFGHIJ"));
            }
        }
        owner.close().await;
    }
}
