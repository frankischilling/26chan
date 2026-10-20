//! Bounded private notice producer. No recipient, token or ciphertext is logged.
use board_domain::{
    challenge_secrets::{ReceiptToken, VerificationKey},
    pass_mail::{Notice, Payload},
    verification_mail::MailKey,
};
use board_store::verification::VerificationStore;

fn request_id(context: &[u8; 32]) -> String {
    let mut bytes: [u8; 16] = context[..16].try_into().expect("fixed prefix");
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
async fn run() -> Result<usize, ()> {
    if std::env::args_os().len() != 1 {
        return Err(());
    }
    let raw_key = std::env::var("VERIFICATION_KEY").map_err(|_| ())?;
    let raw_mail = std::env::var("VERIFICATION_MAIL_KEY").map_err(|_| ())?;
    if raw_key.eq_ignore_ascii_case(&raw_mail) {
        return Err(());
    }
    let key = VerificationKey::parse(&raw_key).ok_or(())?;
    let mail = MailKey::parse(&raw_mail).ok_or(())?;
    let store =
        VerificationStore::connect(&std::env::var("VERIFICATION_DATABASE_URL").map_err(|_| ())?)
            .await
            .map_err(|_| ())?;
    let result = async {
        let candidates = store.pass_due_notices(100).await.map_err(|_| ())?;
        let mut queued = 0;
        let mut incomplete = false;
        for candidate in candidates {
            let attempt = async {
                let identity: [u8; 32] = candidate
                    .identifier_hash
                    .as_slice()
                    .try_into()
                    .map_err(|_| ())?;
                let Some(Payload::Contact(contact)) =
                    mail.open_pass(&identity, &candidate.encrypted_contact)
                else {
                    return Err(());
                };
                if key.pass_identifier(contact.identifier()) != Some(identity)
                    || key
                        .pass_recipient(contact.recipient())
                        .as_ref()
                        .map(|hash| hash.as_slice())
                        != Some(candidate.recipient_hash.as_slice())
                {
                    return Err(());
                }
                let kind = match candidate.notice_kind.as_str() {
                    "reminder" => Notice::Reminder,
                    "expired" => Notice::Expired,
                    _ => return Err(()),
                };
                let payload =
                    Payload::notice(contact, kind, candidate.expiry.timestamp()).ok_or(())?;
                let context = ReceiptToken::generate().map_err(|_| ())?.storage_hash();
                let envelope = mail.seal_pass(&context, &payload).ok_or(())?;
                match store
                    .queue_pass_notice(&candidate, &request_id(&context), &context, &envelope)
                    .await
                    .map_err(|_| ())?
                    .as_str()
                {
                    "queued" => Ok(true),
                    "stale" => Ok(false),
                    _ => Err(()),
                }
            }
            .await;
            match attempt {
                Ok(true) => queued += 1,
                Ok(false) => {}
                Err(()) => incomplete = true,
            }
        }
        if incomplete { Err(()) } else { Ok(queued) }
    }
    .await;
    store.close().await;
    result
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(count) => println!("Pass notices queued: {count}"),
        Err(()) => {
            eprintln!("Pass notice production incomplete");
            std::process::exit(1);
        }
    }
}
