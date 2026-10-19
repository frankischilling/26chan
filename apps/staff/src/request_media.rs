//! Private request custody. Only approved representations cross the read transport.
use crate::{AppError, AppState, auth, janitor_requests};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use board_intake_client::approved::{ApprovedMediaClient, ApprovedRepresentation};
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    board: String,
    tim: i64,
    bytes: usize,
    sha256: String,
    format: String,
    width: i32,
    height: i32,
    thumbnail_bytes: usize,
    thumbnail_sha256: String,
    thumbnail_format: String,
    thumbnail_width: i32,
    thumbnail_height: i32,
}
fn media_type(format: &str) -> Result<(&'static str, &'static str), AppError> {
    match format {
        "png" => Ok((".png", "image/png")),
        "jpeg" => Ok((".jpg", "image/jpeg")),
        "gif" => Ok((".gif", "image/gif")),
        "webm" => Ok((".webm", "video/webm")),
        "mp4" => Ok((".mp4", "video/mp4")),
        "pdf" => Ok((".pdf", "application/pdf")),
        _ => Err(AppError::Internal),
    }
}
fn client(origin: &str, production: bool) -> Result<ApprovedMediaClient, AppError> {
    let origin = url::Url::parse(origin).map_err(|_| AppError::Internal)?;
    if production
        || origin.scheme() != "http"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(AppError::Internal);
    }
    let ip: IpAddr = match origin.host() {
        Some(url::Host::Ipv4(ip)) => ip.into(),
        Some(url::Host::Ipv6(ip)) => ip.into(),
        _ => return Err(AppError::Internal),
    };
    ApprovedMediaClient::new(SocketAddr::new(
        ip,
        origin.port_or_known_default().ok_or(AppError::Internal)?,
    ))
    .map_err(|_| AppError::Internal)
}
fn matches_digest(bytes: &[u8], expected: &str) -> bool {
    use sha2::{Digest, Sha256};
    if !board_intake_client::valid_hex(expected, 64) {
        return false;
    }
    format!("{:x}", Sha256::digest(bytes)) == expected
}
pub(crate) async fn capture(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    board: &str,
    post: i64,
    template: i64,
    csrf: &str,
) -> Result<(Option<Vec<u8>>, Option<Vec<u8>>), AppError> {
    let mut authority = auth::guard(state, headers).await?;
    let token = auth::hash(&auth::cookie(headers, state.config.cookie_name())?);
    let plan: Option<sqlx::types::Json<Plan>> =
        sqlx::query_scalar("SELECT staff_identity.janitor_request_media_plan($1,$2,$3,$4,$5,$6)")
            .bind(token)
            .bind(auth::hash(csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(board)
            .bind(post)
            .bind(template)
            .fetch_one(authority.connection())
            .await
            .map_err(janitor_requests::database)?;
    authority.ensure_current(true).await?;
    authority.finish().await?;
    let Some(sqlx::types::Json(plan)) = plan else {
        return Ok((None, None));
    };
    if plan.board != board
        || plan.width <= 0
        || plan.height <= 0
        || plan.thumbnail_width <= 0
        || plan.thumbnail_height <= 0
    {
        return Err(AppError::Internal);
    }
    let client = client(&state.config.media_origin, state.config.production)?;
    let (suffix, content_type) = media_type(&plan.format)?;
    let (_, thumbnail_type) = media_type(&plan.thumbnail_format)?;
    let full = ApprovedRepresentation {
        board,
        tim: plan.tim,
        suffix,
        content_type,
        bytes: plan.bytes,
    };
    let thumbnail = ApprovedRepresentation {
        board,
        tim: plan.tim,
        suffix: "s.jpg",
        content_type: thumbnail_type,
        bytes: plan.thumbnail_bytes,
    };
    let (body, thumb) = tokio::try_join!(client.read(&full), client.read(&thumbnail))
        .map_err(|_| AppError::Internal)?;
    if !matches_digest(&body, &plan.sha256) || !matches_digest(&thumb, &plan.thumbnail_sha256) {
        return Err(AppError::Internal);
    }
    Ok((Some(body), Some(thumb)))
}
#[derive(sqlx::FromRow)]
struct Retained {
    body: Vec<u8>,
    format: String,
}
pub(crate) async fn serve(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, variant)): Path<(i64, String)>,
) -> Result<Response, AppError> {
    let thumbnail = match variant.as_str() {
        "thumbnail" => true,
        "file" => false,
        _ => return Err(AppError::NotFound),
    };
    if id <= 0 {
        return Err(AppError::NotFound);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let retained: Retained = sqlx::query_as(
        "SELECT body,format FROM staff_identity.janitor_request_media($1,$2,$3,$4,$5)",
    )
    .bind(token)
    .bind(auth::hash(&csrf))
    .bind(state.config.idle_timeout.as_secs() as i32)
    .bind(id)
    .bind(thumbnail)
    .fetch_optional(authority.connection())
    .await
    .map_err(janitor_requests::database)?
    .ok_or(AppError::NotFound)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    let (_, content_type) = media_type(&retained.format)?;
    let mut response = (StatusCode::OK, Body::from(retained.body)).into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    if retained.format == "pdf" {
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=retained.pdf"),
        );
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_reads_never_resolve_or_contact_unconfigured_hosts() {
        assert!(client("http://127.0.0.1:3002", false).is_ok());
        assert!(client("http://[::1]:3002", false).is_ok());
        for origin in [
            "https://127.0.0.1:3002",
            "http://localhost:3002",
            "http://192.0.2.1:3002",
            "http://user@127.0.0.1:3002",
            "http://127.0.0.1:3002/path",
            "http://127.0.0.1:3002/?x=1",
        ] {
            assert!(client(origin, false).is_err(), "{origin}");
        }
        assert!(client("http://127.0.0.1:3002", true).is_err());
    }
    #[test]
    fn expected_digests_and_representation_types_are_exact() {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(b"owned"));
        assert!(matches_digest(b"owned", &digest));
        assert!(!matches_digest(b"other", &digest));
        assert!(!matches_digest(b"owned", &digest.to_uppercase()));
        assert!(media_type("svg").is_err());
        assert_eq!(media_type("pdf").unwrap(), (".pdf", "application/pdf"));
    }
}
