//! Operator-only bounded upgrade of encrypted contacts created before renewal lookup.
use board_domain::{
    challenge_secrets::VerificationKey, pass_mail::Payload, verification_mail::MailKey,
};
use sqlx::postgres::PgPoolOptions;
#[derive(sqlx::FromRow)]
struct LegacyContact {
    id: i64,
    token_hash: Vec<u8>,
    recipient_hash: Vec<u8>,
    encrypted_contact: Vec<u8>,
}
async fn run() -> Result<usize, ()> {
    if std::env::args_os().len() != 1 {
        return Err(());
    }
    let raw = std::env::var("VERIFICATION_KEY").map_err(|_| ())?;
    let raw_mail = std::env::var("VERIFICATION_MAIL_KEY").map_err(|_| ())?;
    if raw.eq_ignore_ascii_case(&raw_mail) {
        return Err(());
    }
    let key = VerificationKey::parse(&raw).ok_or(())?;
    let mail = MailKey::parse(&raw_mail).ok_or(())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(&std::env::var("MIGRATION_DATABASE_URL").map_err(|_| ())?)
        .await
        .map_err(|_| ())?;
    let role: bool = sqlx::query_scalar(
        "SELECT current_user='board_migrator' AND session_user='board_migrator'",
    )
    .fetch_one(&pool)
    .await
    .map_err(|_| ())?;
    if !role {
        return Err(());
    }
    let result=async{
  let mut tx=pool.begin().await.map_err(|_|())?;
  sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *tx).await.map_err(|_|())?;
  sqlx::query("SELECT set_config('lock_timeout','2s',true),set_config('statement_timeout','5s',true)").execute(&mut *tx).await.map_err(|_|())?;
  let ready:bool=sqlx::query_scalar("SELECT content.public_pass_ready()").fetch_one(&mut *tx).await.map_err(|_|())?;if !ready{return Err(())}
  sqlx::query("SELECT singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE").fetch_one(&mut *tx).await.map_err(|_|())?;
  let rows:Vec<LegacyContact>=sqlx::query_as("SELECT e.id,e.token_hash,c.recipient_hash,c.encrypted_contact FROM verification_private.pass_contacts c JOIN verification_private.pass_entitlements e ON e.id=c.entitlement_id WHERE c.renewal_hash IS NULL ORDER BY e.id LIMIT 100 FOR UPDATE OF e,c").fetch_all(&mut *tx).await.map_err(|_|())?;
  let count=rows.len();
  for LegacyContact{id,token_hash:identity,recipient_hash:recipient,encrypted_contact:encrypted} in rows {
   let identity:[u8;32]=identity.try_into().map_err(|_|())?;
   let Some(Payload::Contact(contact))=mail.open_pass(&identity,&encrypted)else{return Err(())};
   if key.pass_identifier(contact.identifier())!=Some(identity)||key.pass_recipient(contact.recipient()).as_ref().map(|hash|hash.as_slice())!=Some(recipient.as_slice()){return Err(())}
   let reference=key.pass_renewal(contact.renewal()).ok_or(())?;
   let result=sqlx::query("UPDATE verification_private.pass_contacts SET renewal_hash=$2 WHERE entitlement_id=$1 AND renewal_hash IS NULL").bind(id).bind(reference.as_slice()).execute(&mut *tx).await.map_err(|_|())?;
   if result.rows_affected()!=1{return Err(())}
  }
  tx.commit().await.map_err(|_|())?;Ok(count)
 }.await;
    pool.close().await;
    result
}
#[tokio::main]
async fn main() {
    match run().await {
        Ok(count) => println!("Renewal references bound: {count}"),
        Err(()) => {
            eprintln!("Renewal-reference maintenance unavailable; no batch changes confirmed");
            std::process::exit(1)
        }
    }
}
