//! Bounded operator-published projections, not a voting or publication API.
use crate::{PgPool, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PollSummary {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PollOption {
    pub id: i64,
    pub caption: String,
    pub score: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollSnapshot {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub vote_count: i64,
    pub options: Vec<PollOption>,
}

/// Preserve the operator's explicit order; no inferred recency or active filter.
pub async fn poll_catalogue(pool: &PgPool) -> Result<Vec<PollSummary>, StoreError> {
    let rows: Vec<PollSummary> = sqlx::query_as(
        "SELECT id,title FROM content.published_polls WHERE catalogue_ordinal IS NOT NULL \
         ORDER BY catalogue_ordinal LIMIT 201",
    )
    .fetch_all(pool)
    .await?;
    if rows.len() > 200 || rows.iter().any(|row| row.id <= 0 || row.title.len() > 512) {
        return Err(StoreError::ReadLimit);
    }
    Ok(rows)
}

/// Metadata, total and nullable option scores share one immutable read snapshot.
/// Integer score bounds do not imply that scores sum to, or cannot exceed, votes.
pub async fn poll_snapshot(pool: &PgPool, id: i64) -> Result<PollSnapshot, StoreError> {
    if id <= 0 {
        return Err(StoreError::NotFound);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let (id, title, description, vote_count): (i64, String, String, i64) = sqlx::query_as(
        "SELECT id,title,description,vote_count FROM content.published_polls WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    if title.len() > 512 || description.len() > 16384 || !(0..=1_000_000_000).contains(&vote_count)
    {
        return Err(StoreError::ReadLimit);
    }
    let options: Vec<PollOption> = sqlx::query_as(
        "SELECT id,caption,score FROM content.published_poll_options WHERE poll_id=$1 \
         ORDER BY ordinal LIMIT 129",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    if options.len() > 128
        || options.iter().any(|option| {
            option.id <= 0
                || option.caption.len() > 1024
                || option
                    .score
                    .is_some_and(|score| !(0..=1_000_000_000).contains(&score))
        })
    {
        return Err(StoreError::ReadLimit);
    }
    tx.commit().await?;
    Ok(PollSnapshot {
        id,
        title,
        description,
        vote_count,
        options,
    })
}

/// Catalog-only readiness: never probes private rows or writes configuration.
pub const POLL_READINESS_SQL: &str = r#"
WITH relations AS (
    SELECT c.*,n.nspname FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname='poll_private' AND c.relname IN ('polls','options'))
       OR (n.nspname='content' AND c.relname IN ('published_polls','published_poll_options'))
)
SELECT (SELECT count(*) FROM relations)=4
AND EXISTS (
    SELECT 1 FROM pg_constraint c
    WHERE c.conrelid=to_regclass('content.boards')
      AND c.conname='boards_reserved_polls_route' AND c.contype='c'
      AND c.convalidated AND NOT c.condeferrable
      AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]()]','','g')
          =regexp_replace('CHECK (slug <> ''polls''::text)','[[:space:]()]','','g')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('board_public'),('board_staff'),('board_auth')) runtime(name)
    WHERE has_schema_privilege(runtime.name,'poll_private','USAGE,CREATE')
)
-- Staff/auth have no poll read or write API, even if private schema USAGE is
-- absent. Check effective relation and column grants, including memberships.
AND NOT EXISTS (
    SELECT 1 FROM relations r CROSS JOIN (VALUES ('board_staff'),('board_auth')) runtime(name)
    WHERE has_table_privilege(runtime.name,r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
       OR has_any_column_privilege(runtime.name,r.oid,'SELECT,INSERT,UPDATE,REFERENCES')
)
-- PUBLIC must have no explicit grant, and the public reader must not be able
-- to delegate its SELECT access. Preserve the operator owner's normal ACL.
AND NOT EXISTS (
    SELECT 1 FROM relations r CROSS JOIN LATERAL aclexplode(r.relacl) acl
    WHERE acl.grantee=0
       OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public') AND acl.is_grantable)
)
AND NOT EXISTS (
    SELECT 1 FROM relations r JOIN pg_attribute a ON a.attrelid=r.oid
    CROSS JOIN LATERAL aclexplode(a.attacl) acl
    WHERE acl.grantee=0
       OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public') AND acl.is_grantable)
)
AND NOT EXISTS (
    SELECT 1 FROM pg_namespace n CROSS JOIN LATERAL aclexplode(n.nspacl) acl
    WHERE n.nspname='poll_private' AND acl.grantee=0
)
AND NOT EXISTS (
    SELECT 1 FROM relations r
    WHERE r.relowner<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
       OR (r.nspname='poll_private' AND (
            r.relkind<>'r'
            OR has_table_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
            OR has_any_column_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
       OR (r.nspname='content' AND (
            r.relkind<>'v'
            OR NOT COALESCE('security_barrier=true'=ANY(r.reloptions),false)
            OR COALESCE('security_invoker=true'=ANY(r.reloptions),false)
            OR NOT has_table_privilege('board_public',r.oid,'SELECT')
            OR has_table_privilege('board_public',r.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
            OR has_any_column_privilege('board_public',r.oid,'INSERT,UPDATE,REFERENCES')))
 )
AND EXISTS (
    SELECT 1 FROM pg_namespace n WHERE n.nspname='poll_private'
    AND n.nspowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
)
-- Compare the entire projection, join and predicate, not merely the view name.
-- pg_get_viewdef adds whitespace/parentheses; ignore only those decorations.
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('published_polls', 'SELECT id,title,description,vote_count,catalogue_ordinal FROM poll_private.polls WHERE published;'),
        ('published_poll_options', 'SELECT o.poll_id,o.id,o.ordinal,o.caption,o.score FROM poll_private.options o JOIN poll_private.polls p ON p.id=o.poll_id WHERE p.published;')
    ) expected(name,definition)
    LEFT JOIN relations r ON r.nspname='content' AND r.relname=expected.name
    WHERE r.oid IS NULL OR regexp_replace(pg_get_viewdef(r.oid,true),'[[:space:]()]','','g')
        IS DISTINCT FROM regexp_replace(expected.definition,'[[:space:]()]','','g')
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','id','bigint',true),('polls','title','text',true),
        ('polls','description','text',true),('polls','vote_count','bigint',true),
        ('polls','published','boolean',true),('polls','catalogue_ordinal','integer',false),
        ('options','poll_id','bigint',true),('options','id','bigint',true),
        ('options','ordinal','integer',true),('options','caption','text',true),
        ('options','score','bigint',false)
    ) expected(relation,name,datatype,required)
    LEFT JOIN relations r ON r.nspname='poll_private' AND r.relname=expected.relation
    LEFT JOIN pg_attribute a ON a.attrelid=r.oid AND a.attname=expected.name AND NOT a.attisdropped
    WHERE a.attnum IS NULL OR a.atttypid<>to_regtype(expected.datatype)
        OR a.attnotnull IS DISTINCT FROM expected.required
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('polls','CHECK (id > 0)'),
        ('polls','CHECK (octet_length(title) <= 512)'),
        ('polls','CHECK (octet_length(description) <= 16384)'),
        ('polls','CHECK (vote_count >= 0 AND vote_count <= 1000000000)'),
        ('polls','CHECK (catalogue_ordinal >= 1 AND catalogue_ordinal <= 200)'),
        ('polls','PRIMARY KEY (id)'),('polls','UNIQUE (catalogue_ordinal)'),
        ('options','CHECK (id > 0)'),
        ('options','CHECK (ordinal >= 1 AND ordinal <= 128)'),
        ('options','CHECK (octet_length(caption) <= 1024)'),
        ('options','CHECK (score >= 0 AND score <= 1000000000)'),
        ('options','PRIMARY KEY (poll_id,id)'),('options','UNIQUE (poll_id,ordinal)'),
        ('options','FOREIGN KEY (poll_id) REFERENCES poll_private.polls(id) ON DELETE CASCADE')
    ) expected(relation,definition)
    LEFT JOIN relations r ON r.nspname='poll_private' AND r.relname=expected.relation
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_constraint c WHERE c.conrelid=r.oid AND c.convalidated
        AND NOT c.condeferrable
        AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]()]','','g')
            =regexp_replace(expected.definition,'[[:space:]()]','','g')
    )
)
"#;
