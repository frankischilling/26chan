//! Shared context projection and bounded aliases for source thread links.
//! The source client identifies threads by the preceding ID, not this context.
use crate::{AppState, handlers, handlers::AppError, views::PostView};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use board_store::Board;

pub(crate) fn is_path(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    let ["", board, "thread", key, context] = parts.as_slice() else {
        return false;
    };
    !board.is_empty()
        && board.len() <= 10
        && board
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && key
            .parse::<i64>()
            .is_ok_and(|id| id > 0 && id.to_string() == *key)
        // cleanup_context_string adds each word length plus one before checking
        // its 50-byte limit, then joins with '-': emitted length is at most 49.
        && context.len() <= 49
        && context.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// Only emit contexts accepted by the inbound alias and interactive-page CSP.
/// Historical context text can contain source whitespace that is not a path.
pub(crate) fn href(board: &str, id: i64, context: &str) -> String {
    let base = format!("/{board}/thread/{id}");
    let candidate = format!("{base}/{context}");
    if is_path(&candidate) { candidate } else { base }
}

// Source computes context when loading the OP, not at insertion. Saved subject,
// formatting stamp, filter payload and randomizer results supply that input.
pub(crate) fn context(post: &PostView, board: &Board) -> Result<String, AppError> {
    if board.staff_only {
        return Ok(String::new());
    }
    let subject = board_domain::source_html_entities(&post.post.subject);
    let projection_error = |_| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not format thread context.",
        )
    };
    let context =
        board_domain::semantic_context::generate(&subject, "", false).map_err(projection_error)?;
    if !context.is_empty() {
        return Ok(context);
    }
    let comment = stored_comment(post);
    // Stored subjects are normalized text. Escape once before source decoding;
    // literal submitted entity spellings must not become generated entities.
    board_domain::semantic_context::generate(&subject, &comment, false).map_err(projection_error)
}

/// Reconstruct saved source formatting as data for escaped text projections.
pub(crate) fn stored_comment(post: &PostView) -> String {
    let mut comment = crate::catalog::teaser::stored_comment(
        &post.lines,
        &post.post.board,
        post.post.comment_format,
    );
    if let Some(dice) = &post.post.dice_result {
        comment = format!(
            "<b>{}<br><br></b>{comment}",
            board_domain::source_html_entities(dice)
        );
    }
    if let Some((text, color)) = post
        .post
        .fortune_text
        .as_deref()
        .zip(post.post.fortune_color.as_deref())
    {
        comment.push_str(&format!(
            "<span class=\"fortune\" style=\"color:{}\"><br><br><b>Your fortune: {}</b></span>",
            board_domain::source_html_entities(color),
            board_domain::source_html_entities(text)
        ));
    }
    comment
}

pub async fn get(
    State(state): State<AppState>,
    Path((board, key, _context)): Path<(String, String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Result<Response, AppError> {
    // Check the original path too, so encoded separators or syntax cannot give
    // the handler and the interactive-page CSP classifier different answers.
    // JSON keys are deliberately not aliases of this HTML-only source shape.
    if !is_path(uri.path()) {
        return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
    }
    handlers::thread(State(state), Path((board, key)), headers, uri).await
}

#[cfg(test)]
mod tests {
    use super::is_path;

    #[test]
    fn emitted_hrefs_share_the_inbound_path_boundary() {
        let id = i64::MAX;
        let base = format!("/test/thread/{id}");
        for context in ["subject-123", &"a".repeat(49)] {
            assert_eq!(
                super::href("test", id, context),
                format!("{base}/{context}")
            );
        }
        for context in [
            "",
            "bad\tcontext",
            "bad/context",
            "bad?context",
            "bad#context",
            "Upper",
            "bad--context",
            &"a".repeat(50),
        ] {
            assert_eq!(super::href("test", id, context), base);
        }
    }

    #[test]
    fn source_word_context_and_full_width_ids_are_bounded() {
        for context in ["test", "subject-with-123", &"a".repeat(49)] {
            assert!(is_path(&format!(
                "/test/thread/9223372036854775807/{context}"
            )));
        }
        for context in [
            "",
            "-word",
            "word-",
            "word--word",
            "Upper",
            "has_under",
            "has.dot",
            "%61",
            "%2f",
            "%3Cscript%3E",
            "one/two",
            &"a".repeat(50),
        ] {
            assert!(!is_path(&format!("/test/thread/1/{context}")), "{context}");
        }
        for key in [
            "0",
            "-1",
            "+1",
            "01",
            "1.json",
            "1-tail.json",
            "9223372036854775808",
        ] {
            assert!(!is_path(&format!("/test/thread/{key}/subject")), "{key}");
        }
        assert!(!is_path("/test/thread/1"));
        assert!(!is_path("//thread/1/subject"));
        assert!(!is_path("/%74est/thread/1/subject"));
        assert!(!is_path("/test/thread/%31/subject"));
        assert!(!is_path("/test/thread/1/subject/"));
    }
}
