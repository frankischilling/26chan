//! Operator-owned, bounded plain-text announcements. Runtime authority is read-only.
use crate::{PgPool, StoreError};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use std::io::Read;

pub const BLOTTER_PAGE_SIZE: usize = 25;
pub const MAX_BLOTTER_CONTENT_BYTES: usize = 8192;
pub const MAX_BLOTTER_INPUT_BYTES: u64 = 32768;
pub const MAX_BLOTTER_TIMESTAMP: i64 = 253402300799;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct BlotterMessage {
    pub id: i64,
    pub published_at: DateTime<Utc>,
    pub content: String,
}
impl BlotterMessage {
    pub fn timestamp(&self) -> i64 {
        self.published_at.timestamp()
    }
    pub fn date(&self) -> String {
        self.published_at.format("%m/%d/%y").to_string()
    }
}

pub struct BlotterPage {
    pub messages: Vec<BlotterMessage>,
    pub next_offset: Option<i64>,
}

fn validate_rows(rows: &[BlotterMessage]) -> Result<(), StoreError> {
    if rows.iter().any(|row| {
        !(1..=10000).contains(&row.id)
            || !(1..=MAX_BLOTTER_TIMESTAMP).contains(&row.timestamp())
            || row.content.is_empty()
            || row.content.len() > MAX_BLOTTER_CONTENT_BYTES
    }) {
        return Err(StoreError::ReadLimit);
    }
    Ok(())
}

/// A single bounded query is a coherent page; cursor is the last displayed ID.
pub async fn blotter_page(pool: &PgPool, offset: Option<i64>) -> Result<BlotterPage, StoreError> {
    if offset.is_some_and(|id| id <= 0) {
        return Err(StoreError::Invalid("Invalid blotter cursor."));
    }
    let mut messages: Vec<BlotterMessage> = sqlx::query_as("SELECT id,published_at,content FROM content.published_blotter WHERE ($1::bigint IS NULL OR id<$1) ORDER BY id DESC LIMIT 26")
        .bind(offset).fetch_all(pool).await?;
    validate_rows(&messages)?;
    let has_next = messages.len() > BLOTTER_PAGE_SIZE;
    messages.truncate(BLOTTER_PAGE_SIZE);
    let next_offset = has_next.then(|| messages.last().expect("full page").id);
    Ok(BlotterPage {
        messages,
        next_offset,
    })
}

/// Called inside the same repeatable-read transaction as board policy/content.
pub(crate) async fn snapshot_blotter(
    tx: &mut Transaction<'_, Postgres>,
    enabled: bool,
) -> Result<Vec<BlotterMessage>, StoreError> {
    if !enabled {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as(
        "SELECT id,published_at,content FROM content.published_blotter ORDER BY id DESC LIMIT 3",
    )
    .fetch_all(&mut **tx)
    .await?;
    validate_rows(&rows)?;
    Ok(rows)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlotterInput {
    version: u8,
    published_at: i64,
    content: String,
}
#[derive(Debug, thiserror::Error)]
pub enum BlotterError {
    #[error("Input must be a version-1 JSON announcement of at most 32768 bytes.")]
    Input,
    #[error(
        "Content must be 1..8192 UTF-8 bytes; only line feed and tab control characters are permitted."
    )]
    Content,
    #[error("Timestamp must be Unix seconds in 1..253402300799.")]
    Timestamp,
    #[error("Maintenance requires the actual board_migrator login.")]
    Role,
    #[error(
        "Timestamp must be newer than every retained announcement, including retracted entries."
    )]
    Stale,
    #[error("The 10000-announcement retention budget is full.")]
    Full,
    #[error("Announcement not found or invalid ID.")]
    NotFound,
    #[error("Database operation failed; credentials and server details are not displayed.")]
    Database,
}
impl BlotterInput {
    fn validate(&self) -> Result<(), BlotterError> {
        if self.version != 1 {
            return Err(BlotterError::Input);
        }
        if !(1..=MAX_BLOTTER_TIMESTAMP).contains(&self.published_at) {
            return Err(BlotterError::Timestamp);
        }
        if self.content.is_empty()
            || self.content.len() > MAX_BLOTTER_CONTENT_BYTES
            || self
                .content
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(BlotterError::Content);
        }
        Ok(())
    }
}

/// Limit the reader before allocation/deserialization, including unknown-size input.
pub fn read_blotter(reader: impl Read) -> Result<BlotterInput, BlotterError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_BLOTTER_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| BlotterError::Input)?;
    if bytes.len() > MAX_BLOTTER_INPUT_BYTES as usize {
        return Err(BlotterError::Input);
    }
    let input: BlotterInput = serde_json::from_slice(&bytes).map_err(|_| BlotterError::Input)?;
    input.validate()?;
    Ok(input)
}

async fn operator_transaction(
    connection: &mut PgConnection,
) -> Result<Transaction<'_, Postgres>, BlotterError> {
    let mut tx = connection
        .begin()
        .await
        .map_err(|_| BlotterError::Database)?;
    let allowed: bool = sqlx::query_scalar(
        "SELECT session_user='board_migrator' AND current_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| BlotterError::Database)?;
    if !allowed {
        tx.rollback().await.map_err(|_| BlotterError::Database)?;
        return Err(BlotterError::Role);
    }
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
    // Serialize all maintenance, including concurrent publishers and retracts.
    sqlx::query("LOCK TABLE blotter_private.messages IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
    Ok(tx)
}

pub async fn publish_blotter(
    connection: &mut PgConnection,
    input: &BlotterInput,
) -> Result<i64, BlotterError> {
    input.validate()?;
    let mut tx = operator_transaction(connection).await?;
    let result = async {
        let (last_id, latest): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
            "SELECT coalesce(max(id),0),max(published_at) FROM blotter_private.messages",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
        if last_id >= 10000 {
            return Err(BlotterError::Full);
        }
        let date =
            DateTime::from_timestamp(input.published_at, 0).ok_or(BlotterError::Timestamp)?;
        if latest.is_some_and(|latest| date <= latest) {
            return Err(BlotterError::Stale);
        }
        let id = last_id + 1;
        sqlx::query(
            "INSERT INTO blotter_private.messages(id,published_at,content) VALUES($1,$2,$3)",
        )
        .bind(id)
        .bind(date)
        .bind(&input.content)
        .execute(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
        let saved: BlotterMessage = sqlx::query_as(
            "SELECT id,published_at,content FROM content.published_blotter WHERE id=$1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)?;
        if saved.id != id || saved.published_at != date || saved.content != input.content {
            return Err(BlotterError::Database);
        }
        Ok(id)
    }
    .await;
    match result {
        Ok(id) => {
            tx.commit().await.map_err(|_| BlotterError::Database)?;
            Ok(id)
        }
        Err(error) => {
            tx.rollback().await.map_err(|_| BlotterError::Database)?;
            Err(error)
        }
    }
}

pub async fn retract_blotter(connection: &mut PgConnection, id: i64) -> Result<(), BlotterError> {
    if !(1..=10000).contains(&id) {
        return Err(BlotterError::NotFound);
    }
    let mut tx = operator_transaction(connection).await?;
    let result = sqlx::query("UPDATE blotter_private.messages SET published=false WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| BlotterError::Database)
        .and_then(|result| {
            if result.rows_affected() == 1 {
                Ok(())
            } else {
                Err(BlotterError::NotFound)
            }
        });
    match result {
        Ok(()) => tx.commit().await.map_err(|_| BlotterError::Database),
        Err(error) => {
            tx.rollback().await.map_err(|_| BlotterError::Database)?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(timestamp: i64, content: &str) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"version":1,"published_at":timestamp,"content":content}),
        )
        .unwrap()
    }
    #[test]
    fn bounds_and_plain_text_are_validated_before_database_access() {
        for date in [1, MAX_BLOTTER_TIMESTAMP] {
            assert!(
                read_blotter(input(date, "<script>x</script>\nhttps://example.org").as_slice())
                    .is_ok()
            );
        }
        for date in [-1, 0, MAX_BLOTTER_TIMESTAMP + 1] {
            assert!(matches!(
                read_blotter(input(date, "ok").as_slice()),
                Err(BlotterError::Timestamp)
            ));
        }
        for content in [
            String::new(),
            "a".repeat(8193),
            "x\0".into(),
            "x\r".into(),
            "x\u{7f}".into(),
        ] {
            assert!(matches!(
                read_blotter(input(1, &content).as_slice()),
                Err(BlotterError::Content)
            ));
        }
        assert!(read_blotter(input(1, &"a".repeat(8192)).as_slice()).is_ok());
        assert!(matches!(
            read_blotter(std::io::repeat(b' ')),
            Err(BlotterError::Input)
        ));
        for json in [
            r#"{"version":1,"version":1,"published_at":1,"content":"ok"}"#,
            r#"{"version":1,"published_at":1,"content":"ok","html":true}"#,
            r#"{"version":1,"published_at":1.5,"content":"ok"}"#,
            r#"{"version":2,"published_at":1,"content":"ok"}"#,
        ] {
            assert!(read_blotter(json.as_bytes()).is_err());
        }
    }
}

/// Verify the operator/private boundary as well as the public projection.
pub const BLOTTER_READINESS_SQL: &str = r#"
WITH relations AS (
 SELECT c.*, n.nspname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE (n.nspname='blotter_private' AND c.relname='messages')
 OR (n.nspname='content' AND c.relname='published_blotter')
)
SELECT (SELECT count(*) FROM relations)=2
AND EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('content.boards') AND attname='show_blotter' AND atttypid='boolean'::regtype AND attnotnull AND NOT attisdropped)
AND EXISTS (SELECT 1 FROM pg_namespace WHERE nspname='blotter_private' AND nspowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator'))
AND NOT EXISTS (
 SELECT 1 FROM (VALUES ('board_public'),('board_staff'),('board_auth')) runtime(name)
 WHERE has_schema_privilege(runtime.name,'blotter_private','USAGE,CREATE')
)
AND NOT EXISTS (
 SELECT 1 FROM relations r CROSS JOIN (VALUES ('board_staff'),('board_auth')) runtime(name)
 WHERE has_table_privilege(runtime.name,r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_any_column_privilege(runtime.name,r.oid,'SELECT,INSERT,UPDATE,REFERENCES')
)
AND NOT EXISTS (
 SELECT 1 FROM relations r CROSS JOIN LATERAL aclexplode(r.relacl) acl
 WHERE acl.grantee=0 OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public') AND acl.is_grantable)
)
AND NOT EXISTS (
 SELECT 1 FROM relations r JOIN pg_attribute a ON a.attrelid=r.oid CROSS JOIN LATERAL aclexplode(a.attacl) acl
 WHERE acl.grantee=0 OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public') AND acl.is_grantable)
)
AND NOT EXISTS (
 SELECT 1 FROM relations r WHERE r.relowner<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
 OR (r.nspname='blotter_private' AND (r.relkind<>'r'
   OR has_table_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
   OR has_any_column_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
 OR (r.nspname='content' AND (r.relkind<>'v'
   OR NOT COALESCE('security_barrier=true'=ANY(r.reloptions),false)
   OR COALESCE('security_invoker=true'=ANY(r.reloptions),false)
   OR NOT has_table_privilege('board_public',r.oid,'SELECT')
   OR has_table_privilege('board_public',r.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
   OR has_any_column_privilege('board_public',r.oid,'INSERT,UPDATE,REFERENCES')))
)
AND EXISTS (
 SELECT 1 FROM relations r WHERE r.nspname='content'
 AND regexp_replace(pg_get_viewdef(r.oid,true),'[[:space:]()]','','g')
 = regexp_replace('SELECT id,published_at,content FROM blotter_private.messages WHERE published;','[[:space:]()]','','g')
)
AND NOT EXISTS (
 SELECT 1 FROM (VALUES ('id','bigint'),('published_at','timestamp with time zone'),('content','text'),('published','boolean')) expected(name,datatype)
 LEFT JOIN pg_attribute a ON a.attrelid=(SELECT oid FROM relations WHERE nspname='blotter_private') AND a.attname=expected.name AND NOT a.attisdropped
 WHERE a.attnum IS NULL OR NOT a.attnotnull OR format_type(a.atttypid,a.atttypmod)<>expected.datatype
)
AND EXISTS (
 SELECT 1 FROM pg_constraint c WHERE c.conrelid=(SELECT oid FROM relations WHERE nspname='blotter_private')
 AND c.convalidated AND NOT c.condeferrable AND c.contype='c'
 AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]()]','','g')
 = regexp_replace(format('CHECK (published_at >= %L::timestamp with time zone AND published_at <= %L::timestamp with time zone)',
 TIMESTAMPTZ '1970-01-01 00:00:01+00',TIMESTAMPTZ '9999-12-31 23:59:59+00'),'[[:space:]()]','','g')
)
AND NOT EXISTS (
 SELECT 1 FROM (VALUES
 ('content.boards','CHECK (slug <> ''blotter''::text)'),
 ('blotter_private.messages','CHECK (id >= 1 AND id <= 10000)'),
 ('blotter_private.messages','CHECK (octet_length(content) >= 1 AND octet_length(content) <= 8192)'),
 ('blotter_private.messages','PRIMARY KEY (id)')
 ) expected(relation,definition)
 WHERE NOT EXISTS (SELECT 1 FROM pg_constraint c WHERE c.conrelid=(SELECT r.oid FROM pg_class r JOIN pg_namespace n ON n.oid=r.relnamespace WHERE n.nspname=split_part(expected.relation,'.',1) AND r.relname=split_part(expected.relation,'.',2))
 AND c.convalidated AND NOT c.condeferrable
 AND regexp_replace(pg_get_constraintdef(c.oid,true),'[[:space:]()]','','g')=regexp_replace(expected.definition,'[[:space:]()]','','g'))
)
"#;

#[cfg(all(test, feature = "database-tests"))]
mod database_tests {
    use super::*;
    #[tokio::test]
    async fn board_policy_and_preview_keep_one_repeatable_read_snapshot() {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let public = crate::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let mut lock = owner.acquire().await.unwrap();
        sqlx::query("SELECT pg_advisory_lock(2250118)")
            .execute(&mut *lock)
            .await
            .unwrap();
        let slug: String =
            sqlx::query_scalar("SELECT 'bs'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned snapshot','Owned blotter snapshot',1000,100,100,100,10)").bind(&slug).execute(&owner).await.unwrap();
        let mut tx = public.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .unwrap();
        let enabled: bool =
            sqlx::query_scalar("SELECT show_blotter FROM content.boards WHERE slug=$1")
                .bind(&slug)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let before = snapshot_blotter(&mut tx, enabled).await.unwrap();
        let date:i64=sqlx::query_scalar("SELECT greatest(coalesce(extract(epoch FROM max(published_at))::bigint,0)+1,extract(epoch FROM now())::bigint) FROM blotter_private.messages").fetch_one(&owner).await.unwrap();
        let input = BlotterInput {
            version: 1,
            published_at: date,
            content: format!("{slug}: owned snapshot"),
        };
        let mut writer = owner.acquire().await.unwrap();
        let id = publish_blotter(&mut writer, &input).await.unwrap();
        drop(writer);
        sqlx::query("UPDATE content.boards SET show_blotter=false WHERE slug=$1")
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
        let result = tokio::spawn({
            let slug = slug.clone();
            let public = public.clone();
            async move {
                assert!(enabled);
                assert_eq!(snapshot_blotter(&mut tx, enabled).await.unwrap(), before);
                assert!(
                    sqlx::query_scalar::<_, bool>(
                        "SELECT show_blotter FROM content.boards WHERE slug=$1"
                    )
                    .bind(&slug)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap()
                );
                tx.commit().await.unwrap();
                let after = crate::board_page_snapshot(
                    &public,
                    &slug,
                    crate::BoardSelection::Page(1),
                    Some(3),
                )
                .await
                .unwrap();
                assert!(!after.snapshot.board.show_blotter);
                assert!(after.blotter.is_empty());
                assert_eq!(
                    blotter_page(&public, None).await.unwrap().messages[0].id,
                    id
                );
            }
        })
        .await;
        let clean_message =
            sqlx::query("DELETE FROM blotter_private.messages WHERE id=$1 AND content=$2")
                .bind(id)
                .bind(&input.content)
                .execute(&owner)
                .await
                .is_ok();
        let clean_board=sqlx::query("DELETE FROM content.boards WHERE slug=$1 AND title='Owned snapshot' AND description='Owned blotter snapshot'").bind(&slug).execute(&owner).await.is_ok();
        let unlock = sqlx::query("SELECT pg_advisory_unlock(2250118)")
            .execute(&mut *lock)
            .await
            .is_ok();
        drop(lock);
        public.close().await;
        owner.close().await;
        result.unwrap();
        assert!(
            clean_message && clean_board && unlock,
            "Owned snapshot cleanup failed"
        );
    }
}
