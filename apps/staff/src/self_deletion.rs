//! Ownership changes report retirement and audit classification only. It never
//! supplies staff authority or overrides board, sticky, session or CSRF checks.
use crate::{AppError, auth::Session};
use board_domain::poster_id::PosterIdKey;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::net::IpAddr;

#[derive(Default)]
pub(crate) struct Evidence {
    actor: Option<[u8; 32]>,
    password: Option<[u8; 32]>,
    explicit_password: bool,
}

pub(crate) async fn prepare(
    pool: &PgPool,
    session: &Session,
    target: (&str, i64, &str),
    password: String,
    identity: Option<(&PosterIdKey, IpAddr)>,
) -> Result<Evidence, AppError> {
    let (board, post, action) = target;
    // Perform the ordinary staff permission checks before a hash lookup or
    // Argon2 work. The transaction repeats them and rechecks live authority.
    crate::store::validate_moderation(session, board, post, action)?;
    if !matches!(action, "remove-post" | "remove-thread" | "remove-file") {
        return if password.is_empty() {
            Ok(Evidence::default())
        } else {
            Err(AppError::Invalid)
        };
    }
    if password.len() > 128 {
        return Err(AppError::Invalid);
    }
    let explicit_password = !password.is_empty();
    let proof = if explicit_password {
        let encoded: Option<String> =
            sqlx::query_scalar("SELECT content.staff_post_deletion_hash($1,$2)")
                .bind(board)
                .bind(post)
                .fetch_one(pool)
                .await?;
        crate::posting_password::ownership_proof(password, encoded).await?
    } else {
        None
    };
    Ok(Evidence {
        actor: identity.map(|(key, peer)| *key.public_posting_rate_identity(peer).as_bytes()),
        password: proof,
        explicit_password,
    })
}

impl Evidence {
    /// Caller owns board -> thread -> post locks. Recheck both forms of
    /// evidence now, not against the pre-Argon2 snapshot.
    pub(crate) async fn matches(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        board: &str,
        post: i64,
    ) -> Result<bool, AppError> {
        let peer: bool = sqlx::query_scalar("SELECT content.staff_post_peer_matches($1,$2,$3)")
            .bind(board)
            .bind(post)
            .bind(self.actor.as_ref().map(|a| a.as_slice()))
            .fetch_one(&mut **tx)
            .await?;
        let password = if let Some(proof) = self.password {
            let current: Option<String> =
                sqlx::query_scalar("SELECT content.staff_post_deletion_hash($1,$2)")
                    .bind(board)
                    .bind(post)
                    .fetch_one(&mut **tx)
                    .await?;
            current.is_some_and(|encoded| {
                <[u8; 32]>::from(Sha256::digest(encoded.as_bytes())) == proof
            })
        } else {
            false
        };
        let own = peer || password;
        // A supplied recovery password asks for self classification. Refuse
        // stale/invalid proof rather than silently clearing the report queue.
        if self.explicit_password && !own {
            return Err(AppError::Forbidden);
        }
        Ok(own)
    }
}

/// Catalog inspection only: no hash lookup or ownership probe runs in readiness.
pub(crate) const READINESS_SQL: &str = r#"
WITH required(signature,owner_name,result_type,source_hash) AS (VALUES
('content.staff_post_deletion_hash(text,bigint)','board_staff_post_owner','text','b0f643d62144c9a8083114d0cb98c99c6d2dd846d565bbe25ef48f213cf7d9b6'),
('content.staff_post_peer_matches(text,bigint,bytea)','board_posting_cooldown_owner','boolean','330667b152d25f78889b3c9fcc0020a1ba52305f5ec8244227729068b2b47a79')
)
SELECT NOT EXISTS (
 SELECT 1 FROM required expected WHERE NOT EXISTS (
  SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_roles r ON r.oid=p.proowner
  WHERE p.oid=to_regprocedure(expected.signature) AND r.rolname=expected.owner_name
    AND p.prorettype=to_regtype(expected.result_type) AND p.prokind='f'
    AND p.provolatile='s' AND p.prosecdef AND NOT p.proleakproof AND p.pronargdefaults=0
    AND p.prolang=(SELECT oid FROM pg_catalog.pg_language WHERE lanname='plpgsql')
    AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
    AND encode(sha256(convert_to(p.prosrc,'UTF8')),'hex')=expected.source_hash
    AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
    AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_auth_members m WHERE m.member=r.oid)
    AND NOT has_schema_privilege(r.oid,'content','CREATE')
    AND has_function_privilege('board_staff',p.oid,'EXECUTE')
    AND NOT EXISTS(
      SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
      WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner
        AND (a.is_grantable OR a.grantee<>(SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_staff'))
    )
 )
)
"#;

pub(crate) async fn ensure_ready(tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
    if !sqlx::query_scalar::<_, bool>(READINESS_SQL)
        .fetch_one(&mut **tx)
        .await?
    {
        return Err(AppError::Internal);
    }
    Ok(())
}
