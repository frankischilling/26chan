//! Public, target-specific category choices. Private weights and filters never
//! cross this boundary. The database resolves the opt-in mode on every request.
use crate::StoreError;
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::PgPool;
use std::collections::HashSet;

pub const MAX_FORM_BYTES: usize = crate::report_catalog::MAX_INPUT_BYTES;
pub const MAX_CATEGORIES: usize = crate::report_catalog::MAX_CATEGORIES;
pub const MAX_TITLE_BYTES: usize = crate::report_catalog::MAX_TITLE_BYTES;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CategoryKind {
    Rule,
    Illegal,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CategoryChoice {
    pub id: i64,
    pub title: String,
    pub kind: CategoryKind,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CategoryForm {
    // Require the field to be present even when the mode is inactive.
    #[serde(deserialize_with = "nullable_revision")]
    pub revision: Option<i64>,
    pub categories: Vec<CategoryChoice>,
}

fn nullable_revision<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    Option::<i64>::deserialize(deserializer)
}

fn invalid_form() -> StoreError {
    StoreError::Database(sqlx::Error::Protocol(
        "Report category form is invalid or exceeds its read budget.".into(),
    ))
}

/// Reject incomplete or excessive output rather than silently truncating the
/// category list, labels, or revision. No configuration is invented here.
pub fn parse_category_form(bytes: &[u8]) -> Result<CategoryForm, StoreError> {
    if bytes.len() > MAX_FORM_BYTES {
        return Err(invalid_form());
    }
    let form: CategoryForm = serde_json::from_slice(bytes).map_err(|_| invalid_form())?;
    if form.revision.is_some_and(|revision| revision <= 0)
        || (form.revision.is_none() && !form.categories.is_empty())
        || form.categories.len() > MAX_CATEGORIES
    {
        return Err(invalid_form());
    }
    let mut ids = HashSet::with_capacity(form.categories.len());
    for category in &form.categories {
        if category.id <= 0
            || !ids.insert(category.id)
            || category.title.len() > MAX_TITLE_BYTES
            || category.title.contains('\0')
            || (category.id == 31) != (category.kind == CategoryKind::Illegal)
        {
            return Err(invalid_form());
        }
    }
    Ok(form)
}

/// Read-only and advisory: POST resolves the target and category again under
/// admission locks. An inactive mode has no revision and no category choices.
pub async fn category_form(
    pool: &PgPool,
    board: &str,
    post: i64,
) -> Result<CategoryForm, StoreError> {
    let json: String = sqlx::query_scalar("SELECT content.report_category_form($1,$2)::text")
        .bind(board)
        .bind(post)
        .fetch_one(pool)
        .await
        .map_err(crate::report_admission::admission_error)?;
    parse_category_form(json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn choice(id: i64) -> serde_json::Value {
        json!({"id": id, "title": " <b>exact label</b> ",
            "kind": if id == 31 { "illegal" } else { "rule" }})
    }

    fn form(categories: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&json!({"revision": 1, "categories": categories})).unwrap()
    }

    #[test]
    fn preserves_labels_order_and_inactive_mode() {
        let parsed = parse_category_form(&form(vec![choice(2), choice(1), choice(31)])).unwrap();
        assert_eq!(parsed.revision, Some(1));
        assert_eq!(
            parsed
                .categories
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            vec![2, 1, 31]
        );
        assert_eq!(parsed.categories[0].title, " <b>exact label</b> ");
        assert_eq!(parsed.categories[2].kind, CategoryKind::Illegal);
        assert_eq!(
            parse_category_form(br#"{"revision":null,"categories":[]}"#)
                .unwrap()
                .revision,
            None
        );
        assert!(parse_category_form(br#"{"revision":1,"categories":[]}"#).is_ok());
        let mut empty_title = choice(1);
        empty_title["title"] = json!("");
        assert!(parse_category_form(&form(vec![empty_title])).is_ok());
    }

    #[test]
    fn rejects_missing_unknown_duplicate_or_wrong_typed_fields() {
        for input in [
            r#"{"categories":[]}"#,
            r#"{"revision":null}"#,
            r#"{"revision":null,"categories":[],"weight":1}"#,
            r#"{"revision":null,"revision":null,"categories":[]}"#,
            r#"{"revision":null,"categories":[],"categories":[]}"#,
            r#"{"revision":"1","categories":[]}"#,
            r#"{"revision":0,"categories":[]}"#,
            r#"{"revision":-1,"categories":[]}"#,
            r#"{"revision":1.5,"categories":[]}"#,
            r#"{"revision":1,"categories":[{"id":1,"id":2,"title":"","kind":"rule"}]}"#,
            r#"{"revision":1,"categories":[]} {}"#,
        ] {
            assert!(parse_category_form(input.as_bytes()).is_err(), "{input}");
        }
        for key in ["id", "title", "kind"] {
            let mut row = choice(1);
            row.as_object_mut().unwrap().remove(key);
            assert!(parse_category_form(&form(vec![row])).is_err(), "{key}");
        }
        for (field, value) in [
            ("id", json!(0)),
            ("id", json!(-1)),
            ("id", json!("1")),
            ("title", json!(null)),
            ("title", json!("\0")),
            ("kind", json!("unknown")),
            ("kind", json!("illegal")),
            ("base_weight", json!(1.0)),
            ("filtered", json!(0)),
        ] {
            let mut row = choice(1);
            row[field] = value;
            assert!(parse_category_form(&form(vec![row])).is_err(), "{field}");
        }
        assert!(parse_category_form(&form(vec![choice(1), choice(1)])).is_err());
        let mut illegal = choice(31);
        illegal["kind"] = json!("rule");
        assert!(parse_category_form(&form(vec![illegal])).is_err());
        let inactive = json!({"revision": null, "categories": [choice(1)]});
        assert!(parse_category_form(&serde_json::to_vec(&inactive).unwrap()).is_err());
        assert!(parse_category_form(&[0xff]).is_err());
    }

    #[test]
    fn rejects_over_budget_without_truncation() {
        let mut row = choice(1);
        row["title"] = json!("x".repeat(MAX_TITLE_BYTES));
        assert!(parse_category_form(&form(vec![row.clone()])).is_ok());
        row["title"] = json!("x".repeat(MAX_TITLE_BYTES + 1));
        assert!(parse_category_form(&form(vec![row.clone()])).is_err());
        row["title"] = json!("é".repeat(MAX_TITLE_BYTES));
        assert!(parse_category_form(&form(vec![row])).is_err());
        let mut rows = (1..=MAX_CATEGORIES as i64).map(choice).collect::<Vec<_>>();
        assert_eq!(
            parse_category_form(&form(rows.clone()))
                .unwrap()
                .categories
                .len(),
            MAX_CATEGORIES
        );
        rows.push(choice(MAX_CATEGORIES as i64 + 1));
        assert!(parse_category_form(&form(rows)).is_err());
        let mut bytes = form(vec![]);
        bytes.resize(MAX_FORM_BYTES, b' ');
        assert!(parse_category_form(&bytes).is_ok());
        bytes.push(b' ');
        assert!(parse_category_form(&bytes).is_err());
    }
}
