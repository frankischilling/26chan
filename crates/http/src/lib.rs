#![forbid(unsafe_code)]

//! Retain request admission until the final response body and its emitted data
//! have been released. This counts admitted responses, not bytes or connections.

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use bytes::Bytes;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::sync::OwnedSemaphorePermit;

#[derive(Clone)]
struct ResponsePermit(Arc<OwnedSemaphorePermit>);

/// Attach an admission permit before response middleware can replace the body.
///
/// Install [`retain_response_body`] outside those middleware layers to transfer
/// ownership to the final body. Body replacements must preserve extensions.
pub fn hold_permit(mut response: Response, permit: OwnedSemaphorePermit) -> Response {
    response
        .extensions_mut()
        .insert(ResponsePermit(Arc::new(permit)));
    response
}

/// Finalize response admission after all body-rewriting middleware has run.
///
/// The private extension is removed, so retaining final response parts does not
/// occupy admission after the body and its emitted data have been released.
pub async fn retain_response_body(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let Some(ResponsePermit(permit)) = response.extensions_mut().remove::<ResponsePermit>() else {
        return response;
    };

    // Only an end-of-stream hint proves no frames remain. An exact zero byte
    // size hint alone can still describe a pending body or trailing headers.
    if response.body().is_end_stream() {
        return response;
    }

    response.map(|body| {
        Body::new(RetainedBody {
            inner: Some(body),
            permit: Some(permit),
        })
    })
}

struct RetainedBody {
    inner: Option<Body>,
    permit: Option<Arc<OwnedSemaphorePermit>>,
}

struct RetainedBytes {
    bytes: Bytes,
    _permit: Arc<OwnedSemaphorePermit>,
}

impl AsRef<[u8]> for RetainedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl HttpBody for RetainedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(None);
        };

        match Pin::new(inner).poll_frame(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(frame))) => {
                let frame = frame.map_data(|bytes| {
                    if bytes.is_empty() {
                        return bytes;
                    }
                    // The original allocation remains in the owner. Clones and
                    // slices of the emitted Bytes retain the same permit without
                    // copying the payload, even after this body is dropped.
                    Bytes::from_owner(RetainedBytes {
                        bytes,
                        _permit: Arc::clone(self.permit.as_ref().expect("active body permit")),
                    })
                });
                if self.inner.as_ref().is_some_and(HttpBody::is_end_stream) {
                    self.inner = None;
                    self.permit = None;
                }
                Poll::Ready(Some(Ok(frame)))
            }
            terminal => {
                // http-body requires discarding a body after an error. Drop its
                // unread data too; earlier emitted data has independent owners.
                self.inner = None;
                self.permit = None;
                terminal
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.as_ref().is_none_or(HttpBody::is_end_stream)
    }

    fn size_hint(&self) -> SizeHint {
        self.inner
            .as_ref()
            .map_or_else(|| SizeHint::with_exact(0), HttpBody::size_hint)
    }
}

#[cfg(test)]
mod tests;
