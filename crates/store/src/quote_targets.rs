//! Bounded public quote identities from the containing page's read snapshot.
use crate::{Post, StoreError};
use board_domain::{
    Line, Token,
    post_quote::{QuoteClassification, QuoteTargetKey},
};
use std::collections::{BTreeMap, BTreeSet};

/// Backend resource limit, independent of source formatting/posting limits.
pub const MAX_QUOTE_TARGETS: usize = 16_384;

#[derive(Clone, Debug, Default)]
pub struct QuoteTargets {
    targets: BTreeMap<QuoteTargetKey, i64>,
    has_dependencies: bool,
}

impl QuoteTargets {
    pub fn thread_id(&self, key: &QuoteTargetKey) -> Option<i64> {
        self.targets.get(key).copied()
    }

    /// Includes missing targets: their later creation changes presentation too.
    pub fn has_dependencies(&self) -> bool {
        self.has_dependencies
    }

    pub fn resolve_lines(
        &self,
        lines: &mut [Line],
        current_board: &str,
        current_thread: Option<i64>,
    ) {
        for token in lines.iter_mut().flat_map(|line| &mut line.tokens) {
            if let Token::PostQuote(quote) = token {
                let thread_id = match quote.classify(current_board) {
                    QuoteClassification::Lookup(key) => self.thread_id(&key),
                    _ => None,
                };
                quote.resolve(current_board, current_thread, thread_id);
            }
        }
    }
}

fn collect_keys(posts: &[Post]) -> Result<BTreeSet<QuoteTargetKey>, StoreError> {
    let mut keys = BTreeSet::new();
    for post in posts {
        // This is the same persisted profile and authorized saved limit used by
        // rendering, including filtered spans, source markup and word breaks.
        for token in post
            .formatted_lines()
            .into_iter()
            .flat_map(|line| line.tokens)
        {
            if let Token::PostQuote(quote) = token
                && let QuoteClassification::Lookup(key) = quote.classify(&post.board)
            {
                keys.insert(key);
                if keys.len() > MAX_QUOTE_TARGETS {
                    return Err(StoreError::ReadLimit);
                }
            }
        }
    }
    Ok(keys)
}

/// The arrays are paired by UNNEST, never independently matched with ANY.
const TARGET_SQL: &str = "SELECT p.board,p.id,p.thread_id \
    FROM unnest($1::text[],$2::bigint[]) AS requested(board,post_id) \
    JOIN content.posts p ON p.board=requested.board AND p.id=requested.post_id \
    JOIN content.visible_threads t ON t.board=p.board AND t.id=p.thread_id \
    JOIN content.posts op ON op.board=t.board AND op.thread_id=t.id AND op.id=t.id AND NOT op.deleted \
    JOIN content.boards b ON b.slug=p.board \
    WHERE NOT p.deleted AND NOT t.deleted AND NOT b.staff_only";

/// Call before committing the existing REPEATABLE READ transaction. No target
/// bodies, titles, metadata or secrets cross this identity-only read boundary.
pub async fn load(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    posts: &[Post],
) -> Result<QuoteTargets, StoreError> {
    let keys = collect_keys(posts)?;
    if keys.is_empty() {
        return Ok(QuoteTargets::default());
    }
    let boards: Vec<&str> = keys.iter().map(QuoteTargetKey::board).collect();
    let ids: Vec<i64> = keys.iter().map(QuoteTargetKey::post_id).collect();
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(TARGET_SQL)
        .bind(&boards)
        .bind(&ids)
        .fetch_all(&mut **tx)
        .await?;
    let mut targets = BTreeMap::new();
    for (board, id, thread_id) in rows {
        if let Some(key) = QuoteTargetKey::new(&board, id) {
            targets.insert(key, thread_id);
        }
    }
    Ok(QuoteTargets {
        targets,
        has_dependencies: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn post(comment: String, format: i16) -> Post {
        Post {
            id: 1,
            board: "g".into(),
            thread_id: 1,
            name: String::new(),
            trip: None,
            poster_id: None,
            json_op_poster_id: None,
            capcode: None,
            country: None,
            country_name: None,
            board_flag: None,
            board_flag_type: String::new(),
            flag_name: None,
            subject: String::new(),
            image_spoiler: false,
            comment,
            comment_format: format,
            staff_authorized_limits: false,
            wordfilter_payload: None,
            dice_result: None,
            fortune_text: None,
            fortune_color: None,
            created_at: chrono::Utc::now(),
            deleted: false,
            attachment: None,
        }
    }
    #[test]
    fn collection_deduplicates_and_enforces_exact_backend_cap() {
        let mut posts: Vec<Post> = (0..MAX_QUOTE_TARGETS)
            .collect::<Vec<_>>()
            .chunks(1000)
            .map(|ids| post(ids.iter().map(|id| format!(">>{} ", id + 1)).collect(), 104))
            .collect();
        posts.push(post(">>1 >>1 >>>/g/00042 >>>/unknown/9".into(), 104));
        assert!(posts.iter().all(|post| !post.staff_authorized_limits
            && post.comment.chars().count() <= board_domain::MAX_COMMENT_CHARS));
        assert_eq!(collect_keys(&posts).unwrap().len(), MAX_QUOTE_TARGETS);
        posts.push(post(format!(">>{}", MAX_QUOTE_TARGETS + 1), 104));
        assert!(matches!(collect_keys(&posts), Err(StoreError::ReadLimit)));
    }
    #[test]
    fn historical_markup_and_forced_dead_references_do_not_consume_budget() {
        let mut mlp = post(">>>/co/42 >>>/b/43 >>>/j/44 >>0".into(), 104);
        mlp.board = "mlp".into();
        assert!(
            collect_keys(&[mlp, post(">>42 >>>/co/43".into(), 0)])
                .unwrap()
                .is_empty()
        );
        let mut lines = post(">>42".into(), 104).formatted_lines();
        let targets = QuoteTargets {
            targets: BTreeMap::new(),
            has_dependencies: true,
        };
        targets.resolve_lines(&mut lines, "g", Some(1));
        assert!(targets.has_dependencies());
        assert_eq!(board_domain::formatting::plain_text(&lines), ">>42");
    }

    #[test]
    fn source_decimal_spellings_share_one_lookup_and_zero_adds_none() {
        let keys = collect_keys(&[post(
            ">>45 >>00045 >>>/g/00045 >>0 >>0000 >>>/unknown/45".into(),
            104,
        )])
        .unwrap();
        assert_eq!(
            keys,
            BTreeSet::from([QuoteTargetKey::new("g", 45).unwrap()])
        );
    }
}
