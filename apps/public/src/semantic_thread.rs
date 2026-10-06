//! Inbound aliases for the ordinary word slugs emitted by the source renderer.
//! The source client identifies threads by the preceding ID, not this context.
use crate::{AppState, handlers, handlers::AppError};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};

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
