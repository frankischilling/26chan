use crate::{AppState, security::error};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use board_media::{ObjectId, still::StillFormat, video::VideoFormat};
use board_store::StoreError;
use board_store::media_assets::MediaFormat;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    Primary,
    Thumbnail,
    Mobile,
}

pub async fn image(
    State(state): State<AppState>,
    Path(name): Path<String>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let (suffix, format, variant) = if name.ends_with(".thumb.png") {
        (".thumb.png", MediaFormat::Png, Variant::Thumbnail)
    } else if name.ends_with(".thumb.jpg") {
        (".thumb.jpg", MediaFormat::Jpeg, Variant::Thumbnail)
    } else if name.ends_with(".mobile.jpg") {
        (".mobile.jpg", MediaFormat::Jpeg, Variant::Mobile)
    } else if name.ends_with(".jpg") {
        (".jpg", MediaFormat::Jpeg, Variant::Primary)
    } else if name.ends_with(".webm") {
        (".webm", MediaFormat::Webm, Variant::Primary)
    } else if name.ends_with(".mp4") {
        (".mp4", MediaFormat::Mp4, Variant::Primary)
    } else if name.ends_with(".swf") {
        (".swf", MediaFormat::Swf, Variant::Primary)
    } else if name.ends_with(".pdf") {
        (".pdf", MediaFormat::Pdf, Variant::Primary)
    } else if name.ends_with(".gif") {
        (".gif", MediaFormat::Gif, Variant::Primary)
    } else {
        (".png", MediaFormat::Png, Variant::Primary)
    };
    let Some(id) = name
        .strip_suffix(suffix)
        .and_then(|s| s.parse::<ObjectId>().ok())
    else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("/media/{id}{suffix}") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = match variant {
        Variant::Thumbnail => state.reader.get_thumbnail(&id.to_string()).await,
        Variant::Mobile => state.reader.get_mobile(&id.to_string()).await,
        Variant::Primary => state.reader.get(&id.to_string()).await,
    };
    serve(state, asset, variant, format, headers, false, method).await
}

pub async fn post_image(
    State(state): State<AppState>,
    Path((board, name)): Path<(String, String)>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if name.ends_with(".swf") {
        if uri.path() != format!("/{board}/{}", board_domain::file_name::url_component(&name)) {
            return error(StatusCode::NOT_FOUND);
        }
        let asset = state.reader.get_named_swf(&board, &name).await;
        let mut response = serve(
            state,
            asset,
            Variant::Primary,
            MediaFormat::Swf,
            headers,
            false,
            method,
        )
        .await;
        if matches!(response.status(), StatusCode::OK | StatusCode::NOT_MODIFIED) {
            response.headers_mut().insert(
                "content-disposition",
                HeaderValue::from_str(&format!(
                    "attachment; filename=\"download.swf\"; filename*=UTF-8''{}",
                    board_domain::file_name::url_component(&name)
                ))
                .expect("encoded path component"),
            );
        }
        return response;
    }
    let (suffix, format, variant) = if name.ends_with("s.jpg") {
        ("s.jpg", MediaFormat::Png, Variant::Thumbnail)
    } else if name.ends_with("m.jpg") {
        ("m.jpg", MediaFormat::Jpeg, Variant::Mobile)
    } else if name.ends_with(".jpg") {
        (".jpg", MediaFormat::Jpeg, Variant::Primary)
    } else if name.ends_with(".webm") {
        (".webm", MediaFormat::Webm, Variant::Primary)
    } else if name.ends_with(".mp4") {
        (".mp4", MediaFormat::Mp4, Variant::Primary)
    } else if name.ends_with(".swf") {
        (".swf", MediaFormat::Swf, Variant::Primary)
    } else if name.ends_with(".pdf") {
        (".pdf", MediaFormat::Pdf, Variant::Primary)
    } else if name.ends_with(".gif") {
        (".gif", MediaFormat::Gif, Variant::Primary)
    } else {
        (".png", MediaFormat::Png, Variant::Primary)
    };
    let Some(tim) = name
        .strip_suffix(suffix)
        .and_then(|n| n.parse::<i64>().ok())
    else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("/{board}/{tim}{suffix}") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = match variant {
        Variant::Mobile => state.reader.get_post_mobile(&board, tim).await,
        _ => {
            state
                .reader
                .get_post(&board, tim, variant == Variant::Thumbnail)
                .await
        }
    };
    // The source thumbnail URL predates format retention. Historical PNG bytes
    // keep their MIME; new JPEG thumbnails use the same public URL.
    let format = if variant == Variant::Thumbnail {
        asset.as_ref().map_or(format, |asset| asset.output_format)
    } else {
        format
    };
    let drawing_png = variant == Variant::Primary
        && format == MediaFormat::Png
        && (method == Method::GET || method == Method::HEAD);
    serve(state, asset, variant, format, headers, drawing_png, method).await
}

/// Canonical records only. Never expose compressed TGKR or private input files.
pub async fn post_replay(
    State(state): State<AppState>,
    Path((board, name)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let Some(tim) = name
        .strip_suffix(".ibr")
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("/replay/{board}/{tim}.ibr") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = match state.reader.get_post_replay(&board, tim).await {
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
            files.read_replay(id, &asset.sha256, size)
        })
        .await
    {
        Ok(bytes) => bytes,
        Err(status) => return error(status),
    };
    // A conditional response must never bypass current visibility or disk checks.
    let unchanged = headers
        .get_all(header::IF_NONE_MATCH)
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
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, no-cache, must-revalidate"),
    );
    h.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("bounded hex etag"),
    );
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"tegaki.ibr\""),
    );
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("none"));
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    let mut origins = headers.get_all(header::ORIGIN).iter();
    if let Some(origin) = origins.next()
        && origins.next().is_none()
        && origin.as_bytes() == state.public_origin.as_bytes()
    {
        h.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            state.public_origin.clone(),
        );
    }
    if !unchanged {
        h.insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).expect("bounded length"),
        );
    }
    response
}

pub(crate) async fn serve(
    state: AppState,
    asset: Result<board_store::media_assets::Asset, StoreError>,
    variant: Variant,
    format: MediaFormat,
    headers: HeaderMap,
    drawing_png: bool,
    method: Method,
) -> Response {
    let asset = match asset {
        Ok(asset) => asset,
        Err(StoreError::NotFound) => return error(StatusCode::NOT_FOUND),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    if asset.output_format != format {
        return error(StatusCode::NOT_FOUND);
    }
    let Ok(id) = asset.id.parse::<ObjectId>() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let etag = format!("\"{}\"", asset.sha256);
    let files = state.files.clone();
    let mut bytes = match state
        .blocking(move || {
            let size = asset
                .bytes
                .try_into()
                .map_err(|_| board_media::MediaError::Conflict)?;
            if variant == Variant::Mobile {
                files.read_still_mobile(id, &asset.sha256, size)
            } else if variant == Variant::Thumbnail {
                if format == MediaFormat::Jpeg {
                    files.read_still_thumbnail(id, &asset.sha256, size)
                } else {
                    files.read_thumbnail(id, &asset.sha256, size)
                }
            } else if format == MediaFormat::Jpeg {
                files.read_still(id, &asset.sha256, size, StillFormat::Jpeg)
            } else if format == MediaFormat::Swf {
                files.read_swf(id, &asset.sha256, size)
            } else if format == MediaFormat::Pdf {
                files.read_pdf(id, &asset.sha256, size)
            } else if format == MediaFormat::Webm {
                files.read_video(id, &asset.sha256, size, VideoFormat::Webm)
            } else if format == MediaFormat::Mp4 {
                files.read_video(id, &asset.sha256, size, VideoFormat::Mp4)
            } else if format == MediaFormat::Gif {
                files.read_gif(id, &asset.sha256, size)
            } else {
                files.read_still(id, &asset.sha256, size, StillFormat::Png)
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
    let total = bytes.len();
    let range = if format.is_video() && method == Method::GET && !unchanged {
        video_range(&headers, &etag, total)
    } else {
        ByteRange::Full
    };
    if range == ByteRange::Unsatisfiable {
        let mut response = error(StatusCode::RANGE_NOT_SATISFIABLE);
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{total}")).expect("bounded length"),
        );
        response
            .headers_mut()
            .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        return response;
    }
    if let ByteRange::Partial(start, end) = range {
        bytes.truncate(end + 1);
        bytes.drain(..start);
    }
    let length = bytes.len();
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else if matches!(range, ByteRange::Partial(_, _)) {
        (StatusCode::PARTIAL_CONTENT, Body::from(bytes)).into_response()
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
    h.insert(
        "content-type",
        HeaderValue::from_static(format.content_type()),
    );
    h.insert(
        "content-disposition",
        HeaderValue::from_str(&format!(
            "{}; filename=\"{id}.{}\"",
            if format == MediaFormat::Swf {
                "attachment"
            } else {
                "inline"
            },
            format.extension()
        ))
        .expect("opaque filename"),
    );
    h.insert(
        "accept-ranges",
        HeaderValue::from_static(if format.is_video() { "bytes" } else { "none" }),
    );
    if let ByteRange::Partial(start, end) = range {
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).expect("bounded range"),
        );
    }
    if drawing_png || format.is_video() {
        // Both representations are verified above, including conditional 304.
        // An eligible response varies even when the request has no Origin or
        // supplies a different or repeated value.
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
        if format.is_video() {
            h.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("Content-Length, Content-Range, Accept-Ranges"),
            );
        }
        let mut origins = headers.get_all(header::ORIGIN).iter();
        if let Some(origin) = origins.next()
            && origins.next().is_none()
            && origin.as_bytes() == state.public_origin.as_bytes()
        {
            h.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                state.public_origin.clone(),
            );
        }
    }
    if !unchanged {
        h.insert(
            "content-length",
            HeaderValue::from_str(&length.to_string()).expect("bounded length"),
        );
    }
    response
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ByteRange {
    Full,
    Partial(usize, usize),
    Unsatisfiable,
}

/// One bounded byte range. Unsupported/malformed syntax is ignored; a valid
/// range wholly outside the object gets 416. Dates/weak validators do not match.
fn video_range(headers: &HeaderMap, etag: &str, length: usize) -> ByteRange {
    let mut conditions = headers.get_all(header::IF_RANGE).iter();
    if let Some(condition) = conditions.next()
        && (conditions.next().is_some() || condition.as_bytes() != etag.as_bytes())
    {
        return ByteRange::Full;
    }
    let mut ranges = headers.get_all(header::RANGE).iter();
    let Some(value) = ranges.next().and_then(|v| v.to_str().ok()) else {
        return ByteRange::Full;
    };
    if ranges.next().is_some() || value.len() > 256 {
        return ByteRange::Full;
    }
    let Some(specification) = value.strip_prefix("bytes=") else {
        return ByteRange::Full;
    };
    let Some((start, end)) = specification.trim().split_once('-') else {
        return ByteRange::Full;
    };
    let number = |value: &str| -> Option<usize> {
        (!value.is_empty() && value.len() <= 20 && value.bytes().all(|b| b.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
    };
    if start.is_empty() {
        let Some(suffix) = number(end) else {
            return ByteRange::Full;
        };
        if suffix == 0 || length == 0 {
            return ByteRange::Unsatisfiable;
        }
        return ByteRange::Partial(length.saturating_sub(suffix), length - 1);
    }
    let Some(start) = number(start) else {
        return ByteRange::Full;
    };
    let end = if end.is_empty() {
        length.saturating_sub(1)
    } else {
        let Some(end) = number(end) else {
            return ByteRange::Full;
        };
        if end < start {
            return ByteRange::Full;
        }
        end
    };
    if start >= length {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Partial(start, end.min(length - 1))
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
    use super::{ByteRange, matches_etag, video_range};
    use axum::http::{HeaderMap, HeaderValue, header};
    use proptest::prelude::*;

    #[test]
    fn bounded_video_ranges_and_strong_validators() {
        let etag = "\"owned\"";
        for (range, expected) in [
            ("bytes=0-0", ByteRange::Partial(0, 0)),
            ("bytes=4-", ByteRange::Partial(4, 9)),
            ("bytes=2-999", ByteRange::Partial(2, 9)),
            ("bytes=-3", ByteRange::Partial(7, 9)),
            ("bytes=-99", ByteRange::Partial(0, 9)),
            ("bytes=10-", ByteRange::Unsatisfiable),
            ("bytes=-0", ByteRange::Unsatisfiable),
            ("bytes=9-3", ByteRange::Full),
            ("bytes=0-1,4-5", ByteRange::Full),
            ("items=0-1", ByteRange::Full),
            ("bytes=+1-2", ByteRange::Full),
            ("bytes=184467440737095516160-", ByteRange::Full),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::RANGE, HeaderValue::from_static(range));
            assert_eq!(video_range(&headers, etag, 10), expected, "{range}");
            headers.insert(header::IF_RANGE, HeaderValue::from_static("\"owned\""));
            assert_eq!(video_range(&headers, etag, 10), expected);
            for stale in ["W/\"owned\"", "\"other\"", "Wed, 21 Oct 2015 07:28:00 GMT"] {
                headers.insert(header::IF_RANGE, HeaderValue::from_static(stale));
                assert_eq!(video_range(&headers, etag, 10), ByteRange::Full);
            }
        }
        let mut headers = HeaderMap::new();
        headers.append(header::RANGE, HeaderValue::from_static("bytes=0-1"));
        headers.append(header::RANGE, HeaderValue::from_static("bytes=4-5"));
        assert_eq!(video_range(&headers, etag, 10), ByteRange::Full);
    }

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
