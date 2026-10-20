//! Untrusted receipt transport into the protected write transaction. SQL checks
//! the private issuer row; constructing this value grants no authority.
use crate::{StoreError, anonymous_session::PostingSession};
use board_domain::challenge_secrets::ReceiptToken;
use sqlx::PgConnection;

pub struct WriteChallengeProof {
    receipt_hash: [u8; 32],
    peer_hash: [u8; 32],
    context_hash: [u8; 32],
    credit_request_eligible: bool,
}

impl WriteChallengeProof {
    pub fn from_issuer(receipt: &str, peer_hash: [u8; 32], context_hash: [u8; 32]) -> Option<Self> {
        Some(Self {
            receipt_hash: ReceiptToken::parse(receipt)?.storage_hash(),
            peer_hash,
            context_hash,
            credit_request_eligible: false,
        })
    }

    /// Opaque issuer references, not a claim that this is a Pass receipt.
    /// The VIP issuer independently checks receipt kind and every bound field.
    pub fn vip_candidate(&self) -> ([u8; 32], [u8; 32]) {
        (self.receipt_hash, self.context_hash)
    }

    /// Request facts only restrict courtesy-credit issuance. The private issuer
    /// receipt still decides whether the solved challenge is valid.
    pub fn with_credit_request(
        mut self,
        request: board_domain::content_admission::classifiers::CreditRequest<'_>,
    ) -> Self {
        use board_domain::content_admission::{classifiers, network::ProviderValue};
        self.credit_request_eligible = matches!(
            classifiers::withhold_captcha_credit(request),
            ProviderValue::Available(false)
        );
        self
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
    proof: Option<&WriteChallengeProof>,
    session: Option<&PostingSession>,
) -> Result<(), StoreError> {
    let carrier = match (proof, session) {
        (Some(proof), Some(session)) => proof.carrier(session),
        (Some(_), None) => return Err(StoreError::Invalid("Challenge session is unavailable.")),
        (None, _) => String::new(),
    };
    // Always override any pooled connection's prior value, including when the
    // caller has no proof. Local scope ends with the enclosing transaction.
    sqlx::query("SELECT set_config('board.challenge_context',$1,true),set_config('board.challenge_credit_request',$2,true)")
        .bind(carrier)
        .bind(if proof.is_some_and(|p| p.credit_request_eligible) { "eligible" } else { "" })
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn install_report_epoch(
    connection: &mut PgConnection,
    identity: &board_domain::poster_id::PublicReportRateIdentity,
) -> Result<(), StoreError> {
    let epoch: String = identity
        .report_history_epoch()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    sqlx::query("SELECT set_config('board.report_history_epoch',$1,true)")
        .bind(epoch)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn install_report_exemption(
    connection: &mut PgConnection,
    identity: &board_domain::poster_id::PublicReportRateIdentity,
    source_cookies_absent: bool,
) -> Result<(), StoreError> {
    let eligible = source_cookies_absent && identity.permits_source_report_exemption();
    sqlx::query("SELECT set_config('board.report_exemption',$1,true)")
        .bind(if eligible { "eligible" } else { "" })
        .execute(connection)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credit_eligibility_defaults_closed_and_never_changes_receipt_carrier() {
        use board_domain::content_admission::{
            classifiers::CreditRequest, network::ProviderValue::Available as A,
        };
        let receipt = ReceiptToken::generate().unwrap();
        let proof = WriteChallengeProof::from_issuer(&receipt.encode(), [1; 32], [2; 32]).unwrap();
        assert!(!proof.credit_request_eligible);
        let proof = proof.with_credit_request(CreditRequest {
            accept_language: A(Some(b"")),
            accept: A(Some(b"")),
            user_agent: A(None),
            referer: A(None),
        });
        assert!(proof.credit_request_eligible);
        let proof = proof.with_credit_request(CreditRequest {
            accept_language: A(None),
            accept: A(None),
            user_agent: A(None),
            referer: A(None),
        });
        assert!(!proof.credit_request_eligible);
        assert_eq!(proof.receipt_hash, receipt.storage_hash());
        assert_eq!(proof.peer_hash, [1; 32]);
        assert_eq!(proof.context_hash, [2; 32]);
    }

    #[test]
    fn proof_transport_accepts_only_receipt_tokens() {
        for prefix in ["", "t1_", "e1_", "c1_"] {
            assert!(
                WriteChallengeProof::from_issuer(
                    &format!("{prefix}{}", "ab".repeat(32)),
                    [1; 32],
                    [2; 32]
                )
                .is_none()
            );
        }
        assert!(
            WriteChallengeProof::from_issuer(&format!("r1_{}", "ab".repeat(32)), [1; 32], [2; 32])
                .is_some()
        );
    }

    #[test]
    fn carrier_is_bounded_and_uses_the_expected_session() {
        let receipt = ReceiptToken::generate().unwrap();
        let proof = WriteChallengeProof::from_issuer(&receipt.encode(), [1; 32], [2; 32]).unwrap();
        let mut session = PostingSession {
            fingerprints: board_domain::anonymous_session::Fingerprints {
                token: [3; 32],
                network: [4; 32],
                address: [5; 32],
                environment: [6; 32],
            },
            minted: false,
            now: chrono::Utc::now(),
        };
        let carrier = proof.carrier(&session);
        assert!(carrier.len() < 512);
        assert!(!carrier.contains(&receipt.encode()));
        let fields: serde_json::Value = serde_json::from_str(&carrier).unwrap();
        assert_eq!(fields.as_object().unwrap().len(), 4);
        assert_eq!(fields["session"], "03".repeat(32));
        assert_eq!(fields["peer"], "01".repeat(32));
        assert_eq!(fields["context"], "02".repeat(32));
        session.fingerprints.token = [7; 32];
        assert_ne!(proof.carrier(&session), carrier);
    }
}
