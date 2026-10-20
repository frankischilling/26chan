//! Only the private issuer can turn a credential option into a posting receipt.
use crate::{AppState, anonymous_session::Session, handlers::AppError};
use axum::http::StatusCode;
use board_verification_client::protocol;
use std::net::IpAddr;

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "VIP verification is unavailable. Please try again.",
    )
}

pub(crate) async fn proof(
    state: &AppState,
    session: &Session,
    peer: Option<IpAddr>,
    board: &str,
    target: i64,
    options: &str,
    challenge: Option<&board_store::WriteChallengeProof>,
) -> Result<Option<board_store::WriteVipProof>, AppError> {
    let prepared = board_domain::posting_options::without_sage(options);
    if board_domain::vip_capcode::credentials(&prepared, true).is_none() {
        return Ok(None);
    }
    let pass_proof = challenge.map(|proof| {
        let (receipt_hash, context_hash) = proof.vip_candidate();
        protocol::VipPassProof {
            receipt_hash,
            context_hash,
        }
    });
    let result = state
        .verification
        .as_ref()
        .ok_or_else(unavailable)?
        .vip_write_receipt(&protocol::VipWriteRequest {
            session: crate::signin::trusted(session, state, peer)?,
            board: board.into(),
            target,
            options: options.into(),
            pass_proof,
        })
        .await
        .map_err(|_| unavailable())?;
    match result {
        protocol::VipWriteResult::None {} => Ok(None),
        protocol::VipWriteResult::Capacity {} => Err(unavailable()),
        protocol::VipWriteResult::Ready {
            receipt,
            peer_hash,
            context_hash,
        } => board_store::WriteVipProof::from_issuer(&receipt, peer_hash, context_hash)
            .map(Some)
            .ok_or_else(unavailable),
    }
}

/// Credential-bearing options never enter SQL. Keep only the ordinary Pass-year
/// token: sage and redirect/name behavior were parsed separately from the input.
pub(crate) fn public_options(raw: &str) -> String {
    if !board_domain::vip_capcode::private_preference(raw) {
        return raw.into();
    }
    if board_domain::posting_options::pass_year_requested(
        &board_domain::posting_options::without_sage(raw),
    ) {
        "since4pass".into()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_options_are_reduced_before_database_transport() {
        for raw in [
            "capcode_!owned!secret",
            "CAPCODE_!owned!secret",
            "sagecapcode_!owned!secret",
            "capcode_!owned!",
        ] {
            assert_eq!(public_options(raw), "");
        }
        assert_eq!(
            public_options("capcode_!owned!secret since4pass"),
            "since4pass"
        );
        assert_eq!(public_options("sageNONOKO"), "sageNONOKO");
        assert_eq!(public_options("bypass_r9k"), "bypass_r9k");
    }
}
