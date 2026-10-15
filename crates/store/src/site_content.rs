//! Bounded, escaped local content with operator-only maintenance.
use crate::StoreError;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};
use std::io::Read;

const PAGE_KEYS: &[&str] = &[
    "about",
    "faq",
    "rules",
    "contact",
    "feedback",
    "legal",
    "press",
    "advertise",
    "support",
    "japanese",
    "blog",
];
const INPUT_LIMIT: u64 = 32768;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SitePage {
    pub key: String,
    pub board: Option<String>,
    pub title: String,
    pub body: String,
    pub revision: i64,
    pub updated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NewsItem {
    pub author: String,
    pub id: i64,
    pub published_at: DateTime<Utc>,
    pub title: String,
    pub body: String,
}
impl NewsItem {
    pub fn date(&self) -> String {
        self.published_at.format("%m/%d/%y @ %H:%M UTC").to_string()
    }
}

pub fn valid_page_key(key: &str) -> bool {
    PAGE_KEYS.contains(&key)
        || key
            .strip_prefix("rules/")
            .is_some_and(|board| board_domain::BoardSlug::parse(board).is_ok())
}
fn plain(text: &str, maximum: usize, multiline: bool) -> bool {
    !text.trim().is_empty()
        && text.len() <= maximum
        && !text
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
}

pub async fn site_page(pool: &PgPool, key: &str) -> Result<Option<SitePage>, StoreError> {
    if !valid_page_key(key) {
        return Err(StoreError::NotFound);
    }
    let row: Option<SitePage> = sqlx::query_as("SELECT key,board,title,body,revision,updated_at FROM content.published_site_pages WHERE key=$1")
        .bind(key).fetch_optional(pool).await?;
    if row
        .as_ref()
        .is_some_and(|p| !plain(&p.title, 256, false) || !plain(&p.body, 8192, true))
    {
        return Err(StoreError::ReadLimit);
    }
    Ok(row)
}

pub async fn news_page(
    pool: &PgPool,
    before: Option<i64>,
    limit: i64,
) -> Result<Vec<NewsItem>, StoreError> {
    if !(1..=26).contains(&limit) || before.is_some_and(|id| !(1..=10000).contains(&id)) {
        return Err(StoreError::Invalid("Invalid news page."));
    }
    let rows: Vec<NewsItem> = sqlx::query_as("SELECT id,published_at,title,body,author FROM content.published_news WHERE ($1::bigint IS NULL OR id<$1) ORDER BY id DESC LIMIT $2")
        .bind(before).bind(limit).fetch_all(pool).await?;
    if rows.iter().any(|p| {
        !(1..=10000).contains(&p.id)
            || !plain(&p.title, 256, false)
            || !plain(&p.body, 16384, true)
            || (!p.author.is_empty() && !plain(&p.author, 128, false))
    }) {
        return Err(StoreError::ReadLimit);
    }
    Ok(rows)
}

pub async fn news_item(pool: &PgPool, id: i64) -> Result<Option<NewsItem>, StoreError> {
    if !(1..=10000).contains(&id) {
        return Err(StoreError::NotFound);
    }
    let row: Option<NewsItem> = sqlx::query_as(
        "SELECT id,published_at,title,body,author FROM content.published_news WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    if row.as_ref().is_some_and(|p| {
        !plain(&p.title, 256, false)
            || !plain(&p.body, 16384, true)
            || (!p.author.is_empty() && !plain(&p.author, 128, false))
    }) {
        return Err(StoreError::ReadLimit);
    }
    Ok(row)
}

pub async fn board_rules(pool: &PgPool) -> Result<Vec<SitePage>, StoreError> {
    let rows: Vec<SitePage> = sqlx::query_as("SELECT key,board,title,body,revision,updated_at FROM content.published_site_pages WHERE board IS NOT NULL ORDER BY board LIMIT 100")
        .fetch_all(pool).await?;
    if rows
        .iter()
        .any(|p| !plain(&p.title, 256, false) || !plain(&p.body, 8192, true))
    {
        return Err(StoreError::ReadLimit);
    }
    Ok(rows)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Page {
        version: u8,
        key: String,
        expected_revision: i64,
        title: String,
        body: String,
    },
    News {
        #[serde(default)]
        author: String,
        version: u8,
        published_at: i64,
        title: String,
        body: String,
    },
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Expected bounded version-1 plain-text site content with a valid key and timestamp.")]
    Input,
    #[error("Maintenance requires the actual board_migrator login.")]
    Role,
    #[error("The page revision changed; read the current revision before replacing it.")]
    Revision,
    #[error("News must have a newer timestamp than retained entries and at most 10000 entries.")]
    NewsOrder,
    #[error("Content was not found.")]
    NotFound,
    #[error("Database operation failed; connection details are not displayed.")]
    Database,
}
impl Input {
    fn validate(&self) -> Result<(), Error> {
        let valid = match self {
            Self::Page {
                version,
                key,
                expected_revision,
                title,
                body,
            } => {
                *version == 1
                    && valid_page_key(key)
                    && (0..2147483647).contains(expected_revision)
                    && plain(title, 256, false)
                    && plain(body, 8192, true)
            }
            Self::News {
                author,
                version,
                published_at,
                title,
                body,
            } => {
                *version == 1
                    && (1..=253402300799).contains(published_at)
                    && plain(title, 256, false)
                    && plain(body, 16384, true)
                    && (author.is_empty() || plain(author, 128, false))
            }
        };
        if valid { Ok(()) } else { Err(Error::Input) }
    }
}
pub fn read_input(reader: impl Read) -> Result<Input, Error> {
    let mut bytes = Vec::new();
    reader
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    if bytes.len() > INPUT_LIMIT as usize {
        return Err(Error::Input);
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|_| Error::Input)?;
    input.validate()?;
    Ok(input)
}
async fn transaction(connection: &mut PgConnection) -> Result<Transaction<'_, Postgres>, Error> {
    let mut tx = connection.begin().await.map_err(|_| Error::Database)?;
    let result = async {
        let allowed: bool = sqlx::query_scalar(
            "SELECT session_user='board_migrator' AND current_user='board_migrator'",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| Error::Database)?;
        if !allowed {
            return Err(Error::Role);
        }
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
        sqlx::query("SET LOCAL statement_timeout='30s'")
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
        sqlx::query("LOCK TABLE site_private.pages,site_private.news IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        tx.rollback().await.map_err(|_| Error::Database)?;
        return Err(error);
    }
    Ok(tx)
}

async fn finish<T>(tx: Transaction<'_, Postgres>, result: Result<T, Error>) -> Result<T, Error> {
    match result {
        Ok(value) => {
            tx.commit().await.map_err(|_| Error::Database)?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await.map_err(|_| Error::Database)?;
            Err(error)
        }
    }
}

/// Returns the new page revision or news ID. All writes remain in one transaction.
pub async fn publish(connection: &mut PgConnection, input: &Input) -> Result<i64, Error> {
    input.validate()?;
    let mut tx = transaction(connection).await?;
    let result = async {
    let number = match input {
        Input::Page {
            key,
            expected_revision,
            title,
            body,
            ..
        } => {
            let revision: Option<i64> =
                sqlx::query_scalar("SELECT revision FROM site_private.pages WHERE key=$1")
                    .bind(key)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|_| Error::Database)?;
            if revision.unwrap_or(0) != *expected_revision {
                return Err(Error::Revision);
            }
            let revision = expected_revision + 1;
            sqlx::query("INSERT INTO site_private.pages(key,board,title,body,revision) VALUES($1,$2,$3,$4,$5) ON CONFLICT(key) DO UPDATE SET title=excluded.title,body=excluded.body,revision=excluded.revision,updated_at=clock_timestamp(),published=true")
                .bind(key).bind(key.strip_prefix("rules/")).bind(title).bind(body).bind(revision)
                .execute(&mut *tx).await.map_err(|_| Error::Database)?;
            revision
        }
        Input::News {
            author,
            published_at,
            title,
            body,
            ..
        } => {
            let (last, latest): (i64, Option<DateTime<Utc>>) = sqlx::query_as(
                "SELECT coalesce(max(id),0),max(published_at) FROM site_private.news",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
            let date = DateTime::from_timestamp(*published_at, 0).ok_or(Error::Input)?;
            if last >= 10000 || latest.is_some_and(|latest| date <= latest) {
                return Err(Error::NewsOrder);
            }
            sqlx::query(
                "INSERT INTO site_private.news(id,published_at,title,body,author) VALUES($1,$2,$3,$4,$5)",
            )
            .bind(last + 1)
            .bind(date)
            .bind(title)
            .bind(body)
            .bind(author)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
            last + 1
        }
    };
    Ok(number)
    }.await;
    finish(tx, result).await
}

pub async fn retract_news(connection: &mut PgConnection, id: i64) -> Result<(), Error> {
    if !(1..=10000).contains(&id) {
        return Err(Error::NotFound);
    }
    let mut tx = transaction(connection).await?;
    let result = async {
        let result = sqlx::query("UPDATE site_private.news SET published=false WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| Error::Database)?;
        if result.rows_affected() != 1 {
            return Err(Error::NotFound);
        }
        Ok(())
    }
    .await;
    finish(tx, result).await
}

pub async fn page_revision(connection: &mut PgConnection, key: &str) -> Result<i64, Error> {
    if !valid_page_key(key) {
        return Err(Error::Input);
    }
    let mut tx = transaction(connection).await?;
    let result = async {
        let revision: Option<i64> =
            sqlx::query_scalar("SELECT revision FROM site_private.pages WHERE key=$1")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|_| Error::Database)?;
        Ok(revision.unwrap_or(0))
    }
    .await;
    finish(tx, result).await
}

pub async fn retract_page(
    connection: &mut PgConnection,
    key: &str,
    expected: i64,
) -> Result<(), Error> {
    if !valid_page_key(key) || !(1..2147483647).contains(&expected) {
        return Err(Error::Input);
    }
    let mut tx = transaction(connection).await?;
    let result = async {
    let changed = sqlx::query("UPDATE site_private.pages SET published=false,revision=revision+1,updated_at=clock_timestamp() WHERE key=$1 AND revision=$2")
        .bind(key).bind(expected).execute(&mut *tx).await.map_err(|_| Error::Database)?;
    if changed.rows_affected() != 1 {
        return Err(Error::Revision);
    }
    Ok(())
    }.await;
    finish(tx, result).await
}

/// Exact publication filters and read-only runtime grants are part of readiness.
pub const READINESS_SQL: &str = r#"
WITH relations AS (
 SELECT c.*,n.nspname FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE (n.nspname='site_private' AND c.relname IN ('pages','news'))
 OR (n.nspname='content' AND c.relname IN ('published_site_pages','published_news'))
)
SELECT (SELECT count(*) FROM relations)=4
AND EXISTS(SELECT 1 FROM pg_namespace WHERE nspname='site_private'
 AND nspowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator'))
AND NOT EXISTS(SELECT 1 FROM pg_roles runtime WHERE runtime.rolname LIKE 'board\_%' ESCAPE '\'
 AND runtime.rolname<>'board_migrator' AND has_schema_privilege(runtime.oid,'site_private','USAGE,CREATE'))
AND NOT EXISTS(SELECT 1 FROM relations r CROSS JOIN LATERAL aclexplode(r.relacl) acl
 WHERE acl.grantee NOT IN ((SELECT oid FROM pg_roles WHERE rolname='board_migrator'),
 CASE WHEN r.nspname='content' THEN (SELECT oid FROM pg_roles WHERE rolname='board_public') ELSE 0::oid END)
 OR acl.grantee=0 OR (acl.grantee=(SELECT oid FROM pg_roles WHERE rolname='board_public')
 AND (acl.privilege_type<>'SELECT' OR acl.is_grantable)))
AND NOT EXISTS(SELECT 1 FROM relations r JOIN pg_attribute a ON a.attrelid=r.oid
 CROSS JOIN LATERAL aclexplode(a.attacl) acl
 WHERE acl.grantee<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator'))
AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.relowner<>(SELECT oid FROM pg_roles WHERE rolname='board_migrator')
 OR (r.nspname='site_private' AND (r.relkind<>'r'
 OR has_table_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_any_column_privilege('board_public',r.oid,'SELECT,INSERT,UPDATE,REFERENCES')))
 OR (r.nspname='content' AND (r.relkind<>'v'
 OR NOT COALESCE('security_barrier=true'=ANY(r.reloptions),false)
 OR COALESCE('security_invoker=true'=ANY(r.reloptions),false)
 OR NOT has_table_privilege('board_public',r.oid,'SELECT')
 OR has_table_privilege('board_public',r.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))))
AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('published_site_pages',$definition$ SELECT key,
    board,
    title,
    body,
    revision,
    updated_at
   FROM site_private.pages p
  WHERE published AND (board IS NULL OR (EXISTS ( SELECT 1
           FROM content.boards b
          WHERE b.slug = p.board AND NOT b.staff_only)));$definition$),
 ('published_news',$definition$ SELECT id,
    published_at,
    title,
    body,
    author
   FROM site_private.news
  WHERE published AND published_at <= CURRENT_TIMESTAMP;$definition$)
 ) expected(name,definition)
 WHERE NOT EXISTS(SELECT 1 FROM relations r WHERE r.nspname='content' AND r.relname=expected.name
 AND pg_get_viewdef(r.oid,true)=expected.definition))
AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('pages','PRIMARY KEY (key)'),('news','PRIMARY KEY (id)'),
 ('pages','FOREIGN KEY (board) REFERENCES content.boards(slug) ON DELETE CASCADE'),
 ('pages','CHECK (octet_length(title) >= 1 AND octet_length(title) <= 256)'),
 ('pages','CHECK (octet_length(body) >= 1 AND octet_length(body) <= 8192)'),
 ('pages','CHECK (revision >= 1 AND revision <= 2147483647)'),
 ('news','CHECK (octet_length(title) >= 1 AND octet_length(title) <= 256)'),
 ('news','CHECK (octet_length(body) >= 1 AND octet_length(body) <= 16384)'),
 ('news','CHECK (id >= 1 AND id <= 10000)')
 ) expected(name,definition)
 WHERE NOT EXISTS(SELECT 1 FROM pg_constraint c JOIN relations r ON r.oid=c.conrelid
 WHERE r.nspname='site_private' AND r.relname=expected.name AND c.convalidated AND NOT c.condeferrable
 AND pg_get_constraintdef(c.oid,true)=expected.definition))
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_boundaries_and_plain_text_are_explicit() {
        for key in ["faq", "rules/g", "contact"] {
            assert!(valid_page_key(key));
        }
        for key in ["", "news", "rules/../g", "rules/g?x", "FAQ"] {
            assert!(!valid_page_key(key));
        }
        let valid = r#"{"kind":"page","version":1,"key":"faq","expected_revision":0,"title":"Local FAQ","body":"<script>text stays escaped</script>"}"#;
        assert!(read_input(valid.as_bytes()).is_ok());
        for invalid in [
            valid.replace("\"version\":1", "\"version\":2"),
            valid.replace("\"expected_revision\":0", "\"expected_revision\":-1"),
            valid.replace("Local FAQ", " "),
            valid.replace("Local FAQ", "Line\\nBreak"),
            valid.replace("\"kind\":\"page\"", "\"unknown\":true,\"kind\":\"page\""),
        ] {
            assert!(read_input(invalid.as_bytes()).is_err());
        }
        assert!(read_input(vec![b' '; INPUT_LIMIT as usize + 1].as_slice()).is_err());
        assert!(plain("owned\ntext\tline", 32, true));
        assert!(!plain("owned\rtext", 32, true));
        assert!(!plain("owned\0text", 32, true));
    }
}
