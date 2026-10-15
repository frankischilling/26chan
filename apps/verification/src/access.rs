use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Access {
    key: Arc<hmac::Key>,
    tag: [u8; 32],
    requests: Arc<Semaphore>,
}
impl Access {
    pub(crate) fn new(token: &str) -> Result<Self, crate::config::ConfigError> {
        if !board_verification_client::valid_token(token) {
            return Err(crate::config::ConfigError);
        }
        let mut material = [0; 32];
        SystemRandom::new()
            .fill(&mut material)
            .map_err(|_| crate::config::ConfigError)?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, &material);
        let tag = hmac::sign(&key, token.as_bytes())
            .as_ref()
            .try_into()
            .map_err(|_| crate::config::ConfigError)?;
        Ok(Self {
            key: Arc::new(key),
            tag,
            requests: Arc::new(Semaphore::new(8)),
        })
    }
    fn authorized(&self, headers: &HeaderMap) -> bool {
        if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
            return false;
        }
        let Some(token) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .filter(|v| board_verification_client::valid_token(v))
        else {
            return false;
        };
        hmac::verify(&self.key, token.as_bytes(), &self.tag).is_ok()
    }
}

pub(crate) fn error(status: StatusCode) -> Response {
    (
        status,
        axum::Json(serde_json::json!({"error":"Verification is unavailable."})),
    )
        .into_response()
}
pub(crate) async fn gate(State(access): State<Access>, request: Request, next: Next) -> Response {
    let mut response = if !access.authorized(request.headers()) {
        error(StatusCode::UNAUTHORIZED)
    } else if let Ok(_permit) = access.requests.clone().try_acquire_owned() {
        match tokio::time::timeout(Duration::from_secs(6), next.run(request)).await {
            Ok(response) => response,
            Err(_) => error(StatusCode::GATEWAY_TIMEOUT),
        }
    } else {
        error(StatusCode::TOO_MANY_REQUESTS)
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("static header"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().expect("static header"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::post};
    use tower::ServiceExt;
    #[tokio::test]
    async fn fixed_authentication_precedes_handlers_and_does_not_accept_duplicate_credentials() {
        let access = Access::new(&"11".repeat(32)).unwrap();
        let app = Router::new()
            .route("/owned", post(|| async { StatusCode::NO_CONTENT }))
            .layer(middleware::from_fn_with_state(access.clone(), gate));
        for token in [None, Some("22".repeat(32)), Some("11".repeat(32))] {
            let mut request = Request::post("/owned");
            if let Some(token) = token.as_ref() {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if token.as_deref() == Some(&"11".repeat(32)) {
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        let request = Request::post("/owned")
            .header(header::AUTHORIZATION, format!("Bearer {}", "11".repeat(32)))
            .header(header::AUTHORIZATION, format!("Bearer {}", "11".repeat(32)))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let _all = access.requests.clone().acquire_many_owned(8).await.unwrap();
        let request = Request::post("/owned")
            .header(header::AUTHORIZATION, format!("Bearer {}", "11".repeat(32)))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
