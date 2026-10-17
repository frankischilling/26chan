//! Source network bypass requires a current staff identity, not a badge.
use crate::{StaffPostAuthority, StoreError};

pub(crate) async fn network_bypass(
    authority: &StaffPostAuthority<'_>,
    board: &str,
) -> Result<bool, StoreError> {
    if authority.session_hash.len() != 32
        || authority.csrf_hash.len() != 32
        || !(60..=3600).contains(&authority.idle_seconds)
    {
        return Err(StoreError::AuthorizationChanged);
    }
    // This read grants only the range-filter exemption. The independently
    // issued, row-bound posting proof still rechecks authority after content
    // locks and before a post can commit. Holding these auth locks here would
    // conflict with that issuer's separate transaction.
    let valid: bool = sqlx::query_scalar(
        "SELECT current_user='board_auth' AND EXISTS(\
         SELECT 1 FROM staff_identity.sessions s \
         JOIN staff_identity.accounts a ON a.id=s.account_id \
         JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id \
         WHERE s.token_hash=$1 AND s.csrf_hash=$2 AND s.expires_at>clock_timestamp() \
         AND s.last_activity_at>clock_timestamp()-make_interval(secs=>$3) \
         AND s.authenticated_at>clock_timestamp()-interval '10 minutes' \
         AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin') \
         AND staff_identity.has_board_access(a.id,$4) \
         AND ($4<>'test' OR (a.role IN ('moderator','manager','admin') AND 'developer'=ANY(a.flags))))",
    )
    .bind(authority.session_hash)
    .bind(authority.csrf_hash)
    .bind(authority.idle_seconds)
    .bind(board)
    .fetch_one(authority.auth_pool)
    .await?;
    if !valid {
        return Err(StoreError::AuthorizationChanged);
    }
    Ok(true)
}
