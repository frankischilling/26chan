use std::{
    collections::VecDeque,
    future::{Future, pending},
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt;
use tokio::sync::Semaphore;
use tower::ServiceExt;

use super::{hold_permit, retain_response_body};

fn admitted(body: Body) -> (Arc<Semaphore>, Response) {
    let semaphore = Arc::new(Semaphore::new(1));
    let permit = semaphore.clone().try_acquire_owned().unwrap();
    (semaphore, hold_permit(Response::new(body), permit))
}

fn respond_once(response: Response) -> Router {
    let response = Arc::new(Mutex::new(Some(response)));
    Router::new().fallback(move || {
        let response = response.lock().unwrap().take().unwrap();
        async move { response }
    })
}

async fn finalize(response: Response) -> Response {
    respond_once(response)
        .layer(middleware::from_fn(retain_response_body))
        .oneshot(Request::new(Body::empty()))
        .await
        .unwrap()
}

async fn data(body: &mut Body) -> Bytes {
    body.frame().await.unwrap().unwrap().into_data().unwrap()
}

// A bounded body whose EOF is only discoverable by polling. It also lets the
// ownership wrapper exercise trailers and errors without a socket or database.
struct Frames(VecDeque<Result<Frame<Bytes>, io::Error>>);

impl Frames {
    fn body(frames: impl IntoIterator<Item = Result<Frame<Bytes>, io::Error>>) -> Body {
        Body::new(Self(frames.into_iter().collect()))
    }
}

impl HttpBody for Frames {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        Poll::Ready(self.0.pop_front())
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(
            self.0
                .iter()
                .filter_map(|frame| frame.as_ref().ok()?.data_ref())
                .map(|bytes| bytes.len() as u64)
                .sum(),
        )
    }
}

struct PendingBody;

impl HttpBody for PendingBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        Poll::Pending
    }
}

#[tokio::test]
async fn unconsumed_response_retains_admission_until_drop() {
    let (semaphore, response) = admitted(Body::from("held"));
    assert_eq!(semaphore.available_permits(), 0);
    let response = finalize(response).await;
    assert_eq!(semaphore.available_permits(), 0);
    drop(response);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn dropping_response_before_finalization_releases_admission() {
    let (semaphore, response) = admitted(Body::from("held"));
    assert_eq!(semaphore.available_permits(), 0);
    drop(response);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn emitted_data_clones_and_slices_retain_admission_after_body_drop() {
    let source = Bytes::from_static(b"retained");
    let source_pointer = source.as_ptr();
    let (semaphore, response) = admitted(Body::from(source));
    let mut body = finalize(response).await.into_body();
    let bytes = data(&mut body).await;
    assert_eq!(bytes, "retained");
    assert_eq!(bytes.as_ptr(), source_pointer, "payload was copied");
    let clone = bytes.clone();
    let slice = bytes.slice(2..6);
    drop(body);
    drop(bytes);
    assert_eq!(semaphore.available_permits(), 0);
    drop(clone);
    assert_eq!(semaphore.available_permits(), 0);
    assert_eq!(slice, "tain");
    drop(slice);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn known_last_frame_releases_body_ownership_without_an_extra_poll() {
    let (semaphore, response) = admitted(Body::from("last"));
    let (parts, mut body) = finalize(response).await.into_parts();
    assert_eq!(body.size_hint().exact(), Some(4));
    let bytes = data(&mut body).await;
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
    assert_eq!(semaphore.available_permits(), 0);
    drop(bytes);
    assert_eq!(semaphore.available_permits(), 1);
    // Keeping final response parts alive must not keep the private permit.
    drop(parts);
}

#[tokio::test]
async fn pending_body_retains_admission_until_cancelled() {
    let (semaphore, response) = admitted(Body::new(PendingBody));
    let mut body = finalize(response).await.into_body();
    assert!(
        Pin::new(&mut body)
            .poll_frame(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(semaphore.available_permits(), 0);
    drop(body);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn eof_releases_body_owner_but_keeps_emitted_data_admitted() {
    let (semaphore, response) = admitted(Frames::body([
        Ok(Frame::data(Bytes::from_static(b"one"))),
        Ok(Frame::data(Bytes::from_static(b"two"))),
    ]));
    let mut body = finalize(response).await.into_body();
    assert_eq!(body.size_hint().exact(), Some(6));
    let first = data(&mut body).await;
    assert_eq!(body.size_hint().exact(), Some(3));
    let last = data(&mut body).await;
    assert_eq!(first, "one");
    assert_eq!(last, "two");
    drop(last);
    assert!(body.frame().await.is_none());
    assert!(body.is_end_stream());
    assert_eq!(semaphore.available_permits(), 0);
    drop(first);
    assert_eq!(semaphore.available_permits(), 1);
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn error_is_preserved_and_terminal_while_prior_data_retains_admission() {
    let (semaphore, response) = admitted(Frames::body([
        Ok(Frame::data(Bytes::from_static(b"first"))),
        Err(io::Error::other("fixture body error")),
        Ok(Frame::data(Bytes::from_static(b"discard after error"))),
    ]));
    let mut body = finalize(response).await.into_body();
    let first = data(&mut body).await;
    let error = body.frame().await.unwrap().unwrap_err();
    assert_eq!(error.to_string(), "fixture body error");
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(body.frame().await.is_none());
    assert_eq!(semaphore.available_permits(), 0);
    drop(first);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn trailers_are_preserved_and_eof_releases_admission() {
    let mut trailers = HeaderMap::new();
    trailers.insert("x-checksum", HeaderValue::from_static("abc"));
    let (semaphore, response) = admitted(Frames::body([Ok(Frame::trailers(trailers))]));
    let mut body = finalize(response).await.into_body();
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(!body.is_end_stream());
    assert_eq!(semaphore.available_permits(), 0);
    let trailers = body
        .frame()
        .await
        .unwrap()
        .unwrap()
        .into_trailers()
        .unwrap();
    assert_eq!(trailers["x-checksum"], "abc");
    assert!(body.frame().await.is_none());
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn empty_final_body_releases_admission_with_response_still_alive() {
    let (semaphore, response) = admitted(Body::empty());
    assert_eq!(semaphore.available_permits(), 0);
    let response = finalize(response).await;
    assert!(response.body().is_end_stream());
    assert_eq!(response.body().size_hint().exact(), Some(0));
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn empty_data_frames_do_not_retain_admission_after_eof() {
    let (semaphore, response) = admitted(Frames::body([Ok(Frame::data(Bytes::new()))]));
    let mut body = finalize(response).await.into_body();
    let empty = data(&mut body).await;
    assert!(empty.is_empty());
    assert_eq!(semaphore.available_permits(), 0);
    assert!(body.frame().await.is_none());
    assert_eq!(semaphore.available_permits(), 1);
    drop(empty);
}

async fn replace_body(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    *response.body_mut() = Body::from("replacement");
    response
}

#[tokio::test]
async fn replaced_body_retains_admission_and_preserves_response_parts() {
    let (semaphore, mut response) = admitted(Body::empty());
    *response.status_mut() = StatusCode::BAD_REQUEST;
    response
        .headers_mut()
        .insert("x-test", HeaderValue::from_static("preserved"));
    response.extensions_mut().insert(17_u32);
    let response = respond_once(response)
        .layer(middleware::from_fn(replace_body))
        .layer(middleware::from_fn(retain_response_body))
        .oneshot(Request::new(Body::empty()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["x-test"], "preserved");
    assert_eq!(response.extensions().get::<u32>(), Some(&17));
    assert_eq!(response.extensions().len(), 1);
    let (parts, mut body) = response.into_parts();
    assert_eq!(body.size_hint().exact(), Some(11));
    let bytes = data(&mut body).await;
    assert_eq!(bytes, "replacement");
    drop(body);
    assert_eq!(semaphore.available_permits(), 0);
    drop(bytes);
    assert_eq!(semaphore.available_permits(), 1);
    drop(parts);
}

#[tokio::test]
async fn cancellation_after_admission_before_finalization_releases_permit() {
    let (semaphore, response) = admitted(Body::from("held"));
    let app = respond_once(response)
        .layer(middleware::from_fn(|request, next: Next| async move {
            let response = next.run(request).await;
            pending::<()>().await;
            response
        }))
        .layer(middleware::from_fn(retain_response_body));
    let mut request = Box::pin(app.oneshot(Request::new(Body::empty())));
    assert!(
        request
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(semaphore.available_permits(), 0);
    drop(request);
    assert_eq!(semaphore.available_permits(), 1);
}

#[tokio::test]
async fn responses_without_admission_preserve_data_and_headers() {
    let mut response = Response::new(Body::from("unadmitted"));
    *response.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
    let response = finalize(response).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.extensions().is_empty());
    assert_eq!(response.body().size_hint().exact(), Some(10));
    let mut body = response.into_body();
    assert_eq!(data(&mut body).await, "unadmitted");
}
