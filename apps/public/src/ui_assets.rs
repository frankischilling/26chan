//! Release-owned public UI bytes, never upload storage or a filesystem server.
use axum::{Router, http::header, routing::get};

pub(crate) const CATALOG_SCRIPT_PATH: &str = "/static/catalog-preferences.v1.js";

const ASSETS: &[(&str, &str, &[u8])] = &[
    (
        "/static/catalog/filedeleted-res.gif",
        "image/gif",
        include_bytes!("../static/catalog/filedeleted-res.gif"),
    ),
    (
        "/static/catalog/nofile.png",
        "image/png",
        include_bytes!("../static/catalog/nofile.png"),
    ),
    (
        "/static/catalog/spoiler.png",
        "image/png",
        include_bytes!("../static/catalog/spoiler.png"),
    ),
    (
        "/static/catalog/sticky.gif",
        "image/gif",
        include_bytes!("../static/catalog/sticky.gif"),
    ),
    (
        "/static/catalog/closed.gif",
        "image/gif",
        include_bytes!("../static/catalog/closed.gif"),
    ),
    (
        "/static/catalog/sticky@2x.gif",
        "image/gif",
        include_bytes!("../static/catalog/sticky@2x.gif"),
    ),
    (
        "/static/catalog/closed@2x.gif",
        "image/gif",
        include_bytes!("../static/catalog/closed@2x.gif"),
    ),
];

pub(crate) fn routes<S: Clone + Send + Sync + 'static>() -> Router<S> {
    let mut router = Router::new();
    for &(path, mime, bytes) in ASSETS {
        router = router.route(
            path,
            get(move || async move {
                (
                    [
                        (header::CONTENT_TYPE, mime),
                        (header::CACHE_CONTROL, "public, max-age=0, must-revalidate"),
                    ],
                    bytes,
                )
            }),
        );
    }
    router.route(
        CATALOG_SCRIPT_PATH,
        get(|| async {
            (
                [
                    (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                    (header::CACHE_CONTROL, "public, max-age=0, must-revalidate"),
                ],
                include_bytes!("../static/catalog-preferences.v1.js").as_slice(),
            )
        }),
    )
}

pub(crate) fn image_sources(origin: &str) -> String {
    ASSETS
        .iter()
        .map(|(path, _, _)| format!("{origin}{path}"))
        .collect::<Vec<_>>()
        .join(" ")
}
