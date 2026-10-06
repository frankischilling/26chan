//! Grouped source Thread Options, kept separate from isolated moderation actions.
use crate::{AppError, AppState, auth, auth::Session};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::{Html, Redirect},
};
use std::{collections::BTreeMap, sync::Arc};

/// Private staff-only projection. Sticky rank is not a shared/public Thread field.
#[derive(sqlx::FromRow)]
struct Options {
    sticky: bool,
    sticky_rank: i16,
    closed: bool,
    permasage: bool,
    permaage: bool,
    undead: bool,
    archived: bool,
}
impl Options {
    fn mask(&self) -> i16 {
        i16::from(self.sticky)
            | (i16::from(self.permasage) << 1)
            | (i16::from(self.closed) << 2)
            | (i16::from(self.permaage) << 3)
            | (i16::from(self.undead) << 4)
    }
}

#[derive(Template)]
#[template(path = "thread_options.html")]
struct Page {
    board: String,
    target: i64,
    csrf: String,
    options: Options,
    can_permaage: bool,
    recent: bool,
}

#[derive(Debug)]
struct Input {
    board: String,
    target: i64,
    csrf: String,
    sticky: bool,
    sticky_rank: i16,
    closed: bool,
    permasage: bool,
    permaage: bool,
    undead: bool,
}

impl Input {
    fn effective(&self, old: &Options, can_permaage: bool) -> Options {
        Options {
            sticky: self.sticky,
            sticky_rank: self.sticky_rank,
            closed: self.closed,
            permasage: self.permasage,
            undead: self.undead,
            archived: false,
            permaage: if can_permaage {
                self.permaage
            } else {
                old.permaage
            },
        }
    }
}

// Decode strictly rather than inheriting URL/PHP replacement, array, duplicate,
// or numeric-prefix coercions. Bounds apply even to subsequently ignored values.
fn decode(raw: &[u8]) -> Result<String, AppError> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut pos = 0;
    while pos < raw.len() {
        match raw[pos] {
            b'%' => {
                let pair = raw.get(pos + 1..pos + 3).ok_or(AppError::Invalid)?;
                let high = (pair[0] as char).to_digit(16).ok_or(AppError::Invalid)?;
                let low = (pair[1] as char).to_digit(16).ok_or(AppError::Invalid)?;
                bytes.push((high * 16 + low) as u8);
                pos += 3;
            }
            b'+' => {
                bytes.push(b' ');
                pos += 1;
            }
            byte => {
                bytes.push(byte);
                pos += 1;
            }
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::Invalid)
}

fn fields(raw: &[u8], allowed: &[&str]) -> Result<BTreeMap<String, String>, AppError> {
    if raw.is_empty() || raw.len() > 4096 {
        return Err(AppError::Invalid);
    }
    let mut fields = BTreeMap::new();
    for pair in raw.split(|byte| *byte == b'&') {
        let separator = pair
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or(AppError::Invalid)?;
        let name = decode(&pair[..separator])?;
        let value = decode(&pair[separator + 1..])?;
        if !allowed.contains(&name.as_str()) || fields.insert(name, value).is_some() {
            return Err(AppError::Invalid);
        }
    }
    Ok(fields)
}

fn target(fields: &BTreeMap<String, String>) -> Result<(String, i64), AppError> {
    let board = fields.get("board").ok_or(AppError::Invalid)?;
    board_domain::BoardSlug::parse(board).map_err(|_| AppError::Invalid)?;
    let target = fields.get("target").ok_or(AppError::Invalid)?;
    if target.is_empty() || target.len() > 19 || !target.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Invalid);
    }
    let target = target.parse::<i64>().map_err(|_| AppError::Invalid)?;
    if target <= 0 {
        return Err(AppError::Invalid);
    }
    Ok((board.clone(), target))
}

fn checkbox(fields: &BTreeMap<String, String>, name: &str) -> Result<bool, AppError> {
    match fields.get(name).map(String::as_str) {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        _ => Err(AppError::Invalid),
    }
}

fn parse(raw: &[u8]) -> Result<Input, AppError> {
    let fields = fields(
        raw,
        &[
            "board",
            "target",
            "csrf",
            "sticky",
            "sticky_rank",
            "closed",
            "permasage",
            "permaage",
            "undead",
        ],
    )?;
    let (board, target) = target(&fields)?;
    let csrf = fields.get("csrf").ok_or(AppError::Invalid)?.clone();
    let rank = fields.get("sticky_rank").map_or("0", String::as_str);
    if rank.is_empty() || rank.len() > 2 || !rank.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Invalid);
    }
    let sticky_rank = rank.parse::<i16>().map_err(|_| AppError::Invalid)?;
    if sticky_rank > 60 {
        return Err(AppError::Invalid);
    }
    let sticky = checkbox(&fields, "sticky")?;
    Ok(Input {
        board,
        target,
        csrf,
        sticky,
        sticky_rank: if sticky { sticky_rank } else { 0 },
        closed: checkbox(&fields, "closed")?,
        permasage: checkbox(&fields, "permasage")?,
        permaage: checkbox(&fields, "permaage")?,
        undead: checkbox(&fields, "undead")?,
    })
}

fn authorize(session: &Session, board: &str) -> Result<(), AppError> {
    if !session
        .permissions
        .action_allowed(&session.role, board, "thread-options")
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

const SELECT_OPTIONS: &str = "SELECT t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at IS NOT NULL AS archived FROM content.threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id AND p.thread_id=t.id WHERE t.board=$1 AND t.id=$2 AND NOT t.deleted AND NOT p.deleted";
const SELECT_OPTIONS_LOCKED: &str = "SELECT t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at IS NOT NULL AS archived FROM content.threads t JOIN content.posts p ON p.board=t.board AND p.id=t.id AND p.thread_id=t.id WHERE t.board=$1 AND t.id=$2 AND NOT t.deleted AND NOT p.deleted FOR UPDATE OF t";

pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Html<String>, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    let session = &authority.session;
    let (board, target) = target(&fields(
        query.as_deref().unwrap_or("").as_bytes(),
        &["board", "target"],
    )?)?;
    authorize(session, &board)?;
    let csrf = auth::cookie(&headers, &format!("{}-csrf", state.config.cookie_name()))?;
    auth::csrf(session, &csrf)?;
    let options: Options = sqlx::query_as(SELECT_OPTIONS)
        .bind(&board)
        .bind(target)
        .fetch_optional(&state.staff)
        .await?
        .ok_or(AppError::NotFound)?;
    if options.archived {
        return Err(AppError::Invalid);
    }
    let html = Page {
        board,
        target,
        csrf,
        options,
        can_permaage: session.permissions.can_set_permaage(&session.role),
        recent: session.recent,
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}

pub(crate) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if headers.get_all("sec-fetch-site").iter().count() != 1 {
        return Err(AppError::Forbidden);
    }
    if headers.get_all("content-type").iter().count() != 1
        || headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            != Some("application/x-www-form-urlencoded")
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let input = parse(&body)?;
    let session = &authority.session;
    auth::csrf(session, &input.csrf)?;
    authorize(session, &input.board)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let mut tx = state.staff.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    // Same board -> thread ordering as isolated moderation and posting.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&input.board)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let old: Options = sqlx::query_as(SELECT_OPTIONS_LOCKED)
        .bind(&input.board)
        .bind(input.target)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if old.archived {
        return Err(AppError::Invalid);
    }
    let new = input.effective(&old, session.permissions.can_set_permaage(&session.role));
    sqlx::query("UPDATE content.threads SET sticky=$3,sticky_rank=$4,closed=$5,permasage=$6,permaage=$7,undead=$8,bumped_at=CASE WHEN $9 THEN clock_timestamp() ELSE bumped_at END,modified_at=clock_timestamp() WHERE board=$1 AND id=$2")
        .bind(&input.board).bind(input.target).bind(new.sticky).bind(new.sticky_rank)
        .bind(new.closed).bind(new.permasage).bind(new.permaage).bind(new.undead)
        .bind(old.sticky && !new.sticky).execute(&mut *tx).await?;
    if old.mask() != new.mask() {
        sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,before_mask,after_mask) VALUES ($1,$2,$3,'thread-options',$4,$5)")
            .bind(session.account_id).bind(&input.board).bind(input.target)
            .bind(old.mask()).bind(new.mask()).execute(&mut *tx).await?;
    }
    authority.ensure_current(true).await?;
    tx.commit().await?;
    authority.finish().await?;
    Ok(Redirect::to("/reports"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_preparation_matches_bounded_source_fixture() {
        use serde_json::Value;
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/staff-grouped-options.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2316);
        let mut parity_cases = 0;
        let mut hardening_cases = 0;
        for case in cases {
            let group = case["group"].as_str().unwrap();
            // The rewrite separates GET and POST rather than using a submit
            // button value to select a branch. These twelve cases are GET-only
            // source observations, not grouped mutation parity claims.
            if matches!(group, "missing_submit" | "empty_submit") {
                assert_eq!(case["source"]["status"], "form_only");
                continue;
            }
            let mut encoded = url::form_urlencoded::Serializer::new(String::new());
            encoded
                .append_pair("board", "test")
                .append_pair("target", "42")
                .append_pair("csrf", "fixture");
            if group == "request_hardening_difference" {
                // Preserve duplicates/array names; discard only the old route's
                // submit discriminator before testing the new route grammar.
                for (name, value) in
                    url::form_urlencoded::parse(case["raw_form"].as_str().unwrap().as_bytes())
                {
                    if name != "submit" {
                        encoded.append_pair(&name, &value);
                    }
                }
            } else {
                for (name, value) in case["post"].as_object().unwrap() {
                    if name == "submit" {
                        continue;
                    }
                    let name = if name == "sticknum" {
                        "sticky_rank"
                    } else {
                        name.as_str()
                    };
                    if let Some(value) = value.as_str() {
                        encoded.append_pair(name, value);
                    } else {
                        assert!(value.is_array());
                        encoded.append_pair(&format!("{name}[]"), "1");
                    }
                }
            }
            let parsed = parse(encoded.finish().as_bytes());
            let rank = case["post"].get("sticknum");
            let rank_invalid = rank.is_some_and(|rank| {
                rank.as_str().is_none_or(|rank| {
                    rank.is_empty()
                        || rank.len() > 2
                        || !rank.bytes().all(|b| b.is_ascii_digit())
                        || rank.parse::<i16>().map_or(true, |rank| rank > 60)
                })
            });
            let malformed_flag =
                group == "noncanonical_php_coercion" && case["field"] != "sticknum";
            if rank_invalid || malformed_flag || group == "request_hardening_difference" {
                assert!(parsed.is_err(), "{case}");
                hardening_cases += 1;
                continue;
            }
            let input = parsed.unwrap();
            let context = &case["context"];
            let permissions = crate::access::Permissions {
                allow_boards: vec![
                    if context["allow_all"] == true {
                        "all"
                    } else {
                        "test"
                    }
                    .into(),
                ],
                deny_boards: if context["deny_noboard"] == true {
                    vec!["noboard".into()]
                } else {
                    vec![]
                },
                flags: if context["developer"] == true {
                    vec!["developer".into()]
                } else {
                    vec![]
                },
            };
            let can_permaage = permissions.can_set_permaage(context["role"].as_str().unwrap());
            assert_eq!(
                can_permaage,
                case["source"]["permaage_allowed"].as_bool().unwrap()
            );
            let old_mask = case["old_mask"].as_i64().unwrap() as i16;
            let old = Options {
                sticky: old_mask & 1 != 0,
                permasage: old_mask & 2 != 0,
                closed: old_mask & 4 != 0,
                permaage: old_mask & 8 != 0,
                undead: old_mask & 16 != 0,
                sticky_rank: case["old_rank"].as_i64().unwrap_or(0) as i16,
                archived: false,
            };
            let new = input.effective(&old, can_permaage);
            let source = &case["source"];
            assert_eq!(source["status"], "prepared");
            assert_eq!(
                new.mask(),
                source["effective_assignment_mask"].as_i64().unwrap() as i16,
                "{case}"
            );
            assert_eq!(
                old.mask() != new.mask(),
                source["logged"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(
                new.sticky_rank,
                if new.sticky {
                    source["parsed_rank"].as_i64().unwrap() as i16
                } else {
                    0
                }
            );
            if source["logged"] == true {
                assert_eq!(
                    source["audit"]["old_mask"].as_i64().unwrap() as i16,
                    old.mask()
                );
                assert_eq!(
                    source["audit"]["new_mask"].as_i64().unwrap() as i16,
                    new.mask()
                );
            }
            parity_cases += 1;
        }
        assert!(parity_cases >= 2200);
        assert!(hardening_cases >= 60);
        assert_eq!(parity_cases + hardening_cases + 12, 2316);
    }

    #[test]
    fn omitted_flags_clear_and_rank_is_bounded_decimal() {
        let prefix = "board=test&target=42&csrf=test";
        let input = parse(prefix.as_bytes()).unwrap();
        assert!(
            !input.sticky && !input.closed && !input.permasage && !input.permaage && !input.undead
        );
        assert_eq!(input.sticky_rank, 0);
        for rank in ["0", "00", "01", "59", "60"] {
            let input = parse(format!("{prefix}&sticky=1&sticky_rank={rank}").as_bytes()).unwrap();
            assert_eq!(input.sticky_rank, rank.parse::<i16>().unwrap());
            assert_eq!(
                parse(format!("{prefix}&sticky_rank={rank}").as_bytes())
                    .unwrap()
                    .sticky_rank,
                0
            );
        }
        for rank in [
            "", "61", "-1", "+1", "1x", "1.0", "1e1", "000", "%201", "%FF", "%", "%0G",
        ] {
            for sticky in ["0", "1"] {
                assert!(
                    parse(format!("{prefix}&sticky={sticky}&sticky_rank={rank}").as_bytes())
                        .is_err(),
                    "{rank}"
                );
            }
        }
    }

    #[test]
    fn flags_duplicates_arrays_and_unknown_fields_are_strict() {
        let prefix = "board=test&target=42&csrf=test";
        for name in ["sticky", "closed", "permasage", "permaage", "undead"] {
            for value in ["0", "1"] {
                assert!(parse(format!("{prefix}&{name}={value}").as_bytes()).is_ok());
            }
            for value in ["", "2", "true", "on", "01", "-1", "1x", "%201"] {
                assert!(parse(format!("{prefix}&{name}={value}").as_bytes()).is_err());
            }
            for suffix in [
                format!("{name}=1&{name}=0"),
                format!("{name}%5B%5D=1"),
                format!("{name}%5Bx%5D=1"),
            ] {
                assert!(parse(format!("{prefix}&{suffix}").as_bytes()).is_err());
            }
        }
        for suffix in [
            "board=test",
            "target=42",
            "csrf=test",
            "unknown=1",
            "sticky_rank=1&sticky_rank=2",
            "",
            "sticky",
        ] {
            assert!(parse(format!("{prefix}&{suffix}").as_bytes()).is_err());
        }
        assert!(fields(b"board=test&target=42&target=42", &["board", "target"]).is_err());
        assert!(parse(b"board=test&target=9223372036854775808&csrf=test").is_err());
    }
}
