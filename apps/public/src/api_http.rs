use crate::{AppState, api, handlers, security};
use axum::{
    Router,
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(ready))
        .route("/boards.json", get(boards))
        .route("/{board}/thread/{key}", get(thread))
        .route("/{board}/{key}", get(board_resource))
        .fallback(not_found)
        .layer(axum::extract::DefaultBodyLimit::max(65_536))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            security::protect,
        ))
        .layer(axum::middleware::from_fn_with_state(state.clone(), cors))
        .with_state(state)
}

async fn boards(State(state): State<AppState>, headers: HeaderMap) -> Response {
    api_response(api::boards(State(state), headers).await)
}

async fn ready(State(state): State<AppState>) -> Response {
    api_response(
        handlers::ready(State(state))
            .await
            .map(IntoResponse::into_response),
    )
}

async fn thread(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let Some(id) = positive_json_number(&key) else {
        return api_error(handlers::AppError(
            StatusCode::NOT_FOUND,
            "Thread not found.",
        ));
    };
    api_response(api::thread(&state, &board, id, &headers).await)
}

async fn board_resource(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let result = match key.as_str() {
        "threads.json" => api::thread_list(&state, &board, &headers).await,
        "catalog.json" => api::catalog(&state, &board, &headers).await,
        _ => match positive_json_number(&key) {
            Some(page) => api::index(&state, &board, page, &headers).await,
            None => return api_error(handlers::AppError(StatusCode::NOT_FOUND, "Page not found.")),
        },
    };
    api_response(result)
}

async fn not_found() -> Response {
    api_error(handlers::AppError(
        StatusCode::NOT_FOUND,
        "API route not found.",
    ))
}

fn positive_json_number(value: &str) -> Option<i64> {
    value
        .strip_suffix(".json")?
        .parse::<i64>()
        .ok()
        .filter(|number| *number > 0)
}

fn api_response(result: Result<Response, handlers::AppError>) -> Response {
    result.unwrap_or_else(api_error)
}

fn api_error(error: handlers::AppError) -> Response {
    (
        error.0,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        serde_json::to_vec(&json!({"error": error.1})).unwrap_or_default(),
    )
        .into_response()
}

pub async fn cors(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let request_method = request.method().clone();
    let endpoint_supported = supported_api_path(request.uri().path());
    let is_preflight = request_method == Method::OPTIONS
        && request
            .headers()
            .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD);
    let origin_allowed = one_header(request.headers(), header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin == state.origin);
    let valid_preflight = endpoint_supported
        && origin_allowed
        && requested_method_allowed(request.headers())
        && requested_headers_allowed(request.headers());
    let mut response = next.run(request).await;
    if request_method == Method::OPTIONS
        && endpoint_supported
        && response.status() == StatusCode::METHOD_NOT_ALLOWED
    {
        if is_preflight && !valid_preflight {
            empty_response(&mut response, StatusCode::BAD_REQUEST);
        } else {
            empty_response(&mut response, StatusCode::NO_CONTENT);
        }
    }
    let cors_allowed = if is_preflight {
        valid_preflight
    } else if request_method == Method::OPTIONS {
        endpoint_supported && origin_allowed
    } else {
        origin_allowed && matches!(request_method, Method::GET | Method::HEAD)
    };
    finish_cors(
        &mut response,
        cors_allowed,
        is_preflight && valid_preflight,
        &state.origin,
    );
    if request_method == Method::HEAD {
        *response.body_mut() = Body::empty();
        response.headers_mut().remove(header::CONTENT_LENGTH);
    }
    response
}

fn one_header(headers: &HeaderMap, name: axum::http::HeaderName) -> Option<&HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn requested_method_allowed(headers: &HeaderMap) -> bool {
    one_header(headers, header::ACCESS_CONTROL_REQUEST_METHOD)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|method| matches!(method, "GET" | "HEAD" | "OPTIONS"))
}

fn requested_headers_allowed(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .iter()
        .all(|value| {
            value.to_str().is_ok_and(|names| {
                names.split(',').all(|name| {
                    matches!(
                        name.trim().to_ascii_lowercase().as_str(),
                        "if-none-match" | "if-modified-since"
                    )
                })
            })
        })
}

fn supported_api_path(path: &str) -> bool {
    if matches!(path, "/healthz" | "/readyz" | "/boards.json") {
        return true;
    }
    let segments: Vec<_> = path.trim_start_matches('/').split('/').collect();
    match segments.as_slice() {
        [_, "thread", key] => positive_json_number(key).is_some(),
        [_, key] => {
            matches!(*key, "threads.json" | "catalog.json") || positive_json_number(key).is_some()
        }
        _ => false,
    }
}

fn empty_response(response: &mut Response, status: StatusCode) {
    *response.status_mut() = status;
    *response.body_mut() = Body::empty();
    response.headers_mut().remove(header::CONTENT_LENGTH);
    response.headers_mut().insert(
        header::ALLOW,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
}

fn finish_cors(response: &mut Response, allowed: bool, methods: bool, origin: &str) {
    let headers = response.headers_mut();
    if !headers.get_all(header::VARY).iter().any(|value| {
        value.to_str().is_ok_and(|vary| {
            vary.split(',')
                .any(|name| name.trim().eq_ignore_ascii_case("origin"))
        })
    }) {
        headers.append(header::VARY, HeaderValue::from_static("Origin"));
    }
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("ETag, Last-Modified"),
    );
    if allowed && let Ok(origin) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    if methods {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD, OPTIONS"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("If-None-Match, If-Modified-Since"),
        );
    }
}
