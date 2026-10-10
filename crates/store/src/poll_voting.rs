//! Private bounded poll vote receipts. The ledger never stores a selected
//! choice, and the result projection never exposes an individual receipt.
use crate::{PgPool, StoreError};

/// Rewrite-defined outcomes. The original poll controller is not supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollVoteOutcome {
    Recorded,
    AlreadyVoted,
    Closed,
    InvalidOption,
    CapacityReached,
}

/// Atomically records one distinct opaque voter digest and updates aggregate
/// tallies; poll and option IDs are checked inside the locked function.
pub async fn cast_poll_vote(
    pool: &PgPool,
    poll_id: i64,
    option_id: i64,
    voter_hash: &[u8; 32],
) -> Result<PollVoteOutcome, StoreError> {
    if poll_id <= 0 {
        return Err(StoreError::NotFound);
    }
    let outcome: i16 = sqlx::query_scalar("SELECT content.cast_poll_vote($1,$2,$3)")
        .bind(poll_id)
        .bind(option_id)
        .bind(voter_hash.as_slice())
        .fetch_one(pool)
        .await?;
    match outcome {
        0 => Ok(PollVoteOutcome::Recorded),
        1 => Ok(PollVoteOutcome::AlreadyVoted),
        2 => Ok(PollVoteOutcome::Closed),
        3 => Ok(PollVoteOutcome::InvalidOption),
        4 => Ok(PollVoteOutcome::CapacityReached),
        5 => Err(StoreError::NotFound),
        _ => Err(StoreError::Database(sqlx::Error::Protocol(
            "Unexpected poll vote outcome.".into(),
        ))),
    }
}

/// Advisory cookie-aware read: SQL returns NULL for absent/unpublished polls
/// and FALSE only for a published poll without this receipt.
pub async fn has_poll_vote(
    pool: &PgPool,
    poll_id: i64,
    voter_hash: &[u8; 32],
) -> Result<bool, StoreError> {
    if poll_id <= 0 {
        return Err(StoreError::NotFound);
    }
    let recorded: Option<bool> = sqlx::query_scalar("SELECT content.has_poll_vote($1,$2)")
        .bind(poll_id)
        .bind(voter_hash.as_slice())
        .fetch_one(pool)
        .await?;
    recorded.ok_or(StoreError::NotFound)
}

/// Catalog-only readiness: verifies the NOLOGIN SQL owner, the caller's narrow
/// EXECUTE grants, immutable private schema and required column grants. This
/// statement never reads a vote row or invokes a function with side effects.
pub const POLL_VOTE_READINESS_SQL: &str = r#"
WITH poll_role AS (
    SELECT oid,rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,rolreplication,rolbypassrls
    FROM pg_roles WHERE rolname='board_poll_owner'
), relations AS (
    SELECT c.oid,c.relname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='poll_private' AND c.relname IN ('polls','options','votes')
), vote_table AS (
    SELECT c.oid,c.relowner,c.relkind,c.relacl
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='poll_private' AND c.relname='votes'
), functions AS (
    SELECT p.oid,p.proowner,p.prosecdef,p.provolatile,p.proconfig,p.proacl,p.prorettype,p.prosrc
    FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='content' AND p.proname IN ('cast_poll_vote','has_poll_vote')
)
SELECT (SELECT count(*) FROM poll_role)=1
AND EXISTS (
    SELECT 1 FROM poll_role r WHERE NOT (
        r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole
        OR r.rolreplication OR r.rolbypassrls)
)
AND EXISTS (
    SELECT 1 FROM pg_auth_members m JOIN poll_role r ON r.oid=m.roleid
    WHERE m.member=(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
        AND m.set_option AND NOT m.inherit_option AND NOT m.admin_option
)
AND NOT EXISTS (
    SELECT 1 FROM pg_auth_members m JOIN poll_role r ON r.oid=m.roleid
    WHERE m.member<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
)
AND NOT EXISTS (SELECT 1 FROM pg_auth_members m JOIN poll_role r ON r.oid=m.member)
AND has_schema_privilege((SELECT oid FROM poll_role),'content','USAGE')
AND has_schema_privilege((SELECT oid FROM poll_role),
    (SELECT oid FROM pg_namespace WHERE nspname='poll_private'),'USAGE')
AND NOT has_schema_privilege((SELECT oid FROM poll_role),'content','CREATE')
AND NOT has_schema_privilege((SELECT oid FROM poll_role),
    (SELECT oid FROM pg_namespace WHERE nspname='poll_private'),'CREATE')
AND NOT EXISTS (
    SELECT 1 FROM pg_namespace n CROSS JOIN LATERAL aclexplode(n.nspacl) a
    WHERE n.nspname='poll_private'
        AND (a.grantee=0
            OR a.grantee NOT IN (n.nspowner,(SELECT oid FROM poll_role))
            OR (a.grantee=(SELECT oid FROM poll_role) AND a.is_grantable))
)
AND (SELECT count(*) FROM vote_table)=1
AND EXISTS (
    SELECT 1 FROM vote_table v WHERE v.relkind='r'
        AND v.relowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','accepting_votes','boolean',true),
        ('polls','new_vote_count','integer',true),
        ('polls','vote_capacity','integer',true),
        ('votes','poll_id','bigint',true),
        ('votes','voter_hash','bytea',true),
        ('votes','voted_at','timestamp with time zone',true)
    ) e(relation,name,data_type,required)
    LEFT JOIN relations c ON c.relname=e.relation
    LEFT JOIN pg_attribute a ON a.attrelid=c.oid AND a.attname=e.name AND NOT a.attisdropped
    WHERE a.attnum IS NULL OR a.atttypid<>to_regtype(e.data_type)
        OR a.attnotnull IS DISTINCT FROM e.required
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','CHECK (vote_capacity >= 1 AND vote_capacity <= 100000)'),
        ('polls','CHECK (new_vote_count >= 0 AND new_vote_count <= vote_capacity)'),
        ('votes','CHECK (octet_length(voter_hash) = 32)'),
        ('votes','PRIMARY KEY (poll_id, voter_hash)'),
        ('votes','FOREIGN KEY (poll_id) REFERENCES poll_private.polls(id) ON DELETE CASCADE')
    ) e(relation,definition)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_constraint c
        WHERE c.conrelid=(SELECT oid FROM relations WHERE relname=e.relation)
            AND c.convalidated AND NOT c.condeferrable
            AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]()]','','g')
                =regexp_replace(e.definition,'[[:space:]()]','','g')
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','accepting_votes','false'),
        ('polls','new_vote_count','0'),
        ('polls','vote_capacity','10000'),
        ('votes','voted_at','clock_timestamp()')
    ) e(relation,name,expression)
    LEFT JOIN relations c ON c.relname=e.relation
    LEFT JOIN pg_attribute a ON a.attrelid=c.oid AND a.attname=e.name AND NOT a.attisdropped
    LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
    WHERE d.oid IS NULL
        OR regexp_replace(pg_get_expr(d.adbin,d.adrelid),'[[:space:]()]','','g')
            IS DISTINCT FROM regexp_replace(e.expression,'[[:space:]()]','','g')
)
AND NOT EXISTS (
    SELECT 1 FROM vote_table v CROSS JOIN (VALUES
        ('board_public'),('board_staff'),('board_auth')
    ) r(name)
    WHERE has_table_privilege(r.name,v.oid,
        'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        OR has_any_column_privilege(r.name,v.oid,'SELECT,INSERT,UPDATE,REFERENCES')
)
AND NOT EXISTS (
    SELECT 1 FROM vote_table v CROSS JOIN LATERAL aclexplode(v.relacl) a
    WHERE a.grantee=0
        OR a.grantee NOT IN (v.relowner,(SELECT oid FROM poll_role))
        OR (a.grantee=(SELECT oid FROM poll_role) AND a.is_grantable)
)
AND NOT EXISTS (
    SELECT 1 FROM vote_table v JOIN pg_attribute c ON c.attrelid=v.oid
    CROSS JOIN LATERAL aclexplode(c.attacl) a
    WHERE a.grantee=0
        OR a.grantee NOT IN (v.relowner,(SELECT oid FROM poll_role))
        OR (a.grantee=(SELECT oid FROM poll_role) AND a.is_grantable)
)
AND NOT EXISTS (
    SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    CROSS JOIN LATERAL aclexplode(c.relacl) a
    WHERE n.nspname='poll_private' AND c.relname IN ('polls','options')
        AND (a.grantee=0
            OR a.grantee NOT IN (c.relowner,(SELECT oid FROM poll_role))
            OR (a.grantee=(SELECT oid FROM poll_role) AND a.is_grantable))
)
AND NOT EXISTS (
    SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    JOIN pg_attribute column_def ON column_def.attrelid=c.oid
    CROSS JOIN LATERAL aclexplode(column_def.attacl) a
    WHERE n.nspname='poll_private' AND c.relname IN ('polls','options')
        AND (a.grantee=0
            OR a.grantee NOT IN (c.relowner,(SELECT oid FROM poll_role))
            OR (a.grantee=(SELECT oid FROM poll_role) AND a.is_grantable))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('polls'),('options'),('votes')) t(name)
    WHERE has_table_privilege((SELECT oid FROM poll_role),
        (SELECT oid FROM relations WHERE relname=t.name),
        'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','id','SELECT'),('polls','published','SELECT'),
        ('polls','accepting_votes','SELECT'),('polls','vote_count','SELECT'),
        ('polls','new_vote_count','SELECT'),('polls','vote_capacity','SELECT'),
        ('polls','vote_count','UPDATE'),('polls','new_vote_count','UPDATE'),
        ('options','poll_id','SELECT'),('options','id','SELECT'),
        ('options','ordinal','SELECT'),('options','score','SELECT'),
        ('options','score','UPDATE'),
        ('votes','poll_id','SELECT'),('votes','voter_hash','SELECT'),
        ('votes','poll_id','INSERT'),('votes','voter_hash','INSERT')
    ) e(relation,name,privilege)
    WHERE NOT coalesce(has_column_privilege((SELECT oid FROM poll_role),
        (SELECT oid FROM relations WHERE relname=e.relation),e.name,e.privilege),false)
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','title','SELECT'),('polls','description','SELECT'),
        ('polls','catalogue_ordinal','SELECT'),
        ('polls','published','UPDATE'),('polls','accepting_votes','UPDATE'),
        ('polls','vote_capacity','UPDATE'),('polls','title','UPDATE'),
        ('options','ordinal','UPDATE'),('options','caption','UPDATE'),
        ('votes','voted_at','SELECT'),('votes','voted_at','INSERT'),
        ('votes','poll_id','UPDATE'),('votes','voter_hash','UPDATE')
    ) e(relation,name,privilege)
    WHERE coalesce(has_column_privilege((SELECT oid FROM poll_role),
        (SELECT oid FROM relations WHERE relname=e.relation),e.name,e.privilege),false)
)
-- Also reject any new column grant absent from the exact allowed set above.
AND NOT EXISTS (
    SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    JOIN pg_attribute a ON a.attrelid=c.oid
    CROSS JOIN (VALUES ('SELECT'),('INSERT'),('UPDATE'),('REFERENCES')) action(name)
    WHERE n.nspname='poll_private' AND c.relname IN ('polls','options','votes')
        AND a.attnum>0 AND NOT a.attisdropped
        AND coalesce(has_column_privilege((SELECT oid FROM poll_role),c.oid,
            a.attname,action.name),false)
        AND NOT EXISTS (
            SELECT 1 FROM (VALUES
                ('polls','id','SELECT'),('polls','published','SELECT'),
                ('polls','accepting_votes','SELECT'),('polls','vote_count','SELECT'),
                ('polls','new_vote_count','SELECT'),('polls','vote_capacity','SELECT'),
                ('polls','vote_count','UPDATE'),('polls','new_vote_count','UPDATE'),
                ('options','poll_id','SELECT'),('options','id','SELECT'),
                ('options','ordinal','SELECT'),('options','score','SELECT'),
                ('options','score','UPDATE'),
                ('votes','poll_id','SELECT'),('votes','voter_hash','SELECT'),
                ('votes','poll_id','INSERT'),('votes','voter_hash','INSERT')
            ) allowed(relation,column_name,privilege)
            WHERE allowed.relation=c.relname AND allowed.column_name=a.attname
                AND allowed.privilege=action.name
        )
)
AND (SELECT count(*) FROM functions)=2
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        (to_regprocedure('content.cast_poll_vote(bigint,bigint,bytea)'),
            'smallint'::regtype,'v',
            '72cbded3beaadf97de6dca4a0ea5149f23984b2677c941b9b70e478c76f58170'),
        (to_regprocedure('content.has_poll_vote(bigint,bytea)'),
            'boolean'::regtype,'s',
            '50d9387559d04374dc6b98b1af5be9c3dc3fa20b5fa27b41e1e99dc17c18f787')
    ) e(oid,result_type,volatility,body_sha256)
    LEFT JOIN functions p ON p.oid=e.oid
    WHERE p.oid IS NULL OR p.proowner<>(SELECT oid FROM poll_role)
        OR NOT p.prosecdef OR p.provolatile<>e.volatility
        OR p.prorettype<>e.result_type
        OR encode(sha256(convert_to(p.prosrc,'UTF8')),'hex')
            IS DISTINCT FROM e.body_sha256
        OR NOT EXISTS (
            SELECT 1 FROM unnest(p.proconfig) conf(value)
            WHERE replace(conf.value,' ','')='search_path=pg_catalog,pg_temp'
        )
        OR NOT has_function_privilege('board_public',p.oid,'EXECUTE')
        OR has_function_privilege('board_staff',p.oid,'EXECUTE')
        OR has_function_privilege('board_auth',p.oid,'EXECUTE')
        OR has_function_privilege('board_migrator',p.oid,'EXECUTE')
        OR EXISTS (
            SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
            WHERE a.grantee=0 OR a.grantee NOT IN (p.proowner,
                (SELECT oid FROM pg_roles WHERE rolname='board_public'))
                OR (a.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public')
                    AND a.is_grantable)
        )
)
"#;
