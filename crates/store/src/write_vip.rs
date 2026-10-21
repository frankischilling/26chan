//! Untrusted receipt transport. Only the private SQL row grants a VIP identity.
use crate::{StoreError, anonymous_session::PostingSession};
use board_domain::challenge_secrets::ReceiptToken;
use sqlx::PgConnection;

pub struct WriteVipProof {
    receipt_hash: [u8; 32],
    peer_hash: [u8; 32],
    context_hash: [u8; 32],
}

impl WriteVipProof {
    pub fn from_issuer(receipt: &str, peer_hash: [u8; 32], context_hash: [u8; 32]) -> Option<Self> {
        Some(Self {
            receipt_hash: ReceiptToken::parse(receipt)?.storage_hash(),
            peer_hash,
            context_hash,
        })
    }

    fn carrier(&self, session: &PostingSession) -> String {
        let hex = |value: &[u8; 32]| {
            value
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        serde_json::json!({
            "receipt": hex(&self.receipt_hash),
            "peer": hex(&self.peer_hash),
            "session": hex(&session.fingerprints.token),
            "context": hex(&self.context_hash),
        })
        .to_string()
    }
}

pub(crate) async fn install(
    connection: &mut PgConnection,
    proof: Option<&WriteVipProof>,
    session: Option<&PostingSession>,
) -> Result<(), StoreError> {
    let carrier = match (proof, session) {
        (Some(proof), Some(session)) => proof.carrier(session),
        (Some(_), None) => return Err(StoreError::Invalid("VIP session is unavailable.")),
        (None, _) => String::new(),
    };
    sqlx::query("SELECT set_config('board.vip_context',$1,true)")
        .bind(carrier)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn posting_name(
    connection: &mut PgConnection,
    board: &str,
    target: i64,
    session: &PostingSession,
) -> Result<String, StoreError> {
    let name: Option<String> = sqlx::query_scalar("SELECT content.vip_posting_name($1,$2,$3)")
        .bind(board)
        .bind(target)
        .bind(session.fingerprints.token.as_slice())
        .fetch_one(connection)
        .await?;
    name.ok_or(StoreError::AuthorizationChanged)
}
