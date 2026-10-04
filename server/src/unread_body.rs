//! Keeps HTTP/1.1 connections reusable when a handler answers without reading
//! the request body: an authentication challenge, an early validation error.
//!
//! hyper then reads only what has already arrived of the body and otherwise
//! closes the connection after the response, without saying so in it. A client
//! that reuses the connection, as every client retrying a 401 with credentials
//! does, fails with EOF on a request it may not replay (a `PUT`). Like Go's
//! net/http server, this layer reads and discards the rest of a small unread
//! body before answering, and otherwise answers with `Connection: close` so the
//! client opens a new connection (docs/adr/0009).

use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, Version},
    middleware::Next,
    response::Response,
};
use bytes::Bytes;
use futures::StreamExt;
use http_body::{Body as HttpBody, Frame, SizeHint};

/// The most of an unread body that is read to keep the connection, as in Go.
const DRAIN_LIMIT: u64 = 256 * 1024;
/// How long the rest of an unread body may take to arrive.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) async fn finish_request_body(req: Request, next: Next) -> Response {
    // HTTP/2 ends unread streams on its own (and forbids `Connection`).
    if req.version() > Version::HTTP_11 || req.body().is_end_stream() {
        return next.run(req).await;
    }
    let expects_continue =
        req.headers().get(header::EXPECT).is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"100-continue"));
    let tracker = Arc::new(Tracker::default());
    let req = req.map(|inner| Body::new(TrackedBody { inner: Some(inner), tracker: tracker.clone() }));
    let mut res = next.run(req).await;
    if tracker.finished.load(Ordering::Acquire) {
        return res;
    }
    let unread = tracker.unread.lock().ok().and_then(|mut unread| unread.take());
    let reusable = match unread {
        // The client sends the body only after `100 Continue`, which reading it would request.
        Some(_) if expects_continue => false,
        Some(body) => drain(body).await,
        // Still held by a task that outlived the handler.
        None => false,
    };
    if !reusable {
        res.headers_mut().insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    res
}

/// Reads and discards the rest of `body`; whether it ended within the limits.
async fn drain(body: Body) -> bool {
    if body.size_hint().lower() > DRAIN_LIMIT {
        return false;
    }
    let mut chunks = body.into_data_stream();
    let read = async {
        let mut left = DRAIN_LIMIT;
        while let Some(chunk) = chunks.next().await {
            let Some(rest) = chunk.ok().and_then(|c| left.checked_sub(c.len() as u64)) else {
                return false;
            };
            left = rest;
        }
        true
    };
    tokio::time::timeout(DRAIN_TIMEOUT, read).await.unwrap_or(false)
}

#[derive(Default)]
struct Tracker {
    /// The body was read to its end.
    finished: AtomicBool,
    /// The rest of the body, handed back when it was dropped unfinished.
    unread: Mutex<Option<Body>>,
}

struct TrackedBody {
    inner: Option<Body>,
    tracker: Arc<Tracker>,
}

impl HttpBody for TrackedBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(None);
        };
        let frame = Pin::new(inner).poll_frame(cx);
        if let Poll::Ready(None) = frame {
            self.tracker.finished.store(true, Ordering::Release);
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.inner.as_ref().is_none_or(|inner| inner.is_end_stream())
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.as_ref().map_or_else(|| SizeHint::with_exact(0), |inner| inner.size_hint())
    }
}

impl Drop for TrackedBody {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        if inner.is_end_stream() {
            self.tracker.finished.store(true, Ordering::Release);
        } else if let Ok(mut unread) = self.tracker.unread.lock() {
            *unread = Some(inner);
        }
    }
}
