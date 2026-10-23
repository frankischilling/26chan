//! Trusted player assets are pinned independently from all uploaded content.
use axum::body::Bytes;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub const PREFIX: &str = "/player-assets/ruffle-0.7.1-memory-cap-v1/";

pub struct PlayerAssets {
    files: BTreeMap<String, Bytes>,
}

impl PlayerAssets {
    /// Load immutable in-memory copies only after exact built-in digest checks.
    /// The adjacent on-disk manifest is not an authority source.
    pub fn open(root: &Path) -> io::Result<Self> {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/ruffle-player-manifest.json"
        ))
        .map_err(io::Error::other)?;
        let entries = manifest["files"].as_object().ok_or_else(rejected)?;
        let mut files = BTreeMap::new();
        for (name, metadata) in entries {
            if name.contains(['/', '\\']) {
                return Err(rejected());
            }
            let size = metadata["bytes"]
                .as_u64()
                .filter(|&size| size <= 16 * 1024 * 1024)
                .ok_or_else(rejected)?;
            let path = root.join(name);
            if !path.symlink_metadata()?.file_type().is_file() {
                return Err(rejected());
            }
            let file = File::open(path)?;
            if !file.metadata()?.is_file() || file.metadata()?.len() != size {
                return Err(rejected());
            }
            let mut bytes = Vec::new();
            file.take(size + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size
                || format!("{:x}", Sha256::digest(&bytes))
                    != metadata["sha256"].as_str().ok_or_else(rejected)?
            {
                return Err(rejected());
            }
            files.insert(name.clone(), Bytes::from(bytes));
        }
        Ok(Self { files })
    }

    pub fn get(&self, name: &str) -> Option<(&'static str, Bytes)> {
        if name == "swf-player.v1.js" {
            return Some((
                "text/javascript; charset=utf-8",
                Bytes::from_static(include_bytes!("../static/swf-player.v1.js")),
            ));
        }
        if name == "swf-player.v1.css" {
            return Some((
                "text/css; charset=utf-8",
                Bytes::from_static(include_bytes!("../static/swf-player.v1.css")),
            ));
        }
        let bytes = self.files.get(name)?.clone();
        let mime = if name.ends_with(".wasm") {
            "application/wasm"
        } else if name.ends_with(".js") {
            "text/javascript; charset=utf-8"
        } else {
            "text/plain; charset=utf-8"
        };
        Some((mime, bytes))
    }
}

fn rejected() -> io::Error {
    io::Error::other("player asset does not match the pinned runtime")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_or_substituted_assets_are_not_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PlayerAssets::open(dir.path()).is_err());
        std::fs::write(
            dir.path().join("172de42e6619eac8f371.wasm"),
            b"untrusted replacement",
        )
        .unwrap();
        assert!(PlayerAssets::open(dir.path()).is_err());
    }

    #[test]
    fn assets_cannot_be_selected_by_path_or_unlisted_name() {
        let assets = PlayerAssets {
            files: BTreeMap::from([("ruffle.js".into(), Bytes::from_static(b"owned"))]),
        };
        assert_eq!(
            assets.get("ruffle.js").unwrap().0,
            "text/javascript; charset=utf-8"
        );
        for name in [
            "../ruffle.js",
            "/ruffle.js",
            "manifest.json",
            "movie.swf",
            "ruffle.js?x",
            "ruffle.js.map",
        ] {
            assert!(assets.get(name).is_none());
        }
    }
    #[test]
    fn player_policy_has_an_opaque_origin_and_exact_network_destinations() {
        let value = policy(
            "http://127.0.0.1:3002",
            "http://127.0.0.1:3000",
            "http://127.0.0.1:3002/player-data/f/123",
        );
        let policy = value.to_str().unwrap();
        assert!(policy.contains("sandbox allow-scripts;"));
        assert!(!policy.contains("allow-same-origin"));
        assert!(!policy.contains("allow-popups"));
        assert!(!policy.contains("allow-top-navigation"));
        assert!(!policy.contains("'unsafe-eval'"));
        assert!(policy.contains("frame-ancestors http://127.0.0.1:3000"));
        let connect = policy
            .split(';')
            .find(|part| part.trim().starts_with("connect-src "))
            .unwrap();
        let allowed: Vec<_> = connect.split_whitespace().skip(1).collect();
        assert_eq!(allowed.len(), 3);
        assert!(allowed[0].ends_with("172de42e6619eac8f371.wasm"));
        assert!(allowed[1].ends_with("878d548199cddef07035.wasm"));
        assert_eq!(allowed[2], "http://127.0.0.1:3002/player-data/f/123");
        assert!(!policy.contains("connect-src 'self'"));
    }

    #[test]
    fn player_routes_accept_only_canonical_board_and_media_numbers() {
        assert!(destination(
            "f",
            123,
            &"/player/f/123".parse().unwrap(),
            "player"
        ));
        for (board, tim, path) in [
            ("f", 123, "/player/f/0123"),
            ("f", 0, "/player/f/0"),
            ("f", 123, "/player/f/%31%32%33"),
            ("F", 123, "/player/F/123"),
            ("../f", 123, "/player/../f/123"),
        ] {
            assert!(!destination(board, tim, &path.parse().unwrap(), "player"));
        }
    }
}

use crate::{AppState, security::error};
use axum::{
    body::Body,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use board_store::{StoreError, media_assets::MediaFormat};

#[derive(Clone)]
pub(crate) struct DocumentPolicy(pub HeaderValue);

fn destination(board: &str, tim: i64, uri: &Uri, prefix: &str) -> bool {
    (1..=16).contains(&board.len())
        && board
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && tim > 0
        && uri.path() == format!("/{prefix}/{board}/{tim}")
}

fn policy(media: &str, public: &str, movie: &str) -> HeaderValue {
    let prefix = format!("{media}{PREFIX}");
    HeaderValue::from_str(&format!(
        "default-src 'none'; script-src 'wasm-unsafe-eval' {prefix}swf-player.v1.js {prefix}ruffle.js {prefix}core.ruffle.16d61072ac7fdc4d5013.js {prefix}core.ruffle.994eeef0a471a88cd0e3.js; connect-src {prefix}172de42e6619eac8f371.wasm {prefix}878d548199cddef07035.wasm {movie}; style-src 'unsafe-inline' {prefix}swf-player.v1.css; img-src data: blob:; media-src blob:; font-src data:; worker-src blob:; sandbox allow-scripts; base-uri 'none'; form-action 'none'; frame-ancestors {public}"
    )).expect("configured origins and canonical numeric movie route")
}

pub(crate) async fn asset(
    State(state): State<AppState>,
    RoutePath(name): RoutePath<String>,
    uri: Uri,
) -> Response {
    let Some(player) = &state.player else {
        return error(StatusCode::NOT_FOUND);
    };
    if uri.path() != format!("{PREFIX}{name}") {
        return error(StatusCode::NOT_FOUND);
    }
    let Some((mime, bytes)) = player.get(&name) else {
        return error(StatusCode::NOT_FOUND);
    };
    (
        [
            ("content-type", mime),
            ("access-control-allow-origin", "*"),
            ("cache-control", "public, max-age=31536000, immutable"),
        ],
        Body::from(bytes),
    )
        .into_response()
}

pub(crate) async fn document(
    State(state): State<AppState>,
    RoutePath((board, tim)): RoutePath<(String, i64)>,
    uri: Uri,
) -> Response {
    if state.player.is_none() || !destination(&board, tim, &uri, "player") {
        return error(StatusCode::NOT_FOUND);
    }
    match state.reader.get_post(&board, tim, false).await {
        Ok(asset) if asset.output_format == MediaFormat::Swf => (),
        Ok(_) | Err(StoreError::NotFound) => return error(StatusCode::NOT_FOUND),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    let filename = match state.reader.get_swf_filename(&board, tim).await {
        Ok(filename) => filename,
        Err(StoreError::NotFound) => return error(StatusCode::NOT_FOUND),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let filename = filename
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;");
    let movie = format!("{}/player-data/{board}/{tim}", state.media_origin);
    let prefix = format!("{}{PREFIX}", state.media_origin);
    // Route URLs use configured origins, an ASCII board and a canonical integer.
    // The filename attribute is escaped above; no uploaded markup is used.
    let html = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Flash player</title><link rel=\"stylesheet\" href=\"{prefix}swf-player.v1.css\"><script defer crossorigin=\"anonymous\" src=\"{prefix}swf-player.v1.js\"></script><script defer crossorigin=\"anonymous\" src=\"{prefix}ruffle.js\"></script><body><div id=\"player\" data-movie=\"{movie}\" data-filename=\"{filename}\"></div><p id=\"status\" role=\"status\">Loading Flash file…</p></body></html>"
    );
    let mut response = Html(html).into_response();
    response.extensions_mut().insert(DocumentPolicy(policy(
        &state.media_origin,
        state.public_origin.to_str().expect("configured origin"),
        &movie,
    )));
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response
}

pub(crate) async fn data(
    State(state): State<AppState>,
    RoutePath((board, tim)): RoutePath<(String, i64)>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if state.player.is_none() || !destination(&board, tim, &uri, "player-data") {
        return error(StatusCode::NOT_FOUND);
    }
    let asset = state.reader.get_post(&board, tim, false).await;
    let mut response = crate::serving::serve(
        state,
        asset,
        crate::serving::Variant::Primary,
        MediaFormat::Swf,
        headers,
        false,
        method,
    )
    .await;
    if matches!(response.status(), StatusCode::OK | StatusCode::NOT_MODIFIED) {
        // Only already-public, approved SWF bytes receive anonymous CORS. The
        // player has an opaque origin and always fetches without credentials.
        response
            .headers_mut()
            .insert("access-control-allow-origin", HeaderValue::from_static("*"));
        response.headers_mut().insert(
            "access-control-expose-headers",
            HeaderValue::from_static("Content-Length"),
        );
    }
    response
}
