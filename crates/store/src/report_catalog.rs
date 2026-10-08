//! Bounded operator-only catalog import and explicit activation. These deployment
//! safety limits are rewrite choices, not claims about legacy configuration limits.
//! Importing a revision does not activate it or change report admission.

use board_domain::report_category::{Catalog, Category, CategoryId};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::{Connection, PgConnection};
use std::io::Read;

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CATEGORIES: usize = 4096;
pub const MAX_BOARD_BYTES: usize = 256;
pub const MAX_TITLE_BYTES: usize = 4096;
pub const MAX_EXCLUSIONS_BYTES: usize = 65_536;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("Catalog exceeds the 8 MiB input budget.")]
    InputLimit,
    #[error("Cannot read catalog input.")]
    Read,
    #[error(
        "Invalid catalog JSON at line {line}, column {column}; all fields must be explicit and unique."
    )]
    Json { line: usize, column: usize },
    #[error("Only catalog envelope version 1 is supported.")]
    Version,
    #[error("Catalog exceeds the 4096-category deployment limit.")]
    RowLimit,
    #[error("Category at position {position} has an invalid {field}.")]
    Field {
        position: usize,
        field: &'static str,
    },
    #[error("Catalog contains duplicate category IDs or a nonfinite weight.")]
    Catalog,
    #[error("Import requires an actual board_migrator login and role.")]
    Role,
    #[error("Catalog import was rejected by database validation.")]
    DatabaseValidation,
    #[error("Catalog revision capacity is unavailable or exhausted.")]
    Capacity,
    #[error("Catalog revision was not found during verification.")]
    Missing,
    #[error("Catalog readback did not match; the import was not committed.")]
    Verification,
    #[error("Database operation failed; no credentials or server details are displayed.")]
    Database,
}

/// An imported revision identifier. Construction excludes zero and negative IDs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogRevision(i64);

impl CatalogRevision {
    pub fn new(revision: i64) -> Option<Self> {
        (revision > 0).then_some(Self(revision))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ActivationError {
    #[error("Catalog activation changes require an actual board_migrator login and role.")]
    Role,
    #[error("Catalog activation requires an existing, nonempty imported revision.")]
    InvalidRevision,
    #[error("Database operation failed; no credentials or server details are displayed.")]
    Database,
}

/// Explicitly switch report admission to an imported catalog, or restore the
/// default free-text mode with `None`. Importing alone never invokes this helper.
/// SQL validates the revision and serializes the switch with report admission.
pub async fn set_report_catalog_active(
    connection: &mut PgConnection,
    revision: Option<CatalogRevision>,
) -> Result<(), ActivationError> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(activation_database_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *transaction)
        .await
        .map_err(activation_database_error)?;
    let role: bool = sqlx::query_scalar(
        "SELECT session_user = 'board_migrator' AND current_user = 'board_migrator'",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(activation_database_error)?;
    if !role {
        return Err(ActivationError::Role);
    }
    sqlx::query("SELECT content.set_report_catalog_active($1::bigint)")
        .bind(revision.map(CatalogRevision::get))
        .execute(&mut *transaction)
        .await
        .map_err(activation_database_error)?;
    transaction
        .commit()
        .await
        .map_err(activation_database_error)?;
    Ok(())
}

fn activation_database_error(error: sqlx::Error) -> ActivationError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("42501") => ActivationError::Role,
        Some("22023" | "P0002") => ActivationError::InvalidRevision,
        _ => ActivationError::Database,
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    categories: Vec<Row>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Row {
    id: i64,
    // A custom deserializer makes absence an error, while retaining explicit null.
    #[serde(deserialize_with = "nullable_string")]
    board: Option<String>,
    op_only: bool,
    reply_only: bool,
    image_only: bool,
    #[serde(deserialize_with = "nullable_string")]
    exclude_boards: Option<String>,
    title: String,
    weight: f64,
    filtered: i64,
}

fn nullable_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

/// Validated, ordered input; fields cannot be mutated after validation.
#[derive(Debug)]
pub struct ValidatedCatalog {
    envelope: Envelope,
}

impl ValidatedCatalog {
    pub fn len(&self) -> usize {
        self.envelope.categories.len()
    }

    pub fn is_empty(&self) -> bool {
        self.envelope.categories.is_empty()
    }
}

/// Read at most budget + one byte, including for pipes and files that grow.
/// Parsing never starts until the complete bounded input has been read.
pub fn read_catalog(reader: impl Read) -> Result<ValidatedCatalog, ImportError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ImportError::Read)?;
    parse_catalog(&bytes)
}

pub fn parse_catalog(bytes: &[u8]) -> Result<ValidatedCatalog, ImportError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ImportError::InputLimit);
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|error| ImportError::Json {
        line: error.line(),
        column: error.column(),
    })?;
    if envelope.version != 1 {
        return Err(ImportError::Version);
    }
    if envelope.categories.len() > MAX_CATEGORIES {
        return Err(ImportError::RowLimit);
    }
    let mut categories = Vec::with_capacity(envelope.categories.len());
    for (index, row) in envelope.categories.iter().enumerate() {
        let invalid = |field| ImportError::Field {
            position: index + 1,
            field,
        };
        let id = CategoryId::new(row.id).ok_or_else(|| invalid("id"))?;
        // PostgreSQL text cannot store U+0000; reject rather than transform it.
        for (field, text, limit) in [
            ("board", row.board.as_deref(), MAX_BOARD_BYTES),
            (
                "exclude_boards",
                row.exclude_boards.as_deref(),
                MAX_EXCLUSIONS_BYTES,
            ),
            ("title", Some(row.title.as_str()), MAX_TITLE_BYTES),
        ] {
            if text.is_some_and(|text| text.len() > limit || text.contains('\0')) {
                return Err(invalid(field));
            }
        }
        categories.push(Category {
            id,
            board: row.board.as_deref(),
            op_only: row.op_only,
            reply_only: row.reply_only,
            image_only: row.image_only,
            exclude_boards: row.exclude_boards.as_deref(),
            title: &row.title,
            weight: row.weight,
            filtered: row.filtered,
        });
    }
    Catalog::new(&categories).map_err(|_| ImportError::Catalog)?;
    Ok(ValidatedCatalog { envelope })
}

/// Append and verify an immutable private revision in one transaction. A failed
/// comparison rolls back. This import helper does not activate a revision, select
/// it for runtime use, or write a report. SQL separately enforces its normalized-JSONB 8 MiB budget and
/// revision capacity, so offline success does not promise import success.
pub async fn import_catalog(
    connection: &mut PgConnection,
    catalog: &ValidatedCatalog,
) -> Result<i64, ImportError> {
    let input = serde_json::to_string(&catalog.envelope).map_err(|_| ImportError::Verification)?;
    let mut transaction = connection.begin().await.map_err(database_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;
    let role: bool = sqlx::query_scalar(
        "SELECT session_user = 'board_migrator' AND current_user = 'board_migrator'",
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(database_error)?;
    if !role {
        return Err(ImportError::Role);
    }
    let revision: i64 = sqlx::query_scalar("SELECT content.import_report_catalog($1::jsonb)")
        .bind(input)
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
    let readback: String = sqlx::query_scalar("SELECT content.read_report_catalog($1)::text")
        .bind(revision)
        .fetch_one(&mut *transaction)
        .await
        .map_err(database_error)?;
    let actual = parse_catalog(readback.as_bytes()).map_err(|_| ImportError::Verification)?;
    if actual.envelope != catalog.envelope {
        return Err(ImportError::Verification);
    }
    transaction.commit().await.map_err(database_error)?;
    Ok(revision)
}

fn database_error(error: sqlx::Error) -> ImportError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("42501") => ImportError::Role,
        Some("22023") => ImportError::DatabaseValidation,
        Some("P0098") => ImportError::Capacity,
        Some("P0002") => ImportError::Missing,
        _ => ImportError::Database,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn row() -> Value {
        json!({"id": 1, "board": null, "op_only": false, "reply_only": false,
            "image_only": false, "exclude_boards": null, "title": "Synthetic fixture",
            "weight": 1.25, "filtered": -7})
    }

    fn bytes(rows: Vec<Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"version": 1, "categories": rows})).unwrap()
    }

    #[test]
    fn revision_requires_a_positive_i64() {
        for value in [i64::MIN, -1, 0] {
            assert!(CatalogRevision::new(value).is_none());
        }
        for value in [1, i64::MAX] {
            assert_eq!(CatalogRevision::new(value).unwrap().get(), value);
        }
    }

    #[test]
    fn activation_errors_never_include_database_details() {
        let error = sqlx::Error::Protocol("secret URL or catalog text".to_owned());
        let displayed = activation_database_error(error).to_string();
        assert!(!displayed.contains("secret"));
        assert!(!displayed.contains("catalog text"));
    }

    #[test]
    fn preserves_order_null_empty_and_exact_text() {
        let mut second = row();
        second["id"] = json!(i64::MAX);
        second["board"] = json!("");
        second["exclude_boards"] = json!(" a,A,,0, ");
        second["title"] = json!(" <b>unchanged</b> ");
        second["weight"] = json!(-3.5);
        second["filtered"] = json!(i64::MIN);
        let input = bytes(vec![second, row()]);
        let parsed = parse_catalog(&input).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            serde_json::to_value(&parsed.envelope).unwrap(),
            serde_json::from_slice::<Value>(&input).unwrap()
        );
        assert_eq!(parsed.envelope.categories[0].id, i64::MAX);
        assert_eq!(parsed.envelope.categories[1].board, None);
        assert!(parse_catalog(&bytes(vec![])).unwrap().is_empty());
    }

    #[test]
    fn finite_weights_roundtrip_without_inventing_magnitude_limits() {
        for weight in [
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            -0.0,
            0.0,
            0.1,
        ] {
            let mut altered = row();
            altered["weight"] = json!(weight);
            let parsed = parse_catalog(&bytes(vec![altered])).unwrap();
            assert_eq!(
                parsed.envelope.categories[0].weight.to_bits(),
                weight.to_bits()
            );
            let encoded = serde_json::to_vec(&parsed.envelope).unwrap();
            assert_eq!(parse_catalog(&encoded).unwrap().envelope, parsed.envelope);
        }
    }

    #[test]
    fn rejects_trailing_data_and_malformed_utf8() {
        let mut input = bytes(vec![]);
        input.extend_from_slice(b" {}");
        assert!(parse_catalog(&input).is_err());
        assert!(parse_catalog(&[0xff]).is_err());
    }

    #[test]
    fn requires_every_field_including_nullable_fields() {
        let fixture = row();
        for key in fixture.as_object().unwrap().keys() {
            let mut altered = fixture.clone();
            altered.as_object_mut().unwrap().remove(key);
            assert!(parse_catalog(&bytes(vec![altered])).is_err(), "{key}");
        }
        for input in [r#"{"categories":[]}"#, r#"{"version":1}"#] {
            assert!(parse_catalog(input.as_bytes()).is_err());
        }
    }

    #[test]
    fn rejects_unknown_and_duplicate_keys_in_both_objects() {
        let mut altered = row();
        altered["extra"] = json!(false);
        assert!(parse_catalog(&bytes(vec![altered])).is_err());
        for input in [
            r#"{"version":1,"categories":[],"extra":false}"#.to_owned(),
            r#"{"version":1,"version":1,"categories":[]}"#.to_owned(),
            r#"{"version":1,"categories":[],"categories":[]}"#.to_owned(),
            String::from_utf8(bytes(vec![row()]))
                .unwrap()
                .replace("\"id\":1", "\"id\":1,\"id\":1"),
            String::from_utf8(bytes(vec![row()]))
                .unwrap()
                .replace("\"board\":null", "\"board\":null,\"board\":null"),
        ] {
            assert!(parse_catalog(input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn rejects_wrong_types_versions_ids_and_duplicate_ids() {
        for (field, value) in [
            ("id", json!(0)),
            ("id", json!(-1)),
            ("id", json!(1.5)),
            ("id", json!("1")),
            ("board", json!(false)),
            ("op_only", json!(0)),
            ("reply_only", json!(null)),
            ("image_only", json!("false")),
            ("exclude_boards", json!([])),
            ("title", json!(null)),
            ("weight", json!("NaN")),
            ("filtered", json!(1.5)),
        ] {
            let mut altered = row();
            altered[field] = value;
            assert!(parse_catalog(&bytes(vec![altered])).is_err(), "{field}");
        }
        assert!(matches!(
            parse_catalog(&bytes(vec![row(), row()])),
            Err(ImportError::Catalog)
        ));
        assert!(matches!(
            parse_catalog(br#"{"version":2,"categories":[]}"#),
            Err(ImportError::Version)
        ));
        let input = String::from_utf8(bytes(vec![row()])).unwrap();
        for replacement in ["1e400", "NaN", "Infinity"] {
            assert!(parse_catalog(input.replace("1.25", replacement).as_bytes()).is_err());
        }
        assert!(
            parse_catalog(
                input
                    .replace("\"id\":1", "\"id\":9223372036854775808")
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn checks_byte_limits_and_postgres_nul_without_normalization() {
        for (field, limit) in [
            ("board", MAX_BOARD_BYTES),
            ("title", MAX_TITLE_BYTES),
            ("exclude_boards", MAX_EXCLUSIONS_BYTES),
        ] {
            let mut altered = row();
            altered[field] = json!("x".repeat(limit));
            assert!(parse_catalog(&bytes(vec![altered.clone()])).is_ok());
            altered[field] = json!("x".repeat(limit + 1));
            assert!(parse_catalog(&bytes(vec![altered.clone()])).is_err());
            altered[field] = json!("é".repeat(limit));
            assert!(parse_catalog(&bytes(vec![altered.clone()])).is_err());
            altered[field] = json!("\0");
            assert!(parse_catalog(&bytes(vec![altered])).is_err());
        }
    }

    #[test]
    fn checks_category_count_and_bounded_reader_before_decoding() {
        let rows: Vec<_> = (1..=MAX_CATEGORIES)
            .map(|id| {
                let mut value = row();
                value["id"] = json!(id);
                value
            })
            .collect();
        assert_eq!(
            parse_catalog(&bytes(rows.clone())).unwrap().len(),
            MAX_CATEGORIES
        );
        let mut too_many = rows;
        too_many.push(row());
        assert!(matches!(
            parse_catalog(&bytes(too_many)),
            Err(ImportError::RowLimit)
        ));
        assert!(matches!(
            read_catalog(std::io::repeat(b' ')),
            Err(ImportError::InputLimit)
        ));
        let mut exact = br#"{"version":1,"categories":[]}"#.to_vec();
        exact.resize(MAX_INPUT_BYTES, b' ');
        assert!(read_catalog(exact.as_slice()).is_ok());
        exact.push(b' ');
        assert!(matches!(
            parse_catalog(&exact),
            Err(ImportError::InputLimit)
        ));
    }
}
