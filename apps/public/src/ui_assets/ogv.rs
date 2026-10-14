//! Fixed upstream WebM fallback assets. No filesystem or user-selected routes.
pub(crate) const ASSETS: &[(&str, &str, &[u8])] = &[
    (
        "/static/ogv/ogv.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv.js"),
    ),
    (
        "/static/ogv/ogv-worker-audio.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-worker-audio.js"),
    ),
    (
        "/static/ogv/ogv-worker-video.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-worker-video.js"),
    ),
    (
        "/static/ogv/ogv-demuxer-webm-wasm.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-demuxer-webm-wasm.js"),
    ),
    (
        "/static/ogv/ogv-demuxer-webm-wasm.wasm",
        "application/wasm",
        include_bytes!("../../static/ogv/ogv-demuxer-webm-wasm.wasm"),
    ),
    (
        "/static/ogv/ogv-decoder-video-vp8-wasm.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-decoder-video-vp8-wasm.js"),
    ),
    (
        "/static/ogv/ogv-decoder-video-vp8-wasm.wasm",
        "application/wasm",
        include_bytes!("../../static/ogv/ogv-decoder-video-vp8-wasm.wasm"),
    ),
    (
        "/static/ogv/ogv-decoder-video-vp9-wasm.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-decoder-video-vp9-wasm.js"),
    ),
    (
        "/static/ogv/ogv-decoder-video-vp9-wasm.wasm",
        "application/wasm",
        include_bytes!("../../static/ogv/ogv-decoder-video-vp9-wasm.wasm"),
    ),
    (
        "/static/ogv/ogv-decoder-audio-opus-wasm.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-decoder-audio-opus-wasm.js"),
    ),
    (
        "/static/ogv/ogv-decoder-audio-opus-wasm.wasm",
        "application/wasm",
        include_bytes!("../../static/ogv/ogv-decoder-audio-opus-wasm.wasm"),
    ),
    (
        "/static/ogv/ogv-decoder-audio-vorbis-wasm.js",
        "text/javascript; charset=utf-8",
        include_bytes!("../../static/ogv/ogv-decoder-audio-vorbis-wasm.js"),
    ),
    (
        "/static/ogv/ogv-decoder-audio-vorbis-wasm.wasm",
        "application/wasm",
        include_bytes!("../../static/ogv/ogv-decoder-audio-vorbis-wasm.wasm"),
    ),
];
pub(crate) fn sources(origin: &str, extension: &str) -> String {
    ASSETS
        .iter()
        .filter(|(path, _, _)| path.ends_with(extension))
        .map(|(path, _, _)| format!("{origin}{path}"))
        .collect::<Vec<_>>()
        .join(" ")
}
pub(crate) fn is_worker(path: &str) -> bool {
    matches!(
        path,
        "/static/ogv/ogv-worker-audio.js" | "/static/ogv/ogv-worker-video.js"
    )
}
