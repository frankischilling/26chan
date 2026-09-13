use crate::{AppState, security::error};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use board_media::ObjectId;
use board_store::StoreError;

pub async fn image(
    State(state): State<AppState>,
    Path(name): Path<String>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let thumbnail = name.ends_with(".thumb.png");
    let suffix = if thumbnail { ".thumb.png" } else { ".png" };
    let Some(id) = name
        .strip_suffix(suffix)
        .and_then(|s| s.parse::<ObjectId>().ok())
    else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("/media/{id}{suffix}") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = if thumbnail {
        state.reader.get_thumbnail(&id.to_string()).await
    } else {
        state.reader.get(&id.to_string()).await
    };
    serve(state, asset, thumbnail, headers).await
}

pub async fn post_image(
    State(state): State<AppState>,
    Path((board, name)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let thumbnail = name.ends_with("s.jpg");
    let suffix = if thumbnail { "s.jpg" } else { ".png" };
    let Some(tim) = name
        .strip_suffix(suffix)
        .and_then(|n| n.parse::<i64>().ok())
    else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("/{board}/{tim}{suffix}") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = state.reader.get_post(&board, tim, thumbnail).await;
    serve(state, asset, thumbnail, headers).await
}

async fn serve(
    state: AppState,
    asset: Result<board_store::media_assets::Asset, StoreError>,
    thumbnail: bool,
    headers: HeaderMap,
) -> Response {
    let asset = match asset {
        Ok(asset) => asset,
        Err(StoreError::NotFound) => return error(StatusCode::NOT_FOUND),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(id) = asset.id.parse::<ObjectId>() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let etag = format!("\"{}\"", asset.sha256);
    let files = state.files.clone();
    let bytes = match state
        .blocking(move || {
            let size = asset
                .bytes
                .try_into()
                .map_err(|_| board_media::MediaError::Conflict)?;
            if thumbnail {
                files.read_thumbnail(id, &asset.sha256, size)
            } else {
                files.read(id, &asset.sha256, size)
            }
        })
        .await
    {
        Ok(bytes) => bytes,
        Err(status) => return error(status),
    };
    // Approval and actual bytes are checked before a conditional response.
    let unchanged = headers
        .get_all("if-none-match")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .any(|value| matches_etag(value, &etag));
    let length = bytes.len();
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Body::from(bytes).into_response()
    };
    let h = response.headers_mut();
    h.insert(
        "cache-control",
        HeaderValue::from_static("public, no-cache, must-revalidate"),
    );
    h.insert(
        "etag",
        HeaderValue::from_str(&etag).expect("bounded hex etag"),
    );
    h.insert("content-type", HeaderValue::from_static("image/png"));
    h.insert(
        "content-disposition",
        HeaderValue::from_str(&format!("inline; filename=\"{id}.png\"")).expect("opaque filename"),
    );
    h.insert("accept-ranges", HeaderValue::from_static("none"));
    if !unchanged {
        h.insert(
            "content-length",
            HeaderValue::from_str(&length.to_string()).expect("bounded length"),
        );
    }
    response
}

fn matches_etag(value: &str, etag: &str) -> bool {
    value.len() <= 4096
        && value.split(',').any(|part| {
            let tag = part.trim_matches([' ', '\t']);
            tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
        })
}

pub async fn ready(State(state): State<AppState>) -> Response {
    if state.reader.ready().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    let files = state.files.clone();
    match state.blocking(move || files.ready()).await {
        Ok(()) => "ready".into_response(),
        Err(status) => error(status),
    }
}

#[cfg(test)]
mod tests {
    use super::matches_etag;
    use proptest::prelude::*;

    #[test]
    fn bounded_conditional_tags_do_not_match_prefixes_or_substrings() {
        let etag = "\"0123abcd\"";
        for value in [
            "0123abcd",
            "\"0123abc\"",
            "\"0123abcdx\"",
            "w/\"0123abcd\"",
            "\"\\\"0123abcd\"",
        ] {
            assert!(!matches_etag(value, etag), "{value}");
        }
        assert!(!matches_etag(&format!("{}*", " ".repeat(4096)), etag));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn arbitrary_text_is_bounded_and_cannot_match_an_absent_tag(value in ".{0,512}") {
            let tag = "\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"";
            if matches_etag(&value, tag) {
                prop_assert!(value.contains('*') || value.contains(tag));
            }
        }
    }
}
